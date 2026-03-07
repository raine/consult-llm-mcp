use std::collections::HashMap;

use crate::monitoring::{self, ProgressStage};

use super::types::Usage;

/// Normalized event from any CLI's JSON stream.
/// Each CLI adapter maps its raw JSON lines to these.
#[derive(Debug, Clone)]
pub enum ParsedStreamEvent {
    SessionStarted { id: String },
    Thinking,
    AssistantText { text: String },
    ToolStarted { call_id: String, label: String },
    ToolFinished { call_id: String, success: bool },
    Usage(Usage),
}

/// Accumulates stream events into a final result and emits monitoring progress.
pub struct StreamReducer {
    pub thread_id: Option<String>,
    pub response: String,
    pub usage: Option<Usage>,
    consultation_id: Option<String>,
    active_tools: HashMap<String, String>,
    last_stage: Option<String>,
}

impl StreamReducer {
    pub fn new(consultation_id: Option<&str>) -> Self {
        Self {
            thread_id: None,
            response: String::new(),
            usage: None,
            consultation_id: consultation_id.map(|s| s.to_string()),
            active_tools: HashMap::new(),
            last_stage: None,
        }
    }

    /// Process a batch of parsed events from a single line.
    pub fn process(&mut self, events: Vec<ParsedStreamEvent>) {
        for event in events {
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
                ParsedStreamEvent::ToolFinished { call_id, success } => {
                    let label = self
                        .active_tools
                        .remove(&call_id)
                        .unwrap_or_else(|| "tool".to_string());
                    self.emit_progress(ProgressStage::ToolResult {
                        tool: label,
                        success,
                    });
                }
                ParsedStreamEvent::Usage(u) => {
                    self.usage = Some(u);
                }
            }
        }
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
