use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

static MAIN_WORKTREE: OnceLock<Option<String>> = OnceLock::new();

pub fn get_main_worktree_path() -> Option<&'static str> {
    MAIN_WORKTREE.get_or_init(detect_main_worktree).as_deref()
}

fn detect_main_worktree() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir", "--git-common-dir"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    let text = String::from_utf8(output.stdout).ok()?;
    let mut lines = text.trim().lines();
    let git_dir = lines.next()?;
    let common_dir = lines.next()?;

    if git_dir == common_dir {
        return None;
    }

    // The common dir is the .git directory of the main worktree.
    // Its parent is the main worktree root.
    let resolved = std::fs::canonicalize(PathBuf::from(common_dir).join("..")).ok()?;
    Some(resolved.to_string_lossy().to_string())
}

/// Resolve a project name from the current working directory.
///
/// For workmux worktree paths like `project/.worktrees/branch` or
/// `project__worktrees/branch`, returns `project/branch`.
/// For regular paths, returns just the directory name.
pub fn resolve_project_name(cwd: &Path) -> String {
    let path_str = cwd.to_string_lossy();

    if let Some(wt_pos) = path_str
        .find("__worktrees/")
        .or_else(|| path_str.find("/.worktrees/"))
    {
        let is_hidden = path_str[wt_pos..].starts_with("/.");
        let separator_len = if is_hidden {
            "/.worktrees/".len()
        } else {
            "__worktrees/".len()
        };

        let before = &path_str[..wt_pos];
        let main_project = Path::new(before)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let after = &path_str[wt_pos + separator_len..];
        let worktree = after.split('/').next().unwrap_or("");

        if !main_project.is_empty() && !worktree.is_empty() {
            return format!("{}/{}", main_project, worktree);
        }
    }

    cwd.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_str.into_owned())
}
