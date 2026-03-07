use async_trait::async_trait;
use std::path::PathBuf;

use super::cli_runner::run_cli_streaming;
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities, Usage};
use crate::config::config;
use crate::external_dirs::get_external_directories;
use crate::git_worktree::get_main_worktree_path;
use crate::monitoring;

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

pub fn parse_codex_jsonl(output: &str) -> (Option<String>, String, Option<Usage>) {
    let mut thread_id = None;
    let mut messages = Vec::new();
    let mut usage = None;

    for line in output.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let event_type = event.get("type").and_then(|t| t.as_str());
            match event_type {
                Some("thread.started") => {
                    if let Some(tid) = event.get("thread_id").and_then(|t| t.as_str()) {
                        thread_id = Some(tid.to_string());
                    }
                }
                Some("item.completed") => {
                    if let Some(item) = event.get("item")
                        && item.get("type").and_then(|t| t.as_str()) == Some("agent_message")
                        && let Some(text) = item.get("text").and_then(|t| t.as_str())
                        && !text.is_empty()
                    {
                        messages.push(text.to_string());
                    }
                }
                Some("turn.completed") => {
                    if let Some(u) = event.get("usage") {
                        let input = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let output = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        usage = Some(Usage {
                            prompt_tokens: input,
                            completion_tokens: output,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    (thread_id, messages.join("\n"), usage)
}

fn emit_codex_progress(consultation_id: &str, line: &str) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }
    let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return;
    };
    let event_type = event.get("type").and_then(|t| t.as_str());

    let stage = match event_type {
        Some("turn.started") => Some(monitoring::ProgressStage::Thinking),
        Some("item.started") => {
            if let Some(item) = event.get("item")
                && item.get("type").and_then(|t| t.as_str()) == Some("command_execution")
            {
                let cmd = item
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("command");
                // Extract the actual command from `/bin/zsh -lc "..."` wrapper
                let tool = extract_shell_command(cmd);
                Some(monitoring::ProgressStage::ToolUse { tool })
            } else {
                None
            }
        }
        Some("item.completed") => {
            if let Some(item) = event.get("item") {
                let item_type = item.get("type").and_then(|t| t.as_str());
                match item_type {
                    Some("command_execution") => {
                        let cmd = item
                            .get("command")
                            .and_then(|c| c.as_str())
                            .unwrap_or("command");
                        let tool = extract_shell_command(cmd);
                        let success =
                            item.get("status").and_then(|s| s.as_str()) == Some("completed");
                        Some(monitoring::ProgressStage::ToolResult { tool, success })
                    }
                    Some("agent_message") => Some(monitoring::ProgressStage::Responding),
                    _ => None,
                }
            } else {
                None
            }
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

/// Extract meaningful command from Codex's `/bin/zsh -lc "actual command"` wrapper
fn extract_shell_command(cmd: &str) -> String {
    // Try to extract the inner command from /bin/zsh -lc "..."
    if let Some(start) = cmd.find("-lc") {
        let rest = &cmd[start + 3..].trim_start();
        // Strip surrounding quotes
        let inner = rest
            .trim_start_matches('"')
            .trim_start_matches('\'')
            .trim_end_matches('"')
            .trim_end_matches('\'');
        // Take first word or truncate long commands
        let short = inner.split_whitespace().next().unwrap_or(inner);
        if short.len() > 30 {
            format!("{}...", &short[..27])
        } else {
            short.to_string()
        }
    } else {
        let short = cmd.split_whitespace().last().unwrap_or(cmd);
        if short.len() > 30 {
            format!("{}...", &short[..27])
        } else {
            short.to_string()
        }
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
impl LlmExecutor for CodexCliExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "codex_cli"
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

        let cid = consultation_id.map(|s| s.to_string());
        let result = run_cli_streaming("codex", &args, move |line| {
            if let Some(ref cid) = cid {
                emit_codex_progress(cid, line);
            }
        })
        .await?;

        if result.code == Some(0) {
            let (parsed_thread_id, response, usage) = parse_codex_jsonl(&result.stdout);
            if response.is_empty() {
                anyhow::bail!("No agent_message found in Codex JSONL output");
            }
            Ok(ExecuteResult {
                response,
                usage,
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
{"type":"turn.completed","usage":{"input_tokens":1000,"output_tokens":200,"cached_input_tokens":500}}
"#;
        let (tid, response, usage) = parse_codex_jsonl(output);
        assert_eq!(tid, Some("thread_abc123".to_string()));
        assert_eq!(response, "Hello world\nSecond message");
        let usage = usage.unwrap();
        assert_eq!(usage.prompt_tokens, 1000);
        assert_eq!(usage.completion_tokens, 200);
    }

    #[test]
    fn test_parse_codex_jsonl_empty() {
        let (tid, response, usage) = parse_codex_jsonl("");
        assert_eq!(tid, None);
        assert!(response.is_empty());
        assert!(usage.is_none());
    }

    #[test]
    fn test_parse_codex_jsonl_non_json_lines() {
        let output =
            "ERROR: some log line\n{\"type\":\"thread.started\",\"thread_id\":\"t1\"}\nnot json\n";
        let (tid, response, _usage) = parse_codex_jsonl(output);
        assert_eq!(tid, Some("t1".to_string()));
        assert!(response.is_empty());
    }

    #[test]
    fn test_extract_shell_command() {
        assert_eq!(
            extract_shell_command(r#"/bin/zsh -lc "wc -l src/server.rs""#),
            "wc"
        );
        assert_eq!(
            extract_shell_command(r#"/bin/zsh -lc 'rg --files src -g *.rs'"#),
            "rg"
        );
        assert_eq!(extract_shell_command("echo hello"), "hello");
    }
}
