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

/// Migrate config files from legacy paths to the XDG config dir, then remove legacy dirs.
///
/// Safe to call every startup — skips files already present at the destination,
/// only deletes files that were successfully migrated, and removes a legacy dir
/// only if it is empty afterwards (`remove_dir` rather than `remove_dir_all`).
pub fn migrate_to_xdg_if_needed() {
    let Some(home) = dirs::home_dir() else {
        return;
    };

    let legacy = home.join(".consult-llm");
    let legacy_mcp = home.join(".consult-llm-mcp");

    if std::fs::symlink_metadata(&legacy).is_err() && !legacy_mcp.exists() {
        return;
    }

    let Some(xdg_dir) = user_config_dir_with_home(Some(&home)) else {
        return;
    };

    // For each known config file, copy from the first legacy path that has it (if the
    // XDG destination doesn't already exist), then track the source for cleanup.
    let filenames = ["config.yaml", "SYSTEM_PROMPT.md"];
    let mut migrated_sources: Vec<PathBuf> = Vec::new();

    for filename in &filenames {
        let dst = xdg_dir.join(filename);
        if dst.exists() {
            continue;
        }
        // ~/.consult-llm may be a symlink to ~/.consult-llm-mcp; try it first so we
        // don't double-count the same file when both entries point at the same data.
        let src = [legacy.join(filename), legacy_mcp.join(filename)]
            .into_iter()
            .find(|p| p.exists());
        let Some(src) = src else { continue };

        // Atomic copy: write to a temp path then rename so a crash mid-copy doesn't
        // leave a partial file that would be treated as authoritative on the next run.
        let tmp = dst.with_extension("tmp");
        let result = std::fs::create_dir_all(&xdg_dir)
            .and_then(|_| std::fs::copy(&src, &tmp).map(|_| ()))
            .and_then(|_| std::fs::rename(&tmp, &dst));

        match result {
            Ok(()) => migrated_sources.push(src),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                eprintln!(
                    "consult-llm: failed to migrate {} to {}: {e}",
                    src.display(),
                    dst.display()
                );
                return; // abort — don't delete any legacy data if a copy failed
            }
        }
    }

    // Remove only the specific files we migrated.
    for src in &migrated_sources {
        let _ = std::fs::remove_file(src);
    }

    // Remove legacy dirs only if they are now empty.
    // remove_dir fails with DirectoryNotEmpty, keeping unrelated user files intact.
    let mut removed = Vec::new();
    for path in [&legacy, &legacy_mcp] {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            continue;
        };
        let result = if meta.file_type().is_symlink() {
            std::fs::remove_file(path)
        } else {
            std::fs::remove_dir(path)
        };
        if result.is_ok() {
            removed.push(path.display().to_string());
        }
    }

    if !removed.is_empty() || !migrated_sources.is_empty() {
        eprintln!(
            "consult-llm: migrated config to {}{}",
            xdg_dir.display(),
            if removed.is_empty() {
                String::new()
            } else {
                format!(" (removed {})", removed.join(", "))
            }
        );
    }
}
