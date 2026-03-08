pub mod api;
pub mod cli_runner;
pub mod codex_cli;
pub mod cursor_cli;
pub mod gemini_cli;
pub mod stream;
pub mod types;

use std::path::PathBuf;

use cli_runner::run_cli_streaming;
use stream::{ParsedStreamEvent, StreamReducer};
use types::ExecuteResult;

/// Format file paths as relative `@path` references appended to the prompt.
/// Used by Codex and Gemini CLI executors.
pub fn append_file_refs(text: &str, file_paths: Option<&[PathBuf]>) -> String {
    match file_paths {
        Some(fps) if !fps.is_empty() => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let file_refs: Vec<String> = fps
                .iter()
                .map(|p| {
                    let rel = pathdiff::diff_paths(p, &cwd).unwrap_or_else(|| p.clone());
                    format!("@{}", rel.display())
                })
                .collect();
            format!("{text}\n\nFiles: {}", file_refs.join(" "))
        }
        _ => text.to_string(),
    }
}

/// Run a CLI tool with streaming, parse output, and return the result.
/// Shared by all CLI executors to avoid duplicating the spawn → stream → check flow.
pub async fn run_cli_executor(
    command: &str,
    args: &[String],
    prompt: &str,
    consultation_id: Option<&str>,
    parse_line: fn(&str) -> Vec<ParsedStreamEvent>,
) -> anyhow::Result<ExecuteResult> {
    let mut reducer = StreamReducer::new(consultation_id, Some(prompt));
    let result = run_cli_streaming(command, args, |line| {
        reducer.process(parse_line(line));
    })
    .await?;

    if result.code == Some(0) {
        let response = reducer.response.trim_end().to_string();
        if response.is_empty() {
            anyhow::bail!("No response found in {command} stream output");
        }
        Ok(ExecuteResult {
            response,
            usage: reducer.usage,
            thread_id: reducer.thread_id,
        })
    } else {
        anyhow::bail!(
            "{command} exited with code {}. Error: {}",
            result.code.unwrap_or(-1),
            result.stderr.trim()
        )
    }
}
