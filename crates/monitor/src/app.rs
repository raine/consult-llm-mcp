use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Utc};

use consult_llm_core::monitoring::{EventEnvelope, HISTORY_FILE, MonitorEvent};
use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::action::Action;
use crate::state::{
    ActiveConsult, AppMode, AppState, CompletedConsult, DetailState, Focus, RowInfo, ServerState,
};

impl AppState {
    pub(crate) fn apply(&mut self, action: Action, dir: &Path) {
        match action {
            Action::Quit => unreachable!("handled in main loop"),
            Action::ToggleFocus => {
                self.focus = match self.focus {
                    Focus::Active => Focus::History,
                    Focus::History => Focus::Active,
                };
            }
            Action::MoveDown => match self.focus {
                Focus::Active => {
                    if self.row_count > 0 {
                        self.selected = (self.selected + 1).min(self.row_count - 1);
                    }
                }
                Focus::History => {
                    if !self.history.is_empty() {
                        self.history_selected =
                            (self.history_selected + 1).min(self.history.len() - 1);
                    }
                }
            },
            Action::MoveUp => match self.focus {
                Focus::Active => {
                    self.selected = self.selected.saturating_sub(1);
                }
                Focus::History => {
                    self.history_selected = self.history_selected.saturating_sub(1);
                }
            },
            Action::EnterDetail(cid) => {
                let from_history = matches!(self.focus, Focus::History);
                self.enter_detail(cid, dir);
                if from_history {
                    // History entries are complete — start at top
                    if let AppMode::Detail(ref mut detail) = self.mode {
                        detail.scroll = 0;
                        detail.auto_scroll = false;
                    }
                }
            }
            Action::ExitDetail => {
                self.mode = AppMode::Table;
            }
            Action::ScrollDown => {
                if let AppMode::Detail(ref mut detail) = self.mode {
                    detail.scroll = detail.scroll.saturating_add(1);
                    detail.auto_scroll = false;
                }
            }
            Action::ScrollUp => {
                if let AppMode::Detail(ref mut detail) = self.mode {
                    detail.scroll = detail.scroll.saturating_sub(1);
                    detail.auto_scroll = false;
                }
            }
            Action::HalfPageDown => {
                if let AppMode::Detail(ref mut detail) = self.mode {
                    let half = self.detail_inner_height / 2;
                    detail.scroll = detail.scroll.saturating_add(half.max(1));
                    detail.auto_scroll = false;
                }
            }
            Action::HalfPageUp => {
                if let AppMode::Detail(ref mut detail) = self.mode {
                    let half = self.detail_inner_height / 2;
                    detail.scroll = detail.scroll.saturating_sub(half.max(1));
                    detail.auto_scroll = false;
                }
            }
            Action::ScrollToBottom => {
                if let AppMode::Detail(ref mut detail) = self.mode {
                    detail.scroll = usize::MAX;
                    detail.auto_scroll = true;
                }
            }
            Action::PromptClearHistory => {
                self.mode = AppMode::ConfirmClearHistory;
            }
            Action::ClearHistory => {
                self.history.clear();
                self.history_offset = 0;
                self.history_selected = 0;
                let path = dir.join(HISTORY_FILE);
                let _ = File::create(&path); // truncate
                self.mode = AppMode::Table;
                self.flash = Some(("History cleared".into(), 20));
            }
            Action::CancelClear => {
                self.mode = AppMode::Table;
                self.flash = None;
            }
            Action::Flash(msg, ttl) => {
                self.flash = Some((msg, ttl));
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
        }
    }

    pub(crate) fn process_event(&mut self, server_id: &str, envelope: &EventEnvelope) {
        match &envelope.event {
            MonitorEvent::ServerStarted {
                version,
                pid,
                project,
            } => {
                if !self.server_order.contains(&server_id.to_string()) {
                    self.server_order.push(server_id.to_string());
                }
                self.servers.insert(
                    server_id.to_string(),
                    ServerState {
                        server_id: server_id.to_string(),
                        pid: *pid,
                        _version: version.clone(),
                        project: project.clone(),
                        stopped: false,
                        dead: false,
                        active_consults: HashMap::new(),
                        completed_consults: Vec::new(),
                        completed_count: 0,
                        failed_count: 0,
                        file_offset: 0,
                    },
                );
            }
            MonitorEvent::ConsultStarted { id, model, backend } => {
                if let Some(server) = self.servers.get_mut(server_id) {
                    let started_at = DateTime::parse_from_rfc3339(&envelope.ts)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    server.active_consults.insert(
                        id.clone(),
                        ActiveConsult {
                            model: model.clone(),
                            backend: backend.clone(),
                            started_at,
                            last_progress: None,
                        },
                    );
                }
            }
            MonitorEvent::ConsultProgress { id, stage } => {
                if let Some(server) = self.servers.get_mut(server_id)
                    && let Some(consult) = server.active_consults.get_mut(id)
                {
                    consult.last_progress = Some(stage.to_string());
                }
            }
            MonitorEvent::ConsultFinished {
                id,
                success,
                duration_ms,
                error,
            } => {
                if let Some(server) = self.servers.get_mut(server_id) {
                    let active = server.active_consults.remove(id);
                    if let Some(ac) = active {
                        server.completed_consults.push(CompletedConsult {
                            id: id.clone(),
                            model: ac.model,
                            backend: ac.backend,
                            duration_ms: *duration_ms,
                            success: *success,
                            error: error.clone(),
                        });
                        // Keep only last 5
                        if server.completed_consults.len() > 5 {
                            server.completed_consults.remove(0);
                        }
                    }
                    if *success {
                        server.completed_count += 1;
                    } else {
                        server.failed_count += 1;
                    }
                }
            }
            MonitorEvent::ServerStopped => {
                if let Some(server) = self.servers.get_mut(server_id) {
                    server.stopped = true;
                }
            }
        }
    }

    pub(crate) fn enter_detail(&mut self, consultation_id: String, dir: &Path) {
        let path = dir.join(format!("{consultation_id}.events.jsonl"));
        let mut events = Vec::new();
        let mut offset = 0u64;

        if let Ok(file) = File::open(&path) {
            let mut reader = BufReader::new(file);
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(bytes_read) => {
                        if !buf.ends_with('\n') {
                            break;
                        }
                        offset += bytes_read as u64;
                        if let Ok(event) = serde_json::from_str::<ParsedStreamEvent>(buf.trim()) {
                            events.push(event);
                        }
                    }
                    Err(_) => break,
                }
            }
        }

        self.mode = AppMode::Detail(DetailState {
            consultation_id,
            events,
            file_offset: offset,
            scroll: usize::MAX, // start at bottom
            auto_scroll: true,
        });
    }

    /// Return server IDs sorted by status: active first, then idle, then stopped/dead.
    /// Within each bucket, preserve insertion order as tiebreaker.
    pub(crate) fn display_server_ids(&self) -> Vec<&str> {
        let mut ids: Vec<(usize, &String)> = self
            .server_order
            .iter()
            .enumerate()
            .filter(|(_, id)| self.servers.contains_key(*id))
            .collect();

        ids.sort_by_key(|(insertion_idx, id)| {
            let server = &self.servers[*id];
            let bucket = if !server.active_consults.is_empty() {
                0 // active first
            } else if !server.stopped && !server.dead {
                1 // idle
            } else {
                2 // stopped/dead
            };
            (bucket, *insertion_idx)
        });

        ids.into_iter().map(|(_, id)| id.as_str()).collect()
    }

    /// Return active consult entries sorted by start time (oldest first).
    pub(crate) fn sorted_active_consults(server: &ServerState) -> Vec<(&String, &ActiveConsult)> {
        let mut entries: Vec<_> = server.active_consults.iter().collect();
        entries.sort_by_key(|(_, c)| c.started_at);
        entries
    }

    /// Build a list of RowInfo for the current table rows.
    pub(crate) fn build_row_infos(&self) -> Vec<RowInfo> {
        let mut infos = Vec::new();
        for server_id in self.display_server_ids() {
            let Some(server) = self.servers.get(server_id) else {
                continue;
            };

            if server.active_consults.is_empty() && server.completed_consults.is_empty() {
                infos.push(RowInfo {
                    server_id: server_id.to_string(),
                    consultation_id: String::new(),
                });
            } else {
                for (cid, _) in Self::sorted_active_consults(server) {
                    infos.push(RowInfo {
                        server_id: server_id.to_string(),
                        consultation_id: cid.clone(),
                    });
                }
                for cc in &server.completed_consults {
                    infos.push(RowInfo {
                        server_id: server_id.to_string(),
                        consultation_id: cc.id.clone(),
                    });
                }
            }
        }
        infos
    }
}
