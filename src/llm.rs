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
    agent: ureq::Agent,
    idle_timeout: std::time::Duration,
}

impl ExecutorProvider {
    pub fn new() -> Self {
        // Socket read-idle: ureq applies this as a per-read deadline (each
        // blocking read gets a fresh budget), so it's the right knob for
        // "the connection went silent" — heartbeat bytes count as liveness
        // and reset the timer naturally. Set per-request in the executors.
        let idle_timeout_secs: u64 = std::env::var("CONSULT_LLM_API_IDLE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);
        let idle_timeout = std::time::Duration::from_secs(idle_timeout_secs);

        // Absolute upper bound on a single request lifetime. Catches
        // pathological cases the per-read idle can't (e.g. a server that
        // trickles a single byte every <120s indefinitely).
        let total_secs: u64 = std::env::var("CONSULT_LLM_API_TOTAL_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30 * 60);

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(std::time::Duration::from_secs(30)))
            // Bound body upload (large prompts) and time-to-headers.
            // Without these a provider that accepts the connection but
            // never reads / never sends headers can hang `.send()` forever.
            .timeout_send_body(Some(std::time::Duration::from_secs(120)))
            .timeout_recv_response(Some(std::time::Duration::from_secs(60)))
            .timeout_global(Some(std::time::Duration::from_secs(total_secs)))
            .build()
            .into();

        Self {
            cache: Mutex::new(HashMap::new()),
            agent,
            idle_timeout,
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
                        self.agent.clone(),
                        key.to_string(),
                        base,
                        idle_timeout,
                    )),
                    ApiProtocol::AnthropicMessages => Arc::new(AnthropicApiExecutor::new(
                        self.agent.clone(),
                        key.to_string(),
                        base,
                        idle_timeout,
                    )),
                }
            }
            Backend::CodexCli => Arc::new(CodexCliExecutor::new(
                cfg.codex_reasoning_effort.clone(),
                cfg.codex_extra_args.clone(),
            )),
            Backend::GeminiCli => Arc::new(GeminiCliExecutor::new(cfg.gemini_extra_args.clone())),
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
