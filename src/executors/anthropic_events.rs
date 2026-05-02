//! Response-side decoding for the Anthropic Messages SSE stream.
//! Owns the `AnthropicEvent` shape and the per-event handler that fills in
//! the streamed response, thinking, usage, and stop-reason.

use std::sync::Mutex;

use serde::Deserialize;

use consult_llm_core::monitoring::{ProgressStage, RunSpool};

use super::api_transport::EventHandler;
use super::sse::SseEvent;

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

pub struct AnthropicStreamHandler<'a> {
    pub response: String,
    pub thinking: String,
    pub input_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
    pub got_usage: bool,
    pub stop_reason: Option<String>,
    pub saw_message_stop: bool,
    pub current_stage: Option<ProgressStage>,
    pub spool: &'a Mutex<RunSpool>,
}

impl<'a> AnthropicStreamHandler<'a> {
    pub fn new(spool: &'a Mutex<RunSpool>) -> Self {
        Self {
            response: String::new(),
            thinking: String::new(),
            input_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: 0,
            got_usage: false,
            stop_reason: None,
            saw_message_stop: false,
            current_stage: None,
            spool,
        }
    }

    fn set_stage_once(&mut self, next: ProgressStage) {
        let needs_update = match &self.current_stage {
            None => true,
            Some(ProgressStage::Thinking) => !matches!(next, ProgressStage::Thinking),
            Some(ProgressStage::Responding) => !matches!(next, ProgressStage::Responding),
            _ => true,
        };
        if needs_update {
            self.spool.lock().unwrap().set_stage(next.clone());
            self.current_stage = Some(next);
        }
    }
}

impl EventHandler for AnthropicStreamHandler<'_> {
    fn on_event(&mut self, ev: &SseEvent) -> anyhow::Result<bool> {
        let Ok(parsed) = serde_json::from_str::<AnthropicEvent>(&ev.data) else {
            return Ok(false);
        };
        match parsed {
            AnthropicEvent::MessageStart { message } => {
                if let Some(u) = message.usage {
                    self.input_tokens = u.input_tokens;
                    self.cache_creation_tokens = u.cache_creation_input_tokens;
                    self.cache_read_tokens = u.cache_read_input_tokens;
                    self.got_usage = true;
                }
            }
            AnthropicEvent::ContentBlockStart { content_block, .. } => {
                if matches!(content_block, ContentBlockKind::Thinking) {
                    self.set_stage_once(ProgressStage::Thinking);
                }
            }
            AnthropicEvent::ContentBlockDelta { delta, .. } => match delta {
                ContentDelta::ThinkingDelta { thinking: t } => {
                    self.set_stage_once(ProgressStage::Thinking);
                    self.thinking.push_str(&t);
                }
                ContentDelta::TextDelta { text } => {
                    self.set_stage_once(ProgressStage::Responding);
                    self.response.push_str(&text);
                }
                ContentDelta::SignatureDelta { .. } | ContentDelta::Other => {}
            },
            AnthropicEvent::ContentBlockStop => {}
            AnthropicEvent::MessageDelta { delta, usage } => {
                self.stop_reason = delta.stop_reason;
                if let Some(u) = usage {
                    self.output_tokens = u.output_tokens;
                    self.got_usage = true;
                }
            }
            AnthropicEvent::MessageStop => {
                self.saw_message_stop = true;
                return Ok(true);
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
        Ok(false)
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
}
