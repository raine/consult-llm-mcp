use chrono::Utc;
use serde::{Deserialize, Serialize};
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
    },
    ConsultStarted {
        id: String,
        model: String,
        backend: String,
    },
    ConsultFinished {
        id: String,
        duration_ms: u128,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    ServerStopped,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EventEnvelope {
    pub ts: String,
    #[serde(flatten)]
    pub event: MonitorEvent,
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
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        let reader = std::io::BufReader::new(file);
        let mut pid: Option<u32> = None;
        let mut stopped = false;
        for line in reader.lines().map_while(Result::ok) {
            if let Ok(env) = serde_json::from_str::<EventEnvelope>(&line) {
                match env.event {
                    MonitorEvent::ServerStarted { pid: p, .. } => pid = Some(p),
                    MonitorEvent::ServerStopped => stopped = true,
                    _ => {}
                }
            }
        }
        if stopped || pid.is_some_and(|p| !is_pid_alive(p)) {
            let _ = fs::remove_file(&path);
        }
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
