use std::env;
use std::sync::OnceLock;

use crate::logger::log_to_file;
use crate::models::ALL_MODELS;

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
}

pub struct Config {
    pub openai_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub deepseek_api_key: Option<String>,
    pub default_model: Option<String>,
    pub gemini_backend: Backend,
    pub openai_backend: Backend,
    pub codex_reasoning_effort: Option<String>,
    pub system_prompt_path: Option<String>,
    pub allowed_models: Vec<String>,
}

/// Single source of truth for model availability — drives both schema and validation
pub struct ModelRegistry {
    pub allowed_models: Vec<String>,
    pub fallback_model: String,
    pub default_model: Option<String>,
}

impl ModelRegistry {
    /// Resolve which model to use:
    /// - If model explicitly provided -> validate against allowed list
    /// - If not provided -> use config.default_model or fallback_model
    pub fn resolve_model(
        &self,
        requested: Option<&str>,
        explicitly_provided: bool,
    ) -> Result<String, crate::errors::AppError> {
        let model = if explicitly_provided {
            requested.unwrap_or(&self.fallback_model)
        } else {
            self.default_model
                .as_deref()
                .unwrap_or(&self.fallback_model)
        };

        if !self.allowed_models.iter().any(|m| m == model) {
            return Err(crate::errors::AppError::InvalidParams(format!(
                "Invalid model '{model}'. Allowed: {}",
                self.allowed_models.join(", ")
            )));
        }

        Ok(model.to_string())
    }
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
        .filter(|model| {
            if model.starts_with("gemini-") {
                providers.gemini_backend != Backend::Api || providers.gemini_api_key.is_some()
            } else if model.starts_with("gpt-") {
                providers.openai_backend != Backend::Api || providers.openai_api_key.is_some()
            } else if model.starts_with("deepseek-") {
                providers.deepseek_api_key.is_some()
            } else {
                true // Unknown prefix (user-added extras) — always include
            }
        })
        .cloned()
        .collect()
}

static CONFIG: OnceLock<Config> = OnceLock::new();
static REGISTRY: OnceLock<ModelRegistry> = OnceLock::new();

pub fn config() -> &'static Config {
    CONFIG.get().expect("config not initialized")
}

pub fn registry() -> &'static ModelRegistry {
    REGISTRY.get().expect("model registry not initialized")
}

/// Initialize config and model registry from environment variables.
/// Must be called before MCP server starts.
/// Exits process on fatal configuration errors.
pub fn init_config() {
    let resolved_gemini_backend = migrate_backend_env(
        env_non_empty("GEMINI_BACKEND").as_deref(),
        env_non_empty("GEMINI_MODE").as_deref(),
        "gemini-cli",
        "GEMINI_MODE",
        "GEMINI_BACKEND",
    );

    let resolved_openai_backend = migrate_backend_env(
        env_non_empty("OPENAI_BACKEND").as_deref(),
        env_non_empty("OPENAI_MODE").as_deref(),
        "codex-cli",
        "OPENAI_MODE",
        "OPENAI_BACKEND",
    );

    let catalog_models = build_model_catalog(
        ALL_MODELS,
        env_non_empty("CONSULT_LLM_EXTRA_MODELS").as_deref(),
        env_non_empty("CONSULT_LLM_ALLOWED_MODELS").as_deref(),
    );

    let gemini_backend = resolved_gemini_backend
        .as_deref()
        .and_then(Backend::from_str)
        .unwrap_or(Backend::Api);

    let openai_backend = resolved_openai_backend
        .as_deref()
        .and_then(Backend::from_str)
        .unwrap_or(Backend::Api);

    let openai_api_key = env_non_empty("OPENAI_API_KEY");
    let gemini_api_key = env_non_empty("GEMINI_API_KEY");
    let deepseek_api_key = env_non_empty("DEEPSEEK_API_KEY");

    let enabled_models = filter_by_availability(
        &catalog_models,
        &ProviderAvailability {
            gemini_api_key: gemini_api_key.clone(),
            gemini_backend: gemini_backend.clone(),
            openai_api_key: openai_api_key.clone(),
            openai_backend: openai_backend.clone(),
            deepseek_api_key: deepseek_api_key.clone(),
        },
    );

    if enabled_models.is_empty() {
        let msg = "Invalid environment variables:\n  No models available. Set API keys or configure CLI backends.";
        log_to_file(&format!("FATAL ERROR:\n{msg}"));
        eprintln!("❌ {msg}");
        std::process::exit(1);
    }

    // Validate backend strings
    if let Some(ref raw) = resolved_gemini_backend
        && Backend::from_str(raw).is_none()
    {
        let msg = format!(
            "Invalid environment variables:\n  geminiBackend: Invalid enum value. Expected 'api' | 'gemini-cli' | 'cursor-cli', received '{raw}'"
        );
        log_to_file(&format!("FATAL ERROR:\n{msg}"));
        eprintln!("❌ {msg}");
        std::process::exit(1);
    }
    if let Some(ref raw) = resolved_openai_backend
        && Backend::from_str(raw).is_none()
    {
        let msg = format!(
            "Invalid environment variables:\n  openaiBackend: Invalid enum value. Expected 'api' | 'codex-cli' | 'cursor-cli', received '{raw}'"
        );
        log_to_file(&format!("FATAL ERROR:\n{msg}"));
        eprintln!("❌ {msg}");
        std::process::exit(1);
    }

    // Validate default model if provided
    let default_model = env::var("CONSULT_LLM_DEFAULT_MODEL").ok();
    if let Some(ref dm) = default_model
        && !enabled_models.contains(dm)
    {
        let msg = format!(
            "Invalid environment variables:\n  defaultModel: Invalid enum value. Expected {}, received '{dm}'",
            enabled_models
                .iter()
                .map(|m| format!("'{m}'"))
                .collect::<Vec<_>>()
                .join(" | ")
        );
        log_to_file(&format!("FATAL ERROR:\n{msg}"));
        eprintln!("❌ {msg}");
        std::process::exit(1);
    }

    // Validate codex reasoning effort
    let codex_reasoning_effort = env_non_empty("CODEX_REASONING_EFFORT");
    if let Some(ref effort) = codex_reasoning_effort {
        let valid = ["none", "minimal", "low", "medium", "high", "xhigh"];
        if !valid.contains(&effort.as_str()) {
            let msg = format!(
                "Invalid environment variables:\n  codexReasoningEffort: Invalid enum value. Expected 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh', received '{effort}'"
            );
            log_to_file(&format!("FATAL ERROR:\n{msg}"));
            eprintln!("❌ {msg}");
            std::process::exit(1);
        }
    }

    let fallback_model = if enabled_models.contains(&"gpt-5.2".to_string()) {
        "gpt-5.2".to_string()
    } else {
        enabled_models[0].clone()
    };

    let _ = CONFIG.set(Config {
        openai_api_key,
        gemini_api_key,
        deepseek_api_key,
        default_model: default_model.clone(),
        gemini_backend,
        openai_backend,
        codex_reasoning_effort,
        system_prompt_path: env_non_empty("CONSULT_LLM_SYSTEM_PROMPT_PATH"),
        allowed_models: enabled_models.clone(),
    });

    let _ = REGISTRY.set(ModelRegistry {
        allowed_models: enabled_models,
        fallback_model,
        default_model,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
            },
        );
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_filter_by_availability_unknown_prefix() {
        let models = vec!["custom-model".into()];
        let result = filter_by_availability(
            &models,
            &ProviderAvailability {
                gemini_api_key: None,
                gemini_backend: Backend::Api,
                openai_api_key: None,
                openai_backend: Backend::Api,
                deepseek_api_key: None,
            },
        );
        assert_eq!(result, vec!["custom-model"]);
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
    fn test_model_registry_resolve_explicit() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.2".into(), "gemini-2.5-pro".into()],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        };
        assert_eq!(
            reg.resolve_model(Some("gemini-2.5-pro"), true).unwrap(),
            "gemini-2.5-pro"
        );
    }

    #[test]
    fn test_model_registry_resolve_default() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.2".into(), "gemini-2.5-pro".into()],
            fallback_model: "gpt-5.2".into(),
            default_model: Some("gemini-2.5-pro".into()),
        };
        assert_eq!(reg.resolve_model(None, false).unwrap(), "gemini-2.5-pro");
    }

    #[test]
    fn test_model_registry_resolve_fallback() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.2".into(), "gemini-2.5-pro".into()],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        };
        assert_eq!(reg.resolve_model(None, false).unwrap(), "gpt-5.2");
    }

    #[test]
    fn test_model_registry_resolve_invalid() {
        let reg = ModelRegistry {
            allowed_models: vec!["gpt-5.2".into()],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        };
        assert!(reg.resolve_model(Some("invalid"), true).is_err());
    }
}
