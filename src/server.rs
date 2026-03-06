use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

use crate::clipboard::copy_to_clipboard;
use crate::config::registry;
use crate::file::process_files;
use crate::git::generate_git_diff;
use crate::llm::ExecutorProvider;
use crate::llm_query::query_llm;
use crate::logger::{log_prompt, log_response, log_tool_call};
use crate::prompt_builder::build_prompt;
use crate::schema::ConsultLlmArgs;
use crate::system_prompt::{DEFAULT_SYSTEM_PROMPT, get_system_prompt};

#[derive(Clone)]
pub struct ConsultServer {
    tool_router: ToolRouter<Self>,
    executor_provider: Arc<ExecutorProvider>,
}

#[tool_router]
impl ConsultServer {
    pub fn new(executor_provider: Arc<ExecutorProvider>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            executor_provider,
        }
    }

    #[tool(
        name = "consult_llm",
        description = "Ask a more powerful AI for help with complex problems. Provide your question in the prompt field and always include relevant code files as context.\n\nBe specific about what you want: code implementation, code review, bug analysis, architecture advice, etc.\n\nIMPORTANT: Do NOT paste file contents into the prompt field. File contents are automatically read and included by the server when you pass file paths in the `files` parameter. The prompt should only contain your question or instructions.\n\nIMPORTANT: Ask neutral, open-ended questions. Avoid suggesting specific solutions or alternatives in your prompt as this can bias the analysis. Instead of \"Should I use X or Y approach?\", ask \"What's the best approach for this problem?\" Let the consultant LLM provide unbiased recommendations.\n\nFor multi-turn conversations with CLI backends (Codex, Gemini CLI, Cursor CLI), the response includes a [thread_id:xxx] prefix. Extract this ID and pass it as the thread_id parameter in follow-up requests to maintain conversation context.",
        input_schema = crate::schema::consult_llm_schema()
    )]
    async fn consult_llm(&self, params: Parameters<Value>) -> Result<String, String> {
        let raw = params.0;
        let model_explicitly_provided = raw.get("model").is_some();

        let args: ConsultLlmArgs = serde_json::from_value(raw.clone())
            .map_err(|e| format!("Invalid request parameters: {e}"))?;

        self.handle_consult_llm(args, model_explicitly_provided, &raw)
            .await
            .map_err(|e| format!("LLM query failed: {e}"))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ConsultServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }
}

impl ConsultServer {
    async fn handle_consult_llm(
        &self,
        args: ConsultLlmArgs,
        model_explicitly_provided: bool,
        raw_args: &Value,
    ) -> anyhow::Result<String> {
        log_tool_call("consult_llm", raw_args);

        let reg = registry();
        let model = reg.resolve_model(args.model.as_deref(), model_explicitly_provided)?;

        let executor = self.executor_provider.get_executor(&model)?;

        if args.thread_id.is_some() && !executor.capabilities().supports_threads {
            anyhow::bail!(
                "thread_id is not supported by the configured backend for model: {model}"
            );
        }

        let (prompt, file_paths) = if args.web_mode || !executor.capabilities().supports_file_refs {
            // API mode or web mode: inline file contents
            let context_files = args
                .files
                .as_ref()
                .map(|f| process_files(f))
                .transpose()?
                .unwrap_or_default();

            let git_diff = args
                .git_diff
                .as_ref()
                .map(|gd| generate_git_diff(gd.repo_path.as_deref(), &gd.files, &gd.base_ref));

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

            let git_diff = args
                .git_diff
                .as_ref()
                .map(|gd| generate_git_diff(gd.repo_path.as_deref(), &gd.files, &gd.base_ref));

            let prompt = match git_diff {
                Some(ref diff) if !diff.trim().is_empty() => {
                    format!("## Git Diff\n```diff\n{diff}\n```\n\n{}", args.prompt)
                }
                _ => args.prompt.clone(),
            };

            (prompt, resolved)
        };

        log_prompt(&model, &prompt);

        if args.web_mode {
            let system_prompt = get_system_prompt(executor.capabilities().is_cli, args.task_mode);
            let full_prompt =
                format!("# System Prompt\n\n{system_prompt}\n\n# User Prompt\n\n{prompt}");
            copy_to_clipboard(&full_prompt)?;

            let mut msg = "✓ Prompt copied to clipboard!\n\nPlease paste it into your browser-based LLM service and share the response here before I proceed with any implementation.".to_string();
            if let Some(ref fps) = file_paths
                && !fps.is_empty()
            {
                msg.push_str("\n\nNote: File paths were included:\n");
                for fp in fps {
                    msg.push_str(&format!("  - {}\n", fp.display()));
                }
            }
            return Ok(msg);
        }

        let result = query_llm(
            &prompt,
            &model,
            &self.executor_provider.get_executor(&model)?,
            file_paths.as_deref(),
            args.thread_id.as_deref(),
            args.task_mode,
        )
        .await?;

        log_response(&model, &result.response, &result.cost_info);

        Ok(match result.thread_id {
            Some(tid) => format!("[thread_id:{tid}]\n\n{}", result.response),
            None => result.response,
        })
    }
}

pub fn init_system_prompt() {
    let home = dirs::home_dir().expect("Could not determine home directory");
    let config_dir = home.join(".consult-llm-mcp");
    let prompt_path = config_dir.join("SYSTEM_PROMPT.md");

    if prompt_path.exists() {
        eprintln!("System prompt already exists at: {}", prompt_path.display());
        eprintln!("Remove it first if you want to reinitialize.");
        std::process::exit(1);
    }

    std::fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    std::fs::write(&prompt_path, DEFAULT_SYSTEM_PROMPT).expect("Failed to write system prompt");
    println!("Created system prompt at: {}", prompt_path.display());
    println!("You can now edit this file to customize the system prompt.");
    std::process::exit(0);
}
