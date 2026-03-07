use async_trait::async_trait;
use std::path::PathBuf;

use super::cli_runner::{run_cli, truncate_at_char_boundary};
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities};
use crate::external_dirs::get_external_directories;
use crate::git_worktree::get_main_worktree_path;
use crate::logger::log_cli_debug;

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

pub fn parse_gemini_json(output: &str) -> anyhow::Result<(Option<String>, String)> {
    let parsed: serde_json::Value = serde_json::from_str(output)?;
    let session_id = parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let response = parsed
        .get("response")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok((session_id, response))
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

        let mut args: Vec<String> = vec![
            "-m".to_string(),
            model.to_string(),
            "-o".to_string(),
            "json".to_string(),
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

        let result = run_cli("gemini", &args).await?;

        if result.code == Some(0) {
            match parse_gemini_json(&result.stdout) {
                Ok((session_id, response)) => {
                    if response.is_empty() {
                        anyhow::bail!("No response found in Gemini JSON output");
                    }
                    Ok(ExecuteResult {
                        response,
                        usage: None,
                        thread_id: session_id,
                    })
                }
                Err(_e) => {
                    log_cli_debug(
                        "Failed to parse Gemini JSON output",
                        Some(&serde_json::json!({ "rawOutput": &result.stdout })),
                    );
                    anyhow::bail!(
                        "Failed to parse Gemini JSON output: {}",
                        &result.stdout[..truncate_at_char_boundary(&result.stdout, 200)]
                    );
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
    fn test_parse_gemini_json_valid() {
        let output = r#"{"session_id":"sess_123","response":"Hello from Gemini"}"#;
        let (sid, response) = parse_gemini_json(output).unwrap();
        assert_eq!(sid, Some("sess_123".to_string()));
        assert_eq!(response, "Hello from Gemini");
    }

    #[test]
    fn test_parse_gemini_json_no_response() {
        let output = r#"{"session_id":"sess_123"}"#;
        let (sid, response) = parse_gemini_json(output).unwrap();
        assert_eq!(sid, Some("sess_123".to_string()));
        assert!(response.is_empty());
    }

    #[test]
    fn test_parse_gemini_json_invalid() {
        assert!(parse_gemini_json("not json").is_err());
    }
}
