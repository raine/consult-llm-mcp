use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Return unique parent directories of files that live outside `cwd`.
pub fn get_external_directories(file_paths: Option<&[PathBuf]>, cwd: &Path) -> Vec<String> {
    let Some(paths) = file_paths else {
        return vec![];
    };
    if paths.is_empty() {
        return vec![];
    }

    let mut dirs = HashSet::new();
    for path in paths {
        // If the path can't be made relative to cwd (i.e. it's outside), include its parent
        if path.strip_prefix(cwd).is_err()
            && let Some(parent) = path.parent()
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
}
