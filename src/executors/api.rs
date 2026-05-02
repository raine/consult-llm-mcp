use std::time::Duration;

use serde::Serialize;

use super::api_chat::ChatStreamHandler;
use super::api_common::{ApiChatSession, warn_unsupported_file_paths};
use super::api_transport::{StreamLabels, StreamRequest, run_stream};
use super::tag_splitter::TagSplitter;
use super::types::{ExecuteResult, ExecutionRequest, LlmExecutor, LlmExecutorCapabilities};
use crate::models::{OpenAiCompatRuntime, OpenAiExtraBody};

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

pub struct ApiExecutor {
    agent: ureq::Agent,
    api_key: String,
    base_url: String,
    idle_timeout: Duration,
    runtime: OpenAiCompatRuntime,
    capabilities: LlmExecutorCapabilities,
}

impl ApiExecutor {
    pub fn new(
        agent: ureq::Agent,
        api_key: String,
        base_url: Option<String>,
        idle_timeout: Duration,
        runtime: OpenAiCompatRuntime,
    ) -> Self {
        Self {
            agent,
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1/".to_string()),
            idle_timeout,
            runtime,
            capabilities: LlmExecutorCapabilities {
                is_cli: false,
                supports_threads: true,
                supports_file_refs: false,
            },
        }
    }
}

fn extra_body(runtime: OpenAiCompatRuntime) -> Option<serde_json::Value> {
    runtime
        .extra_body
        .map(|OpenAiExtraBody::GoogleThinkingConfig| {
            serde_json::json!({
                "google": {
                    "thinking_config": {
                        "thinking_level": "high",
                        "include_thoughts": true
                    }
                }
            })
        })
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

        let extra_body = extra_body(self.runtime);

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

        let splitter = self
            .runtime
            .think_tags
            .map(|tags| TagSplitter::new(tags.start, tags.end));
        let handler = ChatStreamHandler::new(splitter, &spool);

        let outcome = run_stream(
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
            handler,
        )?;

        session.commit_turn(prompt, model, outcome.response, outcome.usage)
    }
}
