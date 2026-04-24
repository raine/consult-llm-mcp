use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::stream_events::ParsedStreamEvent;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressStage {
    Thinking,
    ToolUse { tool: String },
    ToolResult { tool: String, success: bool },
    Responding,
    CliSpawned { pid: u32 },
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
            ProgressStage::CliSpawned { pid } => write!(f, "CLI spawned (PID {pid})"),
        }
    }
}

pub const HISTORY_FILE: &str = "history.jsonl";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistoryRecord {
    pub ts: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consultation_id: Option<String>,
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
    /// Pre-parsed timestamp, populated at ingest time by the monitor.
    #[serde(skip)]
    pub parsed_ts: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_mode: Option<String>,
}

pub fn append_history(record: &HistoryRecord) {
    let dir = sessions_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join(HISTORY_FILE);
    let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    if file.lock_exclusive().is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(record) {
        let mut f = &file;
        let _ = writeln!(f, "{json}");
        let _ = f.flush();
    }
    let _ = FileExt::unlock(&file);
}

pub fn sessions_dir() -> PathBuf {
    let state_home = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(".local/state").to_string_lossy().to_string()
    });
    PathBuf::from(state_home).join("consult-llm/sessions")
}

pub fn active_dir() -> PathBuf {
    let d = sessions_dir().join("active");
    let _ = fs::create_dir_all(&d);
    d
}

pub fn runs_dir() -> PathBuf {
    let d = sessions_dir().join("runs");
    let _ = fs::create_dir_all(&d);
    d
}

/// Check if a process is alive using kill(pid, 0).
#[cfg(unix)]
pub fn is_pid_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) only checks if the process exists, sends no signal.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
pub fn is_pid_alive(_pid: u32) -> bool {
    // Cannot reliably check on non-unix; assume alive to avoid cleanup races.
    true
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RunMeta {
    pub v: u32,
    pub run_id: String,
    pub pid: u32,
    pub started_at: String,
    pub project: String,
    pub cwd: String,
    pub model: String,
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RunEvent {
    pub v: u32,
    pub run_id: String,
    pub seq: u64,
    pub ts: String,
    #[serde(flatten)]
    pub kind: RunEventKind,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEventKind {
    RunStarted,
    Progress {
        stage: ProgressStage,
    },
    Stream {
        event: ParsedStreamEvent,
    },
    RunFinished {
        duration_ms: u64,
        success: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ActiveSnapshot {
    pub v: u32,
    pub run_id: String,
    pub pid: u32,
    pub started_at: String,
    pub model: String,
    pub backend: String,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub last_seq: u64,
    pub last_event_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<ProgressStage>,
}

fn write_atomic(path: &Path, bytes: &[u8]) {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let tmp = path.with_file_name(format!("{name}.tmp"));
    if let Ok(mut f) = fs::File::create(&tmp)
        && f.write_all(bytes).is_ok()
        && f.sync_data().is_ok()
    {
        let _ = fs::rename(&tmp, path);
    }
}

const TEXT_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
const TEXT_FLUSH_BYTES: usize = 1024;

struct SpoolInner {
    run_id: String,
    pid: u32,
    started_at: String,
    meta: RunMeta,
    events: BufWriter<fs::File>,
    active_path: PathBuf,
    seq: u64,
    last_stage: Option<ProgressStage>,
    /// Bytes buffered since last flush (only counts assistant-text stream writes).
    pending_text_bytes: usize,
    /// Instant of last flush (used for the 100 ms text threshold).
    last_flush: Instant,
    /// Thread ID learned from the stream (`SessionStarted`). `meta.thread_id` is
    /// the *requested* thread ID at invocation; this field is what the run
    /// actually resolved to.
    resolved_thread_id: Option<String>,
}

pub struct RunSpool {
    /// `None` when spool initialisation failed — all methods become no-ops.
    /// Monitoring must never take down a consultation.
    inner: Option<SpoolInner>,
}

impl RunSpool {
    pub fn new(meta: RunMeta) -> Self {
        match SpoolInner::try_new(meta) {
            Ok(inner) => Self { inner: Some(inner) },
            Err(e) => {
                eprintln!("consult-llm: monitoring disabled ({e})");
                Self { inner: None }
            }
        }
    }

    /// Construct a no-op spool (all methods become no-ops). Used by tests and
    /// code paths that don't need monitoring.
    pub fn disabled() -> Self {
        Self { inner: None }
    }

    pub fn record(&mut self, kind: RunEventKind, flush: bool) {
        if let Some(i) = self.inner.as_mut() {
            i.record(kind, flush);
        }
    }
    pub fn set_stage(&mut self, stage: ProgressStage) {
        if let Some(i) = self.inner.as_mut() {
            i.set_stage(stage);
        }
    }
    pub fn stream_event(&mut self, event: ParsedStreamEvent) {
        if let Some(i) = self.inner.as_mut() {
            i.stream_event(event);
        }
    }
    pub fn resolve_thread_id(&mut self, id: String) {
        if let Some(i) = self.inner.as_mut() {
            i.resolve_thread_id(id);
        }
    }
    pub fn finish(
        &mut self,
        duration_ms: u64,
        success: bool,
        error: Option<String>,
        history: &HistoryRecord,
    ) {
        if let Some(i) = self.inner.as_mut() {
            i.finish(duration_ms, success, error, history);
        }
    }
    pub fn resolved_thread_id(&self) -> Option<&str> {
        self.inner
            .as_ref()
            .and_then(|i| i.resolved_thread_id.as_deref())
    }
}

impl SpoolInner {
    fn try_new(meta: RunMeta) -> std::io::Result<Self> {
        let run_id = meta.run_id.clone();
        let started_at = meta.started_at.clone();
        let pid = meta.pid;

        let meta_path = runs_dir().join(format!("{run_id}.meta.json"));
        let meta_bytes = serde_json::to_vec_pretty(&meta).unwrap_or_default();
        write_atomic(&meta_path, &meta_bytes);

        let events_path = runs_dir().join(format!("{run_id}.events.jsonl"));
        let events_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)?;

        let active_path = active_dir().join(format!("{run_id}.json"));

        let mut inner = Self {
            run_id,
            pid,
            started_at,
            meta,
            events: BufWriter::new(events_file),
            active_path,
            seq: 0,
            last_stage: None,
            pending_text_bytes: 0,
            last_flush: Instant::now(),
            resolved_thread_id: None,
        };
        inner.write_snapshot();
        inner.record(RunEventKind::RunStarted, true);
        Ok(inner)
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn now() -> String {
        Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    fn record(&mut self, kind: RunEventKind, flush: bool) {
        let event = RunEvent {
            v: 1,
            run_id: self.run_id.clone(),
            seq: self.next_seq(),
            ts: Self::now(),
            kind,
        };
        if let Ok(line) = serde_json::to_string(&event) {
            let bytes = line.len() + 1;
            let _ = writeln!(self.events, "{line}");
            if flush {
                let _ = self.events.flush();
                self.pending_text_bytes = 0;
                self.last_flush = Instant::now();
            } else {
                self.pending_text_bytes += bytes;
            }
        }
    }

    fn stream_event(&mut self, event: ParsedStreamEvent) {
        let is_text = matches!(event, ParsedStreamEvent::AssistantText { .. });
        self.record(RunEventKind::Stream { event }, !is_text);
        if is_text && self.should_flush_text() {
            let _ = self.events.flush();
            self.pending_text_bytes = 0;
            self.last_flush = Instant::now();
        }
    }

    fn should_flush_text(&self) -> bool {
        self.pending_text_bytes >= TEXT_FLUSH_BYTES
            || self.last_flush.elapsed() >= TEXT_FLUSH_INTERVAL
    }

    fn set_stage(&mut self, stage: ProgressStage) {
        if self.last_stage.as_ref() == Some(&stage) {
            return;
        }
        self.last_stage = Some(stage.clone());
        self.record(RunEventKind::Progress { stage }, true);
        self.write_snapshot();
    }

    fn resolve_thread_id(&mut self, id: String) {
        if self.resolved_thread_id.as_deref() == Some(id.as_str()) {
            return;
        }
        self.resolved_thread_id = Some(id);
        self.write_snapshot();
    }

    fn finish(
        &mut self,
        duration_ms: u64,
        success: bool,
        error: Option<String>,
        history: &HistoryRecord,
    ) {
        self.record(
            RunEventKind::RunFinished {
                duration_ms,
                success,
                error,
            },
            true,
        );
        let _ = self.events.flush();
        append_history(history);
        let _ = fs::remove_file(&self.active_path);
    }

    fn write_snapshot(&self) {
        let snap = ActiveSnapshot {
            v: 1,
            run_id: self.run_id.clone(),
            pid: self.pid,
            started_at: self.started_at.clone(),
            model: self.meta.model.clone(),
            backend: self.meta.backend.clone(),
            project: self.meta.project.clone(),
            thread_id: self
                .resolved_thread_id
                .clone()
                .or_else(|| self.meta.thread_id.clone()),
            task_mode: self.meta.task_mode.clone(),
            reasoning_effort: self.meta.reasoning_effort.clone(),
            last_seq: self.seq,
            last_event_at: Self::now(),
            stage: self.last_stage.clone(),
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&snap) {
            write_atomic(&self.active_path, &bytes);
        }
    }
}

impl Drop for SpoolInner {
    fn drop(&mut self) {
        let _ = self.events.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn run_event_roundtrip() {
        let e = RunEvent {
            v: 1,
            run_id: "abc".into(),
            seq: 3,
            ts: "2026-04-24T00:00:00.000Z".into(),
            kind: RunEventKind::Progress {
                stage: ProgressStage::Thinking,
            },
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: RunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.seq, 3);
        assert!(matches!(back.kind, RunEventKind::Progress { .. }));
    }

    #[test]
    fn run_finished_variant_serializes_kind_field() {
        let e = RunEvent {
            v: 1,
            run_id: "abc".into(),
            seq: 9,
            ts: "t".into(),
            kind: RunEventKind::RunFinished {
                duration_ms: 100,
                success: true,
                error: None,
            },
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""kind":"run_finished""#));
    }

    #[test]
    fn spool_lifecycle_writes_expected_files() {
        let tmp = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", tmp.path());
        }
        let meta = RunMeta {
            v: 1,
            run_id: "r1".into(),
            pid: std::process::id(),
            started_at: "t".into(),
            project: "p".into(),
            cwd: "/tmp".into(),
            model: "m".into(),
            backend: "b".into(),
            thread_id: None,
            task_mode: None,
            reasoning_effort: None,
        };
        let mut spool = RunSpool::new(meta);
        spool.set_stage(ProgressStage::Thinking);
        let hist = HistoryRecord {
            ts: "t".into(),
            consultation_id: Some("r1".into()),
            project: "p".into(),
            model: "m".into(),
            backend: "b".into(),
            duration_ms: 1,
            success: true,
            error: None,
            tokens_in: None,
            tokens_out: None,
            parsed_ts: None,
            thread_id: None,
            reasoning_effort: None,
            task_mode: None,
        };
        spool.finish(1, true, None, &hist);

        assert!(runs_dir().join("r1.meta.json").exists());
        assert!(runs_dir().join("r1.events.jsonl").exists());
        assert!(!active_dir().join("r1.json").exists());
        assert!(sessions_dir().join(HISTORY_FILE).exists());
    }
}
