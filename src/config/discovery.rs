use std::path::{Path, PathBuf};

pub struct DiscoveredPaths {
    pub user: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub project_local: Option<PathBuf>,
}

pub fn discover(cwd: &Path, home: Option<&Path>) -> DiscoveredPaths {
    let user = match home {
        Some(h) => crate::paths::resolve_user_config_with_home(h),
        None => crate::paths::resolve_user_config(),
    };

    let mut project = None;
    let mut project_local = None;
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let p = d.join(".consult-llm.yaml");
        let pl = d.join(".consult-llm.local.yaml");
        if p.exists() || pl.exists() {
            if p.exists() {
                project = Some(p);
            }
            if pl.exists() {
                project_local = Some(pl);
            }
            break;
        }
        if home.is_some_and(|h| d == h) {
            break;
        }
        dir = d.parent();
    }

    DiscoveredPaths {
        user,
        project,
        project_local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_walks_through_git_root() {
        // A nested git repo should not block discovery of a config in an
        // ancestor directory.
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let outer = home.join("outer");
        let inner = outer.join("inner");
        let sub = inner.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(inner.join(".git")).unwrap();
        std::fs::write(outer.join(".consult-llm.local.yaml"), "").unwrap();

        let paths = discover(&sub, Some(&home));
        assert_eq!(
            paths.project_local.unwrap(),
            outer.join(".consult-llm.local.yaml")
        );
    }

    #[test]
    fn test_discover_stops_at_home() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let sub = home.join("project").join("src");
        std::fs::create_dir_all(&sub).unwrap();
        // No config files anywhere — should stop at home
        let paths = discover(&sub, Some(&home));
        assert!(paths.project.is_none());
        assert!(paths.project_local.is_none());
    }

    #[test]
    fn test_discover_finds_both_yaml_and_local_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path();
        std::fs::write(project_dir.join(".consult-llm.yaml"), "").unwrap();
        std::fs::write(project_dir.join(".consult-llm.local.yaml"), "").unwrap();

        let paths = discover(project_dir, None);
        assert!(paths.project.is_some());
        assert!(paths.project_local.is_some());
    }

    #[test]
    fn test_discover_walks_up_to_find_config() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sub = root.join("a").join("b").join("c");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(root.join(".consult-llm.yaml"), "").unwrap();

        let paths = discover(&sub, None);
        assert!(paths.project.is_some());
        assert_eq!(paths.project.unwrap(), root.join(".consult-llm.yaml"));
    }

    #[test]
    fn test_discover_user_config() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let user_config_dir = home.join(".config").join("consult-llm");
        std::fs::create_dir_all(&user_config_dir).unwrap();
        std::fs::write(user_config_dir.join("config.yaml"), "").unwrap();

        let cwd = home.join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let paths = discover(&cwd, Some(&home));
        assert!(paths.user.is_some());
    }

    #[test]
    fn test_discover_legacy_user_config() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let user_config_dir = home.join(".consult-llm");
        std::fs::create_dir_all(&user_config_dir).unwrap();
        std::fs::write(user_config_dir.join("config.yaml"), "").unwrap();

        let cwd = home.join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let paths = discover(&cwd, Some(&home));
        assert!(paths.user.is_some());
    }

    #[test]
    fn test_discover_xdg_wins_when_both_exist() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();

        let xdg_dir = home.join(".config").join("consult-llm");
        std::fs::create_dir_all(&xdg_dir).unwrap();
        std::fs::write(xdg_dir.join("config.yaml"), "").unwrap();

        let legacy_dir = home.join(".consult-llm");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("config.yaml"), "").unwrap();

        let cwd = home.join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let paths = discover(&cwd, Some(&home));
        assert_eq!(paths.user.unwrap(), xdg_dir.join("config.yaml"));
    }
}
