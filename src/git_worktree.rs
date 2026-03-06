use std::path::PathBuf;
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
