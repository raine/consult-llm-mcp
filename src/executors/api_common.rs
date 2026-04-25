use std::sync::Mutex;

use consult_llm_core::monitoring::RunSpool;
use consult_llm_core::stream_events::ParsedStreamEvent;

use super::thread_store;
use super::types::{ExecuteResult, Usage};
use crate::logger::log_to_file;

pub struct ApiChatSession {
    pub thread_id: String,
    pub is_new_thread: bool,
    pub history: Vec<thread_store::StoredTurn>,
}

impl ApiChatSession {
    pub fn new(thread_id: Option<String>) -> Self {
        let is_new_thread = thread_id.is_none();
        let active_thread_id = match thread_id.as_deref() {
            Some(id) => id.to_string(),
            None => thread_store::generate_thread_id(),
        };
        Self {
            thread_id: active_thread_id,
            is_new_thread,
            history: Vec::new(),
        }
    }

    pub fn load_history(&mut self) -> anyhow::Result<()> {
        match thread_store::load(&self.thread_id)? {
            Some(t) => {
                self.history = t.turns;
                Ok(())
            }
            None => anyhow::bail!(
                "Thread '{}' not found. It may have expired or never existed.",
                self.thread_id
            ),
        }
    }

    pub fn init(&mut self, spool: &Mutex<RunSpool>, system_prompt: &str, prompt: &str) {
        let mut s = spool.lock().unwrap();
        s.stream_event(ParsedStreamEvent::SystemPrompt {
            text: system_prompt.to_string(),
        });
        s.stream_event(ParsedStreamEvent::Prompt {
            text: prompt.to_string(),
        });
    }

    pub fn finish(
        &self,
        spool: &Mutex<RunSpool>,
        prompt: String,
        model: String,
        response: String,
        thinking: Option<String>,
        usage: Option<Usage>,
    ) -> anyhow::Result<ExecuteResult> {
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

        thread_store::append_turn(
            &self.thread_id,
            thread_store::StoredTurn {
                user_prompt: prompt,
                assistant_response: response.clone(),
                model,
                usage: usage.clone(),
            },
            self.is_new_thread,
        )?;

        Ok(ExecuteResult {
            response,
            usage,
            thread_id: Some(self.thread_id.clone()),
        })
    }
}

pub fn warn_unsupported_file_paths(model: &str, file_paths: Option<&Vec<std::path::PathBuf>>) {
    if let Some(fps) = file_paths
        && !fps.is_empty()
    {
        let msg = format!(
            "File paths were provided but are not supported by the API executor for model {model}. They will be ignored."
        );
        log_to_file(&format!("WARNING: {msg}"));
        eprintln!("Warning: {msg}");
    }
}
