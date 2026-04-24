use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub default_model: Option<String>,
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
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpencodeBlock {
    pub default_provider: Option<String>,
}

impl ConfigFile {
    pub fn parse(yaml: &str) -> Result<Self, serde_yaml::Error> {
        if yaml.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_yaml::from_str(yaml)
    }

    pub fn to_env_map(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        if let Some(v) = &self.default_model {
            m.insert("CONSULT_LLM_DEFAULT_MODEL".into(), v.clone());
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
        // Emit both true and false so higher-precedence `false` can override lower-precedence `true`.
        if let Some(v) = self.no_update_check {
            m.insert(
                "CONSULT_LLM_NO_UPDATE_CHECK".into(),
                if v { "1".into() } else { "0".into() },
            );
        }

        let providers: [(&str, Option<&ProviderBlock>); 5] = [
            ("GEMINI", self.gemini.as_ref()),
            ("OPENAI", self.openai.as_ref()),
            ("ANTHROPIC", self.anthropic.as_ref()),
            ("DEEPSEEK", self.deepseek.as_ref()),
            ("MINIMAX", self.minimax.as_ref()),
        ];
        for (name, block) in providers {
            let Some(b) = block else { continue };
            if let Some(v) = &b.backend {
                m.insert(format!("CONSULT_LLM_{name}_BACKEND"), v.clone());
            }
            if let Some(v) = &b.opencode_provider {
                m.insert(format!("CONSULT_LLM_OPENCODE_{name}_PROVIDER"), v.clone());
            }
        }

        if let Some(b) = &self.openai
            && let Some(v) = &b.reasoning_effort
        {
            m.insert("CONSULT_LLM_CODEX_REASONING_EFFORT".into(), v.clone());
        }

        if let Some(oc) = &self.opencode
            && let Some(v) = &oc.default_provider
        {
            m.insert("CONSULT_LLM_OPENCODE_PROVIDER".into(), v.clone());
        }

        m
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
            allowed_models: Some(vec!["gemini-2.5-pro".into(), "gpt-5.2".into()]),
            extra_models: Some(vec!["custom-model".into()]),
            system_prompt_path: Some("/path/to/prompt.md".into()),
            no_update_check: Some(true),
            gemini: Some(ProviderBlock {
                backend: Some("gemini-cli".into()),
                opencode_provider: Some("google".into()),
                reasoning_effort: None,
            }),
            openai: Some(ProviderBlock {
                backend: Some("api".into()),
                opencode_provider: None,
                reasoning_effort: Some("high".into()),
            }),
            anthropic: None,
            deepseek: None,
            minimax: None,
            opencode: Some(OpencodeBlock {
                default_provider: Some("copilot".into()),
            }),
        };

        let m = cfg.to_env_map();
        assert_eq!(m["CONSULT_LLM_DEFAULT_MODEL"], "gemini");
        assert_eq!(m["CONSULT_LLM_ALLOWED_MODELS"], "gemini-2.5-pro,gpt-5.2");
        assert_eq!(m["CONSULT_LLM_EXTRA_MODELS"], "custom-model");
        assert_eq!(m["CONSULT_LLM_SYSTEM_PROMPT_PATH"], "/path/to/prompt.md");
        assert_eq!(m["CONSULT_LLM_NO_UPDATE_CHECK"], "1");
        assert_eq!(m["CONSULT_LLM_GEMINI_BACKEND"], "gemini-cli");
        assert_eq!(m["CONSULT_LLM_OPENCODE_GEMINI_PROVIDER"], "google");
        assert_eq!(m["CONSULT_LLM_OPENAI_BACKEND"], "api");
        assert_eq!(m["CONSULT_LLM_CODEX_REASONING_EFFORT"], "high");
        assert_eq!(m["CONSULT_LLM_OPENCODE_PROVIDER"], "copilot");
    }

    #[test]
    fn test_to_env_map_skips_unset_fields() {
        let m = ConfigFile::default().to_env_map();
        assert!(m.is_empty());
    }
}
