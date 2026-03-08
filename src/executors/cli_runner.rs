use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::logger::log_cli_debug;

pub struct CliResult {
    pub stdout_bytes: usize,
    pub stderr: String,
    pub code: Option<i32>,
}

/// Run a CLI command, calling `on_line` for each stdout line as it arrives.
pub async fn run_cli_streaming<F>(
    command: &str,
    args: &[String],
    mut on_line: F,
) -> anyhow::Result<CliResult>
where
    F: FnMut(&str) + Send,
{
    let cwd = std::env::current_dir().unwrap_or_default();
    log_cli_debug(
        &format!("Spawning {command} CLI (streaming)"),
        Some(&serde_json::json!({
            "args": args,
            "promptLength": args.last().map(|s| s.len()),
            "cwd": cwd,
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
    let stderr_pipe = child.stderr.take().expect("stderr was piped");

    // Read stdout and stderr concurrently to avoid deadlock when
    // the child fills one pipe buffer while we're blocking on the other.
    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        let mut reader = BufReader::new(stderr_pipe);
        tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut buf).await?;
        Ok::<_, std::io::Error>(buf)
    });

    let mut reader = BufReader::new(stdout).lines();
    let mut stdout_bytes: usize = 0;

    while let Some(line) = reader.next_line().await? {
        stdout_bytes += line.len() + 1;
        on_line(&line);
    }

    let status = child.wait().await?;
    let duration_ms = start.elapsed().as_millis();
    let stderr = stderr_task.await??;

    let result = CliResult {
        stdout_bytes,
        stderr,
        code: status.code(),
    };

    log_cli_debug(
        &format!("{command} CLI process closed"),
        Some(&serde_json::json!({
            "code": result.code,
            "duration": format!("{}ms", duration_ms),
            "stdoutBytes": result.stdout_bytes,
            "stderrLength": result.stderr.len(),
        })),
    );

    Ok(result)
}
