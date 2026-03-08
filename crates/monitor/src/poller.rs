use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use consult_llm_core::jsonl::read_jsonl_from_offset;
use consult_llm_core::monitoring::{
    EventEnvelope, HISTORY_FILE, HistoryRecord, MonitorEvent, is_pid_alive,
};
use consult_llm_core::stream_events::ParsedStreamEvent;

// ── Messages ────────────────────────────────────────────────────────────

pub(crate) enum PollUpdate {
    /// New events parsed from server session files
    Events(Vec<(String, EventEnvelope)>),
    /// New history records (in file order)
    HistoryRecords(Vec<HistoryRecord>),
    /// Server IDs whose PID is no longer alive
    Deaths(Vec<String>),
    /// Server IDs that were pruned (session files deleted)
    Pruned(Vec<String>),
    /// New events for the current detail view
    DetailEvents {
        consultation_id: String,
        events: Vec<ParsedStreamEvent>,
    },
}

pub(crate) enum PollCommand {
    EnterDetail {
        consultation_id: String,
        file_offset: u64,
    },
    ExitDetail,
    ResetHistory,
    Shutdown,
}

// ── Internal state ──────────────────────────────────────────────────────

struct ServerInfo {
    pid: u32,
    stopped: bool,
    dead: bool,
}

struct DetailPollState {
    consultation_id: String,
    file_offset: u64,
}

// ── Spawn ───────────────────────────────────────────────────────────────

pub(crate) fn spawn(
    dir: PathBuf,
    poll_interval: Duration,
) -> (
    mpsc::Receiver<PollUpdate>,
    mpsc::Sender<PollCommand>,
    JoinHandle<()>,
) {
    let (update_tx, update_rx) = mpsc::channel();
    let (cmd_tx, cmd_rx) = mpsc::channel();

    let handle = thread::Builder::new()
        .name("poller".into())
        .spawn(move || run(dir, poll_interval, update_tx, cmd_rx))
        .expect("failed to spawn poller thread");

    (update_rx, cmd_tx, handle)
}

/// Apply a command, returning true if the poller should shut down.
fn apply_command(
    cmd: PollCommand,
    detail: &mut Option<DetailPollState>,
    next_detail: &mut Option<std::time::Instant>,
    history_offset: &mut u64,
) -> bool {
    match cmd {
        PollCommand::Shutdown => return true,
        PollCommand::EnterDetail {
            consultation_id,
            file_offset,
        } => {
            *detail = Some(DetailPollState {
                consultation_id,
                file_offset,
            });
            *next_detail = Some(std::time::Instant::now());
        }
        PollCommand::ExitDetail => {
            *detail = None;
            *next_detail = None;
        }
        PollCommand::ResetHistory => {
            *history_offset = 0;
        }
    }
    false
}

fn run(
    dir: PathBuf,
    heavy_interval: Duration,
    tx: mpsc::Sender<PollUpdate>,
    cmd_rx: mpsc::Receiver<PollCommand>,
) {
    let detail_interval = Duration::from_millis(100);

    let mut file_offsets: HashMap<String, u64> = HashMap::new();
    let mut pruned: HashSet<String> = HashSet::new();
    let mut history_offset: u64 = 0;
    let mut server_info: HashMap<String, ServerInfo> = HashMap::new();
    let mut detail: Option<DetailPollState> = None;

    // Fire heavy poll immediately on first iteration
    let mut next_heavy = std::time::Instant::now();
    let mut next_detail: Option<std::time::Instant> = None;

    loop {
        // Drain pending commands
        loop {
            match cmd_rx.try_recv() {
                Ok(cmd) => {
                    if apply_command(cmd, &mut detail, &mut next_detail, &mut history_offset) {
                        return;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }

        let now = std::time::Instant::now();

        // Detail poll (fast cadence) — run first for better perceived latency
        if let Some(deadline) = next_detail
            && now >= deadline
        {
            if let Some(ref mut d) = detail {
                let new_events = poll_detail_events(&dir, &d.consultation_id, &mut d.file_offset);
                if !new_events.is_empty()
                    && tx
                        .send(PollUpdate::DetailEvents {
                            consultation_id: d.consultation_id.clone(),
                            events: new_events,
                        })
                        .is_err()
                {
                    return;
                }
            }
            next_detail = Some(std::time::Instant::now() + detail_interval);
        }

        // Heavy poll (slow cadence)
        if now >= next_heavy {
            let events = poll_files(&dir, &mut file_offsets, &pruned, &mut server_info);
            if !events.is_empty() && tx.send(PollUpdate::Events(events)).is_err() {
                return;
            }

            let records = poll_history(&dir, &mut history_offset);
            if !records.is_empty() && tx.send(PollUpdate::HistoryRecords(records)).is_err() {
                return;
            }

            let deaths = check_liveness(&mut server_info);
            if !deaths.is_empty() && tx.send(PollUpdate::Deaths(deaths)).is_err() {
                return;
            }

            let pruned_ids = prune_finished(&dir, &mut server_info, &mut file_offsets, &mut pruned);
            if !pruned_ids.is_empty() && tx.send(PollUpdate::Pruned(pruned_ids)).is_err() {
                return;
            }

            next_heavy = std::time::Instant::now() + heavy_interval;
        }

        // Sleep until the next deadline, waking early for commands
        let next_deadline = match next_detail {
            Some(d) => d.min(next_heavy),
            None => next_heavy,
        };
        let wait = next_deadline.saturating_duration_since(std::time::Instant::now());
        match cmd_rx.recv_timeout(wait) {
            Ok(cmd) => {
                if apply_command(cmd, &mut detail, &mut next_detail, &mut history_offset) {
                    return;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

// ── Polling functions ───────────────────────────────────────────────────

fn poll_files(
    dir: &Path,
    file_offsets: &mut HashMap<String, u64>,
    pruned: &HashSet<String>,
    server_info: &mut HashMap<String, ServerInfo>,
) -> Vec<(String, EventEnvelope)> {
    let mut events = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return events;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.ends_with(".events") || stem == "history" {
            continue;
        }
        let server_id = stem.to_string();
        if pruned.contains(&server_id) {
            continue;
        }

        let offset = file_offsets.entry(server_id.clone()).or_insert(0);
        let envelopes: Vec<EventEnvelope> = read_jsonl_from_offset(&path, offset);
        for envelope in envelopes {
            update_server_info(server_info, &server_id, &envelope);
            events.push((server_id.clone(), envelope));
        }
    }

    events
}

fn update_server_info(
    server_info: &mut HashMap<String, ServerInfo>,
    server_id: &str,
    envelope: &EventEnvelope,
) {
    match &envelope.event {
        MonitorEvent::ServerStarted { pid, .. } => {
            server_info.insert(
                server_id.to_string(),
                ServerInfo {
                    pid: *pid,
                    stopped: false,
                    dead: false,
                },
            );
        }
        MonitorEvent::ServerStopped => {
            if let Some(info) = server_info.get_mut(server_id) {
                info.stopped = true;
            }
        }
        _ => {}
    }
}

fn poll_history(dir: &Path, history_offset: &mut u64) -> Vec<HistoryRecord> {
    let path = dir.join(HISTORY_FILE);
    read_jsonl_from_offset(&path, history_offset)
}

fn check_liveness(server_info: &mut HashMap<String, ServerInfo>) -> Vec<String> {
    let mut deaths = Vec::new();
    for (id, info) in server_info.iter_mut() {
        if !info.stopped && !info.dead && !is_pid_alive(info.pid) {
            info.dead = true;
            deaths.push(id.clone());
        }
    }
    deaths
}

fn prune_finished(
    dir: &Path,
    server_info: &mut HashMap<String, ServerInfo>,
    file_offsets: &mut HashMap<String, u64>,
    pruned: &mut HashSet<String>,
) -> Vec<String> {
    let to_prune: Vec<String> = server_info
        .iter()
        .filter(|(_, info)| info.stopped || info.dead)
        .map(|(id, _)| id.clone())
        .collect();

    for id in &to_prune {
        let path = dir.join(format!("{id}.jsonl"));
        let _ = fs::remove_file(&path);
        server_info.remove(id);
        file_offsets.remove(id);
        pruned.insert(id.clone());
    }

    to_prune
}

fn poll_detail_events(
    dir: &Path,
    consultation_id: &str,
    file_offset: &mut u64,
) -> Vec<ParsedStreamEvent> {
    let path = dir.join(format!("{consultation_id}.events.jsonl"));
    read_jsonl_from_offset(&path, file_offset)
}
