use std::fs::{OpenOptions, create_dir_all};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::Local;

static LOG_FILE: OnceLock<Mutex<BufWriter<std::fs::File>>> = OnceLock::new();

fn init_log_file() -> Mutex<BufWriter<std::fs::File>> {
    let state_home = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(".local")
            .join("state")
            .to_string_lossy()
            .to_string()
    });
    let dir = PathBuf::from(state_home).join("consult-llm-mcp");
    let _ = create_dir_all(&dir);
    let path = dir.join("mcp.log");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("failed to open log file");
    Mutex::new(BufWriter::new(file))
}

pub fn log_to_file(content: &str) {
    let timestamp = Local::now().to_rfc3339();
    let entry = format!("[{timestamp}] {content}\n");
    let file = LOG_FILE.get_or_init(init_log_file);
    if let Ok(mut f) = file.lock() {
        let _ = f.write_all(entry.as_bytes());
    }
}

pub fn log_tool_call(name: &str, args: &serde_json::Value) {
    log_to_file(&format!(
        "TOOL CALL: {name}\nArguments: {}\n{}",
        serde_json::to_string_pretty(args).unwrap_or_default(),
        "=".repeat(80)
    ));
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

pub fn log_server_start(version: &str) {
    log_to_file(&format!(
        "MCP SERVER STARTED - consult-llm-mcp v{version}\n{}",
        "=".repeat(80)
    ));
}

pub fn log_configuration(config: &std::collections::HashMap<String, String>) {
    let redacted: std::collections::HashMap<&str, &str> = config
        .iter()
        .map(|(k, v)| {
            let val = if k.to_lowercase().contains("key") || k.to_lowercase().contains("secret") {
                if v.is_empty() { "" } else { "[REDACTED]" }
            } else {
                v.as_str()
            };
            (k.as_str(), val)
        })
        .collect();
    log_to_file(&format!(
        "CONFIGURATION:\n{}\n{}",
        serde_json::to_string_pretty(&redacted).unwrap_or_default(),
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
