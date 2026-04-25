use std::path::{Path, PathBuf};

pub fn user_config_dir() -> Option<PathBuf> {
    user_config_dir_with_home(dirs::home_dir().as_deref())
}

fn user_config_dir_with_home(home: Option<&Path>) -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(".config")))
        .map(|p| p.join("consult-llm"))
}

pub fn legacy_config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".consult-llm"))
}

/// Resolve the user config file path for reading.
/// Returns the XDG path if it exists, otherwise falls back to the legacy path.
pub fn resolve_user_config() -> Option<PathBuf> {
    let new = user_config_dir()?.join("config.yaml");
    if new.exists() {
        return Some(new);
    }
    let old = legacy_config_dir()?.join("config.yaml");
    old.exists().then_some(old)
}

/// Resolve the user config file path using an explicit home directory.
/// Used by config discovery so tests can inject a temp home.
pub fn resolve_user_config_with_home(home: &Path) -> Option<PathBuf> {
    let new = home.join(".config").join("consult-llm").join("config.yaml");
    if new.exists() {
        return Some(new);
    }
    let old = home.join(".consult-llm").join("config.yaml");
    old.exists().then_some(old)
}

/// Get the path to write the user config file.
pub fn user_config_file() -> Option<PathBuf> {
    user_config_dir().map(|d| d.join("config.yaml"))
}

/// Resolve the system prompt file path for reading.
pub fn resolve_system_prompt() -> Option<PathBuf> {
    let new = user_config_dir()?.join("SYSTEM_PROMPT.md");
    if new.exists() {
        return Some(new);
    }
    let old = legacy_config_dir()?.join("SYSTEM_PROMPT.md");
    old.exists().then_some(old)
}

/// Get the path to write the system prompt file.
pub fn system_prompt_file() -> Option<PathBuf> {
    user_config_dir().map(|d| d.join("SYSTEM_PROMPT.md"))
}
