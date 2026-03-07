use async_trait::async_trait;
use std::path::PathBuf;

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
    async fn execute(
        &self,
        prompt: &str,
        model: &str,
        system_prompt: &str,
        file_paths: Option<&[PathBuf]>,
        thread_id: Option<&str>,
        consultation_id: Option<&str>,
    ) -> anyhow::Result<ExecuteResult>;
}
