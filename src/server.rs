use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde_json::Value;
use std::sync::Arc;

use crate::clipboard::copy_to_clipboard;
use crate::logger::log_tool_call;
use crate::schema::ConsultLlmArgs;
use crate::service::{ConsultOutcome, ConsultService};
use crate::system_prompt::DEFAULT_SYSTEM_PROMPT;

#[derive(Clone)]
pub struct ConsultServer {
    tool_router: ToolRouter<Self>,
    service: Arc<ConsultService>,
}

#[tool_router]
impl ConsultServer {
    pub fn new(service: Arc<ConsultService>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            service,
        }
    }

    #[tool(
        name = "consult_llm",
        description = "Ask a more powerful AI for help with complex problems. Provide your question in the prompt field and always include relevant code files as context.\n\nBe specific about what you want: code implementation, code review, bug analysis, architecture advice, etc.\n\nIMPORTANT: Do NOT paste file contents into the prompt field. File contents are automatically read and included by the server when you pass file paths in the `files` parameter. The prompt should only contain your question or instructions.\n\nIMPORTANT: Ask neutral, open-ended questions. Avoid suggesting specific solutions or alternatives in your prompt as this can bias the analysis. Instead of \"Should I use X or Y approach?\", ask \"What's the best approach for this problem?\" Let the consultant LLM provide unbiased recommendations.\n\nFor multi-turn conversations, the response includes a [thread_id:xxx] prefix. Extract this ID and pass it as the thread_id parameter in follow-up requests to maintain conversation context. All backends support threads: CLI backends (Codex, Gemini CLI, Cursor CLI) maintain native sessions, while API backends replay conversation history from disk.\n\nThe `model` parameter accepts either a single model identifier (string) or an array of identifiers (e.g. [\"gemini\", \"openai\"]) to consult multiple models in parallel within a single tool call. When multiple models are used, the top-line `[thread_id:group_xxx]` is a group thread id; pass it back as `thread_id` to resume all the same models together.",
        input_schema = crate::schema::consult_llm_schema()
    )]
    async fn consult_llm(&self, params: Parameters<Value>) -> Result<String, String> {
        let raw = params.0;

        log_tool_call("consult_llm", &raw);

        let args: ConsultLlmArgs =
            serde_json::from_value(raw).map_err(|e| format!("Invalid request parameters: {e}"))?;

        let outcome = self
            .service
            .consult(args)
            .await
            .map_err(|e| format!("LLM query failed: {e:#}"))?;

        match outcome {
            ConsultOutcome::Response { body, .. } => Ok(body),
            ConsultOutcome::WebPrompt { clipboard_text } => {
                copy_to_clipboard(&clipboard_text)
                    .map_err(|e| format!("LLM query failed: {e:#}"))?;

                Ok("✓ Prompt copied to clipboard!\n\nPlease paste it into your browser-based LLM service and share the response here before I proceed with any implementation.".to_string())
            }
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ConsultServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        let mut impl_info = Implementation::default();
        impl_info.name = "consult_llm".to_string();
        impl_info.version = format!("{}+{}", env!("CARGO_PKG_VERSION"), env!("GIT_HASH"));
        info.server_info = impl_info;
        info
    }
}

pub fn init_system_prompt() -> anyhow::Result<()> {
    let home = dirs::home_dir().expect("Could not determine home directory");
    let config_dir = home.join(".consult-llm-mcp");
    let prompt_path = config_dir.join("SYSTEM_PROMPT.md");

    if prompt_path.exists() {
        anyhow::bail!(
            "System prompt already exists at: {}\nRemove it first if you want to reinitialize.",
            prompt_path.display()
        );
    }

    std::fs::create_dir_all(&config_dir)?;
    std::fs::write(&prompt_path, DEFAULT_SYSTEM_PROMPT)?;
    println!("Created system prompt at: {}", prompt_path.display());
    println!("You can now edit this file to customize the system prompt.");
    Ok(())
}
