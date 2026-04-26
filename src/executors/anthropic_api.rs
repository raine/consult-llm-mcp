use std::time::Duration;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use consult_llm_core::monitoring::ProgressStage;
use consult_llm_core::stream_events::ParsedStreamEvent;

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
    stream: bool,
}

// --- Anthropic SSE event types ---

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicEvent {
    MessageStart {
        message: MessageStartData,
    },
    ContentBlockStart {
        content_block: ContentBlockKind,
    },
    ContentBlockDelta {
        delta: ContentDelta,
    },
    ContentBlockStop,
    MessageDelta {
        delta: MessageDeltaData,
        #[serde(default)]
        usage: Option<MessageDeltaUsage>,
    },
    MessageStop,
    Ping,
    Error {
        error: AnthropicError,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct MessageStartData {
    #[serde(default)]
    usage: Option<MessageStartUsage>,
}

#[derive(Deserialize, Default)]
struct MessageStartUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlockKind {
    Text,
    Thinking,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentDelta {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    SignatureDelta {
        #[allow(dead_code)]
        signature: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct MessageDeltaData {
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct MessageDeltaUsage {
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Deserialize)]
struct AnthropicError {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

pub struct AnthropicApiExecutor {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    idle_timeout: Duration,
    capabilities: LlmExecutorCapabilities,
}

impl AnthropicApiExecutor {
    pub fn new(
        client: reqwest::Client,
        api_key: String,
        base_url: Option<String>,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            client,
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
            stream: true,
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

        let mut event_stream = resp.bytes_stream().eventsource();
        let mut response = String::new();
        let mut thinking = String::new();
        let mut input_tokens: u64 = 0;
        let mut cache_creation_tokens: u64 = 0;
        let mut cache_read_tokens: u64 = 0;
        let mut output_tokens: u64 = 0;
        let mut stop_reason: Option<String> = None;
        let mut saw_message_stop = false;
        let mut current_stage: Option<ProgressStage> = None;
        let idle_timeout = self.idle_timeout;
        let idle_secs = idle_timeout.as_secs();

        loop {
            match tokio::time::timeout(idle_timeout, event_stream.next()).await {
                Err(_) => anyhow::bail!(
                    "Anthropic stream idle timeout: no data from {model} for {idle_secs}s"
                ),
                Ok(None) => break,
                Ok(Some(Err(e))) => anyhow::bail!("Anthropic stream error for {model}: {e}"),
                Ok(Some(Ok(event))) => {
                    let Ok(parsed) = serde_json::from_str::<AnthropicEvent>(&event.data) else {
                        continue;
                    };
                    match parsed {
                        AnthropicEvent::MessageStart { message } => {
                            if let Some(u) = message.usage {
                                input_tokens = u.input_tokens;
                                cache_creation_tokens = u.cache_creation_input_tokens;
                                cache_read_tokens = u.cache_read_input_tokens;
                            }
                        }
                        AnthropicEvent::ContentBlockStart { content_block, .. } => {
                            if matches!(content_block, ContentBlockKind::Thinking) {
                                set_stage_once(&mut current_stage, ProgressStage::Thinking, &spool);
                            }
                        }
                        AnthropicEvent::ContentBlockDelta { delta, .. } => match delta {
                            ContentDelta::ThinkingDelta { thinking: t } => {
                                set_stage_once(&mut current_stage, ProgressStage::Thinking, &spool);
                                thinking.push_str(&t);
                            }
                            ContentDelta::TextDelta { text } => {
                                set_stage_once(
                                    &mut current_stage,
                                    ProgressStage::Responding,
                                    &spool,
                                );
                                response.push_str(&text);
                            }
                            ContentDelta::SignatureDelta { .. } | ContentDelta::Other => {}
                        },
                        AnthropicEvent::ContentBlockStop => {}
                        AnthropicEvent::MessageDelta { delta, usage } => {
                            stop_reason = delta.stop_reason;
                            if let Some(u) = usage {
                                output_tokens = u.output_tokens;
                            }
                        }
                        AnthropicEvent::MessageStop => {
                            saw_message_stop = true;
                            break;
                        }
                        AnthropicEvent::Error { error } => {
                            anyhow::bail!(
                                "Anthropic stream error {}: {}",
                                error.error_type,
                                error.message
                            );
                        }
                        AnthropicEvent::Ping | AnthropicEvent::Unknown => {}
                    }
                }
            }
        }

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

        let usage = Some(Usage {
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

/// Update the spool stage only when it differs from the current value.
fn set_stage_once(
    current: &mut Option<ProgressStage>,
    next: ProgressStage,
    spool: &std::sync::Mutex<consult_llm_core::monitoring::RunSpool>,
) {
    let needs_update = match current {
        None => true,
        Some(ProgressStage::Thinking) => !matches!(next, ProgressStage::Thinking),
        Some(ProgressStage::Responding) => !matches!(next, ProgressStage::Responding),
        _ => true,
    };
    if needs_update {
        spool.lock().unwrap().set_stage(next.clone());
        *current = Some(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_block_delta_text() {
        let json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}"#;
        let e: AnthropicEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(e, AnthropicEvent::ContentBlockDelta { delta: ContentDelta::TextDelta { ref text }, .. } if text == "hello")
        );
    }

    #[test]
    fn parses_thinking_delta() {
        let json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"step1"}}"#;
        let e: AnthropicEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(e, AnthropicEvent::ContentBlockDelta { delta: ContentDelta::ThinkingDelta { ref thinking }, .. } if thinking == "step1")
        );
    }

    #[test]
    fn parses_message_delta_stop_reason() {
        let json = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#;
        let e: AnthropicEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(e, AnthropicEvent::MessageDelta { ref delta, ref usage }
                if delta.stop_reason.as_deref() == Some("end_turn")
                && usage.as_ref().map(|u| u.output_tokens) == Some(42))
        );
    }

    #[test]
    fn parses_message_stop() {
        let json = r#"{"type":"message_stop"}"#;
        let e: AnthropicEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(e, AnthropicEvent::MessageStop));
    }

    #[test]
    fn parses_ping() {
        let json = r#"{"type":"ping"}"#;
        let e: AnthropicEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(e, AnthropicEvent::Ping));
    }

    #[test]
    fn parses_error_event() {
        let json = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let e: AnthropicEvent = serde_json::from_str(json).unwrap();
        assert!(
            matches!(e, AnthropicEvent::Error { ref error } if error.error_type == "overloaded_error")
        );
    }

    #[test]
    fn parses_message_start_usage() {
        let json = r#"{"type":"message_start","message":{"usage":{"input_tokens":10,"cache_creation_input_tokens":5,"cache_read_input_tokens":2}}}"#;
        let e: AnthropicEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(e, AnthropicEvent::MessageStart { ref message }
                if message.usage.as_ref().map(|u| u.input_tokens) == Some(10)
                && message.usage.as_ref().map(|u| u.cache_creation_input_tokens) == Some(5)));
    }

    #[test]
    fn unknown_event_type_is_ignored() {
        let json = r#"{"type":"some_future_event","data":"whatever"}"#;
        let e: AnthropicEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(e, AnthropicEvent::Unknown));
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
            stream: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("\"system\""), "empty system must be omitted");
        assert!(json.contains("\"stream\":true"));
    }
}
