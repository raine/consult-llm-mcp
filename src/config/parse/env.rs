use super::super::migrate::migrate_prefixed_env;
use super::super::types::ConfigError;

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

pub fn resolve_api_idle_timeout(env: &impl Fn(&str) -> Option<String>) -> std::time::Duration {
    let secs = env("CONSULT_LLM_API_IDLE_TIMEOUT_SECS")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(300);
    std::time::Duration::from_secs(secs)
}

pub fn resolve_codex_reasoning_effort(
    env: &impl Fn(&str) -> Option<String>,
) -> Result<String, ConfigError> {
    let value = migrate_prefixed_env(
        env("CONSULT_LLM_CODEX_REASONING_EFFORT").as_deref(),
        env("CODEX_REASONING_EFFORT").as_deref(),
        "CODEX_REASONING_EFFORT",
        "CONSULT_LLM_CODEX_REASONING_EFFORT",
    )
    .unwrap_or_else(|| "high".to_string());
    let valid = ["none", "minimal", "low", "medium", "high", "xhigh"];
    if !valid.contains(&value.as_str()) {
        return Err(ConfigError::InvalidCodexReasoningEffort(value));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::super::parse_config;
    use super::super::test_helpers::env_from;
    use super::*;
    use crate::config::types::ConfigError;

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
    fn test_parse_config_invalid_codex_reasoning_effort() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_CODEX_REASONING_EFFORT", "extreme"),
        ]);
        let err = parse_config(env).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidCodexReasoningEffort(ref s) if s == "extreme"));
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
    fn test_parse_config_api_idle_timeout_valid() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_API_IDLE_TIMEOUT_SECS", "42"),
        ]);
        let (config, _) = parse_config(env).unwrap();
        assert_eq!(config.api_idle_timeout, std::time::Duration::from_secs(42));
    }

    #[test]
    fn test_parse_config_api_idle_timeout_defaults_when_absent() {
        let env = env_from(&[("OPENAI_API_KEY", "key")]);
        let (config, _) = parse_config(env).unwrap();
        assert_eq!(config.api_idle_timeout, std::time::Duration::from_secs(300));
    }

    #[test]
    fn test_parse_config_api_idle_timeout_defaults_when_invalid() {
        let env = env_from(&[
            ("OPENAI_API_KEY", "key"),
            ("CONSULT_LLM_API_IDLE_TIMEOUT_SECS", "not-a-number"),
        ]);
        let (config, _) = parse_config(env).unwrap();
        assert_eq!(config.api_idle_timeout, std::time::Duration::from_secs(300));
    }
}
