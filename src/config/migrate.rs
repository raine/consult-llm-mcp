use crate::logger::log_to_file;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
