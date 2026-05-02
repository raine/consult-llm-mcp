//! Response-side decoding for the OpenAI-compatible chat-completions stream.
//! Owns the streaming `ChatChunk` shape, the per-event handler, and the
//! token-accounting quirk for providers (Gemini) that report thinking tokens
//! only in `total_tokens`.

use std::sync::Mutex;

use serde::Deserialize;

use consult_llm_core::monitoring::{ProgressStage, RunSpool};
use consult_llm_core::stream_events::ParsedStreamEvent;

use super::api_transport::StreamHandler;
use super::sse::SseEvent;
use super::tag_splitter::{Segment, TagSplitter};
use super::types::Usage;

#[derive(Deserialize)]
struct ChatChunk {
    #[serde(default)]
    choices: Vec<ChatChunkChoice>,
    usage: Option<ApiUsage>,
}

#[derive(Deserialize)]
struct ChatChunkChoice {
    delta: ChatDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct ChatDelta {
    content: Option<String>,
    /// DeepSeek-style chain-of-thought field, separate from response content.
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(Deserialize)]
struct ApiUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    /// Includes prompt + completion + thinking tokens. Used to derive thinking
    /// token count for models (like Gemini) where `completion_tokens` excludes
    /// thinking tokens.
    total_tokens: Option<u64>,
}

pub struct ChatStreamOutcome {
    pub response: String,
    pub usage: Option<Usage>,
}

pub struct ChatStreamHandler<'a> {
    splitter: Option<TagSplitter>,
    raw_content: String,
    raw_thinking: String,
    usage: Option<Usage>,
    current_stage: Option<ProgressStage>,
    finish_reason: Option<String>,
    spool: &'a Mutex<RunSpool>,
}

impl<'a> ChatStreamHandler<'a> {
    pub fn new(splitter: Option<TagSplitter>, spool: &'a Mutex<RunSpool>) -> Self {
        Self {
            splitter,
            raw_content: String::new(),
            raw_thinking: String::new(),
            usage: None,
            current_stage: None,
            finish_reason: None,
            spool,
        }
    }
}

impl StreamHandler for ChatStreamHandler<'_> {
    type Outcome = ChatStreamOutcome;

    fn on_event(&mut self, ev: &SseEvent) -> anyhow::Result<bool> {
        if ev.data == "[DONE]" {
            return Ok(true);
        }
        let Ok(chunk) = serde_json::from_str::<ChatChunk>(&ev.data) else {
            return Ok(false);
        };
        if let Some(u) = chunk.usage {
            self.usage = Some(effective_usage(u));
        }
        if chunk.choices.is_empty() {
            return Ok(false);
        }
        let choice = &chunk.choices[0];
        if let Some(ref r) = choice.finish_reason {
            self.finish_reason = Some(r.clone());
        }
        let delta = &choice.delta;

        let mut segments: Vec<Segment> = Vec::new();
        if let Some(t) = delta.reasoning_content.as_deref()
            && !t.is_empty()
        {
            segments.push(Segment::Thinking(t.to_string()));
        }
        if let Some(text) = delta.content.as_deref()
            && !text.is_empty()
        {
            match self.splitter.as_mut() {
                Some(s) => segments.extend(s.push(text)),
                None => segments.push(Segment::Answer(text.to_string())),
            }
        }

        emit_segments(
            segments,
            &mut self.raw_content,
            &mut self.raw_thinking,
            &mut self.current_stage,
            self.spool,
        );
        Ok(false)
    }

    fn finish(self, model: &str) -> anyhow::Result<Self::Outcome> {
        let Self {
            splitter,
            mut raw_content,
            mut raw_thinking,
            usage,
            mut current_stage,
            finish_reason,
            spool,
        } = self;

        let unclosed_thought = splitter.as_ref().is_some_and(|s| s.in_thinking());
        if let Some(s) = splitter
            && let Some(seg) = s.flush()
        {
            emit_segments(
                vec![seg],
                &mut raw_content,
                &mut raw_thinking,
                &mut current_stage,
                spool,
            );
        }

        if raw_content.trim().is_empty() && unclosed_thought && !raw_thinking.is_empty() {
            crate::logger::log_to_file(&format!(
                "WARNING: API response for {model} had unclosed thought tag; treating thinking as response"
            ));
            eprintln!("Warning: unclosed thought tag in stream; treating thinking as response");
            raw_content = std::mem::take(&mut raw_thinking);
        }

        let response = raw_content.trim().to_string();
        if response.is_empty() {
            anyhow::bail!("No response from the model via API");
        }

        match finish_reason.as_deref() {
            Some("length") => {
                crate::logger::log_to_file(&format!(
                    "WARNING: API response for {model} truncated by max_tokens"
                ));
                eprintln!("Warning: response truncated by max_tokens");
            }
            Some("content_filter") => {
                crate::logger::log_to_file(&format!(
                    "WARNING: API response for {model} stopped by content filter"
                ));
                eprintln!("Warning: response stopped by content filter");
            }
            _ => {}
        }

        {
            let mut s = spool.lock().unwrap();
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

        Ok(ChatStreamOutcome { response, usage })
    }
}

fn emit_segments(
    segments: Vec<Segment>,
    raw_content: &mut String,
    raw_thinking: &mut String,
    current_stage: &mut Option<ProgressStage>,
    spool: &Mutex<RunSpool>,
) {
    if segments.is_empty() {
        return;
    }
    // Single lock acquisition: interleave stage updates with event emissions
    // so progress and stream events stay in causal order (e.g. for a mixed
    // [Thinking, Answer] chunk, the timeline is
    // Progress(Thinking), Stream(Thinking), Progress(Responding), Stream(Answer)).
    let mut s = spool.lock().unwrap();
    for seg in segments {
        let next = match &seg {
            Segment::Thinking(_) => ProgressStage::Thinking,
            Segment::Answer(_) => ProgressStage::Responding,
        };
        if current_stage.as_ref() != Some(&next) {
            s.set_stage(next.clone());
            *current_stage = Some(next);
        }
        match seg {
            Segment::Thinking(text) => {
                if !text.is_empty() {
                    raw_thinking.push_str(&text);
                    s.stream_event(ParsedStreamEvent::Thinking { text });
                }
            }
            Segment::Answer(text) => {
                if !text.is_empty() {
                    raw_content.push_str(&text);
                }
            }
        }
    }
}

fn effective_usage(u: ApiUsage) -> Usage {
    // Gemini thinking models report thinking tokens only in total_tokens,
    // not in completion_tokens. Derive the full output cost from total.
    let effective_completion = match u.total_tokens {
        Some(total) if total > u.prompt_tokens + u.completion_tokens => total - u.prompt_tokens,
        _ => u.completion_tokens,
    };
    Usage {
        prompt_tokens: u.prompt_tokens,
        completion_tokens: effective_completion,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_uses_unclosed_thinking_as_response() {
        let spool = Mutex::new(RunSpool::disabled());
        let mut handler =
            ChatStreamHandler::new(Some(TagSplitter::new("<think>", "</think>")), &spool);
        handler
            .on_event(&SseEvent {
                data: r#"{"choices":[{"delta":{"content":"<think>fallback"}}]}"#.to_string(),
                event: None,
            })
            .unwrap();

        let outcome = handler.finish("test-model").unwrap();

        assert_eq!(outcome.response, "fallback");
        assert!(outcome.usage.is_none());
    }
}
