use std::process::Command;

/// Generate git diff. Returns error string on failure (never propagates as Err).
pub fn generate_git_diff(repo_path: Option<&str>, files: &[String], base_ref: &str) -> String {
    if files.is_empty() {
        return "Error generating git diff: No files specified for git diff".to_string();
    }

    if base_ref.starts_with('-') {
        return "Error generating git diff: invalid base_ref".to_string();
    }

    let cwd = repo_path.unwrap_or(".");
    let mut args = vec!["diff".to_string(), base_ref.to_string(), "--".to_string()];
    args.extend(files.iter().cloned());

    match Command::new("git").args(&args).current_dir(cwd).output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("Error generating git diff: {}", stderr.trim())
        }
        Err(e) => format!("Error generating git diff: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rejects_leading_dash_base_ref() {
        let result = generate_git_diff(None, &["file.rs".to_string()], "--output=/tmp/pwned");
        assert_eq!(result, "Error generating git diff: invalid base_ref");
    }

    #[test]
    fn test_rejects_empty_files() {
        let result = generate_git_diff(None, &[], "HEAD");
        assert!(result.contains("No files specified"));
    }
}
