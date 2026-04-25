use async_trait::async_trait;

use super::stream::{ParsedStreamEvent, StreamEvents};
use super::types::{ExecuteResult, ExecutionRequest, LlmExecutor, LlmExecutorCapabilities};
use super::{append_file_refs, build_extra_dir_args, run_cli_executor};
pub struct CodexCliExecutor {
    capabilities: LlmExecutorCapabilities,
    codex_reasoning_effort: String,
}

impl CodexCliExecutor {
    pub fn new(codex_reasoning_effort: String) -> Self {
        Self {
            capabilities: LlmExecutorCapabilities {
                is_cli: true,
                supports_threads: true,
                supports_file_refs: true,
            },
            codex_reasoning_effort,
        }
    }
}

pub fn parse_codex_line(line: &str) -> StreamEvents {
    use smallvec::smallvec;

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return smallvec![];
    }
    let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return smallvec![];
    };
    let event_type = event.get("type").and_then(|t| t.as_str());

    match event_type {
        Some("thread.started") => {
            if let Some(tid) = event.get("thread_id").and_then(|t| t.as_str()) {
                smallvec![ParsedStreamEvent::SessionStarted {
                    id: tid.to_string(),
                }]
            } else {
                smallvec![]
            }
        }
        Some("turn.started") => smallvec![ParsedStreamEvent::Thinking {
            text: String::new(),
        }],
        Some("item.started") => {
            if let Some(item) = event.get("item")
                && item.get("type").and_then(|t| t.as_str()) == Some("command_execution")
            {
                let cmd = item
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("command");
                smallvec![ParsedStreamEvent::ToolStarted {
                    call_id: item
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string(),
                    label: extract_shell_command(cmd),
                }]
            } else {
                smallvec![]
            }
        }
        Some("item.completed") => {
            if let Some(item) = event.get("item") {
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("command_execution") => {
                        let status = item.get("status").and_then(|s| s.as_str());
                        let success = status == Some("completed");
                        let error = if success {
                            None
                        } else {
                            status.map(|s| s.to_string())
                        };
                        smallvec![ParsedStreamEvent::ToolFinished {
                            call_id: item
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("")
                                .to_string(),
                            success,
                            error,
                        }]
                    }
                    Some("agent_message") => {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str())
                            && !text.is_empty()
                        {
                            smallvec![ParsedStreamEvent::AssistantText {
                                text: format!("{text}\n"),
                            }]
                        } else {
                            smallvec![]
                        }
                    }
                    _ => smallvec![],
                }
            } else {
                smallvec![]
            }
        }
        Some("turn.completed") => {
            if let Some(u) = event.get("usage") {
                let input = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let output = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
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

#[async_trait]
impl LlmExecutor for CodexCliExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "codex_cli"
    }

    fn reasoning_effort(&self, _model: &str) -> Option<&str> {
        Some(&self.codex_reasoning_effort)
    }

    async fn execute(&self, req: ExecutionRequest) -> anyhow::Result<ExecuteResult> {
        let ExecutionRequest {
            prompt,
            model,
            system_prompt,
            file_paths,
            thread_id,
            spool,
        } = req;
        let fps = file_paths.as_deref();
        let tid = thread_id.as_deref();

        let message = append_file_refs(&prompt, fps);
        let full_prompt = if tid.is_some() {
            message
        } else {
            format!("{system_prompt}\n\n{message}")
        };

        let mut args: Vec<String> = vec!["exec".to_string()];
        if tid.is_some() {
            args.push("resume".to_string());
        }
        args.extend(["--json".to_string(), "--skip-git-repo-check".to_string()]);
        args.push("-c".to_string());
        args.push(format!(
            "model_reasoning_effort=\"{}\"",
            self.codex_reasoning_effort
        ));

        // --add-dir is not supported by `codex exec resume`
        if tid.is_none() {
            args.extend(build_extra_dir_args(fps, "--add-dir"));
        }

        args.push("-m".to_string());
        args.push(model.clone());
        if let Some(t) = tid {
            args.push(t.to_string());
        }

        run_cli_executor(
            "codex",
            &args,
            &full_prompt,
            &prompt,
            &system_prompt,
            spool,
            parse_codex_line,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executors::stream::StreamReducer;

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
        let mut reducer = StreamReducer::new(
            std::sync::Arc::new(std::sync::Mutex::new(
                consult_llm_core::monitoring::RunSpool::disabled(),
            )),
            None,
            None,
        );
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
