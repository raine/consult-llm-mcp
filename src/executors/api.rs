use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use consult_llm_core::monitoring::RunSpool;
use consult_llm_core::stream_events::ParsedStreamEvent;

use super::thread_store;
use super::types::{ExecuteResult, LlmExecutor, LlmExecutorCapabilities, Usage};
use crate::logger::log_to_file;

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

    async fn execute(
        &self,
        prompt: &str,
        model: &str,
        system_prompt: &str,
        file_paths: Option<&[PathBuf]>,
        thread_id: Option<&str>,
        spool: Arc<Mutex<RunSpool>>,
    ) -> anyhow::Result<ExecuteResult> {
        if let Some(fps) = file_paths
            && !fps.is_empty()
        {
            let msg = format!(
                "File paths were provided but are not supported by the API executor for model {model}. They will be ignored."
            );
            log_to_file(&format!("WARNING: {msg}"));
            eprintln!("Warning: {msg}");
        }

        // Resolve thread ID: use existing or generate new
        let is_new_thread = thread_id.is_none();
        let active_thread_id = match thread_id {
            Some(id) => id.to_string(),
            None => thread_store::generate_thread_id(),
        };

        // Load existing thread history
        let history = if thread_id.is_some() {
            match thread_store::load(&active_thread_id)? {
                Some(t) => t.turns,
                None => anyhow::bail!(
                    "Thread '{}' not found. It may have expired or never existed.",
                    active_thread_id
                ),
            }
        } else {
            Vec::new()
        };

        {
            let mut s = spool.lock().unwrap();
            s.stream_event(ParsedStreamEvent::SystemPrompt {
                text: system_prompt.to_string(),
            });
            s.stream_event(ParsedStreamEvent::Prompt {
                text: prompt.to_string(),
            });
        }

        let base = if self.base_url.ends_with('/') {
            self.base_url.clone()
        } else {
            format!("{}/", self.base_url)
        };
        let url = format!("{base}chat/completions");

        // Build messages: system prompt + history turns + current prompt
        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        }];
        for turn in &history {
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
            content: prompt.to_string(),
        });

        let request = ChatRequest {
            model: model.to_string(),
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

        {
            let mut s = spool.lock().unwrap();
            if let Some(ref thinking) = thinking {
                s.stream_event(ParsedStreamEvent::Thinking {
                    text: thinking.clone(),
                });
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

        // Persist turn to disk
        thread_store::append_turn(
            &active_thread_id,
            thread_store::StoredTurn {
                user_prompt: prompt.to_string(),
                assistant_response: response.clone(),
                model: model.to_string(),
                usage: usage.clone(),
            },
            is_new_thread,
        )?;

        Ok(ExecuteResult {
            response,
            usage,
            thread_id: Some(active_thread_id),
        })
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
