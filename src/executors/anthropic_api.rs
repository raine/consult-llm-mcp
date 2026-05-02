use std::time::Duration;

use serde::Serialize;

use super::anthropic_events::AnthropicStreamHandler;
use super::api_common::{ApiChatSession, warn_unsupported_file_paths};
use super::api_transport::{PreparedStreamRequest, StreamLabels, run_stream};
use super::types::{ExecuteResult, ExecutionRequest, LlmExecutor, LlmExecutorCapabilities};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 32_000;
const LABELS: StreamLabels = StreamLabels {
    request: "Anthropic API request",
    stream: "Anthropic stream",
};

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct MessagesRequest {
    model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    system: String,
    messages: Vec<Message>,
    max_tokens: u32,
    stream: bool,
}

pub struct AnthropicApiExecutor {
    agent: ureq::Agent,
    api_key: String,
    base_url: String,
    idle_timeout: Duration,
    capabilities: LlmExecutorCapabilities,
}

impl AnthropicApiExecutor {
    pub fn new(
        agent: ureq::Agent,
        api_key: String,
        base_url: Option<String>,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            agent,
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            idle_timeout,
            capabilities: LlmExecutorCapabilities {
                is_cli: false,
                supports_threads: true,
                supports_file_refs: false,
            },
        }
    }

    pub(super) fn build_stream_request(
        &self,
        model: String,
        system_prompt: String,
        prompt: String,
        history: impl IntoIterator<Item = (String, String)>,
    ) -> anyhow::Result<PreparedStreamRequest> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{base}/v1/messages");

        let mut messages = Vec::new();
        for (user_prompt, assistant_response) in history {
            messages.push(Message {
                role: "user".to_string(),
                content: user_prompt,
            });
            messages.push(Message {
                role: "assistant".to_string(),
                content: assistant_response,
            });
        }
        messages.push(Message {
            role: "user".to_string(),
            content: prompt,
        });

        let request = MessagesRequest {
            model: model.clone(),
            system: system_prompt,
            messages,
            max_tokens: DEFAULT_MAX_TOKENS,
            stream: true,
        };
        let body = serde_json::to_vec(&request)?;

        Ok(PreparedStreamRequest {
            url,
            headers: vec![
                ("x-api-key", self.api_key.clone()),
                ("anthropic-version", ANTHROPIC_VERSION.to_string()),
                ("Content-Type", "application/json".to_string()),
            ],
            body,
            idle_timeout: self.idle_timeout,
            model,
            labels: LABELS,
        })
    }
}

impl LlmExecutor for AnthropicApiExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "api"
    }

    fn execute(&self, req: ExecutionRequest) -> anyhow::Result<ExecuteResult> {
        let ExecutionRequest {
            prompt,
            model,
            system_prompt,
            file_paths,
            thread_id,
            spool,
        } = req;

        warn_unsupported_file_paths(&model, file_paths.as_ref());

        let session = ApiChatSession::start(thread_id, &spool, &system_prompt, &prompt)?;

        let history = session
            .history()
            .iter()
            .map(|turn| (turn.user_prompt.clone(), turn.assistant_response.clone()));
        let prepared =
            self.build_stream_request(model.clone(), system_prompt, prompt.clone(), history)?;

        let handler = AnthropicStreamHandler::new(&spool, DEFAULT_MAX_TOKENS);
        let outcome = run_stream(prepared.into_stream_request(&self.agent), handler)?;

        session.commit_turn(prompt, model, outcome.response, outcome.usage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_omits_empty_system() {
        let req = MessagesRequest {
            model: "claude-opus-4-7".into(),
            system: String::new(),
            messages: vec![Message {
                role: "user".into(),
                content: "hi".into(),
            }],
            max_tokens: 1024,
            stream: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("\"system\""), "empty system must be omitted");
        assert!(json.contains("\"stream\":true"));
    }
}
