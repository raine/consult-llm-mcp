use async_trait::async_trait;
use std::path::PathBuf;

use super::stream::{ParsedStreamEvent, StreamEvents};
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities};
use super::{append_file_refs, run_cli_executor};

pub struct OpenCodeCliExecutor {
    capabilities: LlmExecutorCapabilities,
    /// OpenCode provider prefix (e.g. "minimax", "copilot", "google")
    provider_prefix: String,
}

impl OpenCodeCliExecutor {
    pub fn new(provider_prefix: String) -> Self {
        Self {
            capabilities: LlmExecutorCapabilities {
                is_cli: true,
                supports_threads: true,
                supports_file_refs: true,
            },
            provider_prefix,
        }
    }
}

pub fn parse_opencode_line(line: &str) -> StreamEvents {
    use smallvec::smallvec;

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return smallvec![];
    }
    let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return smallvec![];
    };

    match event.get("type").and_then(|t| t.as_str()) {
        Some("step_start") => {
            let mut events = smallvec![ParsedStreamEvent::Thinking {
                text: String::new(),
            }];
            if let Some(sid) = event.get("sessionID").and_then(|v| v.as_str()) {
                events.insert(
                    0,
                    ParsedStreamEvent::SessionStarted {
                        id: sid.to_string(),
                    },
                );
            }
            events
        }
        Some("text") => {
            if let Some(part) = event.get("part")
                && let Some(text) = part.get("text").and_then(|t| t.as_str())
            {
                smallvec![ParsedStreamEvent::AssistantText {
                    text: text.to_string(),
                }]
            } else {
                smallvec![]
            }
        }
        Some("step_finish") => {
            if let Some(part) = event.get("part")
                && let Some(tokens) = part.get("tokens")
            {
                let input = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
                let output = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
                smallvec![ParsedStreamEvent::Usage {
                    prompt_tokens: input,
                    completion_tokens: output,
                }]
            } else {
                smallvec![]
            }
        }
        Some("error") => {
            if let Some(err) = event.get("error") {
                let msg = err
                    .get("data")
                    .and_then(|d| d.get("message"))
                    .and_then(|m| m.as_str())
                    .or_else(|| err.get("name").and_then(|n| n.as_str()))
                    .unwrap_or("unknown error");
                smallvec![ParsedStreamEvent::AssistantText {
                    text: format!("Error: {msg}"),
                }]
            } else {
                smallvec![]
            }
        }
        _ => smallvec![],
    }
}

#[async_trait]
impl LlmExecutor for OpenCodeCliExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "opencode_cli"
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
        let message = append_file_refs(prompt, file_paths);
        let full_prompt = if thread_id.is_some() {
            message
        } else {
            format!("{system_prompt}\n\n{message}")
        };

        let opencode_model = format!("{}/{model}", self.provider_prefix);

        let mut args: Vec<String> = vec![
            "run".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--model".to_string(),
            opencode_model,
        ];

        if let Some(tid) = thread_id {
            args.push("--session".to_string());
            args.push(tid.to_string());
        }

        if let Some(fps) = file_paths
            && !fps.is_empty()
        {
            for fp in fps {
                args.push("--file".to_string());
                args.push(fp.display().to_string());
            }
        }

        run_cli_executor(
            "opencode",
            &args,
            &full_prompt,
            prompt,
            system_prompt,
            consultation_id,
            parse_opencode_line,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executors::stream::StreamReducer;

    #[test]
    fn test_parse_opencode_line_step_start() {
        let events = parse_opencode_line(
            r#"{"type":"step_start","timestamp":1234,"sessionID":"ses_abc123","part":{"type":"step-start"}}"#,
        );
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], ParsedStreamEvent::SessionStarted { id } if id == "ses_abc123")
        );
        assert!(matches!(&events[1], ParsedStreamEvent::Thinking { text } if text.is_empty()));
    }

    #[test]
    fn test_parse_opencode_line_text() {
        let events = parse_opencode_line(
            r#"{"type":"text","sessionID":"ses_abc","part":{"type":"text","text":"Hello world"}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::AssistantText { text } if text == "Hello world")
        );
    }

    #[test]
    fn test_parse_opencode_line_step_finish() {
        let events = parse_opencode_line(
            r#"{"type":"step_finish","sessionID":"ses_abc","part":{"type":"step-finish","reason":"stop","tokens":{"input":1000,"output":50,"reasoning":10}}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ParsedStreamEvent::Usage {
                prompt_tokens: 1000,
                completion_tokens: 50
            }
        ));
    }

    #[test]
    fn test_parse_opencode_line_error() {
        let events = parse_opencode_line(
            r#"{"type":"error","sessionID":"ses_abc","error":{"name":"ProviderAuthError","data":{"message":"API key missing"}}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ParsedStreamEvent::AssistantText { text } if text.contains("API key missing"))
        );
    }

    #[test]
    fn test_parse_opencode_line_empty() {
        assert!(parse_opencode_line("").is_empty());
        assert!(parse_opencode_line("  ").is_empty());
        assert!(parse_opencode_line("not json").is_empty());
    }

    #[test]
    fn test_reducer_full_sequence() {
        let mut reducer = StreamReducer::new(None, None, None);
        let lines = vec![
            r#"{"type":"step_start","sessionID":"ses_abc","part":{"type":"step-start"}}"#,
            r#"{"type":"text","sessionID":"ses_abc","part":{"type":"text","text":"4"}}"#,
            r#"{"type":"step_finish","sessionID":"ses_abc","part":{"type":"step-finish","reason":"stop","tokens":{"input":15000,"output":1,"reasoning":0}}}"#,
        ];
        for line in &lines {
            reducer.process(parse_opencode_line(line));
        }
        assert_eq!(reducer.thread_id, Some("ses_abc".to_string()));
        assert_eq!(reducer.response, "4");
        assert!(reducer.usage.is_some());
        let usage = reducer.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 15000);
        assert_eq!(usage.completion_tokens, 1);
    }
}
