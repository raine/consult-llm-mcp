use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::logger::log_cli_debug;

#[allow(dead_code)]
pub struct CliResult {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
    pub duration_ms: u128,
}

/// Find the largest byte index <= `max_bytes` that is a valid UTF-8 char boundary.
pub fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut i = max_bytes;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Run a CLI command, calling `on_line` for each stdout line as it arrives.
/// Still accumulates full stdout/stderr for the final CliResult.
pub async fn run_cli_streaming<F>(
    command: &str,
    args: &[String],
    mut on_line: F,
) -> anyhow::Result<CliResult>
where
    F: FnMut(&str) + Send,
{
    log_cli_debug(
        &format!("Spawning {command} CLI (streaming)"),
        Some(&serde_json::json!({
            "args": args,
            "promptLength": args.last().map(|s| s.len()),
        })),
    );

    let start = Instant::now();
    let mut child = Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn {command} CLI. Is it installed and in PATH? Error: {e}"
            )
        })?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let mut reader = BufReader::new(stdout).lines();
    let mut all_stdout = String::new();

    while let Some(line) = reader.next_line().await? {
        on_line(&line);
        all_stdout.push_str(&line);
        all_stdout.push('\n');
    }

    let output = child.wait_with_output().await?;
    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    let result = CliResult {
        stdout: all_stdout,
        stderr,
        code: output.status.code(),
        duration_ms,
    };

    log_cli_debug(
        &format!("{command} CLI process closed"),
        Some(&serde_json::json!({
            "code": result.code,
            "duration": format!("{}ms", duration_ms),
            "stdoutLength": result.stdout.len(),
            "stderrLength": result.stderr.len(),
        })),
    );

    Ok(result)
}
