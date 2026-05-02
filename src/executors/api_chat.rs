//! Response-side decoding for the OpenAI-compatible chat-completions stream.
//! Owns the streaming `ChatChunk` shape, the per-event handler, and the
//! token-accounting quirk for providers (Gemini) that report thinking tokens
//! only in `total_tokens`.

use std::sync::Mutex;

use serde::Deserialize;

use consult_llm_core::monitoring::{ProgressStage, RunSpool};
use consult_llm_core::stream_events::ParsedStreamEvent;

use super::api_transport::EventHandler;
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

pub struct ChatStreamHandler<'a> {
    pub splitter: Option<TagSplitter>,
    pub raw_content: String,
    pub raw_thinking: String,
    pub usage: Option<Usage>,
    pub current_stage: Option<ProgressStage>,
    pub finish_reason: Option<String>,
    pub spool: &'a Mutex<RunSpool>,
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

impl EventHandler for ChatStreamHandler<'_> {
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
}

pub fn emit_segments(
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
