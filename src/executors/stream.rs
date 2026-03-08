use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};

use consult_llm_core::monitoring::{self, ProgressStage};

use super::types::Usage;
pub use consult_llm_core::stream_events::ParsedStreamEvent;

/// Format a tool label with an optional detail (file path, pattern, etc.)
/// e.g. ("read", Some("src/main.rs")) → "read src/main.rs"
pub fn tool_label(name: &str, detail: Option<&str>) -> String {
    match detail {
        Some(d) => format!("{name} {d}"),
        None => name.to_string(),
    }
}

/// Writes ParsedStreamEvent entries to a sidecar `.events.jsonl` file.
/// Used by both streaming CLI executors and the API executor.
pub struct SidecarWriter {
    writer: Option<BufWriter<File>>,
}

impl SidecarWriter {
    pub fn new(consultation_id: Option<&str>) -> Self {
        let writer = consultation_id.and_then(|cid| {
            let dir = monitoring::sessions_dir();
            let path = dir.join(format!("{cid}.events.jsonl"));
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(BufWriter::new)
        });
        Self { writer }
    }

    pub fn write(&mut self, event: &ParsedStreamEvent) {
        if let Some(ref mut w) = self.writer
            && let Ok(line) = serde_json::to_string(event)
        {
            let _ = writeln!(w, "{line}");
        }
    }

    pub fn flush(&mut self) {
        if let Some(ref mut w) = self.writer {
            let _ = w.flush();
        }
    }
}

/// Accumulates stream events into a final result and emits monitoring progress.
pub struct StreamReducer {
    pub thread_id: Option<String>,
    pub response: String,
    pub usage: Option<Usage>,
    consultation_id: Option<String>,
    active_tools: HashMap<String, String>,
    last_stage: Option<String>,
    sidecar: SidecarWriter,
}

impl StreamReducer {
    pub fn new(consultation_id: Option<&str>, prompt: Option<&str>) -> Self {
        let mut sidecar = SidecarWriter::new(consultation_id);
        if let Some(text) = prompt {
            sidecar.write(&ParsedStreamEvent::Prompt {
                text: text.to_string(),
            });
        }
        Self {
            thread_id: None,
            response: String::new(),
            usage: None,
            consultation_id: consultation_id.map(|s| s.to_string()),
            active_tools: HashMap::new(),
            last_stage: None,
            sidecar,
        }
    }

    /// Process a batch of parsed events from a single line.
    /// Flushes the sidecar file once per batch rather than per event.
    pub fn process(&mut self, events: Vec<ParsedStreamEvent>) {
        for event in events {
            self.sidecar.write(&event);
            match event {
                ParsedStreamEvent::SessionStarted { id } => {
                    self.thread_id = Some(id);
                }
                ParsedStreamEvent::Thinking => {
                    self.emit_progress(ProgressStage::Thinking);
                }
                ParsedStreamEvent::AssistantText { text } => {
                    self.response.push_str(&text);
                    self.emit_progress(ProgressStage::Responding);
                }
                ParsedStreamEvent::ToolStarted { call_id, label } => {
                    self.active_tools.insert(call_id.clone(), label.clone());
                    self.emit_progress(ProgressStage::ToolUse { tool: label });
                }
                ParsedStreamEvent::ToolFinished { call_id, .. } => {
                    self.active_tools.remove(&call_id);
                }
                ParsedStreamEvent::Prompt { .. } => {}
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
        }
        self.sidecar.flush();
    }

    fn emit_progress(&mut self, stage: ProgressStage) {
        let stage_str = stage.to_string();
        if self.last_stage.as_ref() == Some(&stage_str) {
            return;
        }
        self.last_stage = Some(stage_str);
        if let Some(ref cid) = self.consultation_id {
            monitoring::emit(monitoring::MonitorEvent::ConsultProgress {
                id: cid.clone(),
                stage,
            });
        }
    }
}
