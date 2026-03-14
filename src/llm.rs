use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::config::{Backend, config};
use crate::executors::api::ApiExecutor;
use crate::executors::codex_cli::CodexCliExecutor;
use crate::executors::cursor_cli::CursorCliExecutor;
use crate::executors::gemini_cli::GeminiCliExecutor;
use crate::executors::types::LlmExecutor;
use crate::models::Provider;

pub struct ExecutorProvider {
    cache: Mutex<HashMap<String, Arc<dyn LlmExecutor>>>,
    http_client: reqwest::Client,
}

impl ExecutorProvider {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
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
                Arc::new(ApiExecutor::new(
                    self.http_client.clone(),
                    key.to_string(),
                    provider.api_base_url().map(|s| s.to_string()),
                ))
            }
            Backend::CodexCli => {
                Arc::new(CodexCliExecutor::new(cfg.codex_reasoning_effort.clone()))
            }
            Backend::GeminiCli => Arc::new(GeminiCliExecutor::new()),
            Backend::CursorCli => {
                Arc::new(CursorCliExecutor::new(cfg.codex_reasoning_effort.clone()))
            }
        };

        cache.insert(cache_key, executor.clone());
        Ok(executor)
    }
}
