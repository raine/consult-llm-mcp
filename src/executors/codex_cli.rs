use async_trait::async_trait;
use std::path::PathBuf;

use super::cli_runner::run_cli;
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities};
use crate::config::config;
use crate::external_dirs::get_external_directories;
use crate::git_worktree::get_main_worktree_path;

pub struct CodexCliExecutor {
    capabilities: LlmExecutorCapabilities,
}

impl CodexCliExecutor {
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

pub fn parse_codex_jsonl(output: &str) -> (Option<String>, String) {
    let mut thread_id = None;
    let mut messages = Vec::new();

    for line in output.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if event.get("type").and_then(|t| t.as_str()) == Some("thread.started") {
                if let Some(tid) = event.get("thread_id").and_then(|t| t.as_str()) {
                    thread_id = Some(tid.to_string());
                }
            } else if event.get("type").and_then(|t| t.as_str()) == Some("item.completed")
                && let Some(item) = event.get("item")
                && item.get("type").and_then(|t| t.as_str()) == Some("agent_message")
                && let Some(text) = item.get("text").and_then(|t| t.as_str())
                && !text.is_empty()
            {
                messages.push(text.to_string());
            }
        }
        // Skip non-JSON lines
    }

    (thread_id, messages.join("\n"))
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
impl LlmExecutor for CodexCliExecutor {
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
        let message = append_files(prompt, file_paths);
        let full_prompt = if thread_id.is_some() {
            message.clone()
        } else {
            format!("{system_prompt}\n\n{message}")
        };

        let cfg = config();
        let mut args: Vec<String> = vec!["exec".to_string()];
        if thread_id.is_some() {
            args.push("resume".to_string());
        }
        args.extend(["--json".to_string(), "--skip-git-repo-check".to_string()]);
        if let Some(ref effort) = cfg.codex_reasoning_effort {
            args.push("-c".to_string());
            args.push(format!("model_reasoning_effort=\"{effort}\""));
        }

        // --add-dir is not supported by `codex exec resume`
        if thread_id.is_none() {
            let cwd = std::env::current_dir().unwrap_or_default();
            let mut extra_dirs: Vec<String> = Vec::new();
            if let Some(wt) = get_main_worktree_path() {
                extra_dirs.push(wt.to_string());
            }
            let resolved_paths: Option<Vec<PathBuf>> = file_paths.map(|fps| fps.to_vec());
            extra_dirs.extend(get_external_directories(resolved_paths.as_deref(), &cwd));
            for dir in &extra_dirs {
                args.push("--add-dir".to_string());
                args.push(dir.clone());
            }
        }

        args.push("-m".to_string());
        args.push(model.to_string());
        if let Some(tid) = thread_id {
            args.push(tid.to_string());
        }
        args.push(full_prompt);

        let result = run_cli("codex", &args).await?;

        if result.code == Some(0) {
            let (parsed_thread_id, response) = parse_codex_jsonl(&result.stdout);
            if response.is_empty() {
                anyhow::bail!("No agent_message found in Codex JSONL output");
            }
            Ok(ExecuteResult {
                response,
                usage: None,
                thread_id: parsed_thread_id,
            })
        } else {
            anyhow::bail!(
                "Codex CLI exited with code {}. Error: {}",
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
    fn test_parse_codex_jsonl_valid() {
        let output = r#"{"type":"thread.started","thread_id":"thread_abc123"}
{"type":"item.completed","item":{"type":"agent_message","text":"Hello world"}}
{"type":"item.completed","item":{"type":"agent_message","text":"Second message"}}
"#;
        let (tid, response) = parse_codex_jsonl(output);
        assert_eq!(tid, Some("thread_abc123".to_string()));
        assert_eq!(response, "Hello world\nSecond message");
    }

    #[test]
    fn test_parse_codex_jsonl_empty() {
        let (tid, response) = parse_codex_jsonl("");
        assert_eq!(tid, None);
        assert!(response.is_empty());
    }

    #[test]
    fn test_parse_codex_jsonl_non_json_lines() {
        let output =
            "ERROR: some log line\n{\"type\":\"thread.started\",\"thread_id\":\"t1\"}\nnot json\n";
        let (tid, response) = parse_codex_jsonl(output);
        assert_eq!(tid, Some("t1".to_string()));
        assert!(response.is_empty());
    }
}
