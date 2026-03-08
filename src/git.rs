use std::process::Command;

/// Generate git diff. Returns an error if git fails or inputs are invalid.
pub fn generate_git_diff(
    repo_path: Option<&str>,
    files: &[String],
    base_ref: &str,
) -> anyhow::Result<String> {
    if files.is_empty() {
        anyhow::bail!("No files specified for git diff");
    }

    if base_ref.starts_with('-') {
        anyhow::bail!("invalid base_ref");
    }

    let cwd = repo_path.unwrap_or(".");
    let mut args = vec!["diff".to_string(), base_ref.to_string(), "--".to_string()];
    args.extend(files.iter().cloned());

    match Command::new("git").args(&args).current_dir(cwd).output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("{}", stderr.trim())
        }
        Err(e) => anyhow::bail!("{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rejects_leading_dash_base_ref() {
        let result = generate_git_diff(None, &["file.rs".to_string()], "--output=/tmp/pwned");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid base_ref"));
    }

    #[test]
    fn test_rejects_empty_files() {
        let result = generate_git_diff(None, &[], "HEAD");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No files specified"));
    }
}
