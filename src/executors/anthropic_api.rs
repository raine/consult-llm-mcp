use std::time::Duration;

use serde::Serialize;

use consult_llm_core::stream_events::ParsedStreamEvent;

use super::anthropic_events::AnthropicStreamHandler;
use super::api_common::{ApiChatSession, warn_unsupported_file_paths};
use super::api_transport::{StreamLabels, StreamRequest, run_stream};
use super::types::{ExecuteResult, ExecutionRequest, LlmExecutor, LlmExecutorCapabilities, Usage};
use crate::logger::log_to_file;

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

        let base = self.base_url.trim_end_matches('/');
        let url = format!("{base}/v1/messages");

        let mut messages = Vec::new();
        for turn in session.history() {
            messages.push(Message {
                role: "user".to_string(),
                content: turn.user_prompt.clone(),
            });
            messages.push(Message {
                role: "assistant".to_string(),
                content: turn.assistant_response.clone(),
            });
        }
        messages.push(Message {
            role: "user".to_string(),
            content: prompt.clone(),
        });

        let request = MessagesRequest {
            model: model.clone(),
            system: system_prompt,
            messages,
            max_tokens: DEFAULT_MAX_TOKENS,
            stream: true,
        };
        let body = serde_json::to_vec(&request)?;

        let mut handler = AnthropicStreamHandler::new(&spool);
        run_stream(
            StreamRequest {
                agent: &self.agent,
                url,
                headers: vec![
                    ("x-api-key", self.api_key.clone()),
                    ("anthropic-version", ANTHROPIC_VERSION.to_string()),
                    ("Content-Type", "application/json".to_string()),
                ],
                body,
                idle_timeout: self.idle_timeout,
                model: model.clone(),
                labels: LABELS,
            },
            &mut handler,
        )?;

        let AnthropicStreamHandler {
            response,
            thinking,
            input_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            output_tokens,
            got_usage,
            stop_reason,
            saw_message_stop,
            ..
        } = handler;

        if !saw_message_stop {
            anyhow::bail!("Anthropic stream for {model} ended without message_stop");
        }

        match stop_reason.as_deref() {
            Some("pause_turn") => anyhow::bail!(
                "Anthropic API returned pause_turn — long-running turn was paused mid-stream"
            ),
            Some("max_tokens") => {
                log_to_file(&format!(
                    "WARNING: Anthropic response for {model} truncated by max_tokens ({DEFAULT_MAX_TOKENS})"
                ));
                eprintln!("Warning: response truncated by max_tokens ({DEFAULT_MAX_TOKENS})");
            }
            Some("model_context_window_exceeded") => {
                log_to_file(&format!(
                    "WARNING: Anthropic response for {model} truncated — model context window exceeded"
                ));
                eprintln!("Warning: response truncated — model context window exceeded");
            }
            Some("refusal") => {
                log_to_file(&format!(
                    "WARNING: Anthropic response for {model} stopped with refusal — model declined to answer"
                ));
                eprintln!("Warning: model declined to answer (refusal)");
            }
            _ => {}
        }

        if response.is_empty() {
            anyhow::bail!("No text content in Anthropic API response");
        }

        let usage = got_usage.then(|| Usage {
            prompt_tokens: input_tokens + cache_creation_tokens + cache_read_tokens,
            completion_tokens: output_tokens,
        });
        let thinking_opt = if thinking.is_empty() {
            None
        } else {
            Some(thinking)
        };

        {
            let mut s = spool.lock().unwrap();
            if let Some(ref t) = thinking_opt {
                s.stream_event(ParsedStreamEvent::Thinking { text: t.clone() });
            }
            s.stream_event(ParsedStreamEvent::AssistantText {
                text: response.clone(),
            });
            if let Some(ref u) = usage {
                s.stream_event(ParsedStreamEvent::Usage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                });
            }
        }

        session.commit_turn(prompt, model, response, usage)
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
