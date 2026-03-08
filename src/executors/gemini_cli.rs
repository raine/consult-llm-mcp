use async_trait::async_trait;
use std::path::PathBuf;

use super::cli_runner::run_cli_streaming;
use super::stream::{ParsedStreamEvent, StreamReducer, tool_label};
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities};
use crate::external_dirs::get_external_directories;
use crate::git_worktree::get_main_worktree_path;

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

pub fn parse_gemini_line(line: &str) -> Vec<ParsedStreamEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return vec![];
    };

    match event.get("type").and_then(|t| t.as_str()) {
        Some("init") => {
            let mut events = vec![ParsedStreamEvent::Thinking];
            if let Some(sid) = event.get("session_id").and_then(|v| v.as_str()) {
                events.insert(
                    0,
                    ParsedStreamEvent::SessionStarted {
                        id: sid.to_string(),
                    },
                );
            }
            events
        }
        Some("message") => {
            if event.get("role").and_then(|r| r.as_str()) == Some("assistant")
                && event.get("delta").and_then(|d| d.as_bool()) == Some(true)
            {
                if let Some(content) = event.get("content").and_then(|c| c.as_str()) {
                    vec![ParsedStreamEvent::AssistantText {
                        text: content.to_string(),
                    }]
                } else {
                    vec![]
                }
            } else {
                vec![]
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
            vec![ParsedStreamEvent::ToolStarted { call_id, label }]
        }
        Some("tool_result") => {
            let call_id = event
                .get("tool_id")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let success = event.get("status").and_then(|s| s.as_str()) == Some("success");
            vec![ParsedStreamEvent::ToolFinished { call_id, success }]
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
                vec![ParsedStreamEvent::Usage {
                    prompt_tokens: input,
                    completion_tokens: output,
                }]
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}

fn append_files(text: &str, file_paths: Option<&[PathBuf]>) -> String {
    match file_paths {
        Some(fps) if !fps.is_empty() => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let file_refs: Vec<String> = fps
                .iter()
                .map(|p| {
                    let rel = pathdiff::diff_paths(p, &cwd).unwrap_or_else(|| p.clone());
                    format!("@{}", rel.display())
                })
                .collect();
            format!("{text}\n\nFiles: {}", file_refs.join(" "))
        }
        _ => text.to_string(),
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
        let message_with_files = append_files(prompt, file_paths);
        let message = if thread_id.is_some() {
            message_with_files.clone()
        } else {
            format!("{system_prompt}\n\n{message_with_files}")
        };

        let mut args: Vec<String> = vec![
            "-m".to_string(),
            model.to_string(),
            "-o".to_string(),
            "stream-json".to_string(),
        ];

        let cwd = std::env::current_dir().unwrap_or_default();
        let mut extra_dirs: Vec<String> = Vec::new();
        if let Some(wt) = get_main_worktree_path() {
            extra_dirs.push(wt.to_string());
        }
        let resolved_paths: Option<Vec<PathBuf>> = file_paths.map(|fps| fps.to_vec());
        extra_dirs.extend(get_external_directories(resolved_paths.as_deref(), &cwd));
        for dir in &extra_dirs {
            args.push("--include-directories".to_string());
            args.push(dir.clone());
        }

        if let Some(tid) = thread_id {
            args.push("-r".to_string());
            args.push(tid.to_string());
        }
        args.push("-p".to_string());
        args.push(message);

        let mut reducer = StreamReducer::new(consultation_id, Some(prompt));
        let result = run_cli_streaming("gemini", &args, |line| {
            reducer.process(parse_gemini_line(line));
        })
        .await?;

        if result.code == Some(0) {
            if reducer.response.is_empty() {
                anyhow::bail!("No response found in Gemini stream-json output");
            }
            Ok(ExecuteResult {
                response: reducer.response,
                usage: reducer.usage,
                thread_id: reducer.thread_id,
            })
        } else {
            if result.stderr.contains("RESOURCE_EXHAUSTED") {
                anyhow::bail!(
                    "Gemini quota exceeded. Consider using gemini-2.0-flash model. Error: {}",
                    result.stderr.trim()
                );
            }
            anyhow::bail!(
                "Gemini CLI exited with code {}. Error: {}",
                result.code.unwrap_or(-1),
                result.stderr.trim()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gemini_line_init() {
        let events = parse_gemini_line(
            r#"{"type":"init","timestamp":"...","session_id":"sess_123","model":"gemini-3"}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], ParsedStreamEvent::SessionStarted { id } if id == "sess_123"));
        assert!(matches!(&events[1], ParsedStreamEvent::Thinking));
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
            matches!(&events[0], ParsedStreamEvent::ToolFinished { call_id, success } if call_id == "read_file_123" && *success)
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
