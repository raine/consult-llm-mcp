use async_trait::async_trait;
use consult_llm_core::monitoring::RunSpool;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct LlmExecutorCapabilities {
    pub is_cli: bool,
    pub supports_threads: bool,
    pub supports_file_refs: bool,
}

pub use consult_llm_core::stream_events::Usage;

#[derive(Debug, Clone)]
pub struct ExecuteResult {
    pub response: String,
    pub usage: Option<Usage>,
    pub thread_id: Option<String>,
}

#[async_trait]
pub trait LlmExecutor: Send + Sync {
    fn capabilities(&self) -> &LlmExecutorCapabilities;
    fn backend_name(&self) -> &'static str;
    fn reasoning_effort(&self, _model: &str) -> Option<&str> {
        None
    }
    async fn execute(
        &self,
        prompt: &str,
        model: &str,
        system_prompt: &str,
        file_paths: Option<&[PathBuf]>,
        thread_id: Option<&str>,
        spool: Arc<Mutex<RunSpool>>,
    ) -> anyhow::Result<ExecuteResult>;
}
