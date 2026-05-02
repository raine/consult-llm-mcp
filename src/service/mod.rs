use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::catalog::ModelRegistry;
use crate::config::Config;
use crate::executors::types::{LlmExecutor, Usage};
use crate::file::process_files;
use crate::git::generate_git_diff;
use crate::group_thread_store::{self, GroupEntry, StoredGroup, is_group_id};
use crate::llm::ExecutorProvider;
use crate::prompt_builder::build_prompt;
use crate::schema::{ConsultLlmArgs, GitDiffArgs, TaskMode};
use crate::system_prompt::get_system_prompt;

pub mod plan;
pub mod runner;

use plan::{OutputShape, normalize_models, plan_resume};
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

#[derive(Clone)]
pub struct ConsultJob {
    pub model: String,
    pub prompt: String,
    pub thread_id: Option<String>,
    pub entry_index: Option<usize>,
}

/// A fully-resolved set of model jobs together with the output shape that
/// determines how their results are rendered. Both `consult` and
/// `consult_jobs` build a `RunPlan` and hand it to `execute`.
pub struct RunPlan {
    pub jobs: Vec<ConsultJob>,
    pub output: OutputShape,
}

pub struct ConsultService {
    config: Arc<Config>,
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

fn merge_group_entries(
    existing_group: Option<&StoredGroup>,
    results: &[SingleResult],
) -> anyhow::Result<Vec<GroupEntry>> {
    let mut entries = existing_group
        .map(|g| g.entries.clone())
        .unwrap_or_default();

    for r in results {
        if r.failed {
            continue;
        }
        let Some(tid) = &r.thread_id else {
            continue;
        };
        let entry = GroupEntry {
            model: r.model.clone(),
            thread_id: tid.clone(),
        };
        if let Some(idx) = r.entry_index {
            let Some(slot) = entries.get_mut(idx) else {
                anyhow::bail!("matched group entry index {idx} is out of bounds");
            };
            *slot = entry;
        } else {
            entries.push(entry);
        }
    }

    Ok(entries)
}

fn assemble_group_markdown(group_id: &str, results: &[SingleResult]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for r in results {
        *counts.entry(&r.model).or_default() += 1;
    }

    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut out = format!("[thread_id:{group_id}]");
    for (idx, r) in results.iter().enumerate() {
        if idx == 0 {
            out.push_str("\n\n");
        } else {
            out.push_str("\n\n---\n\n");
        }
        let label = if counts[&r.model.as_str()] > 1 {
            let n = seen.entry(&r.model).or_default();
            *n += 1;
            format!("{}#{}", r.model, *n)
        } else {
            r.model.clone()
        };
        out.push_str(&format!("## Model: {label}\n[model:{label}]"));
        if let Some(tid) = &r.thread_id {
            out.push_str(&format!(" [thread_id:{tid}]"));
        }
        out.push_str("\n\n");
        out.push_str(r.body.trim_end());
    }
    out
}

/// Single-model output assembly: surface the worker error directly.
fn single_outcome(outcomes: Vec<anyhow::Result<SingleResult>>) -> anyhow::Result<ConsultOutcome> {
    let r = outcomes
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("single-model run produced no outcome"))??;
    Ok(ConsultOutcome::Response {
        body: r.body,
        usage: r.usage,
        model: r.model,
        thread_id: r.thread_id,
    })
}

/// Group output assembly: render per-model errors inline; only bail when
/// every model failed.
fn collect_group_results(
    jobs: &[ConsultJob],
    outcomes: Vec<anyhow::Result<SingleResult>>,
) -> anyhow::Result<Vec<SingleResult>> {
    let mut results: Vec<SingleResult> = Vec::with_capacity(outcomes.len());
    for (job, outcome) in jobs.iter().zip(outcomes) {
        match outcome {
            Ok(r) => results.push(r),
            Err(e) => results.push(SingleResult {
                model: job.model.clone(),
                body: format!("**Error:** {e:#}"),
                usage: None,
                thread_id: None,
                entry_index: job.entry_index,
                failed: true,
            }),
        }
    }
    if results.iter().all(|r| r.failed) {
        let details = results
            .iter()
            .map(|r| format!("{}: {}", r.model, r.body))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::bail!("all model consultations failed\n{details}");
    }
    Ok(results)
}

impl ConsultService {
    pub fn new(
        config: Arc<Config>,
        registry: Arc<ModelRegistry>,
        executor_provider: Arc<ExecutorProvider>,
    ) -> Self {
        Self {
            config,
            registry,
            executor_provider,
        }
    }

    #[cfg(test)]
    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    pub fn consult(&self, args: ConsultLlmArgs) -> anyhow::Result<ConsultOutcome> {
        if args.web_mode {
            return self.handle_web_mode(args);
        }

        let loaded_group = match args.thread_id.as_deref() {
            Some(tid) if is_group_id(tid) => group_thread_store::load(tid)?,
            _ => None,
        };

        let models = normalize_models(&self.registry, args.model.clone(), loaded_group.as_ref())?;
        let resume = plan_resume(args.thread_id.as_deref(), &models, loaded_group)?;
        let shared = build_shared_inputs(&args)?;

        let jobs: Vec<ConsultJob> = models
            .into_iter()
            .zip(resume.threads)
            .zip(resume.matched_entry_indices)
            .map(|((model, thread_id), entry_index)| ConsultJob {
                model,
                prompt: args.prompt.clone(),
                thread_id,
                entry_index,
            })
            .collect();

        self.execute(
            RunPlan {
                jobs,
                output: resume.output,
            },
            shared,
            args.task_mode,
        )
    }

    /// Run a set of pre-built jobs with per-job prompts in parallel. Always
    /// renders as a group document — the `--run` surface never unwraps to a
    /// plain Response.
    pub fn consult_jobs(
        &self,
        jobs: Vec<ConsultJob>,
        files: &[String],
        git_diff_args: Option<&GitDiffArgs>,
        task_mode: TaskMode,
        existing_group_id: Option<String>,
    ) -> anyhow::Result<ConsultOutcome> {
        let shared = build_shared_inputs_from_files(files, git_diff_args)?;
        self.execute(
            RunPlan {
                jobs,
                output: OutputShape::Group {
                    existing_id: existing_group_id,
                },
            },
            shared,
            task_mode,
        )
    }

    fn execute(
        &self,
        plan: RunPlan,
        shared: SharedInputs,
        task_mode: TaskMode,
    ) -> anyhow::Result<ConsultOutcome> {
        let RunPlan { jobs, output } = plan;
        let outcomes = self.run_workers(&jobs, &shared, task_mode)?;

        match output {
            OutputShape::Single => single_outcome(outcomes),
            OutputShape::Group { existing_id } => {
                let results = collect_group_results(&jobs, outcomes)?;
                self.persist_group_and_render(existing_id, results)
            }
        }
    }

    fn run_workers(
        &self,
        jobs: &[ConsultJob],
        shared: &SharedInputs,
        task_mode: TaskMode,
    ) -> anyhow::Result<Vec<anyhow::Result<SingleResult>>> {
        let executors: Vec<Arc<dyn LlmExecutor>> = jobs
            .iter()
            .map(|j| self.executor_provider.get_executor(&j.model))
            .collect::<anyhow::Result<Vec<_>>>()?;

        // Fan out one OS thread per job. `std::thread::scope` joins them
        // deterministically and lets each thread borrow `shared`.
        Ok(std::thread::scope(|s| {
            let handles: Vec<_> = jobs
                .iter()
                .cloned()
                .zip(executors)
                .map(|(job, exec)| {
                    let config = Arc::clone(&self.config);
                    s.spawn(move || {
                        runner::run_single_model(
                            &config,
                            shared,
                            job.model,
                            exec,
                            job.thread_id,
                            job.entry_index,
                            job.prompt,
                            task_mode,
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| match h.join() {
                    Ok(r) => r,
                    Err(_) => Err(anyhow::anyhow!("worker thread panicked")),
                })
                .collect()
        }))
    }

    fn persist_group_and_render(
        &self,
        existing_group_id: Option<String>,
        results: Vec<SingleResult>,
    ) -> anyhow::Result<ConsultOutcome> {
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

            // Resume case where the group file vanished between when we picked
            // it up and when we got the lock (e.g. cleanup_expired raced).
            // Saving fresh would drop every prior member; bail instead.
            if let Some(id) = existing_group_id.as_deref()
                && existing_group.is_none()
            {
                anyhow::bail!(
                    "Group '{id}' disappeared during the call (likely cleaned up); refusing to recreate with only the new turn"
                );
            }

            group_thread_store::save(&StoredGroup {
                id: group_id.clone(),
                entries: merge_group_entries(existing_group.as_ref(), &results)?,
            })
        })?;

        if existing_group_id.is_none() {
            // Synchronous: directory scan is fast (<1ms typical) and detached
            // threads aren't reliable to run before process exit. See plan
            // ledger entry `untestable-cleanup-background`.
            let _ = group_thread_store::cleanup_expired(7);
        }

        Ok(ConsultOutcome::GroupResponse {
            body: assemble_group_markdown(&group_id, &results),
            usage: None,
        })
    }

    fn handle_web_mode(&self, args: ConsultLlmArgs) -> anyhow::Result<ConsultOutcome> {
        let context_files = args
            .files
            .as_ref()
            .map(|f| process_files(f))
            .transpose()?
            .unwrap_or_default();

        let git_diff = resolve_git_diff(args.git_diff.as_ref());

        let prompt = build_prompt(&args.prompt, &context_files, git_diff.as_deref());
        let system_prompt = get_system_prompt(&self.config, false, args.task_mode);
        let clipboard_text =
            format!("# System Prompt\n\n{system_prompt}\n\n# User Prompt\n\n{prompt}");
        let file_count = context_files.len();

        Ok(ConsultOutcome::WebPrompt {
            clipboard_text,
            file_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse::parse_config;
    use crate::llm::ExecutorProvider;

    fn build_service(system_prompt_path: &str) -> ConsultService {
        let path = system_prompt_path.to_string();
        let env = move |key: &str| match key {
            "CONSULT_LLM_SYSTEM_PROMPT_PATH" => Some(path.clone()),
            "OPENAI_API_KEY" => Some("sk-test".into()),
            _ => None,
        };
        let (config, registry) = parse_config(env).expect("parse_config");
        let config = Arc::new(config);
        let executor_provider = Arc::new(ExecutorProvider::new(Arc::clone(&config)));
        ConsultService::new(config, registry, executor_provider)
    }

    #[test]
    fn two_services_hold_distinct_configs() {
        let a = build_service("/tmp/prompt-a.md");
        let b = build_service("/tmp/prompt-b.md");
        assert_eq!(
            a.config().system_prompt_path.as_deref(),
            Some("/tmp/prompt-a.md")
        );
        assert_eq!(
            b.config().system_prompt_path.as_deref(),
            Some("/tmp/prompt-b.md")
        );
    }

    fn result(
        model: &str,
        thread_id: Option<&str>,
        entry_index: Option<usize>,
        failed: bool,
    ) -> SingleResult {
        SingleResult {
            model: model.into(),
            body: format!("body for {model}"),
            usage: None,
            thread_id: thread_id.map(str::to_string),
            entry_index,
            failed,
        }
    }

    fn job(model: &str) -> ConsultJob {
        ConsultJob {
            model: model.into(),
            prompt: "p".into(),
            thread_id: None,
            entry_index: None,
        }
    }

    #[test]
    fn group_markdown_suffixes_only_duplicate_models() {
        let out = assemble_group_markdown(
            "group_abc",
            &[
                result("gpt-5.2", Some("api_1"), None, false),
                result("gemini-2.5-pro", Some("api_g"), None, false),
                result("gpt-5.2", Some("api_2"), None, false),
            ],
        );
        assert!(out.contains("## Model: gpt-5.2#1\n[model:gpt-5.2#1] [thread_id:api_1]"));
        assert!(out.contains("## Model: gemini-2.5-pro\n[model:gemini-2.5-pro] [thread_id:api_g]"));
        assert!(out.contains("## Model: gpt-5.2#2\n[model:gpt-5.2#2] [thread_id:api_2]"));
    }

    #[test]
    fn group_markdown_distinct_models_stays_plain() {
        let out = assemble_group_markdown(
            "group_abc",
            &[
                result("gpt-5.2", Some("api_1"), None, false),
                result("gemini-2.5-pro", Some("api_g"), None, false),
            ],
        );
        assert!(out.contains("## Model: gpt-5.2\n[model:gpt-5.2] [thread_id:api_1]"));
        assert!(out.contains("## Model: gemini-2.5-pro\n[model:gemini-2.5-pro] [thread_id:api_g]"));
        assert!(!out.contains("#1"));
    }

    #[test]
    fn merge_group_entries_preserves_failed_resume_position() {
        let existing = StoredGroup {
            id: "group_abc".into(),
            entries: vec![
                GroupEntry {
                    model: "gpt-5.2".into(),
                    thread_id: "api_old_1".into(),
                },
                GroupEntry {
                    model: "gpt-5.2".into(),
                    thread_id: "api_old_2".into(),
                },
            ],
        };
        let entries = merge_group_entries(
            Some(&existing),
            &[
                result("gpt-5.2", None, Some(0), true),
                result("gpt-5.2", Some("api_new_2"), Some(1), false),
            ],
        )
        .unwrap();
        assert_eq!(
            entries,
            vec![
                GroupEntry {
                    model: "gpt-5.2".into(),
                    thread_id: "api_old_1".into(),
                },
                GroupEntry {
                    model: "gpt-5.2".into(),
                    thread_id: "api_new_2".into(),
                },
            ]
        );
    }

    #[test]
    fn merge_group_entries_appends_first_turn_successes() {
        let entries = merge_group_entries(
            None,
            &[
                result("gpt-5.2", Some("api_1"), None, false),
                result("gpt-5.2", None, None, true),
                result("gpt-5.2", Some("api_3"), None, false),
            ],
        )
        .unwrap();
        assert_eq!(
            entries,
            vec![
                GroupEntry {
                    model: "gpt-5.2".into(),
                    thread_id: "api_1".into(),
                },
                GroupEntry {
                    model: "gpt-5.2".into(),
                    thread_id: "api_3".into(),
                },
            ]
        );
    }

    #[test]
    fn single_outcome_propagates_worker_error() {
        let err = match single_outcome(vec![Err(anyhow::anyhow!("boom"))]) {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn single_outcome_returns_response_on_success() {
        let outcome =
            single_outcome(vec![Ok(result("gpt-5.2", Some("api_1"), None, false))]).unwrap();
        match outcome {
            ConsultOutcome::Response {
                model, thread_id, ..
            } => {
                assert_eq!(model, "gpt-5.2");
                assert_eq!(thread_id.as_deref(), Some("api_1"));
            }
            _ => panic!("expected Response"),
        }
    }

    #[test]
    fn multi_outcome_renders_partial_failures_inline() {
        let jobs = vec![job("gpt-5.2"), job("gemini-2.5-pro")];
        let outcomes = vec![
            Ok(result("gpt-5.2", Some("api_1"), None, false)),
            Err(anyhow::anyhow!("network kaput")),
        ];
        let results = collect_group_results(&jobs, outcomes).unwrap();
        assert_eq!(results.len(), 2);
        assert!(!results[0].failed);
        assert!(results[1].failed);
        assert!(results[1].body.contains("network kaput"));
        assert_eq!(results[1].model, "gemini-2.5-pro");
    }

    #[test]
    fn multi_outcome_bails_when_all_fail() {
        let jobs = vec![job("gpt-5.2"), job("gemini-2.5-pro")];
        let outcomes = vec![
            Err(anyhow::anyhow!("first failed")),
            Err(anyhow::anyhow!("second failed")),
        ];
        let err = match collect_group_results(&jobs, outcomes) {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(msg.contains("all model consultations failed"));
        assert!(msg.contains("first failed"));
        assert!(msg.contains("second failed"));
    }
}
