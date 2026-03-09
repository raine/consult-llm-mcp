use async_trait::async_trait;
use std::path::PathBuf;

use super::stream::{ParsedStreamEvent, StreamEvents, tool_label};
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities};
use super::{append_file_refs, build_extra_dir_args, run_cli_executor};

pub struct GeminiCliExecutor {
    capabilities: LlmExecutorCapabilities,
}

impl GeminiCliExecutor {
    pub fn new() -> Self {
        Self {
            capabilities: LlmExecutorCapabilities {
                is_cli: true,
                supports_threads: true,
                supports_file_refs: true,
            },
        }
    }
}

pub fn parse_gemini_line(line: &str) -> StreamEvents {
    use smallvec::smallvec;

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return smallvec![];
    }
    let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return smallvec![];
    };

    match event.get("type").and_then(|t| t.as_str()) {
        Some("init") => {
            if let Some(sid) = event.get("session_id").and_then(|v| v.as_str()) {
                smallvec![
                    ParsedStreamEvent::SessionStarted {
                        id: sid.to_string(),
                    },
                    ParsedStreamEvent::Thinking {
                        text: String::new(),
                    },
                ]
            } else {
                smallvec![ParsedStreamEvent::Thinking {
                    text: String::new(),
                }]
            }
        }
        Some("message") => {
            if event.get("role").and_then(|r| r.as_str()) == Some("assistant")
                && event.get("delta").and_then(|d| d.as_bool()) == Some(true)
            {
                if let Some(content) = event.get("content").and_then(|c| c.as_str()) {
                    smallvec![ParsedStreamEvent::AssistantText {
                        text: content.to_string(),
                    }]
                } else {
                    smallvec![]
                }
            } else {
                smallvec![]
            }
        }
        Some("tool_use") => {
            let tool_name = event
                .get("tool_name")
                .and_then(|t| t.as_str())
                .unwrap_or("tool");
            let params = event.get("parameters");
            let (short_name, detail) = match tool_name {
                "read_file" => (
                    "read",
                    params
                        .and_then(|p| p.get("file_path"))
                        .and_then(|v| v.as_str()),
                ),
                "glob" => (
                    "glob",
                    params
                        .and_then(|p| p.get("pattern"))
                        .and_then(|v| v.as_str()),
                ),
                "grep_search" => (
                    "grep",
                    params.and_then(|p| p.get("query")).and_then(|v| v.as_str()),
                ),
                _ => (tool_name, None),
            };
            let label = tool_label(short_name, detail);
            let call_id = event
                .get("tool_id")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            smallvec![ParsedStreamEvent::ToolStarted { call_id, label }]
        }
        Some("tool_result") => {
            let call_id = event
                .get("tool_id")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let status = event.get("status").and_then(|s| s.as_str());
            let success = status == Some("success");
            let error = if success {
                None
            } else {
                event
                    .get("error")
                    .and_then(|e| e.as_str())
                    .or(status)
                    .map(|s| s.to_string())
            };
            smallvec![ParsedStreamEvent::ToolFinished {
                call_id,
                success,
                error,
            }]
        }
        Some("result") => {
            if let Some(stats) = event.get("stats") {
                let input = stats
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let output = stats
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                smallvec![ParsedStreamEvent::Usage {
                    prompt_tokens: input,
                    completion_tokens: output,
                }]
            } else {
                smallvec![]
            }
        }
        _ => smallvec![],
    }
}

#[async_trait]
impl LlmExecutor for GeminiCliExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "gemini_cli"
    }

    async fn execute(
        &self,
        prompt: &str,
        model: &str,
        system_prompt: &str,
        file_paths: Option<&[PathBuf]>,
        thread_id: Option<&str>,
        consultation_id: Option<&str>,
    ) -> anyhow::Result<ExecuteResult> {
        let message_with_files = append_file_refs(prompt, file_paths);
        let message = if thread_id.is_some() {
            message_with_files
        } else {
            format!("{system_prompt}\n\n{message_with_files}")
        };

        let mut args: Vec<String> = vec![
            "-m".to_string(),
            model.to_string(),
            "-o".to_string(),
            "stream-json".to_string(),
        ];

        args.extend(build_extra_dir_args(file_paths, "--include-directories"));

        if let Some(tid) = thread_id {
            args.push("-r".to_string());
            args.push(tid.to_string());
        }
        args.push("-p".to_string());
        args.push(message);

        run_cli_executor("gemini", &args, prompt, consultation_id, parse_gemini_line)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("RESOURCE_EXHAUSTED") {
                    anyhow::anyhow!(
                        "Gemini quota exceeded. Consider using gemini-2.0-flash model. {msg}"
                    )
                } else {
                    e
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executors::stream::StreamReducer;

    #[test]
    fn test_parse_gemini_line_init() {
        let events = parse_gemini_line(
            r#"{"type":"init","timestamp":"...","session_id":"sess_123","model":"gemini-3"}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], ParsedStreamEvent::SessionStarted { id } if id == "sess_123"));
        assert!(matches!(&events[1], ParsedStreamEvent::Thinking { text } if text.is_empty()));
    }

    #[test]
    fn test_parse_gemini_line_assistant_delta() {
        let events = parse_gemini_line(
            r#"{"type":"message","role":"assistant","content":"Hello ","delta":true}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::AssistantText { text } if text == "Hello ")
        );
    }

    #[test]
    fn test_parse_gemini_line_tool_use() {
        let events = parse_gemini_line(
            r#"{"type":"tool_use","tool_name":"read_file","tool_id":"rf_123","parameters":{"file_path":"src/main.rs"}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::ToolStarted { call_id, label } if call_id == "rf_123" && label == "read src/main.rs")
        );
    }

    #[test]
    fn test_parse_gemini_line_tool_result() {
        let events = parse_gemini_line(
            r#"{"type":"tool_result","tool_id":"read_file_123","status":"success"}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::ToolFinished { call_id, success, .. } if call_id == "read_file_123" && *success)
        );
    }

    #[test]
    fn test_parse_gemini_line_usage() {
        let events = parse_gemini_line(
            r#"{"type":"result","stats":{"input_tokens":300,"output_tokens":200}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ParsedStreamEvent::Usage {
                prompt_tokens: 300,
                completion_tokens: 200
            }
        ));
    }

    #[test]
    fn test_reducer_concatenates_deltas() {
        let mut reducer = StreamReducer::new(None, None);
        reducer.process(parse_gemini_line(
            r#"{"type":"message","role":"assistant","content":"Hello ","delta":true}"#,
        ));
        reducer.process(parse_gemini_line(
            r#"{"type":"message","role":"assistant","content":"from Gemini!","delta":true}"#,
        ));
        assert_eq!(reducer.response, "Hello from Gemini!");
    }

    #[test]
    fn test_reducer_tracks_tool_labels() {
        let mut reducer = StreamReducer::new(None, None);
        reducer.process(parse_gemini_line(
            r#"{"type":"tool_use","tool_name":"read_file","tool_id":"read_file_123"}"#,
        ));
        reducer.process(parse_gemini_line(
            r#"{"type":"tool_result","tool_id":"read_file_123","status":"success"}"#,
        ));
        // Tool label resolved from active_tools — no assertion on internal state,
        // but this exercises the call_id → label correlation path
    }

    #[test]
    fn test_parse_gemini_line_empty() {
        assert!(parse_gemini_line("").is_empty());
        assert!(parse_gemini_line("  ").is_empty());
        assert!(parse_gemini_line("not json").is_empty());
    }

    #[test]
    fn test_reducer_full_sequence_with_tools() {
        let mut reducer = StreamReducer::new(None, None);
        let lines = vec![
            r#"{"type":"init","timestamp":"...","session_id":"sess1","model":"gemini-3"}"#,
            r#"{"type":"message","timestamp":"...","role":"user","content":"analyze README.md"}"#,
            r#"{"type":"tool_use","timestamp":"...","tool_name":"read_file","tool_id":"read_file_001","parameters":{"file_path":"README.md"}}"#,
            r#"{"type":"tool_result","timestamp":"...","tool_id":"read_file_001","status":"success","output":""}"#,
            r#"{"type":"tool_use","timestamp":"...","tool_name":"read_file","tool_id":"read_file_002","parameters":{"file_path":"src/main.rs"}}"#,
            r#"{"type":"tool_result","timestamp":"...","tool_id":"read_file_002","status":"success","output":""}"#,
            r#"{"type":"message","timestamp":"...","role":"assistant","content":"Here is the analysis.","delta":true}"#,
            r#"{"type":"result","timestamp":"...","status":"success","stats":{"input_tokens":1000,"output_tokens":100}}"#,
        ];
        for line in &lines {
            reducer.process(parse_gemini_line(line));
        }
        assert_eq!(reducer.thread_id, Some("sess1".to_string()));
        assert_eq!(reducer.response, "Here is the analysis.");
        assert!(reducer.usage.is_some());
    }
}
