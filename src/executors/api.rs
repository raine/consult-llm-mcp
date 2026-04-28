use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use consult_llm_core::monitoring::ProgressStage;
use consult_llm_core::stream_events::ParsedStreamEvent;

use super::api_common::{ApiChatSession, warn_unsupported_file_paths};
use super::sse::SseParser;
use super::types::{ExecuteResult, ExecutionRequest, LlmExecutor, LlmExecutorCapabilities, Usage};

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra_body: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

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

/// Per-provider quirks of the OpenAI-compatible chat-completions stream.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Dialect {
    /// No tag-based thinking in content.
    Generic,
    /// Gemini wraps thought summaries in <thought>...</thought> in `delta.content`.
    Gemini,
    /// MiniMax M2 embeds <think>...</think> in `delta.content`.
    MiniMax,
}

fn detect_dialect(base_url: &str) -> Dialect {
    if base_url.contains("generativelanguage.googleapis.com") {
        Dialect::Gemini
    } else if base_url.contains("api.minimax.io") {
        Dialect::MiniMax
    } else {
        Dialect::Generic
    }
}

#[derive(Debug, PartialEq)]
enum Segment {
    Thinking(String),
    Answer(String),
}

/// Stateful parser that splits a streamed content body on tag boundaries.
/// Carries a small buffer between chunks so that tags split across chunk
/// boundaries (e.g. "<tho" + "ught>") are still detected.
struct TagSplitter {
    open_tag: &'static str,
    close_tag: &'static str,
    in_thinking: bool,
    buffer: String,
}

impl TagSplitter {
    fn new(open_tag: &'static str, close_tag: &'static str) -> Self {
        Self {
            open_tag,
            close_tag,
            in_thinking: false,
            buffer: String::new(),
        }
    }

    fn push(&mut self, chunk: &str) -> Vec<Segment> {
        self.buffer.push_str(chunk);
        let mut out = Vec::new();
        loop {
            let target = if self.in_thinking {
                self.close_tag
            } else {
                self.open_tag
            };
            if let Some(idx) = self.buffer.find(target) {
                if idx > 0 {
                    let segment: String = self.buffer.drain(..idx).collect();
                    out.push(self.classify(segment));
                }
                self.buffer.drain(..target.len());
                self.in_thinking = !self.in_thinking;
                // After closing a thinking block, drop a single trailing newline
                // for parity with the previous extract_think_tags behavior.
                if !self.in_thinking && self.buffer.starts_with('\n') {
                    self.buffer.drain(..1);
                }
            } else {
                let hold = partial_suffix_len(&self.buffer, target);
                let emit_len = self.buffer.len() - hold;
                if emit_len > 0 {
                    let segment: String = self.buffer.drain(..emit_len).collect();
                    out.push(self.classify(segment));
                }
                break;
            }
        }
        out
    }

    fn flush(mut self) -> Option<Segment> {
        if self.buffer.is_empty() {
            None
        } else {
            let text = std::mem::take(&mut self.buffer);
            Some(self.classify(text))
        }
    }

    fn classify(&self, text: String) -> Segment {
        if self.in_thinking {
            Segment::Thinking(text)
        } else {
            Segment::Answer(text)
        }
    }
}

/// How much of `tag`'s prefix appears at the end of `buf` — bytes we must
/// hold back in case the next chunk completes the tag.
fn partial_suffix_len(buf: &str, tag: &str) -> usize {
    let max = std::cmp::min(tag.len() - 1, buf.len());
    for i in (1..=max).rev() {
        if buf.ends_with(&tag[..i]) {
            return i;
        }
    }
    0
}

pub struct ApiExecutor {
    agent: ureq::Agent,
    api_key: String,
    base_url: String,
    idle_timeout: Duration,
    capabilities: LlmExecutorCapabilities,
}

impl ApiExecutor {
    pub fn new(
        agent: ureq::Agent,
        api_key: String,
        base_url: Option<String>,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            agent,
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1/".to_string()),
            idle_timeout,
            capabilities: LlmExecutorCapabilities {
                is_cli: false,
                supports_threads: true,
                supports_file_refs: false,
            },
        }
    }
}

impl LlmExecutor for ApiExecutor {
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

        let base = if self.base_url.ends_with('/') {
            self.base_url.clone()
        } else {
            format!("{}/", self.base_url)
        };
        let url = format!("{base}chat/completions");

        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt.clone(),
        }];
        for turn in session.history() {
            messages.push(ChatMessage {
                role: "user".to_string(),
                content: turn.user_prompt.clone(),
            });
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: turn.assistant_response.clone(),
            });
        }
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: prompt.clone(),
        });

        let dialect = detect_dialect(&self.base_url);
        let extra_body = match dialect {
            Dialect::Gemini => Some(serde_json::json!({
                "google": {
                    "thinking_config": {
                        "thinking_level": "high",
                        "include_thoughts": true
                    }
                }
            })),
            _ => None,
        };

        let request = ChatRequest {
            model: model.clone(),
            messages,
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
            extra_body,
        };

        let idle_timeout = self.idle_timeout;
        let idle_secs = idle_timeout.as_secs();
        let body_json = serde_json::to_vec(&request)?;
        let resp = self
            .agent
            .post(&url)
            .config()
            // Per-read socket idle: ureq applies this as a fresh budget on
            // every read, so heartbeat bytes (and any data, parsed or not)
            // reset the timer. This is the actual "stream went silent"
            // detector. The agent-level timeout_global bounds the whole
            // request as an absolute backstop.
            .timeout_recv_body(Some(idle_timeout))
            .http_status_as_error(false)
            .build()
            .header("Authorization", format!("Bearer {}", &self.api_key))
            .header("Content-Type", "application/json")
            .send(&body_json[..]);

        let mut resp = match resp {
            Ok(r) => r,
            Err(e) => anyhow::bail!("API request to {model} failed: {e}"),
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            anyhow::bail!("API request failed with status {status}: {body}");
        }

        let mut reader = resp.into_body().into_reader();
        let mut sse = SseParser::new();
        let mut buf = [0u8; 8192];

        let mut splitter = match dialect {
            Dialect::Gemini => Some(TagSplitter::new("<thought>", "</thought>")),
            Dialect::MiniMax => Some(TagSplitter::new("<think>", "</think>")),
            Dialect::Generic => None,
        };
        let mut raw_content = String::new();
        let mut raw_thinking = String::new();
        let mut usage: Option<Usage> = None;
        let mut current_stage: Option<ProgressStage> = None;
        let mut finish_reason: Option<String> = None;

        'outer: loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    if is_timeout_err(&e) {
                        anyhow::bail!(
                            "API stream idle timeout: no bytes from {model} for {idle_secs}s"
                        );
                    }
                    anyhow::bail!("API stream error for {model}: {e}");
                }
            };
            let events = sse
                .feed(&buf[..n])
                .map_err(|e| anyhow::anyhow!("API stream parse error for {model}: {e}"))?;
            for ev in events {
                if ev.data == "[DONE]" {
                    break 'outer;
                }
                let Ok(chunk) = serde_json::from_str::<ChatChunk>(&ev.data) else {
                    continue;
                };
                if let Some(u) = chunk.usage {
                    usage = Some(effective_usage(u));
                }
                if chunk.choices.is_empty() {
                    continue;
                }
                let choice = &chunk.choices[0];
                if let Some(ref r) = choice.finish_reason {
                    finish_reason = Some(r.clone());
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
                    match splitter.as_mut() {
                        Some(s) => segments.extend(s.push(text)),
                        None => segments.push(Segment::Answer(text.to_string())),
                    }
                }

                emit_segments(
                    segments,
                    &mut raw_content,
                    &mut raw_thinking,
                    &mut current_stage,
                    &spool,
                );
            }
        }
        // Drain any final event that arrived without a trailing blank line.
        if let Some(ev) = sse.flush()
            && ev.data != "[DONE]"
            && let Ok(chunk) = serde_json::from_str::<ChatChunk>(&ev.data)
        {
            if let Some(u) = chunk.usage {
                usage = Some(effective_usage(u));
            }
            if let Some(choice) = chunk.choices.first() {
                if let Some(ref r) = choice.finish_reason {
                    finish_reason = Some(r.clone());
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
                    match splitter.as_mut() {
                        Some(s) => segments.extend(s.push(text)),
                        None => segments.push(Segment::Answer(text.to_string())),
                    }
                }
                emit_segments(
                    segments,
                    &mut raw_content,
                    &mut raw_thinking,
                    &mut current_stage,
                    &spool,
                );
            }
        }

        // Flush any tag-splitter remainder (e.g. content after the last tag).
        let unclosed_thought = splitter.as_ref().is_some_and(|s| s.in_thinking);
        if let Some(s) = splitter
            && let Some(seg) = s.flush()
        {
            emit_segments(
                vec![seg],
                &mut raw_content,
                &mut raw_thinking,
                &mut current_stage,
                &spool,
            );
        }

        // Recovery: if a `<think>`/`<thought>` tag opened but never closed,
        // every chunk was misclassified as Thinking and `raw_content` is
        // empty. Fall back to the streamed thinking text so the user gets
        // *something* rather than a "No response" error.
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

        session.commit_turn(prompt, model, response, usage)
    }
}

fn emit_segments(
    segments: Vec<Segment>,
    raw_content: &mut String,
    raw_thinking: &mut String,
    current_stage: &mut Option<ProgressStage>,
    spool: &std::sync::Mutex<consult_llm_core::monitoring::RunSpool>,
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

/// True if the IO error originated from a ureq timeout (or stdlib TimedOut).
/// `timeout_recv_body` raises a `ureq::Error::Timeout` which surfaces as
/// `io::Error` with kind `TimedOut` when reading from `BodyReader`.
pub(super) fn is_timeout_err(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::TimedOut {
        return true;
    }
    // ureq wraps its own Error in io::Error::Other; fall back to a string match.
    let s = e.to_string();
    s.contains("timeout") || s.contains("Timeout")
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
    fn splitter_full_thought_and_answer_in_separate_chunks() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        let segs = p.push("<thought>plan A</thought>answer");
        assert_eq!(
            segs,
            vec![
                Segment::Thinking("plan A".into()),
                Segment::Answer("answer".into())
            ]
        );
    }

    #[test]
    fn splitter_split_open_tag_across_chunks() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        assert_eq!(p.push("<tho"), vec![]);
        assert_eq!(p.push("ught>plan"), vec![Segment::Thinking("plan".into())]);
    }

    #[test]
    fn splitter_split_close_tag_across_chunks() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        let _ = p.push("<thought>plan");
        assert_eq!(p.push("</thou"), vec![]);
        assert_eq!(p.push("ght>answer"), vec![Segment::Answer("answer".into())]);
    }

    #[test]
    fn splitter_close_tag_at_start_of_answer_chunk() {
        // Mirrors the real Gemini boundary: thought chunk has only opening
        // tag, answer chunk starts with closing tag.
        let mut p = TagSplitter::new("<thought>", "</thought>");
        assert_eq!(
            p.push("<thought>thinking text"),
            vec![Segment::Thinking("thinking text".into())]
        );
        assert_eq!(
            p.push("</thought>**Answer**"),
            vec![Segment::Answer("**Answer**".into())]
        );
    }

    #[test]
    fn splitter_no_tags_passthrough() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        assert_eq!(
            p.push("plain answer text"),
            vec![Segment::Answer("plain answer text".into())]
        );
        assert!(p.flush().is_none());
    }

    #[test]
    fn splitter_strips_trailing_newline_after_close() {
        let mut p = TagSplitter::new("<think>", "</think>");
        let segs = p.push("<think>x</think>\nanswer");
        assert_eq!(
            segs,
            vec![
                Segment::Thinking("x".into()),
                Segment::Answer("answer".into())
            ]
        );
    }

    #[test]
    fn splitter_holds_partial_suffix_that_is_not_tag() {
        // A trailing '<' could start an open tag; must be held back.
        let mut p = TagSplitter::new("<thought>", "</thought>");
        let s1 = p.push("hello <");
        assert_eq!(s1, vec![Segment::Answer("hello ".into())]);
        let s2 = p.push("world");
        assert_eq!(s2, vec![Segment::Answer("<world".into())]);
    }

    #[test]
    fn splitter_unicode_safe_when_buffer_ends_non_ascii() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        let segs = p.push("café 🍰");
        assert_eq!(segs, vec![Segment::Answer("café 🍰".into())]);
    }

    #[test]
    fn detect_dialect_known_providers() {
        assert_eq!(
            detect_dialect("https://generativelanguage.googleapis.com/v1beta/openai/"),
            Dialect::Gemini
        );
        assert_eq!(
            detect_dialect("https://api.minimax.io/v1"),
            Dialect::MiniMax
        );
        assert_eq!(
            detect_dialect("https://api.openai.com/v1/"),
            Dialect::Generic
        );
        assert_eq!(detect_dialect("https://api.deepseek.com"), Dialect::Generic);
    }
}
