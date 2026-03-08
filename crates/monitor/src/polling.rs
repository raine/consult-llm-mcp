use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use consult_llm_core::monitoring::{EventEnvelope, HISTORY_FILE, HistoryRecord, is_pid_alive};
use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::state::{AppMode, AppState};

impl AppState {
    /// Scan sessions dir, read new lines from each file using read_line()
    /// to correctly handle partial writes and track byte offsets.
    pub(crate) fn poll_files(&mut self, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // Skip sidecar event files and history file
            if stem.ends_with(".events") || stem == "history" {
                continue;
            }
            let server_id = stem.to_string();

            // Skip pruned servers
            if self.pruned.contains(&server_id) {
                continue;
            }

            let Ok(file) = File::open(&path) else {
                continue;
            };
            let offset = self
                .servers
                .get(&server_id)
                .map(|s| s.file_offset)
                .unwrap_or(0);

            let mut reader = BufReader::new(file);
            let _ = reader.seek(SeekFrom::Start(offset));

            let mut new_offset = offset;
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(bytes_read) => {
                        // Only process complete lines (ending with newline)
                        if !buf.ends_with('\n') {
                            break; // Partial write, wait for next poll
                        }
                        new_offset += bytes_read as u64;
                        if let Ok(envelope) = serde_json::from_str::<EventEnvelope>(buf.trim()) {
                            self.process_event(&server_id, &envelope);
                        }
                    }
                    Err(_) => break,
                }
            }

            if let Some(server) = self.servers.get_mut(&server_id) {
                server.file_offset = new_offset;
            }
        }
    }

    pub(crate) fn poll_history(&mut self, dir: &Path) {
        let path = dir.join(HISTORY_FILE);
        let Ok(file) = File::open(&path) else {
            return;
        };
        let mut reader = BufReader::new(file);
        let _ = reader.seek(SeekFrom::Start(self.history_offset));

        let mut buf = String::new();
        loop {
            buf.clear();
            match reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(bytes_read) => {
                    if !buf.ends_with('\n') {
                        break;
                    }
                    self.history_offset += bytes_read as u64;
                    if let Ok(record) = serde_json::from_str::<HistoryRecord>(buf.trim()) {
                        self.history.push_front(record);
                        // Shift selection to track the same row after push_front
                        if !self.history.is_empty() {
                            self.history_selected =
                                (self.history_selected + 1).min(self.history.len() - 1);
                        }
                        if self.history.len() > 100 {
                            self.history.pop_back();
                            // Clamp selection if the previously-last row was removed
                            if self.history_selected >= self.history.len() {
                                self.history_selected = self.history.len().saturating_sub(1);
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }

    pub(crate) fn check_liveness(&mut self) {
        for server in self.servers.values_mut() {
            if !server.stopped && !server.dead && !is_pid_alive(server.pid) {
                server.dead = true;
                server.active_consults.clear();
            }
        }
    }

    /// Remove stopped/dead servers from the view and delete their files.
    pub(crate) fn prune_finished(&mut self, dir: &Path) {
        let to_prune: Vec<String> = self
            .servers
            .iter()
            .filter(|(_, s)| s.stopped || s.dead)
            .map(|(id, _)| id.clone())
            .collect();

        for id in &to_prune {
            // Delete the session file so it doesn't reappear on next poll.
            // Keep sidecar event files — they are needed for viewing history logs.
            let path = dir.join(format!("{id}.jsonl"));
            let _ = fs::remove_file(&path);
            self.servers.remove(id);
            self.pruned.insert(id.clone());
        }
        self.server_order.retain(|id| self.servers.contains_key(id));
    }

    /// Poll the sidecar file for new events in detail mode.
    pub(crate) fn poll_detail_events(&mut self, dir: &Path) {
        let AppMode::Detail(ref mut detail) = self.mode else {
            return;
        };

        let path = dir.join(format!("{}.events.jsonl", detail.consultation_id));
        let Ok(file) = File::open(&path) else {
            return;
        };

        let mut reader = BufReader::new(file);
        let _ = reader.seek(SeekFrom::Start(detail.file_offset));

        let mut buf = String::new();
        let mut got_new = false;
        loop {
            buf.clear();
            match reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(bytes_read) => {
                    if !buf.ends_with('\n') {
                        break;
                    }
                    detail.file_offset += bytes_read as u64;
                    if let Ok(event) = serde_json::from_str::<ParsedStreamEvent>(buf.trim()) {
                        detail.events.push(event);
                        got_new = true;
                    }
                }
                Err(_) => break,
            }
        }

        if got_new && detail.auto_scroll {
            // Will be clamped during render
            detail.scroll = usize::MAX;
        }
    }
}
