use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    usage: Option<ApiUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
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
    capabilities: LlmExecutorCapabilities,
}

impl ApiExecutor {
    pub fn new(client: reqwest::Client, api_key: String, base_url: Option<String>) -> Self {
        Self {
            client,
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1/".to_string()),
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

        let chat_resp: ChatResponse = resp.json().await?;
        let raw_content = chat_resp
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();
        let (thinking, response) = extract_think_tags(&raw_content);
        if response.is_empty() {
            anyhow::bail!("No response from the model via API");
        }

        let usage = chat_resp.usage.map(|u| {
            // Gemini thinking models report thinking tokens only in total_tokens,
            // not in completion_tokens. Derive the full output cost from total.
            let effective_completion = match u.total_tokens {
                Some(total) if total > u.prompt_tokens + u.completion_tokens => {
                    total - u.prompt_tokens
                }
                _ => u.completion_tokens,
            };
            Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: effective_completion,
            }
        });

        session.finish(&spool, prompt, model, response, thinking, usage)
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
