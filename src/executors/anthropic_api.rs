use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::api_common::{ApiChatSession, warn_unsupported_file_paths};
use super::types::{ExecuteResult, ExecutionRequest, LlmExecutor, LlmExecutorCapabilities, Usage};
use crate::logger::log_to_file;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 32_000;

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
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

pub struct AnthropicApiExecutor {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    capabilities: LlmExecutorCapabilities,
}

impl AnthropicApiExecutor {
    pub fn new(client: reqwest::Client, api_key: String, base_url: Option<String>) -> Self {
        Self {
            client,
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            capabilities: LlmExecutorCapabilities {
                is_cli: false,
                supports_threads: true,
                supports_file_refs: false,
            },
        }
    }
}

#[async_trait]
impl LlmExecutor for AnthropicApiExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "api"
    }

    async fn execute(&self, req: ExecutionRequest) -> anyhow::Result<ExecuteResult> {
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
        };

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&request)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API request failed with status {status}: {body}");
        }

        let msg_resp: MessagesResponse = resp.json().await?;

        match msg_resp.stop_reason.as_deref() {
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

        let mut thinking = String::new();
        let mut response = String::new();
        for block in msg_resp.content {
            match block {
                ContentBlock::Text { text } => response.push_str(&text),
                ContentBlock::Thinking { thinking: t } => thinking.push_str(&t),
                ContentBlock::Other => {}
            }
        }

        if response.is_empty() {
            anyhow::bail!("No text content in Anthropic API response");
        }

        let usage = msg_resp.usage.map(|u| Usage {
            prompt_tokens: u.input_tokens
                + u.cache_creation_input_tokens
                + u.cache_read_input_tokens,
            completion_tokens: u.output_tokens,
        });

        let thinking = if thinking.is_empty() {
            None
        } else {
            Some(thinking)
        };

        session.finish(&spool, prompt, model, response, thinking, usage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_text_only_response() {
        let json = r#"{
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "hello"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }"#;
        let r: MessagesResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(r.content[0], ContentBlock::Text { ref text } if text == "hello"));
        let u = r.usage.unwrap();
        assert_eq!(u.input_tokens, 10);
        assert_eq!(u.output_tokens, 5);
        assert_eq!(u.cache_read_input_tokens, 0);
        assert_eq!(r.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn deserializes_thinking_and_cache_tokens() {
        let json = r#"{
            "content": [
                {"type": "thinking", "thinking": "step 1"},
                {"type": "text", "text": "final"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 2,
                "output_tokens": 3,
                "cache_creation_input_tokens": 100,
                "cache_read_input_tokens": 500
            }
        }"#;
        let r: MessagesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.content.len(), 2);
        assert!(
            matches!(r.content[0], ContentBlock::Thinking { ref thinking } if thinking == "step 1")
        );
        assert!(matches!(r.content[1], ContentBlock::Text { ref text } if text == "final"));
        let u = r.usage.unwrap();
        assert_eq!(u.cache_creation_input_tokens, 100);
        assert_eq!(u.cache_read_input_tokens, 500);
    }

    #[test]
    fn deserializes_unknown_block_as_other() {
        let json = r#"{
            "content": [{"type": "tool_use", "id": "x", "name": "y", "input": {}}],
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }"#;
        let r: MessagesResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(r.content[0], ContentBlock::Other));
    }

    #[test]
    fn deserializes_refusal_with_content() {
        // Refusals come back as a successful response with content; we should
        // surface the content rather than drop it.
        let json = r#"{
            "content": [{"type": "text", "text": "I can't help with that."}],
            "stop_reason": "refusal",
            "usage": {"input_tokens": 5, "output_tokens": 7}
        }"#;
        let r: MessagesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.stop_reason.as_deref(), Some("refusal"));
        assert!(
            matches!(r.content[0], ContentBlock::Text { ref text } if text.contains("can't help"))
        );
    }

    #[test]
    fn deserializes_context_window_exceeded() {
        let json = r#"{
            "content": [{"type": "text", "text": "partial"}],
            "stop_reason": "model_context_window_exceeded",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        }"#;
        let r: MessagesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            r.stop_reason.as_deref(),
            Some("model_context_window_exceeded")
        );
    }

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
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("\"system\""), "empty system must be omitted");
    }
}
