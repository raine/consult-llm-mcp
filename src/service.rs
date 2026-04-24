use std::path::PathBuf;
use std::sync::Arc;

use crate::config::ModelRegistry;
use crate::executors::stream::SidecarWriter;
use crate::executors::types::{LlmExecutor, Usage};
use crate::file::process_files;
use crate::git::generate_git_diff;
use crate::group_thread_store::{self, StoredGroup, is_group_id};
use crate::llm::ExecutorProvider;
use crate::llm_query::query_llm;
use crate::logger::{log_prompt, log_response};
use crate::prompt_builder::build_prompt;
use crate::schema::{ConsultLlmArgs, GitDiffArgs, ModelSelector};
use crate::system_prompt::get_system_prompt;
use consult_llm_core::stream_events::ParsedStreamEvent;

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
    },
    WebPrompt {
        clipboard_text: String,
    },
}

pub struct ConsultService {
    registry: Arc<ModelRegistry>,
    executor_provider: Arc<ExecutorProvider>,
}

struct SharedInputs {
    /// Inlined API-mode prompt with file contents + git diff baked in.
    /// `None` when no executor in this fan-out needs it.
    api_context_block: Option<String>,
    /// Absolute file paths for CLI-mode executors. `None` when no files passed.
    abs_file_paths: Option<Vec<PathBuf>>,
    git_diff: Option<String>,
    /// Original file list, for FilesContext sidecar event.
    raw_files: Vec<String>,
}

#[cfg_attr(test, derive(Debug))]
struct ResumePlan {
    threads: std::collections::HashMap<String, Option<String>>,
    group_id: Option<String>,
    unwrap_single: bool,
}

struct SingleResult {
    model: String,
    body: String,
    usage: Option<Usage>,
    thread_id: Option<String>,
}

fn normalize_models(
    registry: &ModelRegistry,
    selector: Option<ModelSelector>,
    group_fallback: Option<&StoredGroup>,
) -> anyhow::Result<Vec<String>> {
    let raw = match selector {
        Some(s) => s.into_vec(),
        None => match group_fallback {
            Some(g) => g.members.keys().cloned().collect(),
            None => vec![registry.resolve_model(None)?],
        },
    };
    if raw.is_empty() {
        anyhow::bail!("`model` array must contain at least one entry");
    }
    if raw.len() > 5 {
        anyhow::bail!("max 5 models per call (got {})", raw.len());
    }

    let mut resolved_ordered: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for item in raw {
        let concrete = registry.resolve_model(Some(&item))?;
        if seen.insert(concrete.clone()) {
            resolved_ordered.push(concrete);
        }
    }
    Ok(resolved_ordered)
}

fn plan_resume(
    thread_id: Option<&str>,
    resolved_models: &[String],
    loaded_group: Option<StoredGroup>,
) -> anyhow::Result<ResumePlan> {
    match (thread_id, loaded_group) {
        (None, _) => Ok(ResumePlan {
            threads: resolved_models.iter().map(|m| (m.clone(), None)).collect(),
            group_id: None,
            unwrap_single: resolved_models.len() == 1,
        }),
        (Some(tid), Some(group)) if is_group_id(tid) => {
            let mut threads = std::collections::HashMap::new();
            for m in resolved_models {
                let Some(member_tid) = group.members.get(m) else {
                    anyhow::bail!(
                        "group {tid} has no member for model {m}; group members: {:?}",
                        group.members.keys().collect::<Vec<_>>()
                    );
                };
                threads.insert(m.clone(), Some(member_tid.clone()));
            }
            Ok(ResumePlan {
                threads,
                group_id: Some(tid.to_string()),
                unwrap_single: resolved_models.len() == 1,
            })
        }
        (Some(tid), _) if is_group_id(tid) => {
            anyhow::bail!("group thread not found: {tid}")
        }
        (Some(tid), _) => {
            if resolved_models.len() > 1 {
                anyhow::bail!(
                    "per-model thread_id cannot be used with multiple models; pass a group thread id or omit thread_id"
                );
            }
            let mut threads = std::collections::HashMap::new();
            threads.insert(resolved_models[0].clone(), Some(tid.to_string()));
            Ok(ResumePlan {
                threads,
                group_id: None,
                unwrap_single: true,
            })
        }
    }
}

fn build_shared_inputs(
    args: &ConsultLlmArgs,
    executors: &[Arc<dyn LlmExecutor>],
) -> anyhow::Result<SharedInputs> {
    let git_diff = resolve_git_diff(args.git_diff.as_ref());

    let any_api_mode = executors
        .iter()
        .any(|e| !e.capabilities().supports_file_refs);
    let api_context_block = if any_api_mode {
        let context_files = args
            .files
            .as_ref()
            .map(|f| process_files(f))
            .transpose()?
            .unwrap_or_default();
        Some(build_prompt(
            &args.prompt,
            &context_files,
            git_diff.as_deref(),
        ))
    } else {
        None
    };

    let abs_file_paths: Option<Vec<PathBuf>> = args.files.as_ref().map(|files| {
        let cwd = std::env::current_dir().unwrap_or_default();
        files
            .iter()
            .map(|f| {
                let p = PathBuf::from(f);
                if p.is_absolute() { p } else { cwd.join(f) }
            })
            .collect()
    });

    Ok(SharedInputs {
        api_context_block,
        abs_file_paths,
        git_diff,
        raw_files: args.files.clone().unwrap_or_default(),
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
            if let Some(ModelSelector::Many(v)) = &args.model
                && v.len() > 1
            {
                anyhow::bail!("web_mode requires a single model");
            }
            return self.handle_web_mode(args).await;
        }

        // Load group first so normalize_models can fall back to members when
        // `model` is omitted.
        let loaded_group = match args.thread_id.as_deref() {
            Some(tid) if is_group_id(tid) => group_thread_store::load(tid)?,
            _ => None,
        };

        let models = normalize_models(&self.registry, args.model.clone(), loaded_group.as_ref())?;

        let executors: Vec<Arc<dyn LlmExecutor>> = models
            .iter()
            .map(|m| self.executor_provider.get_executor(m))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let shared = build_shared_inputs(&args, &executors)?;
        let plan = plan_resume(args.thread_id.as_deref(), &models, loaded_group)?;

        let args_ref = &args;
        let shared_ref = &shared;
        let plan_ref = &plan;
        let futures = models
            .iter()
            .cloned()
            .zip(executors.into_iter())
            .map(|(m, exec)| {
                let tid = plan_ref.threads.get(&m).cloned().flatten();
                self.run_single_model(args_ref, shared_ref, m, exec, tid)
            });
        let mut results = futures::future::try_join_all(futures).await?;

        if plan.unwrap_single {
            let r = results.pop().unwrap();
            let mut prefix = format!("[model:{}]", r.model);
            if let Some(tid) = &r.thread_id {
                prefix.push_str(&format!(" [thread_id:{tid}]"));
            }
            return Ok(ConsultOutcome::Response {
                body: format!("{prefix}\n\n{}", r.body),
                usage: r.usage,
            });
        }

        let group_id = plan
            .group_id
            .clone()
            .unwrap_or_else(group_thread_store::generate_group_id);

        let mut members = match &plan.group_id {
            Some(gid) => group_thread_store::load(gid)?
                .map(|g| g.members)
                .unwrap_or_default(),
            None => Default::default(),
        };
        for r in &results {
            if let Some(tid) = &r.thread_id {
                members.insert(r.model.clone(), tid.clone());
            }
        }
        group_thread_store::save(&StoredGroup {
            id: group_id.clone(),
            members,
        })?;

        if plan.group_id.is_none() {
            std::thread::spawn(|| {
                let _ = group_thread_store::cleanup_expired(7);
            });
        }

        Ok(ConsultOutcome::Response {
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

        Ok(ConsultOutcome::WebPrompt { clipboard_text })
    }

    async fn run_single_model(
        &self,
        args: &ConsultLlmArgs,
        shared: &SharedInputs,
        model: String,
        executor: Arc<dyn LlmExecutor>,
        thread_id: Option<String>,
    ) -> anyhow::Result<SingleResult> {
        if thread_id.is_some() && !executor.capabilities().supports_threads {
            anyhow::bail!(
                "thread_id is not supported by the configured backend for model: {model}"
            );
        }

        let consultation_id = uuid::Uuid::new_v4().simple().to_string();
        let backend_name = executor.backend_name().to_string();
        let task_mode_str = match args.task_mode {
            crate::schema::TaskMode::General => None,
            other => Some(format!("{other:?}").to_lowercase()),
        };
        let reasoning_effort = executor.reasoning_effort(&model).map(|s| s.to_string());

        consult_llm_core::monitoring::emit(
            consult_llm_core::monitoring::MonitorEvent::ConsultStarted {
                id: consultation_id.clone(),
                model: model.clone(),
                backend: backend_name.clone(),
                thread_id: thread_id.clone(),
                task_mode: task_mode_str.clone(),
                reasoning_effort: reasoning_effort.clone(),
            },
        );

        let start = std::time::Instant::now();

        let (prompt, file_paths) = if !executor.capabilities().supports_file_refs {
            // API mode: use shared pre-built context block.
            let block = shared
                .api_context_block
                .clone()
                .expect("api_context_block must be built when any executor is API-mode");
            (block, None)
        } else {
            // CLI mode: inline git diff into prompt, pass file paths separately.
            let prompt = match &shared.git_diff {
                Some(diff) if !diff.trim().is_empty() => {
                    format!("## Git Diff\n```diff\n{diff}\n```\n\n{}", args.prompt)
                }
                _ => args.prompt.clone(),
            };
            (prompt, shared.abs_file_paths.clone())
        };

        log_prompt(&model, &prompt);

        if !shared.raw_files.is_empty() {
            let mut sidecar = SidecarWriter::new(Some(&consultation_id));
            sidecar.write(&ParsedStreamEvent::FilesContext {
                files: shared.raw_files.clone(),
            });
            sidecar.flush();
        }

        let system_prompt = get_system_prompt(executor.capabilities().is_cli, args.task_mode);

        let run = query_llm(
            &prompt,
            &model,
            &executor,
            file_paths.as_deref(),
            thread_id.as_deref(),
            &system_prompt,
            Some(&consultation_id),
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        let (success, error_msg, usage, body, tid) = match &run {
            Ok(r) => {
                log_response(&model, &r.response, &r.cost_info);
                (
                    true,
                    None,
                    r.usage.clone(),
                    r.response.clone(),
                    r.thread_id.clone().or_else(|| thread_id.clone()),
                )
            }
            Err(e) => (false, Some(e.to_string()), None, String::new(), None),
        };

        consult_llm_core::monitoring::emit(
            consult_llm_core::monitoring::MonitorEvent::ConsultFinished {
                id: consultation_id.clone(),
                duration_ms,
                success,
                error: error_msg.clone(),
            },
        );

        let project = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "unknown".to_string());

        consult_llm_core::monitoring::append_history(
            &consult_llm_core::monitoring::HistoryRecord {
                ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                consultation_id: Some(consultation_id),
                project,
                model: model.clone(),
                backend: backend_name,
                duration_ms,
                success,
                error: error_msg,
                tokens_in: usage.as_ref().map(|u| u.prompt_tokens),
                tokens_out: usage.as_ref().map(|u| u.completion_tokens),
                parsed_ts: None,
                thread_id: tid.clone(),
                reasoning_effort,
                task_mode: task_mode_str,
            },
        );

        run.map(|_| SingleResult {
            model,
            body,
            usage,
            thread_id: tid,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> ModelRegistry {
        ModelRegistry {
            allowed_models: vec![
                "gpt-5.2".into(),
                "gpt-5.4".into(),
                "gemini-2.5-pro".into(),
                "gemini-3.1-pro-preview".into(),
            ],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        }
    }

    #[test]
    fn normalize_none_with_no_group_uses_default() {
        let out = normalize_models(&reg(), None, None).unwrap();
        assert_eq!(out, vec!["gpt-5.2"]);
    }

    #[test]
    fn normalize_single_string() {
        let out =
            normalize_models(&reg(), Some(ModelSelector::One("gemini".into())), None).unwrap();
        assert_eq!(out, vec!["gemini-3.1-pro-preview"]);
    }

    #[test]
    fn normalize_dedupes_after_resolution() {
        let out = normalize_models(
            &reg(),
            Some(ModelSelector::Many(vec![
                "gemini".into(),
                "gemini-3.1-pro-preview".into(),
                "openai".into(),
            ])),
            None,
        )
        .unwrap();
        // "gemini" + explicit gemini-3.1 collapse to one; openai adds gpt-5.4
        assert_eq!(out, vec!["gemini-3.1-pro-preview", "gpt-5.4"]);
    }

    #[test]
    fn normalize_caps_at_5() {
        let out = normalize_models(&reg(), Some(ModelSelector::Many(vec!["a".into(); 6])), None);
        assert!(out.is_err());
        assert!(out.unwrap_err().to_string().contains("max 5"));
    }

    #[test]
    fn normalize_empty_array_errors() {
        let out = normalize_models(&reg(), Some(ModelSelector::Many(vec![])), None);
        assert!(out.is_err());
    }

    #[test]
    fn normalize_falls_back_to_group_members() {
        use std::collections::BTreeMap;
        let mut members = BTreeMap::new();
        members.insert("gpt-5.2".to_string(), "api_x".to_string());
        members.insert("gemini-2.5-pro".to_string(), "api_y".to_string());
        let group = StoredGroup {
            id: "group_abc".into(),
            members,
        };
        let out = normalize_models(&reg(), None, Some(&group)).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out.contains(&"gpt-5.2".to_string()));
        assert!(out.contains(&"gemini-2.5-pro".to_string()));
    }

    #[test]
    fn plan_resume_no_tid_single_unwraps() {
        let plan = plan_resume(None, &["gpt-5.2".into()], None).unwrap();
        assert!(plan.unwrap_single);
        assert!(plan.group_id.is_none());
    }

    #[test]
    fn plan_resume_no_tid_multi_no_unwrap() {
        let plan = plan_resume(None, &["gpt-5.2".into(), "gemini-2.5-pro".into()], None).unwrap();
        assert!(!plan.unwrap_single);
        assert!(plan.group_id.is_none());
    }

    #[test]
    fn plan_resume_per_model_tid_with_multi_errors() {
        let err = plan_resume(
            Some("api_xxx"),
            &["gpt-5.2".into(), "gemini-2.5-pro".into()],
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("per-model thread_id"));
    }

    #[test]
    fn plan_resume_per_model_tid_single() {
        let plan = plan_resume(Some("api_xxx"), &["gpt-5.2".into()], None).unwrap();
        assert!(plan.unwrap_single);
        assert_eq!(
            plan.threads.get("gpt-5.2").unwrap(),
            &Some("api_xxx".to_string())
        );
    }

    #[test]
    fn plan_resume_group_tid_member_subset() {
        use std::collections::BTreeMap;
        let mut members = BTreeMap::new();
        members.insert("gpt-5.2".to_string(), "api_x".to_string());
        members.insert("gemini-2.5-pro".to_string(), "api_y".to_string());
        let group = StoredGroup {
            id: "group_abc".into(),
            members,
        };
        let plan = plan_resume(
            Some("group_abc"),
            &["gpt-5.2".into(), "gemini-2.5-pro".into()],
            Some(group),
        )
        .unwrap();
        assert_eq!(plan.group_id.as_deref(), Some("group_abc"));
        assert_eq!(
            plan.threads.get("gpt-5.2").unwrap(),
            &Some("api_x".to_string())
        );
        assert_eq!(
            plan.threads.get("gemini-2.5-pro").unwrap(),
            &Some("api_y".to_string())
        );
    }

    #[test]
    fn plan_resume_group_tid_single_member_unwraps() {
        use std::collections::BTreeMap;
        let mut members = BTreeMap::new();
        members.insert("gpt-5.2".to_string(), "api_x".to_string());
        members.insert("gemini-2.5-pro".to_string(), "api_y".to_string());
        let group = StoredGroup {
            id: "group_abc".into(),
            members,
        };
        let plan = plan_resume(Some("group_abc"), &["gpt-5.2".into()], Some(group)).unwrap();
        assert!(plan.unwrap_single);
        assert_eq!(plan.group_id.as_deref(), Some("group_abc"));
    }

    #[test]
    fn plan_resume_group_tid_model_not_in_group_errors() {
        use std::collections::BTreeMap;
        let mut members = BTreeMap::new();
        members.insert("gpt-5.2".to_string(), "api_x".to_string());
        let group = StoredGroup {
            id: "group_abc".into(),
            members,
        };
        let err =
            plan_resume(Some("group_abc"), &["gemini-2.5-pro".into()], Some(group)).unwrap_err();
        assert!(err.to_string().contains("no member for model"));
    }

    #[test]
    fn plan_resume_group_tid_not_found_errors() {
        let err = plan_resume(Some("group_missing"), &["gpt-5.2".into()], None).unwrap_err();
        assert!(err.to_string().contains("group thread not found"));
    }
}
