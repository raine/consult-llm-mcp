use async_trait::async_trait;
use std::path::PathBuf;

use super::cli_runner::run_cli_streaming;
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities, Usage};
use crate::external_dirs::get_external_directories;
use crate::git_worktree::get_main_worktree_path;
use crate::monitoring;

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

/// Parse NDJSON output from `gemini -o stream-json`.
/// Returns (session_id, concatenated response text, usage).
pub fn parse_gemini_stream_jsonl(
    output: &str,
) -> anyhow::Result<(Option<String>, String, Option<Usage>)> {
    let mut session_id = None;
    let mut response_parts = Vec::new();
    let mut usage = None;

    for line in output.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let event_type = event.get("type").and_then(|t| t.as_str());
        match event_type {
            Some("init") => {
                if let Some(sid) = event.get("session_id").and_then(|v| v.as_str()) {
                    session_id = Some(sid.to_string());
                }
            }
            Some("message") => {
                if event.get("role").and_then(|r| r.as_str()) == Some("assistant")
                    && event.get("delta").and_then(|d| d.as_bool()) == Some(true)
                    && let Some(content) = event.get("content").and_then(|c| c.as_str())
                {
                    response_parts.push(content.to_string());
                }
            }
            Some("result") => {
                if let Some(stats) = event.get("stats") {
                    let input = stats
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let output_tokens = stats
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    usage = Some(Usage {
                        prompt_tokens: input,
                        completion_tokens: output_tokens,
                    });
                }
            }
            _ => {}
        }
    }

    Ok((session_id, response_parts.concat(), usage))
}

fn emit_gemini_progress(consultation_id: &str, line: &str) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }
    let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return;
    };
    let event_type = event.get("type").and_then(|t| t.as_str());

    let stage = match event_type {
        Some("message") => {
            if event.get("role").and_then(|r| r.as_str()) == Some("assistant")
                && event.get("delta").and_then(|d| d.as_bool()) == Some(true)
            {
                Some(monitoring::ProgressStage::Responding)
            } else {
                None
            }
        }
        Some("tool_use") => {
            let tool = event
                .get("tool_name")
                .and_then(|t| t.as_str())
                .unwrap_or("tool")
                .to_string();
            Some(monitoring::ProgressStage::ToolUse { tool })
        }
        Some("tool_result") => {
            let tool = event
                .get("tool_id")
                .and_then(|t| t.as_str())
                .and_then(|id| id.split('_').next())
                .unwrap_or("tool")
                .to_string();
            let success = event.get("status").and_then(|s| s.as_str()) == Some("success");
            Some(monitoring::ProgressStage::ToolResult { tool, success })
        }
        _ => None,
    };

    if let Some(stage) = stage {
        monitoring::emit(monitoring::MonitorEvent::ConsultProgress {
            id: consultation_id.to_string(),
            stage,
        });
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

        let cid = consultation_id.map(|s| s.to_string());
        let result = run_cli_streaming("gemini", &args, move |line| {
            if let Some(ref cid) = cid {
                emit_gemini_progress(cid, line);
            }
        })
        .await?;

        if result.code == Some(0) {
            match parse_gemini_stream_jsonl(&result.stdout) {
                Ok((session_id, response, usage)) => {
                    if response.is_empty() {
                        anyhow::bail!("No response found in Gemini stream-json output");
                    }
                    Ok(ExecuteResult {
                        response,
                        usage,
                        thread_id: session_id,
                    })
                }
                Err(e) => {
                    anyhow::bail!("Failed to parse Gemini stream-json output: {e}");
                }
            }
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
    fn test_parse_gemini_stream_jsonl_valid() {
        let output = r#"{"type":"init","timestamp":"2026-03-07T13:40:51.667Z","session_id":"sess_123","model":"gemini-3"}
{"type":"message","timestamp":"...","role":"user","content":"hello"}
{"type":"message","timestamp":"...","role":"assistant","content":"Hello ","delta":true}
{"type":"message","timestamp":"...","role":"assistant","content":"from Gemini!","delta":true}
{"type":"result","timestamp":"...","status":"success","stats":{"total_tokens":500,"input_tokens":300,"output_tokens":200,"cached":100,"input":200,"duration_ms":5000,"tool_calls":0}}
"#;
        let (sid, response, usage) = parse_gemini_stream_jsonl(output).unwrap();
        assert_eq!(sid, Some("sess_123".to_string()));
        assert_eq!(response, "Hello from Gemini!");
        let usage = usage.unwrap();
        assert_eq!(usage.prompt_tokens, 300);
        assert_eq!(usage.completion_tokens, 200);
    }

    #[test]
    fn test_parse_gemini_stream_jsonl_with_tools() {
        let output = r#"{"type":"init","timestamp":"...","session_id":"s1","model":"gemini-3"}
{"type":"message","timestamp":"...","role":"assistant","content":"Let me read that file.\n","delta":true}
{"type":"tool_use","timestamp":"...","tool_name":"read_file","tool_id":"read_file_123","parameters":{"file_path":"src/lib.rs"}}
{"type":"tool_result","timestamp":"...","tool_id":"read_file_123","status":"success","output":""}
{"type":"message","timestamp":"...","role":"assistant","content":"The file exports one module.","delta":true}
{"type":"result","timestamp":"...","status":"success","stats":{"total_tokens":1000,"input_tokens":800,"output_tokens":200,"cached":0,"input":800,"duration_ms":3000,"tool_calls":1}}
"#;
        let (sid, response, _usage) = parse_gemini_stream_jsonl(output).unwrap();
        assert_eq!(sid, Some("s1".to_string()));
        assert_eq!(
            response,
            "Let me read that file.\nThe file exports one module."
        );
    }

    #[test]
    fn test_parse_gemini_stream_jsonl_empty() {
        let (sid, response, usage) = parse_gemini_stream_jsonl("").unwrap();
        assert_eq!(sid, None);
        assert!(response.is_empty());
        assert!(usage.is_none());
    }
}
