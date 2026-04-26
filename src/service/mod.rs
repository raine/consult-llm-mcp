use std::path::PathBuf;
use std::sync::Arc;

use crate::catalog::ModelRegistry;
use crate::executors::types::{LlmExecutor, Usage};
use crate::file::process_files;
use crate::git::generate_git_diff;
use crate::group_thread_store::{self, StoredGroup, is_group_id};
use crate::llm::ExecutorProvider;
use crate::prompt_builder::build_prompt;
use crate::schema::{ConsultLlmArgs, GitDiffArgs, TaskMode};
use crate::system_prompt::get_system_prompt;

pub mod plan;
pub mod runner;

use plan::{normalize_models, plan_resume};
use runner::SingleResult;

fn resolve_git_diff(git_diff: Option<&GitDiffArgs>) -> Option<String> {
    let gd = git_diff?;
    match generate_git_diff(gd.repo_path.as_deref(), &gd.files, &gd.base_ref) {
        Ok(diff) => Some(diff),
        Err(e) => {
            eprintln!("Warning: git diff failed: {e}");
            None
        }
    }
}

pub enum ConsultOutcome {
    Response {
        body: String,
        #[allow(dead_code)]
        usage: Option<Usage>,
        model: String,
        thread_id: Option<String>,
    },
    GroupResponse {
        body: String,
        #[allow(dead_code)]
        usage: Option<Usage>,
    },
    WebPrompt {
        clipboard_text: String,
        file_count: usize,
    },
}

pub struct ConsultJob {
    pub model: String,
    pub prompt: String,
    pub thread_id: Option<String>,
}

pub struct ConsultService {
    registry: Arc<ModelRegistry>,
    executor_provider: Arc<ExecutorProvider>,
}

/// Pre-built inputs shared across all parallel model runs.
pub struct SharedInputs {
    /// Parsed file contents for building API-mode prompts.
    pub context_files: Vec<(String, String)>,
    /// Absolute file paths for CLI-mode executors.
    pub abs_file_paths: Option<Vec<PathBuf>>,
    pub git_diff: Option<String>,
    pub raw_files: Vec<String>,
}

fn build_shared_inputs(args: &ConsultLlmArgs) -> anyhow::Result<SharedInputs> {
    build_shared_inputs_from_files(
        args.files.as_deref().unwrap_or_default(),
        args.git_diff.as_ref(),
    )
}

fn build_shared_inputs_from_files(
    files: &[String],
    git_diff_args: Option<&GitDiffArgs>,
) -> anyhow::Result<SharedInputs> {
    let git_diff = resolve_git_diff(git_diff_args);

    let context_files = if !files.is_empty() {
        process_files(files)?
    } else {
        vec![]
    };

    let abs_file_paths = if !files.is_empty() {
        let cwd = std::env::current_dir()?;
        Some(files.iter().map(|f| cwd.join(f)).collect())
    } else {
        None
    };

    Ok(SharedInputs {
        context_files,
        abs_file_paths,
        git_diff,
        raw_files: files.to_vec(),
    })
}

fn assemble_group_markdown(group_id: &str, results: &[SingleResult]) -> String {
    let mut out = format!("[thread_id:{group_id}]");
    for (idx, r) in results.iter().enumerate() {
        if idx == 0 {
            out.push_str("\n\n");
        } else {
            out.push_str("\n\n---\n\n");
        }
        out.push_str(&format!("## Model: {}\n[model:{}]", r.model, r.model));
        if let Some(tid) = &r.thread_id {
            out.push_str(&format!(" [thread_id:{tid}]"));
        }
        out.push_str("\n\n");
        out.push_str(r.body.trim_end());
    }
    out
}

impl ConsultService {
    pub fn new(registry: Arc<ModelRegistry>, executor_provider: Arc<ExecutorProvider>) -> Self {
        Self {
            registry,
            executor_provider,
        }
    }

    pub async fn consult(&self, args: ConsultLlmArgs) -> anyhow::Result<ConsultOutcome> {
        if args.web_mode {
            return self.handle_web_mode(args).await;
        }

        let loaded_group = match args.thread_id.as_deref() {
            Some(tid) if is_group_id(tid) => group_thread_store::load(tid)?,
            _ => None,
        };

        let models = normalize_models(&self.registry, args.model.clone(), loaded_group.as_ref())?;
        let plan = plan_resume(args.thread_id.as_deref(), &models, loaded_group)?;
        let shared = build_shared_inputs(&args)?;

        let jobs: Vec<ConsultJob> = models
            .iter()
            .map(|m| ConsultJob {
                model: m.clone(),
                prompt: args.prompt.clone(),
                thread_id: plan.threads.get(m).cloned().flatten(),
            })
            .collect();

        self.run_jobs(
            jobs,
            shared,
            args.task_mode,
            plan.group_id,
            plan.unwrap_single,
        )
        .await
    }

    /// Run a set of pre-built jobs with per-job prompts in parallel.
    pub async fn consult_jobs(
        &self,
        jobs: Vec<ConsultJob>,
        files: &[String],
        git_diff_args: Option<&GitDiffArgs>,
        task_mode: TaskMode,
        existing_group_id: Option<String>,
    ) -> anyhow::Result<ConsultOutcome> {
        let shared = build_shared_inputs_from_files(files, git_diff_args)?;
        // --run always uses the group output path; never unwrap to a plain Response.
        self.run_jobs(jobs, shared, task_mode, existing_group_id, false)
            .await
    }

    async fn run_jobs(
        &self,
        jobs: Vec<ConsultJob>,
        shared: SharedInputs,
        task_mode: TaskMode,
        existing_group_id: Option<String>,
        unwrap_single: bool,
    ) -> anyhow::Result<ConsultOutcome> {
        let models: Vec<String> = jobs.iter().map(|j| j.model.clone()).collect();

        let executors: Vec<Arc<dyn LlmExecutor>> = models
            .iter()
            .map(|m| self.executor_provider.get_executor(m))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let futures: Vec<_> = jobs
            .into_iter()
            .zip(executors.into_iter())
            .map(|(job, exec)| {
                runner::run_single_model(
                    &shared,
                    job.model,
                    exec,
                    job.thread_id,
                    job.prompt,
                    task_mode,
                )
            })
            .collect();

        let outcomes = futures::future::join_all(futures).await;

        if unwrap_single {
            // Single-model path: propagate errors directly.
            let r = outcomes.into_iter().next().unwrap()?;
            return Ok(ConsultOutcome::Response {
                body: r.body,
                usage: r.usage,
                model: r.model,
                thread_id: r.thread_id,
            });
        }

        // Multi-model path: collect successes, render errors inline.
        let mut results: Vec<SingleResult> = Vec::new();
        for (model, outcome) in models.iter().zip(outcomes) {
            match outcome {
                Ok(r) => results.push(r),
                Err(e) => results.push(SingleResult {
                    model: model.clone(),
                    body: format!("**Error:** {e:#}"),
                    usage: None,
                    thread_id: None,
                    failed: true,
                }),
            }
        }
        if results.iter().all(|r| r.failed) {
            anyhow::bail!("all model consultations failed");
        }

        let group_id = existing_group_id
            .clone()
            .unwrap_or_else(group_thread_store::generate_group_id);

        // Take an exclusive lock around load → merge → save so concurrent
        // runs against the same group can't both read the old state and
        // clobber each other on write.
        group_thread_store::with_lock(&group_id, || {
            let existing_group = existing_group_id
                .as_deref()
                .map(group_thread_store::load)
                .transpose()?
                .flatten();

            let mut members = existing_group
                .as_ref()
                .map(|g| g.members.clone())
                .unwrap_or_default();

            // Preserve the existing display order; only append models new to this group.
            let mut member_order: Vec<String> = existing_group
                .as_ref()
                .map(|g| {
                    if g.member_order.is_empty() {
                        g.members.keys().cloned().collect()
                    } else {
                        g.member_order.clone()
                    }
                })
                .unwrap_or_default();

            for r in &results {
                // Only record a model in the group if we actually captured a
                // thread id for it — otherwise a later resume that picks the
                // model from member_order would fail in plan_resume because
                // there's no member entry to look up.
                if !r.failed
                    && let Some(tid) = &r.thread_id
                {
                    members.insert(r.model.clone(), tid.clone());
                    if !member_order.contains(&r.model) {
                        member_order.push(r.model.clone());
                    }
                }
            }

            group_thread_store::save(&StoredGroup {
                id: group_id.clone(),
                members,
                member_order,
            })
        })?;

        if existing_group_id.is_none() {
            tokio::task::spawn_blocking(|| {
                let _ = group_thread_store::cleanup_expired(7);
            });
        }

        Ok(ConsultOutcome::GroupResponse {
            body: assemble_group_markdown(&group_id, &results),
            usage: None,
        })
    }

    async fn handle_web_mode(&self, args: ConsultLlmArgs) -> anyhow::Result<ConsultOutcome> {
        let context_files = args
            .files
            .as_ref()
            .map(|f| process_files(f))
            .transpose()?
            .unwrap_or_default();

        let git_diff = resolve_git_diff(args.git_diff.as_ref());

        let prompt = build_prompt(&args.prompt, &context_files, git_diff.as_deref());
        let system_prompt = get_system_prompt(false, args.task_mode);
        let clipboard_text =
            format!("# System Prompt\n\n{system_prompt}\n\n# User Prompt\n\n{prompt}");
        let file_count = context_files.len();

        Ok(ConsultOutcome::WebPrompt {
            clipboard_text,
            file_count,
        })
    }
}
