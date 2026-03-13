use std::path::PathBuf;
use std::sync::Arc;

use crate::config::ModelRegistry;
use crate::executors::types::{LlmExecutor, Usage};
use crate::file::process_files;
use crate::git::generate_git_diff;
use crate::llm::ExecutorProvider;
use crate::llm_query::query_llm;
use crate::logger::{log_prompt, log_response};
use crate::prompt_builder::build_prompt;
use crate::schema::ConsultLlmArgs;
use crate::system_prompt::get_system_prompt;

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

        let reasoning_effort = executor.reasoning_effort().map(|s| s.to_string());
        consult_llm_core::monitoring::emit(
            consult_llm_core::monitoring::MonitorEvent::ConsultStarted {
                id: consultation_id.clone(),
                model: model.clone(),
                backend: backend_name.clone(),
                thread_id: args.thread_id.clone(),
                task_mode: task_mode_str.clone(),
                reasoning_effort: reasoning_effort.clone(),
            },
        );

        let project = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "unknown".to_string());

        let start_time = std::time::Instant::now();
        let result = self
            .run_consult(args, &model, executor, &consultation_id)
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

        consult_llm_core::monitoring::emit(
            consult_llm_core::monitoring::MonitorEvent::ConsultFinished {
                id: consultation_id.clone(),
                duration_ms,
                success,
                error: error.clone(),
            },
        );
        consult_llm_core::monitoring::append_history(
            &consult_llm_core::monitoring::HistoryRecord {
                ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                consultation_id: Some(consultation_id),
                project,
                model,
                backend: backend_name,
                duration_ms,
                success,
                error,
                tokens_in,
                tokens_out,
                parsed_ts: None,
                thread_id,
                reasoning_effort,
                task_mode: task_mode_str,
            },
        );

        result.map(|(body, usage, _)| ConsultOutcome::Response { body, usage })
    }

    async fn handle_web_mode(&self, args: ConsultLlmArgs) -> anyhow::Result<ConsultOutcome> {
        let context_files = args
            .files
            .as_ref()
            .map(|f| process_files(f))
            .transpose()?
            .unwrap_or_default();

        let git_diff = match args.git_diff.as_ref() {
            Some(gd) => match generate_git_diff(gd.repo_path.as_deref(), &gd.files, &gd.base_ref) {
                Ok(diff) => Some(diff),
                Err(e) => {
                    eprintln!("Warning: git diff failed: {e}");
                    None
                }
            },
            None => None,
        };

        let prompt = build_prompt(&args.prompt, &context_files, git_diff.as_deref());
        let system_prompt = get_system_prompt(false, args.task_mode);
        let clipboard_text =
            format!("# System Prompt\n\n{system_prompt}\n\n# User Prompt\n\n{prompt}");

        Ok(ConsultOutcome::WebPrompt { clipboard_text })
    }

    async fn run_consult(
        &self,
        args: ConsultLlmArgs,
        model: &str,
        executor: Arc<dyn LlmExecutor>,
        consultation_id: &str,
    ) -> anyhow::Result<(String, Option<Usage>, Option<String>)> {
        let git_diff = match args.git_diff.as_ref() {
            Some(gd) => match generate_git_diff(gd.repo_path.as_deref(), &gd.files, &gd.base_ref) {
                Ok(diff) => Some(diff),
                Err(e) => {
                    eprintln!("Warning: git diff failed: {e}");
                    None
                }
            },
            None => None,
        };

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

        let system_prompt = get_system_prompt(executor.capabilities().is_cli, args.task_mode);

        let result = query_llm(
            &prompt,
            model,
            &executor,
            file_paths.as_deref(),
            args.thread_id.as_deref(),
            &system_prompt,
            Some(consultation_id),
        )
        .await?;

        log_response(model, &result.response, &result.cost_info);

        let thread_id = result.thread_id.or_else(|| args.thread_id.clone());
        let mut prefix = format!("[model:{model}]");
        if let Some(ref tid) = thread_id {
            prefix.push_str(&format!(" [thread_id:{tid}]"));
        }
        let body = format!("{prefix}\n\n{}", result.response);
        Ok((body, result.usage, thread_id))
    }
}
