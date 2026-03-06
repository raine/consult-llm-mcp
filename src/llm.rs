use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::config::{Backend, config};
use crate::executors::api::ApiExecutor;
use crate::executors::codex_cli::CodexCliExecutor;
use crate::executors::cursor_cli::CursorCliExecutor;
use crate::executors::gemini_cli::GeminiCliExecutor;
use crate::executors::types::LlmExecutor;

pub struct ExecutorProvider {
    cache: Mutex<HashMap<String, Arc<dyn LlmExecutor>>>,
    http_client: reqwest::Client,
}

impl ExecutorProvider {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            http_client: reqwest::Client::new(),
        }
    }

    pub fn get_executor(&self, model: &str) -> anyhow::Result<Arc<dyn LlmExecutor>> {
        let cfg = config();
        let cache_key = if model.starts_with("gpt-") {
            format!("{model}-{:?}", cfg.openai_backend)
        } else if model.starts_with("gemini-") {
            format!("{model}-{:?}", cfg.gemini_backend)
        } else {
            model.to_string()
        };

        let mut cache = self.cache.lock().unwrap();
        if let Some(exec) = cache.get(&cache_key) {
            return Ok(exec.clone());
        }

        let executor: Arc<dyn LlmExecutor> = if model.starts_with("gpt-") {
            match cfg.openai_backend {
                Backend::CodexCli => Arc::new(CodexCliExecutor::new()),
                Backend::CursorCli => Arc::new(CursorCliExecutor::new()),
                Backend::Api => {
                    let key = cfg.openai_api_key.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "OPENAI_API_KEY environment variable is required for OpenAI models in API mode"
                        )
                    })?;
                    Arc::new(ApiExecutor::new(
                        self.http_client.clone(),
                        key.clone(),
                        None,
                    ))
                }
                _ => anyhow::bail!("Invalid backend for GPT model"),
            }
        } else if model.starts_with("deepseek-") {
            let key = cfg.deepseek_api_key.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "DEEPSEEK_API_KEY environment variable is required for DeepSeek models"
                )
            })?;
            Arc::new(ApiExecutor::new(
                self.http_client.clone(),
                key.clone(),
                Some("https://api.deepseek.com".to_string()),
            ))
        } else if model.starts_with("gemini-") {
            match cfg.gemini_backend {
                Backend::GeminiCli => Arc::new(GeminiCliExecutor::new()),
                Backend::CursorCli => Arc::new(CursorCliExecutor::new()),
                Backend::Api => {
                    let key = cfg.gemini_api_key.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "GEMINI_API_KEY environment variable is required for Gemini models in API mode"
                        )
                    })?;
                    Arc::new(ApiExecutor::new(
                        self.http_client.clone(),
                        key.clone(),
                        Some(
                            "https://generativelanguage.googleapis.com/v1beta/openai/".to_string(),
                        ),
                    ))
                }
                _ => anyhow::bail!("Invalid backend for Gemini model"),
            }
        } else {
            anyhow::bail!("Unable to determine LLM provider for model: {model}")
        };

        cache.insert(cache_key, executor.clone());
        Ok(executor)
    }
}
