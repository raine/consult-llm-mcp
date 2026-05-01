use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

use super::child_guard::ChildGuard;
use crate::logger::log_cli_debug;

const WORKMUX_DISABLE_SET_WINDOW_STATUS_ENV: &str = "WORKMUX_DISABLE_SET_WINDOW_STATUS";
const WORKMUX_DISABLE_SET_WINDOW_STATUS_VALUE: &str = "1";

fn apply_workmux_disable_env(cmd: &mut Command) {
    cmd.env(
        WORKMUX_DISABLE_SET_WINDOW_STATUS_ENV,
        WORKMUX_DISABLE_SET_WINDOW_STATUS_VALUE,
    );
}

#[derive(Debug)]
pub struct CliResult {
    pub stdout_bytes: usize,
    pub stderr: String,
    pub code: Option<i32>,
}

/// Run a CLI command, calling `on_line` for each stdout line as it arrives.
/// When `stdin_data` is provided, it is written to the child's stdin (then
/// closed) instead of connecting stdin to /dev/null. The stdin write, the
/// stderr drain, and the stdout read run on three threads so a child that
/// emits >~64KB before draining stdin doesn't deadlock.
pub fn run_cli_streaming<F>(
    command: &str,
    args: &[String],
    stdin_data: Option<&str>,
    on_spawn: Option<Box<dyn FnOnce(u32) + Send>>,
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
            "stdinLength": stdin_data.map(|s| s.len()),
            "cwd": cwd,
        })),
    );

    let use_stdin = stdin_data.is_some();
    let start = Instant::now();
    let mut cmd = Command::new(command);
    apply_workmux_disable_env(&mut cmd);
    cmd.args(args)
        .stdin(if use_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut guard = ChildGuard::spawn(&mut cmd).map_err(|e| {
        anyhow::anyhow!("Failed to spawn {command} CLI. Is it installed and in PATH? Error: {e}")
    })?;

    if let Some(cb) = on_spawn {
        cb(guard.id());
    }

    let stdout = guard.child_mut().stdout.take().expect("stdout was piped");
    let stderr_pipe = guard.child_mut().stderr.take().expect("stderr was piped");
    let stdin_pipe = guard.child_mut().stdin.take();

    // Scope so the stdin writer can borrow `stdin_data` directly instead
    // of cloning it (prompts can be megabytes).
    let (stdout_bytes, stderr) = std::thread::scope(|s| -> anyhow::Result<(usize, String)> {
        let stderr_handle = s.spawn(move || -> std::io::Result<String> {
            let mut buf = String::new();
            let mut reader = BufReader::new(stderr_pipe);
            reader.read_to_string(&mut buf)?;
            Ok(buf)
        });

        let stdin_handle = s.spawn(move || -> std::io::Result<()> {
            if let (Some(data), Some(mut stdin)) = (stdin_data, stdin_pipe) {
                // BrokenPipe is suppressed: the child can legitimately exit early.
                match stdin.write_all(data.as_bytes()) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
                    Err(e) => return Err(e),
                }
                drop(stdin);
            }
            Ok(())
        });

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let mut stdout_bytes: usize = 0;
        loop {
            line.clear();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break;
            }
            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
            stdout_bytes += line.len();
            on_line(trimmed);
        }

        let stderr = stderr_handle
            .join()
            .map_err(|_| anyhow::anyhow!("stderr thread panicked"))??;
        match stdin_handle
            .join()
            .map_err(|_| anyhow::anyhow!("stdin thread panicked"))?
        {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(e) => return Err(e.into()),
        }
        Ok((stdout_bytes, stderr))
    })?;

    let status = guard.wait()?;
    let duration_ms = start.elapsed().as_millis();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_stdout_lines() {
        let args = vec!["-c".to_string(), "printf 'a\\nb\\nc\\n'".to_string()];
        let mut lines = Vec::new();
        run_cli_streaming("sh", &args, None, None, |l| lines.push(l.to_string())).expect("run");
        assert_eq!(lines, vec!["a", "b", "c"]);
    }

    #[test]
    fn large_stdin_with_concurrent_stdout_does_not_deadlock() {
        let big: String = "x".repeat(256 * 1024);
        let args = vec![
            "-c".to_string(),
            "cat > /dev/null; head -c 262144 /dev/zero | tr '\\0' 'y'".to_string(),
        ];
        let mut total = 0usize;
        let result =
            run_cli_streaming("sh", &args, Some(&big), None, |l| total += l.len()).expect("run");
        assert_eq!(result.code, Some(0));
        assert_eq!(total, 256 * 1024);
    }

    #[test]
    fn child_exits_before_consuming_stdin() {
        let args = vec!["-c".to_string(), "echo ok".to_string()];
        let prompt = "x".repeat(128 * 1024);
        let mut got = String::new();
        let result = run_cli_streaming("sh", &args, Some(&prompt), None, |l| got = l.to_string())
            .expect("run");
        assert_eq!(result.code, Some(0));
        assert_eq!(got, "ok");
    }

    #[test]
    fn stderr_is_captured() {
        let args = vec!["-c".to_string(), "echo oops 1>&2; exit 7".to_string()];
        let result = run_cli_streaming("sh", &args, None, None, |_| {}).expect("run");
        assert_eq!(result.code, Some(7));
        assert!(result.stderr.contains("oops"));
    }

    #[test]
    fn sets_workmux_disable_env_for_child() {
        let mut cmd = Command::new("sh");
        cmd.env(WORKMUX_DISABLE_SET_WINDOW_STATUS_ENV, "0");
        apply_workmux_disable_env(&mut cmd);

        let value = cmd
            .get_envs()
            .find_map(|(key, value)| {
                (key == WORKMUX_DISABLE_SET_WINDOW_STATUS_ENV).then_some(value)
            })
            .flatten();
        assert_eq!(value, Some(std::ffi::OsStr::new("1")));
    }

    #[test]
    fn workmux_disable_env_reaches_descendant_process() {
        let args = vec![
            "-c".to_string(),
            "sh -c 'printf \"%s\\n\" \"$WORKMUX_DISABLE_SET_WINDOW_STATUS\"'".to_string(),
        ];
        let mut got = String::new();
        let result =
            run_cli_streaming("sh", &args, None, None, |l| got = l.to_string()).expect("run");
        assert_eq!(result.code, Some(0));
        assert_eq!(got, "1");
    }

    #[test]
    fn missing_command_returns_error() {
        let result = run_cli_streaming("consult-llm-no-such-binary-xyz", &[], None, None, |_| {});
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("Failed to spawn"));
    }
}
