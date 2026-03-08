use std::fmt::Write;

pub fn build_prompt(
    user_prompt: &str,
    files: &[(String, String)], // (path, content)
    git_diff_output: Option<&str>,
) -> String {
    let capacity = user_prompt.len()
        + git_diff_output.map_or(0, |d| d.len() + 30)
        + files
            .iter()
            .map(|(p, c)| p.len() + c.len() + 30)
            .sum::<usize>();
    let mut out = String::with_capacity(capacity);

    if let Some(diff) = git_diff_output
        && !diff.trim().is_empty()
    {
        out.push_str("## Git Diff\n```diff\n");
        out.push_str(diff);
        out.push_str("\n```\n\n");
    }

    if !files.is_empty() {
        out.push_str("## Relevant Files\n\n");
        for (path, content) in files {
            let _ = write!(out, "### File: {path}\n```\n");
            out.push_str(content);
            out.push_str("\n```\n\n");
        }
    }

    out.push_str(user_prompt);
    out
}
