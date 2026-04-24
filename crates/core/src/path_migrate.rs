use std::path::PathBuf;

const LEGACY_ROOT_NAME: &str = "consult-llm-mcp";
const CURRENT_ROOT_NAME: &str = "consult-llm";

pub fn migrate_if_needed() {
    let migrated = migrate_pairs(&collect_pairs());
    if !migrated.is_empty() {
        eprintln!(
            "consult-llm: migrated legacy path(s) via symlink: {}",
            migrated
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn collect_pairs() -> Vec<(PathBuf, PathBuf)> {
    let state_home = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".local/state"));
    let home = dirs::home_dir().unwrap_or_default();

    collect_pairs_from(&state_home, &home)
}

fn collect_pairs_from(
    state_home: &std::path::Path,
    home: &std::path::Path,
) -> Vec<(PathBuf, PathBuf)> {
    vec![
        (
            state_home.join(LEGACY_ROOT_NAME),
            state_home.join(CURRENT_ROOT_NAME),
        ),
        (
            home.join(config_dir_name(LEGACY_ROOT_NAME)),
            home.join(config_dir_name(CURRENT_ROOT_NAME)),
        ),
    ]
}

fn config_dir_name(root_name: &str) -> String {
    format!(".{root_name}")
}

fn path_exists(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn migrate_pairs(pairs: &[(PathBuf, PathBuf)]) -> Vec<PathBuf> {
    let mut migrated = Vec::new();

    for (old, new) in pairs {
        if path_exists(new) || !old.exists() {
            continue;
        }
        if let Some(parent) = new.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        #[cfg(unix)]
        if std::os::unix::fs::symlink(old, new).is_ok() {
            migrated.push(new.clone());
        }
    }

    migrated
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_symlink_when_new_missing() {
        let tmp = TempDir::new().unwrap();
        let old = tmp.path().join(LEGACY_ROOT_NAME);
        let new = tmp.path().join(CURRENT_ROOT_NAME);
        std::fs::create_dir_all(&old).unwrap();

        let migrated = migrate_pairs(&[(old.clone(), new.clone())]);

        assert_eq!(migrated, vec![new.clone()]);
        assert!(new.is_symlink());
        assert_eq!(std::fs::read_link(&new).unwrap(), old);
    }

    #[test]
    fn noop_when_new_exists() {
        let tmp = TempDir::new().unwrap();
        let old = tmp.path().join(LEGACY_ROOT_NAME);
        let new = tmp.path().join(CURRENT_ROOT_NAME);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();

        let migrated = migrate_pairs(&[(old, new.clone())]);

        assert!(migrated.is_empty());
        assert!(new.exists());
        assert!(!new.is_symlink());
    }

    #[test]
    fn noop_when_old_missing() {
        let tmp = TempDir::new().unwrap();
        let old = tmp.path().join(LEGACY_ROOT_NAME);
        let new = tmp.path().join(CURRENT_ROOT_NAME);

        let migrated = migrate_pairs(&[(old, new.clone())]);

        assert!(migrated.is_empty());
        assert!(!path_exists(&new));
    }

    #[test]
    fn collects_new_state_and_config_roots() {
        let state_home = PathBuf::from("/tmp/state");
        let home = PathBuf::from("/tmp/home");

        let pairs = collect_pairs_from(&state_home, &home);

        assert_eq!(
            pairs,
            vec![
                (
                    PathBuf::from(format!("/tmp/state/{LEGACY_ROOT_NAME}")),
                    PathBuf::from(format!("/tmp/state/{CURRENT_ROOT_NAME}")),
                ),
                (
                    PathBuf::from(format!("/tmp/home/.{LEGACY_ROOT_NAME}")),
                    PathBuf::from(format!("/tmp/home/.{CURRENT_ROOT_NAME}")),
                ),
            ]
        );
    }
}
