use std::time::Duration;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use consult_llm_core::monitoring::ProgressStage;
use consult_llm_core::stream_events::ParsedStreamEvent;

use super::api_common::{ApiChatSession, warn_unsupported_file_paths};
use super::types::{ExecuteResult, ExecutionRequest, LlmExecutor, LlmExecutorCapabilities, Usage};

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
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Deserialize)]
struct ChatChunk {
    choices: Vec<ChatChunkChoice>,
    usage: Option<ApiUsage>,
}

#[derive(Deserialize)]
struct ChatChunkChoice {
    delta: ChatDelta,
}

#[derive(Deserialize, Default)]
struct ChatDelta {
    content: Option<String>,
}

#[derive(Deserialize)]
struct ApiUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    /// Includes prompt + completion + thinking tokens. Used to derive thinking
    /// token count for models (like Gemini) where `completion_tokens` excludes
    /// thinking tokens.
    total_tokens: Option<u64>,
}

pub struct ApiExecutor {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    idle_timeout: Duration,
    capabilities: LlmExecutorCapabilities,
}

impl ApiExecutor {
    pub fn new(
        client: reqwest::Client,
        api_key: String,
        base_url: Option<String>,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            client,
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

#[async_trait]
impl LlmExecutor for ApiExecutor {
    fn capabilities(&self) -> &LlmExecutorCapabilities {
        &self.capabilities
    }

    fn backend_name(&self) -> &'static str {
        "api"
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

        let request = ChatRequest {
            model: model.clone(),
            messages,
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("API request failed with status {status}: {body}");
        }

        let mut event_stream = resp.bytes_stream().eventsource();
        let mut raw_content = String::new();
        let mut usage: Option<Usage> = None;
        let mut saw_done = false;
        let mut responding = false;
        let idle_timeout = self.idle_timeout;
        let idle_secs = idle_timeout.as_secs();

        loop {
            match tokio::time::timeout(idle_timeout, event_stream.next()).await {
                Err(_) => {
                    anyhow::bail!("API stream idle timeout: no data from {model} for {idle_secs}s")
                }
                Ok(None) => break,
                Ok(Some(Err(e))) => anyhow::bail!("API stream error for {model}: {e}"),
                Ok(Some(Ok(event))) => {
                    if event.data == "[DONE]" {
                        saw_done = true;
                        break;
                    }
                    let Ok(chunk) = serde_json::from_str::<ChatChunk>(&event.data) else {
                        continue;
                    };
                    // Usage-only chunk (empty choices) from stream_options.include_usage
                    if chunk.choices.is_empty() {
                        if let Some(u) = chunk.usage {
                            usage = Some(effective_usage(u));
                        }
                        continue;
                    }
                    if let Some(text) = chunk
                        .choices
                        .first()
                        .and_then(|c| c.delta.content.as_deref())
                        && !text.is_empty()
                    {
                        if !responding {
                            spool.lock().unwrap().set_stage(ProgressStage::Responding);
                            responding = true;
                        }
                        raw_content.push_str(text);
                    }
                }
            }
        }

        if !saw_done {
            anyhow::bail!("API stream for {model} ended without [DONE] terminator");
        }

        let (thinking, response) = extract_think_tags(&raw_content);
        if response.is_empty() {
            anyhow::bail!("No response from the model via API");
        }

        {
            let mut s = spool.lock().unwrap();
            if let Some(ref t) = thinking {
                s.stream_event(ParsedStreamEvent::Thinking { text: t.clone() });
            }
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

fn effective_usage(u: ApiUsage) -> Usage {
    // Gemini thinking models report thinking tokens only in total_tokens,
    // not in completion_tokens. Derive the full output cost from total.
    let effective_completion = match u.total_tokens {
        Some(total) if total > u.prompt_tokens + u.completion_tokens => total - u.prompt_tokens,
        _ => u.completion_tokens,
    };
    Usage {
        prompt_tokens: u.prompt_tokens,
        completion_tokens: effective_completion,
    }
}

/// Extract `<think>...</think>` blocks from reasoning models (e.g. MiniMax M2.7)
/// that embed chain-of-thought in the content field.
/// Returns (thinking_content, stripped_response).
fn extract_think_tags(s: &str) -> (Option<String>, String) {
    let mut thinking = String::new();
    let mut result = s.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result.find("</think>") {
            let content_start = start + "<think>".len();
            thinking.push_str(result[content_start..end].trim());
            let end = end + "</think>".len();
            let end = if result.as_bytes().get(end) == Some(&b'\n') {
                end + 1
            } else {
                end
            };
            result.replace_range(start..end, "");
        } else {
            break;
        }
    }
    let thinking = if thinking.is_empty() {
        None
    } else {
        Some(thinking)
    };
    (thinking, result.trim().to_string())
}
