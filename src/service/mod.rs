use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::catalog::ModelRegistry;
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

#[derive(Clone)]
pub struct ConsultJob {
    pub model: String,
    pub prompt: String,
    pub thread_id: Option<String>,
    pub entry_index: Option<usize>,
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

impl ConsultService {
    pub fn new(registry: Arc<ModelRegistry>, executor_provider: Arc<ExecutorProvider>) -> Self {
        Self {
            registry,
            executor_provider,
        }
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
        let plan = plan_resume(args.thread_id.as_deref(), &models, loaded_group)?;
        let shared = build_shared_inputs(&args)?;

        let jobs: Vec<ConsultJob> = models
            .iter()
            .zip(plan.threads.iter())
            .zip(plan.matched_entry_indices.iter())
            .map(|((m, thread_id), entry_index)| ConsultJob {
                model: m.clone(),
                prompt: args.prompt.clone(),
                thread_id: thread_id.clone(),
                entry_index: *entry_index,
            })
            .collect();

        self.run_jobs(
            jobs,
            shared,
            args.task_mode,
            plan.group_id,
            plan.unwrap_single,
        )
    }

    /// Run a set of pre-built jobs with per-job prompts in parallel.
    pub fn consult_jobs(
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
    }

    fn run_jobs(
        &self,
        jobs: Vec<ConsultJob>,
        shared: SharedInputs,
        task_mode: TaskMode,
        existing_group_id: Option<String>,
        unwrap_single: bool,
    ) -> anyhow::Result<ConsultOutcome> {
        let models: Vec<String> = jobs.iter().map(|j| j.model.clone()).collect();
        let result_jobs = jobs.clone();

        let executors: Vec<Arc<dyn LlmExecutor>> = models
            .iter()
            .map(|m| self.executor_provider.get_executor(m))
            .collect::<anyhow::Result<Vec<_>>>()?;

        // Fan out one OS thread per job. `std::thread::scope` joins them
        // deterministically and lets each thread borrow `shared`.
        let outcomes: Vec<anyhow::Result<runner::SingleResult>> = std::thread::scope(|s| {
            let handles: Vec<_> = jobs
                .into_iter()
                .zip(executors)
                .map(|(job, exec)| {
                    let shared_ref = &shared;
                    s.spawn(move || {
                        runner::run_single_model(
                            shared_ref,
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
        });

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
        for (job, outcome) in result_jobs.iter().zip(outcomes) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
