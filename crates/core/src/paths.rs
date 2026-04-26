use std::path::PathBuf;

/// Resolve the XDG state base directory, with a temp-dir fallback when
/// neither `XDG_STATE_HOME` nor `HOME` is set.
///
/// The fallback exists because `dirs::home_dir().unwrap_or_default()` returns
/// an empty `PathBuf`, which `join(".local/state")` silently turns into a
/// CWD-relative `./.local/state` — scattering state into whatever directory
/// the process happened to be launched from.
pub fn state_home() -> PathBuf {
    if let Ok(v) = std::env::var("XDG_STATE_HOME")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".local").join("state");
    }
    std::env::temp_dir().join("consult-llm-state")
}
