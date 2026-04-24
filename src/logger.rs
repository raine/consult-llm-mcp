use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::Local;

static LOG_FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

fn init_log_file() -> Mutex<std::fs::File> {
    let state_home = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(".local")
            .join("state")
            .to_string_lossy()
            .to_string()
    });
    let dir = PathBuf::from(state_home).join("consult-llm");
    let _ = create_dir_all(&dir);
    let path = dir.join("consult-llm.log");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("failed to open log file");
    Mutex::new(file)
}

pub fn log_to_file(content: &str) {
    let timestamp = Local::now().to_rfc3339();
    let entry = format!("[{timestamp}] {content}\n");
    let file = LOG_FILE.get_or_init(init_log_file);
    if let Ok(mut f) = file.lock() {
        let _ = f.write_all(entry.as_bytes());
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
