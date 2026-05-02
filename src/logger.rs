use std::fs::{File, OpenOptions, create_dir_all};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::Local;

static LOG_FILE: OnceLock<Mutex<LoggerState>> = OnceLock::new();

struct LoggerState {
    file: Option<File>,
    path: Option<PathBuf>,
    warned: bool,
}

impl LoggerState {
    fn new() -> Self {
        Self {
            file: None,
            path: None,
            warned: false,
        }
    }

    fn log_to_file(&mut self, content: &str) {
        let path = log_path();
        if self.path.as_ref() != Some(&path) {
            self.file = match open_log_file(&path) {
                Ok(file) => Some(file),
                Err(err) => {
                    self.warn_once(&path, &err);
                    None
                }
            };
            self.path = Some(path);
        }

        if let Some(file) = self.file.as_mut() {
            let _ = file.write_all(content.as_bytes());
        }
    }

    fn warn_once(&mut self, path: &Path, err: &std::io::Error) {
        if self.warned {
            return;
        }
        self.warned = true;
        eprintln!(
            "Warning: failed to open consult-llm log at {}: {err}",
            path.display()
        );
    }
}

fn log_path() -> PathBuf {
    consult_llm_core::paths::state_home()
        .join("consult-llm")
        .join("consult-llm.log")
}

fn open_log_file(path: &Path) -> std::io::Result<File> {
    if let Some(dir) = path.parent() {
        create_dir_all(dir)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn log_to_file(content: &str) {
    let timestamp = Local::now().to_rfc3339();
    let entry = format!("[{timestamp}] {content}\n");
    let file = LOG_FILE.get_or_init(|| Mutex::new(LoggerState::new()));
    if let Ok(mut f) = file.lock() {
        f.log_to_file(&entry);
    }
}

pub fn log_prompt(model: &str, prompt: &str) {
    log_to_file(&format!(
        "PROMPT (model: {model}):\n{prompt}\n{}",
        "=".repeat(80)
    ));
}

pub fn log_response(model: &str, response: &str, cost_info: &str) {
    log_to_file(&format!(
        "RESPONSE (model: {model}):\n{response}\n{cost_info}\n{}",
        "=".repeat(80)
    ));
}

pub fn log_cli_debug(message: &str, data: Option<&serde_json::Value>) {
    match data {
        Some(d) => log_to_file(&format!(
            "CLI: {message}\n{}",
            serde_json::to_string_pretty(d).unwrap_or_default()
        )),
        None => log_to_file(&format!("CLI: {message}")),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn file_state_home() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state-home-file");
        File::create(&path).unwrap();
        (temp, path)
    }

    #[test]
    fn logger_state_does_not_panic_when_state_home_is_file() {
        let (_temp, path) = file_state_home();
        let _guard = crate::test_util::XdgStateGuard::new(&path);
        let mut state = LoggerState::new();

        state.log_to_file("first\n");
        state.log_to_file("second\n");

        assert!(state.file.is_none());
        assert!(state.warned);
    }

    #[test]
    fn logger_state_recovers_after_prior_initialization() {
        let mut state = LoggerState::new();
        {
            let _guard = crate::test_util::XdgStateGuard::temp();
            state.log_to_file("first\n");
            assert!(state.file.is_some());
        }

        let (_temp, path) = file_state_home();
        let _guard = crate::test_util::XdgStateGuard::new(&path);
        state.log_to_file("second\n");

        assert!(state.file.is_none());
        assert!(state.warned);
    }

    #[test]
    fn log_to_file_does_not_panic_when_state_home_is_file() {
        let (_temp, path) = file_state_home();
        let _guard = crate::test_util::XdgStateGuard::new(&path);

        log_to_file("hello");
    }

    #[test]
    fn logger_state_writes_when_state_home_is_writable() {
        let guard = crate::test_util::XdgStateGuard::temp();
        let mut state = LoggerState::new();

        state.log_to_file("hello\n");

        drop(state.file.take());
        let mut text = String::new();
        File::open(log_path())
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.contains("hello\n"));
        drop(guard);
    }
}
