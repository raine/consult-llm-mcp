use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use consult_llm_core::monitoring::{ProgressStage, RunSpool};
use smallvec::SmallVec;

use super::types::Usage;
pub use consult_llm_core::stream_events::ParsedStreamEvent;

/// Most parsed lines produce 0-2 events; SmallVec avoids heap allocation
/// for the common case.
pub type StreamEvents = SmallVec<[ParsedStreamEvent; 2]>;

/// Format a tool label with an optional detail (file path, pattern, etc.)
/// e.g. ("read", Some("src/main.rs")) → "read src/main.rs"
pub fn tool_label(name: &str, detail: Option<&str>) -> String {
    match detail {
        Some(d) => format!("{name} {d}"),
        None => name.to_string(),
    }
}

/// Accumulates stream events into a final result and forwards them to the spool.
pub struct StreamReducer {
    pub thread_id: Option<String>,
    pub response: String,
    pub usage: Option<Usage>,
    spool: Arc<Mutex<RunSpool>>,
    active_tools: HashMap<String, String>,
}

impl StreamReducer {
    pub fn new(
        spool: Arc<Mutex<RunSpool>>,
        prompt: Option<&str>,
        system_prompt: Option<&str>,
    ) -> Self {
        {
            let mut s = spool.lock().unwrap();
            if let Some(text) = system_prompt {
                s.stream_event(ParsedStreamEvent::SystemPrompt {
                    text: text.to_string(),
                });
            }
            if let Some(text) = prompt {
                s.stream_event(ParsedStreamEvent::Prompt {
                    text: text.to_string(),
                });
            }
        }
        Self {
            thread_id: None,
            response: String::with_capacity(4096),
            usage: None,
            spool,
            active_tools: HashMap::new(),
        }
    }

    pub fn process(&mut self, events: StreamEvents) {
        let mut s = self.spool.lock().unwrap();
        for event in events {
            match event.clone() {
                ParsedStreamEvent::SessionStarted { id } => {
                    self.thread_id = Some(id.clone());
                    s.resolve_thread_id(id);
                }
                ParsedStreamEvent::Thinking { .. } => {
                    s.set_stage(ProgressStage::Thinking);
                }
                ParsedStreamEvent::AssistantText { text } => {
                    self.response.push_str(&text);
                    s.set_stage(ProgressStage::Responding);
                }
                ParsedStreamEvent::ToolStarted {
                    ref call_id,
                    ref label,
                } => {
                    self.active_tools.insert(call_id.clone(), label.clone());
                    s.set_stage(ProgressStage::ToolUse {
                        tool: label.clone(),
                    });
                }
                ParsedStreamEvent::ToolFinished {
                    ref call_id,
                    success,
                    ..
                } => {
                    if let Some(label) = self.active_tools.remove(call_id) {
                        s.set_stage(ProgressStage::ToolResult {
                            tool: label,
                            success,
                        });
                    }
                }
                ParsedStreamEvent::Prompt { .. }
                | ParsedStreamEvent::SystemPrompt { .. }
                | ParsedStreamEvent::FilesContext { .. } => {}
                ParsedStreamEvent::Usage {
                    prompt_tokens,
                    completion_tokens,
                } => {
                    self.usage = Some(Usage {
                        prompt_tokens,
                        completion_tokens,
                    });
                }
            }
            s.stream_event(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use consult_llm_core::monitoring::RunSpool;
    use smallvec::smallvec;

    fn reducer() -> StreamReducer {
        StreamReducer::new(Arc::new(Mutex::new(RunSpool::disabled())), None, None)
    }

    #[test]
    fn complete_stream_session_text_usage() {
        // Happy path: SessionStarted resolves thread_id, AssistantText chunks
        // accumulate, Usage is captured.
        let mut r = reducer();
        r.process(smallvec![ParsedStreamEvent::SessionStarted {
            id: "api_thread_xyz".into(),
        }]);
        r.process(smallvec![
            ParsedStreamEvent::AssistantText {
                text: "Hello ".into()
            },
            ParsedStreamEvent::AssistantText {
                text: "world".into()
            },
        ]);
        r.process(smallvec![ParsedStreamEvent::Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
        }]);
        assert_eq!(r.thread_id.as_deref(), Some("api_thread_xyz"));
        assert_eq!(r.response, "Hello world");
        let u = r.usage.expect("usage captured");
        assert_eq!(u.prompt_tokens, 10);
        assert_eq!(u.completion_tokens, 5);
    }

    #[test]
    fn heartbeat_only_chunks_are_noop() {
        // Empty event batches (e.g. SSE heartbeats that produced no parsed
        // events) must not change reducer state.
        let mut r = reducer();
        r.process(smallvec![]);
        r.process(smallvec![]);
        assert!(r.thread_id.is_none());
        assert!(r.response.is_empty());
        assert!(r.usage.is_none());
    }

    #[test]
    fn usage_event_captured_standalone() {
        let mut r = reducer();
        r.process(smallvec![ParsedStreamEvent::Usage {
            prompt_tokens: 1,
            completion_tokens: 2,
        }]);
        let u = r.usage.expect("usage present");
        assert_eq!(u.prompt_tokens, 1);
        assert_eq!(u.completion_tokens, 2);
        assert!(r.response.is_empty());
        assert!(r.thread_id.is_none());
    }

    #[test]
    fn assistant_text_before_session_started_leaves_thread_id_unset() {
        // Failure-path / out-of-order case: a backend that streams text
        // before announcing its session ID still has its text accumulated,
        // but thread_id stays None until SessionStarted arrives.
        let mut r = reducer();
        r.process(smallvec![ParsedStreamEvent::AssistantText {
            text: "leak".into(),
        }]);
        assert!(r.thread_id.is_none());
        assert_eq!(r.response, "leak");
        // Late SessionStarted still resolves.
        r.process(smallvec![ParsedStreamEvent::SessionStarted {
            id: "api_late".into(),
        }]);
        assert_eq!(r.thread_id.as_deref(), Some("api_late"));
    }

    #[test]
    fn tool_lifecycle_drops_unmatched_finish() {
        // ToolFinished without a prior ToolStarted is silently ignored —
        // pin this so refactors can't turn it into a panic.
        let mut r = reducer();
        r.process(smallvec![ParsedStreamEvent::ToolFinished {
            call_id: "missing".into(),
            success: false,
            error: None,
        }]);
        // No assertion target other than "did not panic"; reducer state
        // remains pristine.
        assert!(r.response.is_empty());
        assert!(r.thread_id.is_none());
    }
}
