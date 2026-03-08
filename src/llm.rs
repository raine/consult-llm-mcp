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
        let provider = Provider::from_model(model);
        let cache_key = match provider {
            Some(Provider::OpenAI) => format!("{model}-{:?}", cfg.openai_backend),
            Some(Provider::Gemini) => format!("{model}-{:?}", cfg.gemini_backend),
            _ => model.to_string(),
        };

        let mut cache = self.cache.lock().unwrap();
        if let Some(exec) = cache.get(&cache_key) {
            return Ok(exec.clone());
        }

        let executor: Arc<dyn LlmExecutor> = match provider {
            Some(Provider::OpenAI) => match cfg.openai_backend {
                Backend::CodexCli => Arc::new(CodexCliExecutor::new(
                    cfg.codex_reasoning_effort.clone(),
                )),
                Backend::CursorCli => Arc::new(CursorCliExecutor::new(
                    cfg.codex_reasoning_effort.clone(),
                )),
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
            },
            Some(Provider::DeepSeek) => {
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
            }
            Some(Provider::Gemini) => match cfg.gemini_backend {
                Backend::GeminiCli => Arc::new(GeminiCliExecutor::new()),
                Backend::CursorCli => Arc::new(CursorCliExecutor::new(
                    cfg.codex_reasoning_effort.clone(),
                )),
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
            },
            None => anyhow::bail!("Unable to determine LLM provider for model: {model}"),
        };

        cache.insert(cache_key, executor.clone());
        Ok(executor)
    }
}
