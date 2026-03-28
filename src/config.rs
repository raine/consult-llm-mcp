use std::env;
use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::logger::log_to_file;
use crate::models::{ALL_MODELS, Provider, SELECTOR_PRIORITIES};

/// Read env var, treating empty strings as unset.
fn env_non_empty(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.is_empty())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    Api,
    CodexCli,
    GeminiCli,
    CursorCli,
}

impl Backend {
    fn from_str(s: &str) -> Option<Backend> {
        match s {
            "api" => Some(Backend::Api),
            "codex-cli" => Some(Backend::CodexCli),
            "gemini-cli" => Some(Backend::GeminiCli),
            "cursor-cli" => Some(Backend::CursorCli),
            _ => None,
        }
    }
}

pub struct ProviderAvailability {
    pub gemini_api_key: Option<String>,
    pub gemini_backend: Backend,
    pub openai_api_key: Option<String>,
    pub openai_backend: Backend,
    pub deepseek_api_key: Option<String>,
    pub minimax_api_key: Option<String>,
}

impl ProviderAvailability {
    pub fn backend_for(&self, provider: Provider) -> &Backend {
        match provider {
            Provider::OpenAI => &self.openai_backend,
            Provider::Gemini => &self.gemini_backend,
            Provider::DeepSeek => &Backend::Api,
            Provider::MiniMax => &Backend::Api,
        }
    }

    pub fn api_key_for(&self, provider: Provider) -> Option<&str> {
        match provider {
            Provider::OpenAI => self.openai_api_key.as_deref(),
            Provider::Gemini => self.gemini_api_key.as_deref(),
            Provider::DeepSeek => self.deepseek_api_key.as_deref(),
            Provider::MiniMax => self.minimax_api_key.as_deref(),
        }
    }
}

#[derive(Debug)]
pub struct Config {
    pub openai_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub deepseek_api_key: Option<String>,
    pub minimax_api_key: Option<String>,
    pub default_model: Option<String>,
    pub gemini_backend: Backend,
    pub openai_backend: Backend,
    pub codex_reasoning_effort: String,
    pub system_prompt_path: Option<String>,
    pub allowed_models: Vec<String>,
}

impl Config {
    /// Get the configured backend for a provider.
    pub fn backend_for(&self, provider: Provider) -> &Backend {
        match provider {
            Provider::OpenAI => &self.openai_backend,
            Provider::Gemini => &self.gemini_backend,
            Provider::DeepSeek => &Backend::Api,
            Provider::MiniMax => &Backend::Api,
        }
    }

    /// Get the API key for a provider (when using API backend).
    pub fn api_key_for(&self, provider: Provider) -> Option<&str> {
        match provider {
            Provider::OpenAI => self.openai_api_key.as_deref(),
            Provider::Gemini => self.gemini_api_key.as_deref(),
            Provider::DeepSeek => self.deepseek_api_key.as_deref(),
            Provider::MiniMax => self.minimax_api_key.as_deref(),
        }
    }
}

/// Single source of truth for model availability — drives both schema and validation
#[derive(Clone, Debug)]
pub struct ModelRegistry {
    pub allowed_models: Vec<String>,
    pub fallback_model: String,
    pub default_model: Option<String>,
}

impl ModelRegistry {
    /// Resolve which model to use:
    /// - If model provided -> resolve as exact ID or selector
    /// - If not provided -> use default_model or fallback_model
    pub fn resolve_model(
        &self,
        requested: Option<&str>,
    ) -> Result<String, crate::errors::AppError> {
        let target = match requested {
            Some(m) => m,
            None => {
                return Ok(self
                    .default_model
                    .as_deref()
                    .unwrap_or(&self.fallback_model)
                    .to_string());
            }
        };

        resolve_selector(target, &self.allowed_models).ok_or_else(|| {
            let selectors: Vec<&str> = SELECTOR_PRIORITIES.iter().map(|(s, _)| *s).collect();
            crate::errors::AppError::InvalidParams(format!(
                "Invalid model or selector '{target}'. Selectors: {}. Available models: {}",
                selectors.join(", "),
                self.allowed_models.join(", ")
            ))
        })
    }
}

/// Resolve a model string as either an exact model ID or an abstract selector.
/// Returns the concrete model ID if found in `allowed_models`, or None.
pub fn resolve_selector(target: &str, allowed_models: &[String]) -> Option<String> {
    // 1. Exact match
    if allowed_models.iter().any(|m| m == target) {
        return Some(target.to_string());
    }

    // 2. Selector match — first available model from priority list
    if let Some((_, priorities)) = SELECTOR_PRIORITIES.iter().find(|(s, _)| *s == target) {
        return priorities
            .iter()
            .find(|&&m| allowed_models.iter().any(|a| a == m))
            .map(|m| m.to_string());
    }

    None
}

#[derive(Debug)]
pub enum ConfigError {
    NoModelsAvailable,
    InvalidGeminiBackend(String),
    InvalidOpenaiBackend(String),
    InvalidDefaultModel { model: String, allowed: Vec<String> },
    InvalidCodexReasoningEffort(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::NoModelsAvailable => write!(
                f,
                "Invalid environment variables:\n  No models available. Set API keys or configure CLI backends."
            ),
            ConfigError::InvalidGeminiBackend(raw) => write!(
                f,
                "Invalid environment variables:\n  geminiBackend: Invalid enum value. Expected 'api' | 'gemini-cli' | 'cursor-cli', received '{raw}'"
            ),
            ConfigError::InvalidOpenaiBackend(raw) => write!(
                f,
                "Invalid environment variables:\n  openaiBackend: Invalid enum value. Expected 'api' | 'codex-cli' | 'cursor-cli', received '{raw}'"
            ),
            ConfigError::InvalidDefaultModel { model, allowed } => {
                let selectors: Vec<&str> = SELECTOR_PRIORITIES.iter().map(|(s, _)| *s).collect();
                let opts = allowed
                    .iter()
                    .map(|m| format!("'{m}'"))
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(
                    f,
                    "Invalid environment variables:\n  defaultModel: Invalid value '{model}'. Expected a selector ({}) or exact model ({opts})",
                    selectors.join(", ")
                )
            }
            ConfigError::InvalidCodexReasoningEffort(effort) => write!(
                f,
                "Invalid environment variables:\n  codexReasoningEffort: Invalid enum value. Expected 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh', received '{effort}'"
            ),
        }
    }
}

/// Prefer CONSULT_LLM_-prefixed env var, fall back to unprefixed with deprecation warning.
pub fn migrate_prefixed_env(
    prefixed: Option<&str>,
    unprefixed: Option<&str>,
    unprefixed_name: &str,
    prefixed_name: &str,
) -> Option<String> {
    if let Some(v) = prefixed {
        return Some(v.to_string());
    }
    if let Some(v) = unprefixed {
        log_to_file(&format!(
            "DEPRECATED: {unprefixed_name}={v} → use {prefixed_name}={v} instead"
        ));
        return Some(v.to_string());
    }
    None
}

pub fn migrate_backend_env(
    new_var: Option<&str>,
    old_var: Option<&str>,
    provider_cli_value: &str,
    legacy_name: &str,
    new_name: &str,
) -> Option<String> {
    if let Some(v) = new_var {
        return Some(v.to_string());
    }
    if let Some(v) = old_var {
        let mapped = if v == "cli" { provider_cli_value } else { v };
        log_to_file(&format!(
            "DEPRECATED: {legacy_name}={v} → use {new_name}={mapped} instead"
        ));
        return Some(mapped.to_string());
    }
    None
}

pub fn build_model_catalog(
    builtin: &[&str],
    extra_raw: Option<&str>,
    allowed_raw: Option<&str>,
) -> Vec<String> {
    let extra: Vec<String> = extra_raw
        .map(|s| {
            s.split(',')
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let mut all: Vec<String> = builtin.iter().map(|s| s.to_string()).collect();
    for m in extra {
        if !all.contains(&m) {
            all.push(m);
        }
    }

    let allowed: Vec<String> = allowed_raw
        .map(|s| {
            s.split(',')
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if allowed.is_empty() {
        all
    } else {
        all.into_iter().filter(|m| allowed.contains(m)).collect()
    }
}

pub fn filter_by_availability(models: &[String], providers: &ProviderAvailability) -> Vec<String> {
    models
        .iter()
        .filter(|model| match Provider::from_model(model) {
            Some(provider) => {
                let backend = providers.backend_for(provider);
                // CLI backends don't need API keys
                *backend != Backend::Api || providers.api_key_for(provider).is_some()
            }
            None => {
                log_to_file(&format!(
                    "WARNING: dropping model '{model}' — unrecognized provider prefix"
                ));
                false
            }
        })
        .cloned()
        .collect()
}

/// Pure config parsing: takes an env-lookup function, returns Config + ModelRegistry or an error.
/// Does not read real env vars, call process::exit, or set globals.
pub fn parse_config(
    env: impl Fn(&str) -> Option<String>,
) -> Result<(Config, Arc<ModelRegistry>), ConfigError> {
    // Priority: CONSULT_LLM_*_BACKEND > *_BACKEND > *_MODE (deprecated)
    let gemini_backend_raw = migrate_prefixed_env(
        env("CONSULT_LLM_GEMINI_BACKEND").as_deref(),
        env("GEMINI_BACKEND").as_deref(),
        "GEMINI_BACKEND",
        "CONSULT_LLM_GEMINI_BACKEND",
    );
    let resolved_gemini_backend = migrate_backend_env(
        gemini_backend_raw.as_deref(),
        env("GEMINI_MODE").as_deref(),
        "gemini-cli",
        "GEMINI_MODE",
        "CONSULT_LLM_GEMINI_BACKEND",
    );

    let openai_backend_raw = migrate_prefixed_env(
        env("CONSULT_LLM_OPENAI_BACKEND").as_deref(),
        env("OPENAI_BACKEND").as_deref(),
        "OPENAI_BACKEND",
        "CONSULT_LLM_OPENAI_BACKEND",
    );
    let resolved_openai_backend = migrate_backend_env(
        openai_backend_raw.as_deref(),
        env("OPENAI_MODE").as_deref(),
        "codex-cli",
        "OPENAI_MODE",
        "CONSULT_LLM_OPENAI_BACKEND",
    );

    let catalog_models = build_model_catalog(
        ALL_MODELS,
        env("CONSULT_LLM_EXTRA_MODELS").as_deref(),
        env("CONSULT_LLM_ALLOWED_MODELS").as_deref(),
    );

    // Validate backend strings against per-provider allowed values
    if let Some(ref raw) = resolved_gemini_backend
        && !matches!(raw.as_str(), "api" | "gemini-cli" | "cursor-cli")
    {
        return Err(ConfigError::InvalidGeminiBackend(raw.clone()));
    }
    if let Some(ref raw) = resolved_openai_backend
        && !matches!(raw.as_str(), "api" | "codex-cli" | "cursor-cli")
    {
        return Err(ConfigError::InvalidOpenaiBackend(raw.clone()));
    }

    let gemini_backend = resolved_gemini_backend
        .as_deref()
        .and_then(Backend::from_str)
        .unwrap_or(Backend::Api);

    let openai_backend = resolved_openai_backend
        .as_deref()
        .and_then(Backend::from_str)
        .unwrap_or(Backend::Api);

    let openai_api_key = env("OPENAI_API_KEY");
    let gemini_api_key = env("GEMINI_API_KEY");
    let deepseek_api_key = env("DEEPSEEK_API_KEY");
    let minimax_api_key = env("MINIMAX_API_KEY");

    let enabled_models = filter_by_availability(
        &catalog_models,
        &ProviderAvailability {
            gemini_api_key: gemini_api_key.clone(),
            gemini_backend: gemini_backend.clone(),
            openai_api_key: openai_api_key.clone(),
            openai_backend: openai_backend.clone(),
            deepseek_api_key: deepseek_api_key.clone(),
            minimax_api_key: minimax_api_key.clone(),
        },
    );

    if enabled_models.is_empty() {
        return Err(ConfigError::NoModelsAvailable);
    }

    // Validate and resolve default model (supports both exact IDs and selectors)
    let default_model = env("CONSULT_LLM_DEFAULT_MODEL");
    let resolved_default = match &default_model {
        Some(dm) => {
            let resolved = resolve_selector(dm, &enabled_models).ok_or_else(|| {
                ConfigError::InvalidDefaultModel {
                    model: dm.clone(),
                    allowed: enabled_models.clone(),
                }
            })?;
            Some(resolved)
        }
        None => None,
    };

    // Validate codex reasoning effort
    let codex_reasoning_effort = migrate_prefixed_env(
        env("CONSULT_LLM_CODEX_REASONING_EFFORT").as_deref(),
        env("CODEX_REASONING_EFFORT").as_deref(),
        "CODEX_REASONING_EFFORT",
        "CONSULT_LLM_CODEX_REASONING_EFFORT",
    )
    .unwrap_or_else(|| "high".to_string());
    let valid = ["none", "minimal", "low", "medium", "high", "xhigh"];
    if !valid.contains(&codex_reasoning_effort.as_str()) {
        return Err(ConfigError::InvalidCodexReasoningEffort(
            codex_reasoning_effort,
        ));
    }

    let fallback_model = if enabled_models.contains(&"gpt-5.2".to_string()) {
        "gpt-5.2".to_string()
    } else {
        enabled_models[0].clone()
    };

    let config = Config {
        openai_api_key,
        gemini_api_key,
        deepseek_api_key,
        minimax_api_key,
        default_model: resolved_default.clone(),
        gemini_backend,
        openai_backend,
        codex_reasoning_effort,
        system_prompt_path: env("CONSULT_LLM_SYSTEM_PROMPT_PATH"),
        allowed_models: enabled_models.clone(),
    };

    let registry = Arc::new(ModelRegistry {
        allowed_models: enabled_models,
        fallback_model,
        default_model: resolved_default,
    });

    Ok((config, registry))
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn config() -> &'static Config {
    CONFIG.get().expect("config not initialized")
}

/// Initialize config and model registry from environment variables.
/// Must be called before MCP server starts.
/// Returns the ModelRegistry for explicit dependency injection.
pub fn init_config() -> Result<Arc<ModelRegistry>, ConfigError> {
    let (config, registry) = parse_config(env_non_empty)?;

    let _ = CONFIG.set(config);

    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn test_build_model_catalog_builtin_only() {
        let result = build_model_catalog(&["a", "b", "c"], None, None);
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_build_model_catalog_with_extras() {
        let result = build_model_catalog(&["a", "b"], Some("c, d, a"), None);
        assert_eq!(result, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_build_model_catalog_with_allowlist() {
        let result = build_model_catalog(&["a", "b", "c"], None, Some("b, c"));
        assert_eq!(result, vec!["b", "c"]);
    }

    #[test]
    fn test_build_model_catalog_extras_and_allowlist() {
        let result = build_model_catalog(&["a", "b"], Some("c, d"), Some("b, c"));
        assert_eq!(result, vec!["b", "c"]);
    }

    #[test]
    fn test_filter_by_availability_api_with_key() {
        let models = vec![
            "gemini-2.5-pro".into(),
            "gpt-5.2".into(),
            "deepseek-reasoner".into(),
        ];
        let result = filter_by_availability(
            &models,
            &ProviderAvailability {
                gemini_api_key: Some("key".into()),
                gemini_backend: Backend::Api,
                openai_api_key: Some("key".into()),
                openai_backend: Backend::Api,
                deepseek_api_key: Some("key".into()),
                minimax_api_key: None,
            },
        );
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_filter_by_availability_api_without_key() {
        let models = vec![
            "gemini-2.5-pro".into(),
            "gpt-5.2".into(),
            "deepseek-reasoner".into(),
        ];
        let result = filter_by_availability(
            &models,
            &ProviderAvailability {
                gemini_api_key: None,
                gemini_backend: Backend::Api,
                openai_api_key: None,
                openai_backend: Backend::Api,
                deepseek_api_key: None,
                minimax_api_key: None,
            },
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_by_availability_cli_no_key_needed() {
        let models = vec!["gemini-2.5-pro".into(), "gpt-5.2".into()];
        let result = filter_by_availability(
            &models,
            &ProviderAvailability {
                gemini_api_key: None,
                gemini_backend: Backend::GeminiCli,
                openai_api_key: None,
                openai_backend: Backend::CodexCli,
                deepseek_api_key: None,
                minimax_api_key: None,
            },
        );
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_filter_by_availability_unknown_prefix_rejected() {
        let models = vec!["custom-model".into()];
        let result = filter_by_availability(
            &models,
            &ProviderAvailability {
                gemini_api_key: None,
                gemini_backend: Backend::Api,
                openai_api_key: None,
                openai_backend: Backend::Api,
                deepseek_api_key: None,
                minimax_api_key: None,
            },
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_migrate_prefixed_env_prefixed_value() {
        let result = migrate_prefixed_env(Some("codex-cli"), Some("api"), "OLD", "NEW");
        assert_eq!(result, Some("codex-cli".into()));
    }

    #[test]
    fn test_migrate_prefixed_env_fallback_unprefixed() {
        let result = migrate_prefixed_env(None, Some("gemini-cli"), "OLD", "NEW");
        assert_eq!(result, Some("gemini-cli".into()));
    }

    #[test]
    fn test_migrate_prefixed_env_both_missing() {
        let result = migrate_prefixed_env(None, None, "OLD", "NEW");
        assert_eq!(result, None);
    }

    #[test]
    fn test_migrate_backend_env_new_var() {
        let result = migrate_backend_env(Some("codex-cli"), Some("cli"), "codex-cli", "OLD", "NEW");
        assert_eq!(result, Some("codex-cli".into()));
    }

    #[test]
    fn test_migrate_backend_env_old_var_cli() {
        let result = migrate_backend_env(None, Some("cli"), "codex-cli", "OLD", "NEW");
        assert_eq!(result, Some("codex-cli".into()));
    }

    #[test]
    fn test_migrate_backend_env_old_var_other() {
        let result = migrate_backend_env(None, Some("api"), "codex-cli", "OLD", "NEW");
        assert_eq!(result, Some("api".into()));
    }

    #[test]
    fn test_migrate_backend_env_none() {
        let result = migrate_backend_env(None, None, "codex-cli", "OLD", "NEW");
        assert_eq!(result, None);
    }

    #[test]
    fn test_model_registry_resolve_exact() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.2".into(), "gemini-2.5-pro".into()],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        };
        assert_eq!(
            reg.resolve_model(Some("gemini-2.5-pro")).unwrap(),
            "gemini-2.5-pro"
        );
    }

    #[test]
    fn test_model_registry_resolve_selector() {
        let reg = ModelRegistry {
            allowed_models: vec![
                "gpt-5.2".into(),
                "gemini-3.1-pro-preview".into(),
                "gemini-2.5-pro".into(),
            ],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        };
        // "gemini" selector should resolve to best available
        assert_eq!(
            reg.resolve_model(Some("gemini")).unwrap(),
            "gemini-3.1-pro-preview"
        );
    }

    #[test]
    fn test_model_registry_resolve_selector_skips_unavailable() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.2".into(), "gemini-2.5-pro".into()],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        };
        // "gemini" selector should skip gemini-3.1-pro-preview (not in allowed) and pick gemini-2.5-pro
        assert_eq!(reg.resolve_model(Some("gemini")).unwrap(), "gemini-2.5-pro");
    }

    #[test]
    fn test_model_registry_resolve_openai_selector() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.4".into(), "gpt-5.3-codex".into(), "gpt-5.2".into()],
            fallback_model: "gpt-5.4".into(),
            default_model: None,
        };
        // "openai" selector should resolve to highest priority: gpt-5.4
        assert_eq!(reg.resolve_model(Some("openai")).unwrap(), "gpt-5.4");
    }

    #[test]
    fn test_model_registry_resolve_openai_selector_falls_to_codex() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.3-codex".into(), "gpt-5.2-codex".into()],
            fallback_model: "gpt-5.3-codex".into(),
            default_model: None,
        };
        // When only codex models available, "openai" should still resolve
        assert_eq!(reg.resolve_model(Some("openai")).unwrap(), "gpt-5.3-codex");
    }

    #[test]
    fn test_model_registry_resolve_default() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.2".into(), "gemini-2.5-pro".into()],
            fallback_model: "gpt-5.2".into(),
            default_model: Some("gemini-2.5-pro".into()),
        };
        assert_eq!(reg.resolve_model(None).unwrap(), "gemini-2.5-pro");
    }

    #[test]
    fn test_model_registry_resolve_fallback() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.2".into(), "gemini-2.5-pro".into()],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        };
        assert_eq!(reg.resolve_model(None).unwrap(), "gpt-5.2");
    }

    #[test]
    fn test_model_registry_resolve_invalid() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.2".into()],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        };
        assert!(reg.resolve_model(Some("invalid")).is_err());
    }

    #[test]
    fn test_resolve_selector_exact_match() {
        let allowed = vec!["gpt-5.2".into(), "gemini-2.5-pro".into()];
        assert_eq!(
            resolve_selector("gpt-5.2", &allowed),
            Some("gpt-5.2".into())
        );
    }

    #[test]
    fn test_resolve_selector_selector_match() {
        let allowed = vec!["gemini-3.1-pro-preview".into(), "gemini-2.5-pro".into()];
        assert_eq!(
            resolve_selector("gemini", &allowed),
            Some("gemini-3.1-pro-preview".into())
        );
    }

    #[test]
    fn test_resolve_selector_no_match() {
        let allowed = vec!["gpt-5.2".into()];
        assert_eq!(resolve_selector("gemini", &allowed), None);
    }

    // --- parse_config tests ---

    #[test]
    fn test_parse_config_with_api_keys() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "sk-test"),
            ("GEMINI_API_KEY", "gem-test"),
        ]);
        let (config, registry) = parse_config(env).unwrap();
        assert!(config.allowed_models.contains(&"gpt-5.2".to_string()));
        assert!(
            config
                .allowed_models
                .contains(&"gemini-2.5-pro".to_string())
        );
        assert_eq!(registry.fallback_model, "gpt-5.2");
    }

    #[test]
    fn test_parse_config_no_models_available() {
        let env = env_from(&[]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(err, ConfigError::NoModelsAvailable));
    }

    #[test]
    fn test_parse_config_invalid_gemini_backend() {
        let env = env_from(&[
            ("CONSULT_LLM_GEMINI_BACKEND", "invalid"),
            ("GEMINI_API_KEY", "key"),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidGeminiBackend(ref s) if s == "invalid"));
    }

    #[test]
    fn test_parse_config_invalid_openai_backend() {
        let env = env_from(&[
            ("CONSULT_LLM_OPENAI_BACKEND", "nope"),
            ("OPENAI_API_KEY", "key"),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidOpenaiBackend(ref s) if s == "nope"));
    }

    #[test]
    fn test_parse_config_invalid_default_model() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_DEFAULT_MODEL", "nonexistent"),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(
            matches!(err, ConfigError::InvalidDefaultModel { ref model, .. } if model == "nonexistent")
        );
    }

    #[test]
    fn test_parse_config_invalid_codex_reasoning_effort() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_CODEX_REASONING_EFFORT", "extreme"),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidCodexReasoningEffort(ref s) if s == "extreme"));
    }

    #[test]
    fn test_parse_config_valid_default_model() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_DEFAULT_MODEL", "gpt-5.2"),
        ]);
        let (config, registry) = parse_config(env).unwrap();
        assert_eq!(config.default_model, Some("gpt-5.2".to_string()));
        assert_eq!(registry.default_model, Some("gpt-5.2".to_string()));
    }

    #[test]
    fn test_parse_config_selector_default_model() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_DEFAULT_MODEL", "openai"),
        ]);
        let (config, registry) = parse_config(env).unwrap();
        // Selector should be resolved to concrete model at startup
        assert_eq!(config.default_model, Some("gpt-5.4".to_string()));
        assert_eq!(registry.default_model, Some("gpt-5.4".to_string()));
    }

    #[test]
    fn test_parse_config_cli_backend_no_key() {
        let env = env_from(&[("CONSULT_LLM_GEMINI_BACKEND", "gemini-cli")]);
        let (config, _) = parse_config(env).unwrap();
        assert_eq!(config.gemini_backend, Backend::GeminiCli);
        assert!(
            config
                .allowed_models
                .iter()
                .any(|m| m.starts_with("gemini"))
        );
    }

    #[test]
    fn test_parse_config_codex_reasoning_effort_valid() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_CODEX_REASONING_EFFORT", "high"),
        ]);
        let (config, _) = parse_config(env).unwrap();
        assert_eq!(config.codex_reasoning_effort, "high");
    }

    #[test]
    fn test_parse_config_system_prompt_path() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_SYSTEM_PROMPT_PATH", "/tmp/prompt.txt"),
        ]);
        let (config, _) = parse_config(env).unwrap();
        assert_eq!(
            config.system_prompt_path,
            Some("/tmp/prompt.txt".to_string())
        );
    }

    #[test]
    fn test_parse_config_fallback_when_no_gpt52() {
        let env = env_from(&[
            ("GEMINI_API_KEY", "key"),
            ("CONSULT_LLM_ALLOWED_MODELS", "gemini-2.5-pro"),
        ]);
        let (_, registry) = parse_config(env).unwrap();
        assert_eq!(registry.fallback_model, "gemini-2.5-pro");
    }
}
