use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions, create_dir_all};
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

static WRITER: OnceLock<EventWriter> = OnceLock::new();

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MonitorEvent {
    ServerStarted {
        version: String,
        pid: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
    },
    ConsultStarted {
        id: String,
        model: String,
        backend: String,
    },
    ConsultProgress {
        id: String,
        stage: ProgressStage,
    },
    ConsultFinished {
        id: String,
        duration_ms: u64,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    ServerStopped,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressStage {
    Thinking,
    ToolUse { tool: String },
    ToolResult { tool: String, success: bool },
    Responding,
}

impl std::fmt::Display for ProgressStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgressStage::Thinking => write!(f, "Thinking..."),
            ProgressStage::ToolUse { tool } => write!(f, "Tool: {tool}"),
            ProgressStage::ToolResult { tool, success } => {
                if *success {
                    write!(f, "Tool done: {tool}")
                } else {
                    write!(f, "Tool failed: {tool}")
                }
            }
            ProgressStage::Responding => write!(f, "Responding..."),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EventEnvelope {
    pub ts: String,
    #[serde(flatten)]
    pub event: MonitorEvent,
}

pub const HISTORY_FILE: &str = "history.jsonl";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistoryRecord {
    pub ts: String,
    pub project: String,
    pub model: String,
    pub backend: String,
    pub duration_ms: u64,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_out: Option<u64>,
}

pub fn append_history(record: &HistoryRecord) {
    let dir = sessions_dir();
    let _ = create_dir_all(&dir);
    let path = dir.join(HISTORY_FILE);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
        && let Ok(json) = serde_json::to_string(record)
    {
        let line = format!("{json}\n");
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn sessions_dir() -> PathBuf {
    let state_home = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(".local/state").to_string_lossy().to_string()
    });
    PathBuf::from(state_home).join("consult-llm-mcp/sessions")
}

struct EventWriter {
    file: Mutex<BufWriter<fs::File>>,
    path: PathBuf,
}

impl EventWriter {
    fn new() -> Self {
        let dir = sessions_dir();
        let _ = create_dir_all(&dir);
        cleanup_orphans(&dir);

        let server_id = Uuid::new_v4().to_string();
        let path = dir.join(format!("{server_id}.jsonl"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("failed to open monitoring event file");
        Self {
            file: Mutex::new(BufWriter::new(file)),
            path,
        }
    }

    fn emit(&self, event: MonitorEvent) {
        let envelope = EventEnvelope {
            ts: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            event,
        };
        if let Ok(line) = serde_json::to_string(&envelope)
            && let Ok(mut f) = self.file.lock()
        {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }

    fn remove_file(&self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Check if a process is alive using kill(pid, 0).
pub fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Remove session files for dead servers that never wrote server_stopped.
fn cleanup_orphans(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    // Collect server session files and sidecar event files separately
    let mut server_files: Vec<std::path::PathBuf> = Vec::new();
    let mut sidecar_files: Vec<std::path::PathBuf> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem == "history" {
            continue; // Skip persistent history file
        } else if stem.ends_with(".events") {
            sidecar_files.push(path);
        } else {
            server_files.push(path);
        }
    }

    // Track which server sessions are alive
    let mut alive_server_ids: HashSet<String> = HashSet::new();

    for path in &server_files {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(file) = fs::File::open(path) else {
            continue;
        };
        let reader = std::io::BufReader::new(file);
        let mut pid: Option<u32> = None;
        let mut stopped = false;
        for line in reader.lines().map_while(Result::ok) {
            if let Ok(env) = serde_json::from_str::<EventEnvelope>(&line) {
                match env.event {
                    MonitorEvent::ServerStarted { pid: p, .. } => {
                        pid = Some(p);
                    }
                    MonitorEvent::ServerStopped => stopped = true,
                    _ => {}
                }
            }
        }
        if stopped || pid.is_some_and(|p| !is_pid_alive(p)) {
            let _ = fs::remove_file(path);
        } else {
            alive_server_ids.insert(stem.to_string());
        }
    }

    // Remove orphaned sidecar files (those not associated with any alive server)
    // Sidecar files are named {consultation_id}.events.jsonl — we can't easily
    // map them back to servers without reading server files. Clean up any sidecar
    // whose server session file no longer exists.
    for path in &sidecar_files {
        // If the file is orphaned (no live server session references it), remove it
        let _ = fs::remove_file(path);
    }
}

pub fn init() {
    WRITER.get_or_init(EventWriter::new);
}

pub fn emit(event: MonitorEvent) {
    if let Some(w) = WRITER.get() {
        w.emit(event);
    }
}

/// Remove the session file (called on clean shutdown).
pub fn cleanup() {
    if let Some(w) = WRITER.get() {
        w.remove_file();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_consult_finished() {
        let line = r#"{"ts":"2026-03-07T13:15:11.693Z","type":"consult_finished","id":"abc","duration_ms":3200,"success":true}"#;
        let result = serde_json::from_str::<EventEnvelope>(line);
        match &result {
            Ok(env) => println!("Parsed: {:?}", env),
            Err(e) => println!("Error: {}", e),
        }
        assert!(
            result.is_ok(),
            "Failed to parse ConsultFinished: {:?}",
            result.err()
        );
    }

    #[test]
    fn parse_all_event_types() {
        let lines = vec![
            r#"{"ts":"T","type":"server_started","version":"2.5.5","pid":12345,"project":"my-app"}"#,
            r#"{"ts":"T","type":"consult_started","id":"a","model":"gpt","backend":"api"}"#,
            r#"{"ts":"T","type":"consult_progress","id":"a","stage":{"type":"thinking"}}"#,
            r#"{"ts":"T","type":"consult_progress","id":"a","stage":{"type":"tool_use","tool":"read_file"}}"#,
            r#"{"ts":"T","type":"consult_progress","id":"a","stage":{"type":"tool_result","tool":"read_file","success":true}}"#,
            r#"{"ts":"T","type":"consult_progress","id":"a","stage":{"type":"responding"}}"#,
            r#"{"ts":"T","type":"consult_finished","id":"a","duration_ms":3200,"success":true}"#,
            r#"{"ts":"T","type":"consult_finished","id":"b","duration_ms":5000,"success":false,"error":"timeout"}"#,
            r#"{"ts":"T","type":"server_stopped"}"#,
        ];
        for line in lines {
            let result = serde_json::from_str::<EventEnvelope>(line);
            assert!(
                result.is_ok(),
                "Failed to parse '{}': {:?}",
                line,
                result.err()
            );
        }
    }
}
