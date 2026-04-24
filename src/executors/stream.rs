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
