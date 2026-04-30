use serde::Deserialize;
use std::collections::HashMap;

use crate::models::PROVIDER_SPECS;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub default_model: Option<String>,
    pub default_models: Option<Vec<String>>,
    pub allowed_models: Option<Vec<String>>,
    pub extra_models: Option<Vec<String>>,
    pub system_prompt_path: Option<String>,
    pub no_update_check: Option<bool>,

    pub gemini: Option<ProviderBlock>,
    pub openai: Option<ProviderBlock>,
    pub anthropic: Option<ProviderBlock>,
    pub deepseek: Option<ProviderBlock>,
    pub minimax: Option<ProviderBlock>,

    pub opencode: Option<OpencodeBlock>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderBlock {
    pub backend: Option<String>,
    pub opencode_provider: Option<String>,
    pub reasoning_effort: Option<String>,
    pub api_key: Option<String>,
    /// Extra CLI args appended to the underlying CLI invocation (codex/gemini).
    /// Tokenized with shell-style quoting; only honored for providers whose
    /// active backend is `codex-cli` (openai) or `gemini-cli` (gemini).
    pub extra_args: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpencodeBlock {
    pub default_provider: Option<String>,
}

/// Whether API keys are permitted in this config file layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApiKeyPolicy {
    /// Keys are allowed (user config, project-local config).
    Allow,
    /// Keys are forbidden (committed project config).
    Forbid,
}

/// Error returned when a committed project config contains API keys.
#[derive(Debug)]
pub struct ApiKeyInProjectConfig {
    pub provider: &'static str,
}

impl std::fmt::Display for ApiKeyInProjectConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "API keys are not allowed in the shared project config (.consult-llm.yaml). \
             Move `{}.api_key` to .consult-llm.local.yaml or ~/.config/consult-llm/config.yaml.",
            self.provider
        )
    }
}

impl ConfigFile {
    pub fn parse(yaml: &str) -> Result<Self, serde_yaml::Error> {
        if yaml.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_yaml::from_str(yaml)
    }

    /// Convert the config file to an env-var map.
    ///
    /// `policy` controls whether `api_key` fields are permitted. Pass
    /// `ApiKeyPolicy::Forbid` for `.consult-llm.yaml` (committed project
    /// config) and `ApiKeyPolicy::Allow` for `.consult-llm.local.yaml` and
    /// user config.
    ///
    /// Empty `api_key` values are silently skipped (consistent with how the
    /// env-var layer treats empty environment variables).
    pub fn to_env_map(
        &self,
        policy: ApiKeyPolicy,
    ) -> Result<HashMap<String, String>, ApiKeyInProjectConfig> {
        let mut m = HashMap::new();
        if let Some(v) = &self.default_model {
            m.insert("CONSULT_LLM_DEFAULT_MODEL".into(), v.clone());
        }
        if let Some(v) = &self.default_models {
            m.insert("CONSULT_LLM_DEFAULT_MODELS".into(), v.join(","));
        }
        if let Some(v) = &self.allowed_models {
            m.insert("CONSULT_LLM_ALLOWED_MODELS".into(), v.join(","));
        }
        if let Some(v) = &self.extra_models {
            m.insert("CONSULT_LLM_EXTRA_MODELS".into(), v.join(","));
        }
        if let Some(v) = &self.system_prompt_path {
            m.insert("CONSULT_LLM_SYSTEM_PROMPT_PATH".into(), v.clone());
        }
        if let Some(v) = self.no_update_check {
            m.insert(
                "CONSULT_LLM_NO_UPDATE_CHECK".into(),
                if v { "1".into() } else { "0".into() },
            );
        }

        let blocks: [(&str, Option<&ProviderBlock>); 5] = [
            ("gemini", self.gemini.as_ref()),
            ("openai", self.openai.as_ref()),
            ("anthropic", self.anthropic.as_ref()),
            ("deepseek", self.deepseek.as_ref()),
            ("minimax", self.minimax.as_ref()),
        ];

        for (id, block) in blocks {
            let Some(b) = block else { continue };
            let spec = PROVIDER_SPECS
                .iter()
                .find(|s| s.id == id)
                .expect("every provider block id must have a ProviderSpec");

            if let Some(v) = &b.api_key {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    if policy == ApiKeyPolicy::Forbid {
                        return Err(ApiKeyInProjectConfig { provider: spec.id });
                    }
                    m.insert(spec.api_key_env.to_string(), trimmed.to_string());
                }
                // blank/whitespace-only: silently skip (treat as unset)
            }

            if let Some(v) = &b.backend {
                m.insert(spec.backend_env.to_string(), v.clone());
            }
            if let Some(v) = &b.opencode_provider {
                m.insert(spec.opencode_env.to_string(), v.clone());
            }
        }

        if let Some(b) = &self.openai
            && let Some(v) = &b.reasoning_effort
        {
            m.insert("CONSULT_LLM_CODEX_REASONING_EFFORT".into(), v.clone());
        }

        if let Some(b) = &self.openai
            && let Some(v) = &b.extra_args
        {
            m.insert("CONSULT_LLM_CODEX_EXTRA_ARGS".into(), v.clone());
        }
        if let Some(b) = &self.gemini
            && let Some(v) = &b.extra_args
        {
            m.insert("CONSULT_LLM_GEMINI_EXTRA_ARGS".into(), v.clone());
        }

        if let Some(oc) = &self.opencode
            && let Some(v) = &oc.default_provider
        {
            m.insert("CONSULT_LLM_OPENCODE_PROVIDER".into(), v.clone());
        }

        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rejects_unknown_top_level_keys() {
        let yaml = "unknown_key: value\n";
        assert!(ConfigFile::parse(yaml).is_err());
    }

    #[test]
    fn test_parse_rejects_unknown_provider_block_keys() {
        let yaml = "gemini:\n  unknown_field: value\n";
        assert!(ConfigFile::parse(yaml).is_err());
    }

    #[test]
    fn test_to_env_map_fully_populated() {
        let cfg = ConfigFile {
            default_model: Some("gemini".into()),
            default_models: Some(vec!["gemini".into(), "openai".into(), "openai".into()]),
            allowed_models: Some(vec!["gemini-2.5-pro".into(), "gpt-5.2".into()]),
            extra_models: Some(vec!["custom-model".into()]),
            system_prompt_path: Some("/path/to/prompt.md".into()),
            no_update_check: Some(true),
            gemini: Some(ProviderBlock {
                backend: Some("gemini-cli".into()),
                opencode_provider: Some("google".into()),
                reasoning_effort: None,
                api_key: None,
                extra_args: Some("--yolo".into()),
            }),
            openai: Some(ProviderBlock {
                backend: Some("api".into()),
                opencode_provider: None,
                reasoning_effort: Some("high".into()),
                api_key: None,
                extra_args: Some("--dangerously-bypass-approvals-and-sandbox".into()),
            }),
            anthropic: None,
            deepseek: None,
            minimax: None,
            opencode: Some(OpencodeBlock {
                default_provider: Some("copilot".into()),
            }),
        };

        let m = cfg.to_env_map(ApiKeyPolicy::Allow).unwrap();
        assert_eq!(m["CONSULT_LLM_DEFAULT_MODEL"], "gemini");
        assert_eq!(m["CONSULT_LLM_DEFAULT_MODELS"], "gemini,openai,openai");
        assert_eq!(m["CONSULT_LLM_ALLOWED_MODELS"], "gemini-2.5-pro,gpt-5.2");
        assert_eq!(m["CONSULT_LLM_EXTRA_MODELS"], "custom-model");
        assert_eq!(m["CONSULT_LLM_SYSTEM_PROMPT_PATH"], "/path/to/prompt.md");
        assert_eq!(m["CONSULT_LLM_NO_UPDATE_CHECK"], "1");
        assert_eq!(m["CONSULT_LLM_GEMINI_BACKEND"], "gemini-cli");
        assert_eq!(m["CONSULT_LLM_OPENCODE_GEMINI_PROVIDER"], "google");
        assert_eq!(m["CONSULT_LLM_OPENAI_BACKEND"], "api");
        assert_eq!(m["CONSULT_LLM_CODEX_REASONING_EFFORT"], "high");
        assert_eq!(
            m["CONSULT_LLM_CODEX_EXTRA_ARGS"],
            "--dangerously-bypass-approvals-and-sandbox"
        );
        assert_eq!(m["CONSULT_LLM_GEMINI_EXTRA_ARGS"], "--yolo");
        assert_eq!(m["CONSULT_LLM_OPENCODE_PROVIDER"], "copilot");
    }

    #[test]
    fn test_to_env_map_skips_unset_fields() {
        let m = ConfigFile::default()
            .to_env_map(ApiKeyPolicy::Allow)
            .unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn test_api_key_emitted_in_allow_layer() {
        let cfg = ConfigFile {
            openai: Some(ProviderBlock {
                api_key: Some("sk-test".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let m = cfg.to_env_map(ApiKeyPolicy::Allow).unwrap();
        assert_eq!(m["OPENAI_API_KEY"], "sk-test");
    }

    #[test]
    fn test_api_key_rejected_in_project_layer() {
        let cfg = ConfigFile {
            gemini: Some(ProviderBlock {
                api_key: Some("key-abc".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let err = cfg.to_env_map(ApiKeyPolicy::Forbid).unwrap_err();
        assert_eq!(err.provider, "gemini");
    }

    #[test]
    fn test_blank_api_key_silently_skipped() {
        for blank in &["", "   ", "\t"] {
            let cfg = ConfigFile {
                openai: Some(ProviderBlock {
                    api_key: Some((*blank).into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            // Not an error even with Forbid — blank key is not a secret
            let m = cfg.to_env_map(ApiKeyPolicy::Forbid).unwrap();
            assert!(
                !m.contains_key("OPENAI_API_KEY"),
                "blank {:?} should be skipped",
                blank
            );
            // Also skipped with Allow
            let m2 = cfg.to_env_map(ApiKeyPolicy::Allow).unwrap();
            assert!(
                !m2.contains_key("OPENAI_API_KEY"),
                "blank {:?} should be skipped",
                blank
            );
        }
    }

    #[test]
    fn test_api_key_is_trimmed() {
        let cfg = ConfigFile {
            openai: Some(ProviderBlock {
                api_key: Some("  sk-padded  ".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let m = cfg.to_env_map(ApiKeyPolicy::Allow).unwrap();
        assert_eq!(m["OPENAI_API_KEY"], "sk-padded");
    }

    #[test]
    fn test_api_key_canonical_env_names() {
        let make = |key: &str| ConfigFile {
            gemini: Some(ProviderBlock {
                api_key: Some(key.into()),
                ..Default::default()
            }),
            anthropic: Some(ProviderBlock {
                api_key: Some(key.into()),
                ..Default::default()
            }),
            deepseek: Some(ProviderBlock {
                api_key: Some(key.into()),
                ..Default::default()
            }),
            minimax: Some(ProviderBlock {
                api_key: Some(key.into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let m = make("x").to_env_map(ApiKeyPolicy::Allow).unwrap();
        assert!(m.contains_key("GEMINI_API_KEY"));
        assert!(m.contains_key("ANTHROPIC_API_KEY"));
        assert!(m.contains_key("DEEPSEEK_API_KEY"));
        assert!(m.contains_key("MINIMAX_API_KEY"));
    }
}
