use std::time::Duration;

use serde::Serialize;

use consult_llm_core::stream_events::ParsedStreamEvent;

use super::api_chat::{ChatStreamHandler, emit_segments};
use super::api_common::{ApiChatSession, warn_unsupported_file_paths};
use super::api_transport::{StreamLabels, StreamRequest, run_stream};
use super::tag_splitter::TagSplitter;
use super::types::{ExecuteResult, ExecutionRequest, LlmExecutor, LlmExecutorCapabilities};

const LABELS: StreamLabels = StreamLabels {
    request: "API request",
    stream: "API stream",
};

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra_body: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

/// Per-provider quirks of the OpenAI-compatible chat-completions stream.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Dialect {
    /// No tag-based thinking in content.
    Generic,
    /// Gemini wraps thought summaries in <thought>...</thought> in `delta.content`.
    Gemini,
    /// MiniMax M2 embeds <think>...</think> in `delta.content`.
    MiniMax,
}

fn detect_dialect(base_url: &str) -> Dialect {
    if base_url.contains("generativelanguage.googleapis.com") {
        Dialect::Gemini
    } else if base_url.contains("api.minimax.io") {
        Dialect::MiniMax
    } else {
        Dialect::Generic
    }
}

pub struct ApiExecutor {
    agent: ureq::Agent,
    api_key: String,
    base_url: String,
    idle_timeout: Duration,
    capabilities: LlmExecutorCapabilities,
}

impl ApiExecutor {
    pub fn new(
        agent: ureq::Agent,
        api_key: String,
        base_url: Option<String>,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            agent,
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1/".to_string()),
            idle_timeout,
            capabilities: LlmExecutorCapabilities {
                is_cli: false,
                supports_threads: true,
                supports_file_refs: false,
            },
        }
    }
}

impl LlmExecutor for ApiExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "api"
    }

    fn execute(&self, req: ExecutionRequest) -> anyhow::Result<ExecuteResult> {
        let ExecutionRequest {
            prompt,
            model,
            system_prompt,
            file_paths,
            thread_id,
            spool,
        } = req;

        warn_unsupported_file_paths(&model, file_paths.as_ref());

        let session = ApiChatSession::start(thread_id, &spool, &system_prompt, &prompt)?;

        let base = if self.base_url.ends_with('/') {
            self.base_url.clone()
        } else {
            format!("{}/", self.base_url)
        };
        let url = format!("{base}chat/completions");

        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt.clone(),
        }];
        for turn in session.history() {
            messages.push(ChatMessage {
                role: "user".to_string(),
                content: turn.user_prompt.clone(),
            });
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: turn.assistant_response.clone(),
            });
        }
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: prompt.clone(),
        });

        let dialect = detect_dialect(&self.base_url);
        let extra_body = match dialect {
            Dialect::Gemini => Some(serde_json::json!({
                "google": {
                    "thinking_config": {
                        "thinking_level": "high",
                        "include_thoughts": true
                    }
                }
            })),
            _ => None,
        };

        let request = ChatRequest {
            model: model.clone(),
            messages,
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
            extra_body,
        };
        let body = serde_json::to_vec(&request)?;

        let splitter = match dialect {
            Dialect::Gemini => Some(TagSplitter::new("<thought>", "</thought>")),
            Dialect::MiniMax => Some(TagSplitter::new("<think>", "</think>")),
            Dialect::Generic => None,
        };
        let mut handler = ChatStreamHandler::new(splitter, &spool);

        run_stream(
            StreamRequest {
                agent: &self.agent,
                url,
                headers: vec![
                    ("Authorization", format!("Bearer {}", &self.api_key)),
                    ("Content-Type", "application/json".to_string()),
                ],
                body,
                idle_timeout: self.idle_timeout,
                model: model.clone(),
                labels: LABELS,
            },
            &mut handler,
        )?;

        let ChatStreamHandler {
            splitter,
            mut raw_content,
            mut raw_thinking,
            usage,
            mut current_stage,
            finish_reason,
            ..
        } = handler;

        // Flush any tag-splitter remainder (e.g. content after the last tag).
        let unclosed_thought = splitter.as_ref().is_some_and(|s| s.in_thinking());
        if let Some(s) = splitter
            && let Some(seg) = s.flush()
        {
            emit_segments(
                vec![seg],
                &mut raw_content,
                &mut raw_thinking,
                &mut current_stage,
                &spool,
            );
        }

        // Recovery: if a `<think>`/`<thought>` tag opened but never closed,
        // every chunk was misclassified as Thinking and `raw_content` is
        // empty. Fall back to the streamed thinking text so the user gets
        // *something* rather than a "No response" error.
        if raw_content.trim().is_empty() && unclosed_thought && !raw_thinking.is_empty() {
            crate::logger::log_to_file(&format!(
                "WARNING: API response for {model} had unclosed thought tag; treating thinking as response"
            ));
            eprintln!("Warning: unclosed thought tag in stream; treating thinking as response");
            raw_content = std::mem::take(&mut raw_thinking);
        }

        let response = raw_content.trim().to_string();
        if response.is_empty() {
            anyhow::bail!("No response from the model via API");
        }

        match finish_reason.as_deref() {
            Some("length") => {
                crate::logger::log_to_file(&format!(
                    "WARNING: API response for {model} truncated by max_tokens"
                ));
                eprintln!("Warning: response truncated by max_tokens");
            }
            Some("content_filter") => {
                crate::logger::log_to_file(&format!(
                    "WARNING: API response for {model} stopped by content filter"
                ));
                eprintln!("Warning: response stopped by content filter");
            }
            _ => {}
        }

        {
            let mut s = spool.lock().unwrap();
            s.stream_event(ParsedStreamEvent::AssistantText {
                text: response.clone(),
            });
            if let Some(ref u) = usage {
                s.stream_event(ParsedStreamEvent::Usage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                });
            }
        }

        session.commit_turn(prompt, model, response, usage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_dialect_known_providers() {
        assert_eq!(
            detect_dialect("https://generativelanguage.googleapis.com/v1beta/openai/"),
            Dialect::Gemini
        );
        assert_eq!(
            detect_dialect("https://api.minimax.io/v1"),
            Dialect::MiniMax
        );
        assert_eq!(
            detect_dialect("https://api.openai.com/v1/"),
            Dialect::Generic
        );
        assert_eq!(detect_dialect("https://api.deepseek.com"), Dialect::Generic);
    }
}
