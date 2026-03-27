use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use consult_llm_core::stream_events::ParsedStreamEvent;

use super::stream::SidecarWriter;
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
        consultation_id: Option<&str>,
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

        let mut sidecar = SidecarWriter::new(consultation_id);
        sidecar.write(&ParsedStreamEvent::SystemPrompt {
            text: system_prompt.to_string(),
        });
        sidecar.write(&ParsedStreamEvent::Prompt {
            text: prompt.to_string(),
        });
        sidecar.flush();

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
        let response = chat_resp
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("No response from the model via API"))?;

        let usage = chat_resp.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
        });

        sidecar.write(&ParsedStreamEvent::AssistantText {
            text: response.clone(),
        });
        if let Some(ref u) = usage {
            sidecar.write(&ParsedStreamEvent::Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
            });
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
