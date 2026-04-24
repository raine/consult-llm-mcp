use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::config::ModelRegistry;
use crate::executors::types::{LlmExecutor, Usage};
use crate::file::process_files;
use crate::git::generate_git_diff;
use crate::llm::ExecutorProvider;
use crate::llm_query::query_llm;
use crate::logger::{log_prompt, log_response};
use crate::prompt_builder::build_prompt;
use crate::schema::{ConsultLlmArgs, GitDiffArgs};
use crate::system_prompt::get_system_prompt;
use consult_llm_core::monitoring::RunSpool;
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
        model: String,
        thread_id: Option<String>,
    },
    WebPrompt {
        clipboard_text: String,
        file_count: usize,
    },
}

pub struct ConsultService {
    registry: Arc<ModelRegistry>,
    executor_provider: Arc<ExecutorProvider>,
}

impl ConsultService {
    pub fn new(registry: Arc<ModelRegistry>, executor_provider: Arc<ExecutorProvider>) -> Self {
        Self {
            registry,
            executor_provider,
        }
    }

    pub async fn consult(&self, args: ConsultLlmArgs) -> anyhow::Result<ConsultOutcome> {
        // Web mode short-circuits before model resolution
        if args.web_mode {
            return self.handle_web_mode(args).await;
        }

        let model = self.registry.resolve_model(args.model.as_deref())?;

        let executor = self.executor_provider.get_executor(&model)?;

        if args.thread_id.is_some() && !executor.capabilities().supports_threads {
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

        let project = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "unknown".to_string());
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let meta = consult_llm_core::monitoring::RunMeta {
            v: 1,
            run_id: consultation_id.clone(),
            pid: std::process::id(),
            started_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            project: project.clone(),
            cwd,
            model: model.clone(),
            backend: backend_name.clone(),
            thread_id: args.thread_id.clone(),
            task_mode: task_mode_str.clone(),
            reasoning_effort: reasoning_effort.clone(),
        };
        let spool = Arc::new(Mutex::new(RunSpool::new(meta)));

        let start_time = std::time::Instant::now();
        let result = self
            .run_consult(args, &model, executor, &consultation_id, Arc::clone(&spool))
            .await;
        let duration_ms = start_time.elapsed().as_millis() as u64;

        let (success, error, tokens_in, tokens_out, thread_id) = match &result {
            Ok((_, usage, tid)) => (
                true,
                None,
                usage.as_ref().map(|u| u.prompt_tokens),
                usage.as_ref().map(|u| u.completion_tokens),
                tid.clone(),
            ),
            Err(e) => (false, Some(e.to_string()), None, None, None),
        };

        let history = consult_llm_core::monitoring::HistoryRecord {
            ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            consultation_id: Some(consultation_id),
            project,
            model: model.clone(),
            backend: backend_name,
            duration_ms,
            success,
            error: error.clone(),
            tokens_in,
            tokens_out,
            parsed_ts: None,
            thread_id: thread_id.clone(),
            reasoning_effort,
            task_mode: task_mode_str,
        };
        spool
            .lock()
            .unwrap()
            .finish(duration_ms, success, error, &history);

        result.map(|(body, usage, tid)| ConsultOutcome::Response {
            body,
            usage,
            model: model.clone(),
            thread_id: tid,
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

    async fn run_consult(
        &self,
        args: ConsultLlmArgs,
        model: &str,
        executor: Arc<dyn LlmExecutor>,
        _consultation_id: &str,
        spool: Arc<Mutex<RunSpool>>,
    ) -> anyhow::Result<(String, Option<Usage>, Option<String>)> {
        let git_diff = resolve_git_diff(args.git_diff.as_ref());

        let (prompt, file_paths) = if !executor.capabilities().supports_file_refs {
            // API mode: inline file contents
            let context_files = args
                .files
                .as_ref()
                .map(|f| process_files(f))
                .transpose()?
                .unwrap_or_default();

            (
                build_prompt(&args.prompt, &context_files, git_diff.as_deref()),
                None,
            )
        } else {
            // CLI mode: pass file paths, inline git diff only
            let resolved: Option<Vec<PathBuf>> = args.files.as_ref().map(|files| {
                let cwd = std::env::current_dir().unwrap_or_default();
                files
                    .iter()
                    .map(|f| {
                        let p = PathBuf::from(f);
                        if p.is_absolute() { p } else { cwd.join(f) }
                    })
                    .collect()
            });

            let prompt = match git_diff {
                Some(ref diff) if !diff.trim().is_empty() => {
                    format!("## Git Diff\n```diff\n{diff}\n```\n\n{}", args.prompt)
                }
                _ => args.prompt.clone(),
            };

            (prompt, resolved)
        };

        log_prompt(model, &prompt);

        // Emit FilesContext event so the monitor TUI can show a compact file list
        if let Some(ref files) = args.files
            && !files.is_empty()
        {
            spool
                .lock()
                .unwrap()
                .stream_event(ParsedStreamEvent::FilesContext {
                    files: files.clone(),
                });
        }

        let system_prompt = get_system_prompt(executor.capabilities().is_cli, args.task_mode);

        let result = query_llm(
            &prompt,
            model,
            &executor,
            file_paths.as_deref(),
            args.thread_id.as_deref(),
            &system_prompt,
            Arc::clone(&spool),
        )
        .await?;

        log_response(model, &result.response, &result.cost_info);

        let thread_id = result.thread_id.or_else(|| args.thread_id.clone());
        Ok((result.response, result.usage, thread_id))
    }
}
