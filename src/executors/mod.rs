pub mod api;
pub mod cli_runner;
pub mod codex_cli;
pub mod cursor_cli;
pub mod gemini_cli;
pub mod opencode_cli;
pub mod stream;
pub mod thread_store;
pub mod types;

use std::path::PathBuf;

use cli_runner::run_cli_streaming;
use stream::{StreamEvents, StreamReducer};
use types::ExecuteResult;

use crate::external_dirs::get_external_directories;
use crate::git_worktree::get_main_worktree_path;

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

/// Build CLI args for extra directories (worktree + external file paths).
pub fn build_extra_dir_args(file_paths: Option<&[PathBuf]>, flag: &str) -> Vec<String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut args = Vec::new();
    if let Some(wt) = get_main_worktree_path() {
        args.push(flag.to_string());
        args.push(wt.to_string());
    }
    let resolved: Option<Vec<PathBuf>> = file_paths.map(|fps| fps.to_vec());
    for dir in get_external_directories(resolved.as_deref(), &cwd) {
        args.push(flag.to_string());
        args.push(dir);
    }
    args
}

/// Run a CLI tool with streaming, parse output, and return the result.
/// Shared by all CLI executors to avoid duplicating the spawn → stream → check flow.
/// The prompt is passed via stdin to keep it out of the process argument list.
pub async fn run_cli_executor(
    command: &str,
    args: &[String],
    stdin_prompt: &str,
    prompt: &str,
    system_prompt: &str,
    consultation_id: Option<&str>,
    parse_line: fn(&str) -> StreamEvents,
) -> anyhow::Result<ExecuteResult> {
    let mut reducer = StreamReducer::new(consultation_id, Some(prompt), Some(system_prompt));
    let on_spawn: Option<Box<dyn FnOnce(u32) + Send>> =
        consultation_id.map(|cid| -> Box<dyn FnOnce(u32) + Send> {
            let cid = cid.to_string();
            Box::new(move |pid| {
                consult_llm_core::monitoring::emit(
                    consult_llm_core::monitoring::MonitorEvent::ConsultProgress {
                        id: cid,
                        stage: consult_llm_core::monitoring::ProgressStage::CliSpawned { pid },
                    },
                );
            })
        });
    let result = run_cli_streaming(command, args, Some(stdin_prompt), on_spawn, |line| {
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
