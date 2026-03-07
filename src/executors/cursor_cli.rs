use async_trait::async_trait;
use std::path::PathBuf;

use super::cli_runner::{run_cli_streaming, truncate_at_char_boundary};
use super::stream::{ParsedStreamEvent, StreamReducer, tool_label};
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities, Usage};
use crate::config::config;
use crate::logger::log_cli_debug;

pub struct CursorCliExecutor {
    capabilities: LlmExecutorCapabilities,
}

impl CursorCliExecutor {
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

pub fn parse_cursor_line(line: &str) -> Vec<ParsedStreamEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return vec![];
    };
    let event_type = event.get("type").and_then(|t| t.as_str());
    let subtype = event.get("subtype").and_then(|t| t.as_str());

    match event_type {
        Some("system") if subtype == Some("init") => {
            if let Some(sid) = event.get("session_id").and_then(|v| v.as_str()) {
                vec![ParsedStreamEvent::SessionStarted {
                    id: sid.to_string(),
                }]
            } else {
                vec![]
            }
        }
        Some("thinking") if subtype == Some("delta") => {
            vec![ParsedStreamEvent::Thinking]
        }
        Some("tool_call") => {
            let tc = event.get("tool_call");
            let call_id = event
                .get("call_id")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            match subtype {
                Some("started") => {
                    let label = tc
                        .map(extract_cursor_tool_name)
                        .unwrap_or_else(|| "tool".to_string());
                    vec![ParsedStreamEvent::ToolStarted { call_id, label }]
                }
                Some("completed") => {
                    let success = tc.map(is_cursor_tool_success).unwrap_or(false);
                    vec![ParsedStreamEvent::ToolFinished { call_id, success }]
                }
                _ => vec![],
            }
        }
        Some("assistant") => {
            // Emit Responding progress without accumulating text —
            // full response comes from the result event
            vec![ParsedStreamEvent::AssistantText {
                text: String::new(),
            }]
        }
        Some("result") => {
            let mut events = vec![];
            if let Some(text) = event.get("result").and_then(|v| v.as_str()) {
                events.push(ParsedStreamEvent::AssistantText {
                    text: text.to_string(),
                });
            }
            if let Some(u) = event.get("usage") {
                let input = u.get("inputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let output = u.get("outputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
                events.push(ParsedStreamEvent::Usage(Usage {
                    prompt_tokens: input,
                    completion_tokens: output,
                }));
            }
            events
        }
        _ => vec![],
    }
}

fn extract_cursor_tool_name(tool_call: &serde_json::Value) -> String {
    if let Some(shell) = tool_call.get("shellToolCall") {
        if let Some(desc) = shell.get("description").and_then(|d| d.as_str()) {
            return desc.to_string();
        }
        if let Some(args) = shell.get("args") {
            if let Some(desc) = args.get("description").and_then(|d| d.as_str()) {
                return desc.to_string();
            }
            if let Some(cmds) = args.get("simpleCommands").and_then(|c| c.as_array())
                && let Some(first) = cmds.first().and_then(|c| c.as_str())
            {
                return first.to_string();
            }
        }
    }
    if let Some(read) = tool_call.get("readToolCall") {
        let path = read
            .get("args")
            .and_then(|a| a.get("path"))
            .or_else(|| read.get("path"))
            .and_then(|v| v.as_str());
        return tool_label("read", path);
    }
    if let Some(glob) = tool_call.get("globToolCall") {
        let pattern = glob
            .get("args")
            .and_then(|a| a.get("globPattern"))
            .or_else(|| glob.get("pattern"))
            .and_then(|v| v.as_str());
        return tool_label("glob", pattern);
    }
    "tool".to_string()
}

fn is_cursor_tool_success(tool_call: &serde_json::Value) -> bool {
    for key in ["readToolCall", "globToolCall", "shellToolCall"] {
        if let Some(tc) = tool_call.get(key)
            && let Some(result) = tc.get("result")
        {
            return result.get("success").is_some();
        }
    }
    false
}

/// Map model IDs to cursor-agent model names
pub fn map_cursor_model(model: &str) -> String {
    let cfg = config();
    let mut cursor_model = model.replace("-preview", "");

    // cursor-agent encodes reasoning effort in the model name
    // e.g. gpt-5.3-codex + high → gpt-5.3-codex-high
    if let Some(ref effort) = cfg.codex_reasoning_effort
        && cursor_model.contains("-codex")
    {
        cursor_model = format!("{cursor_model}-{effort}");
    }

    cursor_model
}

fn append_files(text: &str, file_paths: Option<&[PathBuf]>) -> String {
    match file_paths {
        Some(fps) if !fps.is_empty() => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let file_list: Vec<String> = fps
                .iter()
                .map(|p| {
                    let rel = pathdiff::diff_paths(p, &cwd).unwrap_or_else(|| p.clone());
                    format!("- {}", rel.display())
                })
                .collect();
            format!(
                "{text}\n\nPlease read the following files for context:\n{}",
                file_list.join("\n")
            )
        }
        _ => text.to_string(),
    }
}

#[async_trait]
impl LlmExecutor for CursorCliExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "cursor_cli"
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

        let cursor_model = map_cursor_model(model);

        // --trust is required for headless (--print) mode to skip the interactive
        // workspace trust prompt. --mode ask restricts to read-only operations.
        let mut args: Vec<String> = vec![
            "--print".to_string(),
            "--trust".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--mode".to_string(),
            "ask".to_string(),
            "--model".to_string(),
            cursor_model,
        ];
        if let Some(tid) = thread_id {
            args.push("--resume".to_string());
            args.push(tid.to_string());
        }
        args.push(message);

        let mut reducer = StreamReducer::new(consultation_id);
        let result = run_cli_streaming("cursor-agent", &args, |line| {
            reducer.process(parse_cursor_line(line));
        })
        .await?;

        if result.code == Some(0) {
            if reducer.response.is_empty() {
                log_cli_debug(
                    "No result found in Cursor CLI stream-json output",
                    Some(&serde_json::json!({
                        "rawOutput": &result.stdout[..truncate_at_char_boundary(&result.stdout, 500)]
                    })),
                );
                anyhow::bail!("No result found in Cursor CLI stream-json output");
            }
            Ok(ExecuteResult {
                response: reducer.response,
                usage: reducer.usage,
                thread_id: reducer
                    .thread_id
                    .or_else(|| thread_id.map(|s| s.to_string())),
            })
        } else {
            anyhow::bail!(
                "Cursor CLI exited with code {}. Error: {}",
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
    fn test_parse_cursor_line_init() {
        let events =
            parse_cursor_line(r#"{"type":"system","subtype":"init","session_id":"sess_456"}"#);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ParsedStreamEvent::SessionStarted { id } if id == "sess_456"));
    }

    #[test]
    fn test_parse_cursor_line_thinking() {
        let events =
            parse_cursor_line(r#"{"type":"thinking","subtype":"delta","text":"**Starting"}"#);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ParsedStreamEvent::Thinking));
    }

    #[test]
    fn test_parse_cursor_line_tool_started() {
        let events = parse_cursor_line(
            r#"{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"readToolCall":{"path":"src/lib.rs"}}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::ToolStarted { call_id, label } if call_id == "c1" && label == "read src/lib.rs")
        );
    }

    #[test]
    fn test_parse_cursor_line_assistant() {
        let events = parse_cursor_line(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello"}]},"session_id":"...","timestamp_ms":123}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ParsedStreamEvent::AssistantText { text } if text.is_empty()));
    }

    #[test]
    fn test_parse_cursor_line_result() {
        let events = parse_cursor_line(
            r#"{"type":"result","result":"Final answer","usage":{"inputTokens":500,"outputTokens":100}}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], ParsedStreamEvent::AssistantText { text } if text == "Final answer")
        );
        assert!(
            matches!(&events[1], ParsedStreamEvent::Usage(u) if u.prompt_tokens == 500 && u.completion_tokens == 100)
        );
    }

    #[test]
    fn test_parse_cursor_line_empty() {
        assert!(parse_cursor_line("").is_empty());
        assert!(parse_cursor_line("not json").is_empty());
    }

    #[test]
    fn test_extract_cursor_tool_name_shell() {
        let tc: serde_json::Value =
            serde_json::from_str(r#"{"shellToolCall":{"args":{"simpleCommands":["ls -la"]}}}"#)
                .unwrap();
        assert_eq!(extract_cursor_tool_name(&tc), "ls -la");
    }

    #[test]
    fn test_extract_cursor_tool_name_read() {
        let tc: serde_json::Value =
            serde_json::from_str(r#"{"readToolCall":{"path":"src/lib.rs"}}"#).unwrap();
        assert_eq!(extract_cursor_tool_name(&tc), "read src/lib.rs");
    }

    #[test]
    fn test_extract_cursor_tool_name_glob() {
        let tc: serde_json::Value =
            serde_json::from_str(r#"{"globToolCall":{"pattern":"**/*.rs"}}"#).unwrap();
        assert_eq!(extract_cursor_tool_name(&tc), "glob **/*.rs");
    }
}
