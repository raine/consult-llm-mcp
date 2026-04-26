use std::sync::Mutex;

use consult_llm_core::monitoring::RunSpool;
use consult_llm_core::stream_events::ParsedStreamEvent;

use super::thread_store;
use super::types::{ExecuteResult, Usage};
use crate::logger::log_to_file;

pub struct ApiChatSession {
    thread_id: String,
    is_new_thread: bool,
    history: Vec<thread_store::StoredTurn>,
}

impl ApiChatSession {
    fn new(thread_id: Option<String>) -> Self {
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

    fn load_history(&mut self) -> anyhow::Result<()> {
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

    /// Create a session, load history if resuming, and emit initial spool events.
    /// This is the single entry-point for starting an API chat lifecycle.
    pub fn start(
        thread_id: Option<String>,
        spool: &Mutex<RunSpool>,
        system_prompt: &str,
        prompt: &str,
    ) -> anyhow::Result<Self> {
        let mut session = Self::new(thread_id);
        if !session.is_new_thread {
            session.load_history()?;
        }

        let mut s = spool.lock().unwrap();
        s.stream_event(ParsedStreamEvent::SessionStarted {
            id: session.thread_id.clone(),
        });
        s.resolve_thread_id(session.thread_id.clone());
        s.stream_event(ParsedStreamEvent::SystemPrompt {
            text: system_prompt.to_string(),
        });
        s.stream_event(ParsedStreamEvent::Prompt {
            text: prompt.to_string(),
        });

        Ok(session)
    }

    pub fn history(&self) -> &[thread_store::StoredTurn] {
        &self.history
    }

    /// Persist a completed turn to thread storage and return the result.
    /// Spool events (AssistantText, Thinking, Usage) must be emitted by the
    /// caller before this — this method only handles persistence.
    pub fn commit_turn(
        &self,
        prompt: String,
        model: String,
        response: String,
        usage: Option<Usage>,
    ) -> anyhow::Result<ExecuteResult> {
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
