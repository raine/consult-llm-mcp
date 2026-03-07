use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Normalize a path by resolving `.` and `..` segments lexically (no filesystem access).
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if let Some(last) = components.last()
                    && *last != std::path::Component::RootDir
                {
                    components.pop();
                    continue;
                }
            }
            std::path::Component::CurDir => continue,
            _ => {}
        }
        components.push(component);
    }
    components.iter().collect()
}

/// Return unique parent directories of files that live outside `cwd`.
pub fn get_external_directories(file_paths: Option<&[PathBuf]>, cwd: &Path) -> Vec<String> {
    let Some(paths) = file_paths else {
        return vec![];
    };
    if paths.is_empty() {
        return vec![];
    }

    let normalized_cwd = normalize_path(cwd);
    let mut dirs = HashSet::new();
    for path in paths {
        let normalized = normalize_path(path);
        if normalized.strip_prefix(&normalized_cwd).is_err()
            && let Some(parent) = normalized.parent()
        {
            dirs.insert(parent.to_string_lossy().to_string());
        }
    }
    dirs.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_paths() {
        assert!(get_external_directories(None, Path::new("/home/user")).is_empty());
    }

    #[test]
    fn test_empty_paths() {
        assert!(get_external_directories(Some(&[]), Path::new("/home/user")).is_empty());
    }

    #[test]
    fn test_inside_cwd() {
        let paths = vec![PathBuf::from("/home/user/src/main.rs")];
        let result = get_external_directories(Some(&paths), Path::new("/home/user"));
        assert!(result.is_empty());
    }

    #[test]
    fn test_outside_cwd() {
        let paths = vec![PathBuf::from("/other/project/lib.rs")];
        let result = get_external_directories(Some(&paths), Path::new("/home/user"));
        assert_eq!(result, vec!["/other/project"]);
    }

    #[test]
    fn test_mixed_deduplication() {
        let paths = vec![
            PathBuf::from("/other/project/a.rs"),
            PathBuf::from("/other/project/b.rs"),
            PathBuf::from("/home/user/src/main.rs"),
        ];
        let result = get_external_directories(Some(&paths), Path::new("/home/user"));
        assert_eq!(result.len(), 1);
        assert!(result.contains(&"/other/project".to_string()));
    }

    #[test]
    fn test_dotdot_path_outside_cwd() {
        let paths = vec![PathBuf::from("/home/user/../other/file.rs")];
        let result = get_external_directories(Some(&paths), Path::new("/home/user"));
        assert_eq!(result.len(), 1);
        assert!(result.contains(&"/home/other".to_string()));
    }

    #[test]
    fn test_dotdot_path_inside_cwd() {
        let paths = vec![PathBuf::from("/home/other/../user/file.rs")];
        let result = get_external_directories(Some(&paths), Path::new("/home/user"));
        assert!(result.is_empty());
    }
}
