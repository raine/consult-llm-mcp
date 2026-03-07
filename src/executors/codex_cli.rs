use async_trait::async_trait;
use std::path::PathBuf;

use super::cli_runner::run_cli_streaming;
use super::stream::{ParsedStreamEvent, StreamReducer};
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

pub fn parse_codex_line(line: &str) -> Vec<ParsedStreamEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return vec![];
    };
    let event_type = event.get("type").and_then(|t| t.as_str());

    match event_type {
        Some("thread.started") => {
            if let Some(tid) = event.get("thread_id").and_then(|t| t.as_str()) {
                vec![ParsedStreamEvent::SessionStarted {
                    id: tid.to_string(),
                }]
            } else {
                vec![]
            }
        }
        Some("turn.started") => vec![ParsedStreamEvent::Thinking],
        Some("item.started") => {
            if let Some(item) = event.get("item")
                && item.get("type").and_then(|t| t.as_str()) == Some("command_execution")
            {
                let cmd = item
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("command");
                vec![ParsedStreamEvent::ToolStarted {
                    call_id: item
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string(),
                    label: extract_shell_command(cmd),
                }]
            } else {
                vec![]
            }
        }
        Some("item.completed") => {
            if let Some(item) = event.get("item") {
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("command_execution") => {
                        let success =
                            item.get("status").and_then(|s| s.as_str()) == Some("completed");
                        vec![ParsedStreamEvent::ToolFinished {
                            call_id: item
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("")
                                .to_string(),
                            success,
                        }]
                    }
                    Some("agent_message") => {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str())
                            && !text.is_empty()
                        {
                            vec![ParsedStreamEvent::AssistantText {
                                text: format!("{text}\n"),
                            }]
                        } else {
                            vec![]
                        }
                    }
                    _ => vec![],
                }
            } else {
                vec![]
            }
        }
        Some("turn.completed") => {
            if let Some(u) = event.get("usage") {
                let input = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let output = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
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

/// Extract the inner command from Codex's `/bin/zsh -lc "actual command"` wrapper.
fn extract_shell_command(cmd: &str) -> String {
    if let Some(start) = cmd.find("-lc") {
        let rest = &cmd[start + 3..].trim_start();
        rest.trim_start_matches('"')
            .trim_start_matches('\'')
            .trim_end_matches('"')
            .trim_end_matches('\'')
            .to_string()
    } else {
        cmd.to_string()
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

        let mut reducer = StreamReducer::new(consultation_id);
        let result = run_cli_streaming("codex", &args, |line| {
            reducer.process(parse_codex_line(line));
        })
        .await?;

        if result.code == Some(0) {
            let response = reducer.response.trim_end().to_string();
            if response.is_empty() {
                anyhow::bail!("No agent_message found in Codex JSONL output");
            }
            Ok(ExecuteResult {
                response,
                usage: reducer.usage,
                thread_id: reducer.thread_id,
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
    fn test_parse_codex_line_thread_started() {
        let events = parse_codex_line(r#"{"type":"thread.started","thread_id":"thread_abc123"}"#);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::SessionStarted { id } if id == "thread_abc123")
        );
    }

    #[test]
    fn test_parse_codex_line_agent_message() {
        let events = parse_codex_line(
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"Hello world"}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::AssistantText { text } if text == "Hello world\n")
        );
    }

    #[test]
    fn test_parse_codex_line_usage() {
        let events = parse_codex_line(
            r#"{"type":"turn.completed","usage":{"input_tokens":1000,"output_tokens":200}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ParsedStreamEvent::Usage {
                prompt_tokens: 1000,
                completion_tokens: 200
            }
        ));
    }

    #[test]
    fn test_parse_codex_line_empty() {
        assert!(parse_codex_line("").is_empty());
        assert!(parse_codex_line("  ").is_empty());
        assert!(parse_codex_line("not json").is_empty());
    }

    #[test]
    fn test_reducer_joins_messages() {
        let mut reducer = StreamReducer::new(None);
        reducer.process(parse_codex_line(
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"Hello"}}"#,
        ));
        reducer.process(parse_codex_line(
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"World"}}"#,
        ));
        assert_eq!(reducer.response.trim_end(), "Hello\nWorld");
    }

    #[test]
    fn test_extract_shell_command() {
        assert_eq!(
            extract_shell_command(r#"/bin/zsh -lc "wc -l src/server.rs""#),
            "wc -l src/server.rs"
        );
        assert_eq!(
            extract_shell_command(r#"/bin/zsh -lc 'rg --files src -g *.rs'"#),
            "rg --files src -g *.rs"
        );
        assert_eq!(extract_shell_command("echo hello"), "echo hello");
        assert_eq!(
            extract_shell_command(r#"/bin/zsh -lc "RUSTC_WRAPPER=sccache cargo check""#),
            "RUSTC_WRAPPER=sccache cargo check"
        );
    }
}
