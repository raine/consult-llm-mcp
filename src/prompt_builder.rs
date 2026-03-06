pub fn build_prompt(
    user_prompt: &str,
    files: &[(String, String)], // (path, content)
    git_diff_output: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    if let Some(diff) = git_diff_output
        && !diff.trim().is_empty()
    {
        parts.push("## Git Diff\n```diff".to_string());
        parts.push(diff.to_string());
        parts.push("```\n".to_string());
    }

    if !files.is_empty() {
        parts.push("## Relevant Files\n".to_string());
        for (path, content) in files {
            parts.push(format!("### File: {path}"));
            parts.push("```".to_string());
            parts.push(content.clone());
            parts.push("```\n".to_string());
        }
    }

    parts.push(user_prompt.to_string());
    parts.join("\n")
}
