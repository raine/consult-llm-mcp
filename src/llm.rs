use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::config::{Backend, config};
use crate::executors::anthropic_api::AnthropicApiExecutor;
use crate::executors::api::ApiExecutor;
use crate::executors::codex_cli::CodexCliExecutor;
use crate::executors::cursor_cli::CursorCliExecutor;
use crate::executors::gemini_cli::GeminiCliExecutor;
use crate::executors::opencode_cli::OpenCodeCliExecutor;
use crate::executors::types::LlmExecutor;
use crate::models::{ApiProtocol, Provider};

pub struct ExecutorProvider {
    cache: Mutex<HashMap<String, Arc<dyn LlmExecutor>>>,
    http_client: reqwest::Client,
    idle_timeout: std::time::Duration,
}

impl ExecutorProvider {
    pub fn new() -> Self {
        // Idle timeout: fires if no data is received for this long, resetting on each chunk.
        // Catches hung requests where the server accepts the connection but sends nothing.
        // Configurable via CONSULT_LLM_API_IDLE_TIMEOUT_SECS; defaults to 120s.
        let idle_timeout_secs: u64 = std::env::var("CONSULT_LLM_API_IDLE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);

        let idle_timeout = std::time::Duration::from_secs(idle_timeout_secs);

        Self {
            cache: Mutex::new(HashMap::new()),
            idle_timeout,
            http_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .read_timeout(idle_timeout)
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub fn get_executor(&self, model: &str) -> anyhow::Result<Arc<dyn LlmExecutor>> {
        let cfg = config();
        let provider = Provider::from_model(model).ok_or_else(|| {
            anyhow::anyhow!("Unable to determine LLM provider for model: {model}")
        })?;

        let backend = cfg.backend_for(provider);
        let cache_key = format!("{model}-{backend:?}");

        let mut cache = self.cache.lock().unwrap();
        if let Some(exec) = cache.get(&cache_key) {
            return Ok(exec.clone());
        }

        let executor: Arc<dyn LlmExecutor> = match backend {
            Backend::Api => {
                let key = cfg.api_key_for(provider).ok_or_else(|| {
                    anyhow::anyhow!("API key is required for {provider:?} models in API mode")
                })?;
                let base = provider.api_base_url().map(|s| s.to_string());
                let idle_timeout = self.idle_timeout;
                match provider.api_protocol() {
                    ApiProtocol::OpenAiCompat => Arc::new(ApiExecutor::new(
                        self.http_client.clone(),
                        key.to_string(),
                        base,
                        idle_timeout,
                    )),
                    ApiProtocol::AnthropicMessages => Arc::new(AnthropicApiExecutor::new(
                        self.http_client.clone(),
                        key.to_string(),
                        base,
                        idle_timeout,
                    )),
                }
            }
            Backend::CodexCli => {
                Arc::new(CodexCliExecutor::new(cfg.codex_reasoning_effort.clone()))
            }
            Backend::GeminiCli => Arc::new(GeminiCliExecutor::new()),
            Backend::CursorCli => {
                Arc::new(CursorCliExecutor::new(cfg.codex_reasoning_effort.clone()))
            }
            Backend::OpenCodeCli => {
                let prefix = cfg.opencode_provider_for(provider).to_string();
                Arc::new(OpenCodeCliExecutor::new(prefix))
            }
        };

        cache.insert(cache_key, executor.clone());
        Ok(executor)
    }
}
