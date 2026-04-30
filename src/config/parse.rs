use std::collections::HashMap;
use std::sync::Arc;

use crate::catalog::{ModelRegistry, build_model_catalog, resolve_selector};
use crate::logger::log_to_file;
use crate::models::{PROVIDER_SPECS, Provider, all_builtin_models};

use super::migrate::{migrate_backend_env, migrate_prefixed_env};
use super::types::{Backend, Config, ConfigError, ProviderRuntimeConfig};

/// Tokenize a shell-quoted extra-args string. Empty/whitespace-only returns an
/// empty vec; malformed quoting returns an error.
pub fn parse_extra_args(raw: Option<&str>, env_var: &str) -> Result<Vec<String>, ConfigError> {
    let Some(s) = raw else {
        return Ok(Vec::new());
    };
    if s.trim().is_empty() {
        return Ok(Vec::new());
    }
    shlex::split(s).ok_or_else(|| ConfigError::InvalidExtraArgs {
        env_var: env_var.to_string(),
        raw: s.to_string(),
        message: "could not tokenize value (unbalanced quotes?)".to_string(),
    })
}

pub fn filter_by_availability(
    models: &[String],
    providers: &HashMap<Provider, ProviderRuntimeConfig>,
) -> Vec<String> {
    models
        .iter()
        .filter(|model| match Provider::from_model(model) {
            Some(provider) => {
                let cfg = &providers[&provider];
                // CLI backends don't need API keys
                cfg.backend != Backend::Api || cfg.api_key.is_some()
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

/// Parse a single provider's runtime config from environment variables.
fn parse_provider_config(
    spec: &crate::models::ProviderSpec,
    env: &impl Fn(&str) -> Option<String>,
    opencode_global: &Option<String>,
) -> Result<ProviderRuntimeConfig, ConfigError> {
    // 1. Resolve backend string through migration chain
    let backend_raw = if let Some(legacy_env) = spec.legacy_backend_env {
        migrate_prefixed_env(
            env(spec.backend_env).as_deref(),
            env(legacy_env).as_deref(),
            legacy_env,
            spec.backend_env,
        )
    } else {
        env(spec.backend_env)
    };

    let resolved_backend_str = if let (Some(legacy_mode), Some(cli_value)) =
        (spec.legacy_mode_env, spec.cli_backend_value)
    {
        migrate_backend_env(
            backend_raw.as_deref(),
            env(legacy_mode).as_deref(),
            cli_value,
            legacy_mode,
            spec.backend_env,
        )
    } else {
        backend_raw
    };

    // 2. Validate backend string against provider's allowed values
    if let Some(ref raw) = resolved_backend_str
        && !spec.allowed_backends.contains(&raw.as_str())
    {
        return Err(ConfigError::InvalidBackend {
            env_var: spec.backend_env.to_string(),
            raw: raw.clone(),
            allowed: spec
                .allowed_backends
                .iter()
                .map(|s| s.to_string())
                .collect(),
        });
    }

    let backend = resolved_backend_str
        .as_deref()
        .and_then(Backend::from_str)
        .unwrap_or(Backend::Api);

    // 3. API key
    let api_key = env(spec.api_key_env);

    // 4. OpenCode provider prefix
    let opencode_provider = env(spec.opencode_env)
        .or_else(|| opencode_global.clone())
        .unwrap_or_else(|| spec.default_opencode_provider.to_string());

    Ok(ProviderRuntimeConfig {
        api_key,
        backend,
        opencode_provider,
    })
}

/// Pure config parsing: takes an env-lookup function, returns Config + ModelRegistry or an error.
/// Does not read real env vars, call process::exit, or set globals.
pub fn parse_config(
    env: impl Fn(&str) -> Option<String>,
) -> Result<(Config, Arc<ModelRegistry>), ConfigError> {
    let opencode_global = env("CONSULT_LLM_OPENCODE_PROVIDER");

    // Parse per-provider config via registry loop
    let mut providers = HashMap::new();
    for spec in PROVIDER_SPECS {
        let provider_config = parse_provider_config(spec, &env, &opencode_global)?;
        providers.insert(spec.provider, provider_config);
    }
    debug_assert_eq!(
        providers.len(),
        crate::models::ALL_PROVIDERS.len(),
        "PROVIDER_SPECS is out of sync with ALL_PROVIDERS"
    );

    let builtin = all_builtin_models();
    let catalog_models = build_model_catalog(
        &builtin,
        env("CONSULT_LLM_EXTRA_MODELS").as_deref(),
        env("CONSULT_LLM_ALLOWED_MODELS").as_deref(),
    );

    let enabled_models = filter_by_availability(&catalog_models, &providers);

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

    let default_models = env("CONSULT_LLM_DEFAULT_MODELS");
    let resolved_default_models = match &default_models {
        Some(raw) => {
            let items: Vec<String> = raw
                .split(',')
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect();
            if items.len() > 5 {
                return Err(ConfigError::TooManyDefaultModels { count: items.len() });
            }
            let mut resolved = Vec::with_capacity(items.len());
            for item in items {
                let model = resolve_selector(&item, &enabled_models).ok_or_else(|| {
                    ConfigError::InvalidDefaultModels {
                        model: item.clone(),
                        allowed: enabled_models.clone(),
                    }
                })?;
                resolved.push(model);
            }
            resolved
        }
        None => enabled_models.clone(),
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

    let codex_extra_args = parse_extra_args(
        env("CONSULT_LLM_CODEX_EXTRA_ARGS").as_deref(),
        "CONSULT_LLM_CODEX_EXTRA_ARGS",
    )?;
    let gemini_extra_args = parse_extra_args(
        env("CONSULT_LLM_GEMINI_EXTRA_ARGS").as_deref(),
        "CONSULT_LLM_GEMINI_EXTRA_ARGS",
    )?;

    let fallback_model = if enabled_models.contains(&"gpt-5.2".to_string()) {
        "gpt-5.2".to_string()
    } else {
        enabled_models[0].clone()
    };

    let config = Config {
        providers,
        default_model: resolved_default.clone(),
        default_models: resolved_default_models,
        codex_reasoning_effort,
        codex_extra_args,
        gemini_extra_args,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Provider;

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    fn make_providers(
        entries: &[(Provider, Option<&str>, Backend)],
    ) -> HashMap<Provider, ProviderRuntimeConfig> {
        entries
            .iter()
            .map(|(p, key, backend)| {
                (
                    *p,
                    ProviderRuntimeConfig {
                        api_key: key.map(|k| k.to_string()),
                        backend: backend.clone(),
                        opencode_provider: String::new(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn test_filter_by_availability_api_with_key() {
        let models = vec![
            "gemini-2.5-pro".into(),
            "gpt-5.2".into(),
            "deepseek-v4-pro".into(),
        ];
        let providers = make_providers(&[
            (Provider::Gemini, Some("key"), Backend::Api),
            (Provider::OpenAI, Some("key"), Backend::Api),
            (Provider::DeepSeek, Some("key"), Backend::Api),
            (Provider::MiniMax, None, Backend::Api),
        ]);
        let result = filter_by_availability(&models, &providers);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_filter_by_availability_api_without_key() {
        let models = vec![
            "gemini-2.5-pro".into(),
            "gpt-5.2".into(),
            "deepseek-v4-pro".into(),
        ];
        let providers = make_providers(&[
            (Provider::Gemini, None, Backend::Api),
            (Provider::OpenAI, None, Backend::Api),
            (Provider::DeepSeek, None, Backend::Api),
            (Provider::MiniMax, None, Backend::Api),
        ]);
        let result = filter_by_availability(&models, &providers);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_by_availability_cli_no_key_needed() {
        let models = vec!["gemini-2.5-pro".into(), "gpt-5.2".into()];
        let providers = make_providers(&[
            (Provider::Gemini, None, Backend::GeminiCli),
            (Provider::OpenAI, None, Backend::CodexCli),
            (Provider::DeepSeek, None, Backend::Api),
            (Provider::MiniMax, None, Backend::Api),
        ]);
        let result = filter_by_availability(&models, &providers);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_filter_by_availability_unknown_prefix_rejected() {
        let models = vec!["custom-model".into()];
        let providers = make_providers(&[
            (Provider::Gemini, None, Backend::Api),
            (Provider::OpenAI, None, Backend::Api),
            (Provider::DeepSeek, None, Backend::Api),
            (Provider::MiniMax, None, Backend::Api),
        ]);
        let result = filter_by_availability(&models, &providers);
        assert!(result.is_empty());
    }

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
        assert!(matches!(err, ConfigError::InvalidBackend { ref raw, .. } if raw == "invalid"));
    }

    #[test]
    fn test_parse_config_invalid_openai_backend() {
        let env = env_from(&[
            ("CONSULT_LLM_OPENAI_BACKEND", "nope"),
            ("OPENAI_API_KEY", "key"),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidBackend { ref raw, .. } if raw == "nope"));
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
        assert_eq!(config.default_model, Some("gpt-5.5".to_string()));
        assert_eq!(registry.default_model, Some("gpt-5.5".to_string()));
    }

    #[test]
    fn test_parse_config_default_models_preserve_duplicates() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("GEMINI_API_KEY", "key"),
            ("CONSULT_LLM_DEFAULT_MODELS", "openai,gemini,openai"),
        ]);
        let (config, _) = parse_config(env).unwrap();
        assert_eq!(
            config.default_models,
            vec!["gpt-5.5", "gemini-3.1-pro-preview", "gpt-5.5"]
        );
    }

    #[test]
    fn test_parse_config_default_models_cap_counts_duplicates() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            (
                "CONSULT_LLM_DEFAULT_MODELS",
                "openai,openai,openai,openai,openai,openai",
            ),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::TooManyDefaultModels { count: 6 }
        ));
    }

    #[test]
    fn test_parse_config_default_models_invalid_model() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_DEFAULT_MODELS", "openai,missing"),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(
            matches!(err, ConfigError::InvalidDefaultModels { ref model, .. } if model == "missing")
        );
    }

    #[test]
    fn test_parse_config_cli_backend_no_key() {
        let env = env_from(&[("CONSULT_LLM_GEMINI_BACKEND", "gemini-cli")]);
        let (config, _) = parse_config(env).unwrap();
        assert_eq!(
            config.providers[&Provider::Gemini].backend,
            Backend::GeminiCli
        );
        assert!(
            config
                .allowed_models
                .iter()
                .any(|m| m.starts_with("gemini"))
        );
    }

    #[test]
    fn test_parse_extra_args_empty() {
        assert!(parse_extra_args(None, "X").unwrap().is_empty());
        assert!(parse_extra_args(Some(""), "X").unwrap().is_empty());
        assert!(parse_extra_args(Some("   "), "X").unwrap().is_empty());
    }

    #[test]
    fn test_parse_extra_args_tokenizes() {
        let args = parse_extra_args(
            Some("--dangerously-bypass-approvals-and-sandbox -C /tmp"),
            "X",
        )
        .unwrap();
        assert_eq!(
            args,
            vec!["--dangerously-bypass-approvals-and-sandbox", "-C", "/tmp"]
        );
    }

    #[test]
    fn test_parse_extra_args_handles_quoted() {
        let args =
            parse_extra_args(Some(r#"-c 'sandbox_mode="danger-full-access"'"#), "X").unwrap();
        assert_eq!(args, vec!["-c", r#"sandbox_mode="danger-full-access""#]);
    }

    #[test]
    fn test_parse_extra_args_invalid_quoting() {
        let err = parse_extra_args(
            Some(r#"--foo "unterminated"#),
            "CONSULT_LLM_CODEX_EXTRA_ARGS",
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::InvalidExtraArgs { .. }));
    }

    #[test]
    fn test_parse_config_codex_extra_args() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            (
                "CONSULT_LLM_CODEX_EXTRA_ARGS",
                "--dangerously-bypass-approvals-and-sandbox",
            ),
            ("CONSULT_LLM_GEMINI_EXTRA_ARGS", "--yolo --foo bar"),
        ]);
        let (config, _) = parse_config(env).unwrap();
        assert_eq!(
            config.codex_extra_args,
            vec!["--dangerously-bypass-approvals-and-sandbox"]
        );
        assert_eq!(config.gemini_extra_args, vec!["--yolo", "--foo", "bar"]);
    }

    #[test]
    fn test_parse_config_invalid_extra_args() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_CODEX_EXTRA_ARGS", r#"--foo "unterminated"#),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidExtraArgs { .. }));
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

    #[test]
    fn test_parse_config_invalid_deepseek_backend() {
        let env = env_from(&[
            ("CONSULT_LLM_DEEPSEEK_BACKEND", "codex-cli"),
            ("DEEPSEEK_API_KEY", "key"),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidBackend { ref raw, .. } if raw == "codex-cli"));
    }

    #[test]
    fn test_parse_config_with_anthropic_key() {
        let env = env_from(&[("ANTHROPIC_API_KEY", "sk-ant-test")]);
        let (config, registry) = parse_config(env).unwrap();
        assert!(
            config
                .allowed_models
                .contains(&"claude-opus-4-7".to_string())
        );
        assert_eq!(config.providers[&Provider::Anthropic].backend, Backend::Api);
        assert_eq!(
            registry.resolve_model(Some("anthropic")).unwrap(),
            "claude-opus-4-7"
        );
    }

    #[test]
    fn test_parse_config_invalid_anthropic_backend() {
        let env = env_from(&[
            ("CONSULT_LLM_ANTHROPIC_BACKEND", "codex-cli"),
            ("ANTHROPIC_API_KEY", "key"),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidBackend { ref raw, .. } if raw == "codex-cli"));
    }

    #[test]
    fn test_provider_registry_completeness() {
        use crate::models::ALL_PROVIDERS;

        // Every provider in ALL_PROVIDERS must have a spec in PROVIDER_SPECS
        for provider in ALL_PROVIDERS {
            let spec = provider.spec();
            assert!(!spec.model_prefixes.is_empty());
            assert!(!spec.builtin_models.is_empty());
            assert!(!spec.allowed_backends.is_empty());
            assert!(!spec.id.is_empty());
        }

        // Every spec must correspond to a provider in ALL_PROVIDERS (no duplicates, no orphans)
        assert_eq!(PROVIDER_SPECS.len(), ALL_PROVIDERS.len());
        let mut seen = std::collections::HashSet::new();
        for spec in PROVIDER_SPECS {
            assert!(
                seen.insert(spec.provider),
                "Duplicate ProviderSpec for {:?}",
                spec.provider
            );
        }
    }

    #[test]
    fn test_all_builtin_models_order() {
        use crate::models::all_builtin_models;

        // Verify the model catalog order matches the original ALL_MODELS constant.
        // Order matters: enabled_models[0] is the fallback when gpt-5.2 is absent.
        let models = all_builtin_models();
        assert_eq!(
            models,
            vec![
                "gemini-2.5-pro",
                "gemini-3-pro-preview",
                "gemini-3.1-pro-preview",
                "deepseek-v4-pro",
                "gpt-5.2",
                "gpt-5.4",
                "gpt-5.5",
                "gpt-5.3-codex",
                "gpt-5.2-codex",
                "MiniMax-M2.7",
                "claude-opus-4-7",
            ]
        );
    }

    #[test]
    fn test_anthropic_provider_uses_messages_protocol() {
        assert_eq!(
            Provider::Anthropic.api_protocol(),
            crate::models::ApiProtocol::AnthropicMessages
        );
        for p in [
            Provider::OpenAI,
            Provider::Gemini,
            Provider::DeepSeek,
            Provider::MiniMax,
        ] {
            assert_eq!(p.api_protocol(), crate::models::ApiProtocol::OpenAiCompat);
        }
    }

    #[test]
    fn test_backend_as_str_roundtrip() {
        let backends = [
            Backend::Api,
            Backend::CodexCli,
            Backend::GeminiCli,
            Backend::CursorCli,
            Backend::OpenCodeCli,
        ];
        for b in &backends {
            assert_eq!(Backend::from_str(b.as_str()), Some(b.clone()));
        }
    }
}
