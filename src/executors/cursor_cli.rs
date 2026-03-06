use async_trait::async_trait;
use std::path::PathBuf;

use super::cli_runner::run_cli;
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities};
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

pub fn parse_cursor_json(output: &str) -> anyhow::Result<(Option<String>, String)> {
    let parsed: serde_json::Value = serde_json::from_str(output)?;
    let session_id = parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let response = parsed
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok((session_id, response))
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

    async fn execute(
        &self,
        prompt: &str,
        model: &str,
        system_prompt: &str,
        file_paths: Option<&[PathBuf]>,
        thread_id: Option<&str>,
    ) -> anyhow::Result<ExecuteResult> {
        let message_with_files = append_files(prompt, file_paths);
        let message = if thread_id.is_some() {
            message_with_files.clone()
        } else {
            format!("{system_prompt}\n\n{message_with_files}")
        };

        let cursor_model = map_cursor_model(model);

        let mut args: Vec<String> = vec![
            "--print".to_string(),
            "--trust".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
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

        let result = run_cli("cursor-agent", &args).await?;

        if result.code == Some(0) {
            match parse_cursor_json(&result.stdout) {
                Ok((session_id, response)) => {
                    if response.is_empty() {
                        anyhow::bail!("No result found in Cursor CLI JSON output");
                    }
                    Ok(ExecuteResult {
                        response,
                        usage: None,
                        thread_id: session_id.or_else(|| thread_id.map(|s| s.to_string())),
                    })
                }
                Err(_) => {
                    log_cli_debug(
                        "Failed to parse Cursor CLI JSON output",
                        Some(&serde_json::json!({ "rawOutput": &result.stdout })),
                    );
                    anyhow::bail!(
                        "Failed to parse Cursor CLI JSON output: {}",
                        &result.stdout[..result.stdout.len().min(200)]
                    );
                }
            }
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
    fn test_parse_cursor_json_valid() {
        let output = r#"{"session_id":"sess_456","result":"Hello from Cursor"}"#;
        let (sid, response) = parse_cursor_json(output).unwrap();
        assert_eq!(sid, Some("sess_456".to_string()));
        assert_eq!(response, "Hello from Cursor");
    }

    #[test]
    fn test_parse_cursor_json_no_result() {
        let output = r#"{"session_id":"sess_456"}"#;
        let (sid, response) = parse_cursor_json(output).unwrap();
        assert_eq!(sid, Some("sess_456".to_string()));
        assert!(response.is_empty());
    }

    #[test]
    fn test_parse_cursor_json_invalid() {
        assert!(parse_cursor_json("not json").is_err());
    }
}
