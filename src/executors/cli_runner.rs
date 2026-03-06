use std::time::Instant;
use tokio::process::Command;

use crate::logger::log_cli_debug;

#[allow(dead_code)]
pub struct CliResult {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
    pub duration_ms: u128,
}

pub async fn run_cli(command: &str, args: &[String]) -> anyhow::Result<CliResult> {
    log_cli_debug(
        &format!("Spawning {command} CLI"),
        Some(&serde_json::json!({
            "args": args,
            "promptLength": args.last().map(|s| s.len()),
        })),
    );

    let start = Instant::now();
    let output = Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn {command} CLI. Is it installed and in PATH? Error: {e}"
            )
        })?
        .wait_with_output()
        .await?;

    let duration_ms = start.elapsed().as_millis();
    let result = CliResult {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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
