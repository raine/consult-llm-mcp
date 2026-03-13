use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Utc};

use arboard::Clipboard;
use consult_llm_core::jsonl::read_jsonl_from_offset;
use consult_llm_core::monitoring::{EventEnvelope, MonitorEvent};
use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::action::Action;
use crate::poller::PollUpdate;
use crate::state::{
    ActiveConsult, AppMode, AppState, CompletedConsult, DetailMetadata, DetailState, Focus,
    RowInfo, ServerState, ThreadDetailState,
};

impl AppState {
    pub(crate) fn apply(&mut self, action: Action, dir: &Path) {
        match action {
            Action::Quit => unreachable!("handled in main loop"),
            Action::ToggleFocus => {
                self.focus = match self.focus {
                    Focus::Active => {
                        self.history_selected = 0;
                        Focus::History
                    }
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
                    let count = self.build_history_display_rows().len();
                    if count > 0 {
                        self.history_selected = (self.history_selected + 1).min(count - 1);
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
            Action::EnterThreadDetail(thread_id) => {
                self.enter_thread_detail(thread_id, dir);
            }
            Action::PrevTurn => {
                if let AppMode::ThreadDetail(ref mut detail) = self.mode
                    && detail.selected_turn > 0
                {
                    detail.selected_turn -= 1;
                    if let Some(&offset) = detail.turn_line_offsets.get(detail.selected_turn) {
                        detail.scroll = offset;
                        detail.auto_scroll = false;
                    }
                }
            }
            Action::NextTurn => {
                if let AppMode::ThreadDetail(ref mut detail) = self.mode
                    && detail.selected_turn + 1 < detail.turn_count
                {
                    detail.selected_turn += 1;
                    if let Some(&offset) = detail.turn_line_offsets.get(detail.selected_turn) {
                        detail.scroll = offset;
                        detail.auto_scroll = false;
                    }
                }
            }
            Action::ExitDetail => {
                self.mode = AppMode::Table;
            }
            Action::ScrollDown => self.mutate_scroll(|scroll, _, _| {
                *scroll = scroll.saturating_add(1);
            }),
            Action::ScrollUp => self.mutate_scroll(|scroll, auto_scroll, _| {
                *scroll = scroll.saturating_sub(1);
                *auto_scroll = false;
            }),
            Action::HalfPageDown => self.mutate_scroll(|scroll, _, height| {
                let half = height / 2;
                *scroll = scroll.saturating_add(half.max(1));
            }),
            Action::HalfPageUp => self.mutate_scroll(|scroll, auto_scroll, height| {
                let half = height / 2;
                *scroll = scroll.saturating_sub(half.max(1));
                *auto_scroll = false;
            }),
            Action::PageDown => self.mutate_scroll(|scroll, _, height| {
                *scroll = scroll.saturating_add(height.max(1));
            }),
            Action::PageUp => self.mutate_scroll(|scroll, auto_scroll, height| {
                *scroll = scroll.saturating_sub(height.max(1));
                *auto_scroll = false;
            }),
            Action::ScrollToBottom => self.mutate_scroll(|scroll, auto_scroll, _| {
                *scroll = usize::MAX;
                *auto_scroll = true;
            }),
            Action::PromptClearHistory => {
                self.mode = AppMode::ConfirmClearHistory;
            }
            Action::ClearHistory => {
                self.history.clear();
                self.history_selected = 0;
                self.invalidate_filter_cache();
                self.mode = AppMode::Table;
                self.flash = Some(("History cleared".into(), 20));
                // File truncation is handled by the caller (main loop)
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
            Action::ToggleSystemPrompt => {
                if let AppMode::Detail(ref mut detail) = self.mode {
                    detail.show_system_prompt = !detail.show_system_prompt;
                    // Invalidate render cache since content changed
                    detail.cached_lines = None;
                }
            }
            Action::YankResponse => {
                let events: Option<&[ParsedStreamEvent]> = match &self.mode {
                    AppMode::Detail(detail) => Some(&detail.events),
                    AppMode::ThreadDetail(detail) => Some(&detail.active_events),
                    _ => None,
                };
                if let Some(events) = events {
                    let last_text = events.iter().rev().find_map(|e| match e {
                        ParsedStreamEvent::AssistantText { text } if !text.is_empty() => {
                            Some(text.clone())
                        }
                        _ => None,
                    });

                    match last_text {
                        Some(text) => match Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
                            Ok(()) => {
                                self.flash = Some(("Copied to clipboard".into(), 20));
                            }
                            Err(e) => {
                                self.flash = Some((format!("Clipboard error: {e}"), 20));
                            }
                        },
                        None => {
                            self.flash = Some(("No assistant response to copy".into(), 20));
                        }
                    }
                }
            }
            Action::StartFilter => {
                self.filter_editing = true;
                self.focus = Focus::History;
            }
            Action::FilterInput(c) => {
                self.filter_text.push(c);
                self.invalidate_filter_cache();
                self.clamp_history_selection();
            }
            Action::FilterBackspace => {
                self.filter_text.pop();
                self.invalidate_filter_cache();
                self.clamp_history_selection();
            }
            Action::FilterAccept => {
                self.filter_editing = false;
                // keep filter_text active
            }
            Action::FilterCancel => {
                self.filter_editing = false;
                self.filter_text.clear();
                self.invalidate_filter_cache();
                self.clamp_history_selection();
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
                    },
                );
            }
            MonitorEvent::ConsultStarted {
                id,
                model,
                backend,
                thread_id,
                task_mode,
                reasoning_effort,
            } => {
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
                            thread_id: thread_id.clone(),
                            task_mode: task_mode.clone(),
                            reasoning_effort: reasoning_effort.clone(),
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
                            task_mode: ac.task_mode,
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
        let mut offset = 0u64;
        let events: Vec<ParsedStreamEvent> = read_jsonl_from_offset(&path, &mut offset);

        let is_active = self.is_consultation_active(&consultation_id);

        // Look up metadata from active consults, completed consults, or history
        let meta = self.lookup_consult_metadata(&consultation_id);

        self.mode = AppMode::Detail(DetailState {
            consultation_id,
            events,
            file_offset: offset,
            scroll: if is_active { usize::MAX } else { 0 },
            auto_scroll: is_active,
            model: meta.model,
            backend: meta.backend,
            reasoning_effort: meta.reasoning_effort,
            started_at: meta.started_at,
            duration_ms: meta.duration_ms,
            success: meta.success,
            project: meta.project,
            task_mode: meta.task_mode,
            cached_lines: None,
            cached_event_count: 0,
            cached_width: 0,
            cached_has_active_tools: false,
            show_system_prompt: false,
        });
    }

    pub(crate) fn enter_thread_detail(&mut self, thread_id: String, dir: &Path) {
        // Collect all history records for this thread, sorted chronologically (oldest first)
        let mut thread_records: Vec<_> = self
            .history
            .iter()
            .filter(|h| h.thread_id.as_deref() == Some(&thread_id))
            .collect();
        thread_records.sort_by(|a, b| a.ts.cmp(&b.ts));

        if thread_records.is_empty() {
            self.flash = Some(("No history records for thread".into(), 20));
            return;
        }

        let turn_ids: Vec<String> = thread_records
            .iter()
            .filter_map(|r| r.consultation_id.clone())
            .collect();

        // Load events from all completed turns (all except the last)
        let mut historical_turns: Vec<Vec<ParsedStreamEvent>> = Vec::new();
        for cid in turn_ids.iter().take(turn_ids.len().saturating_sub(1)) {
            let mut turn_events = Vec::new();
            let path = dir.join(format!("{cid}.events.jsonl"));
            if let Ok(file) = File::open(&path) {
                let reader = BufReader::new(file);
                for line in reader.lines().map_while(Result::ok) {
                    if let Ok(event) = serde_json::from_str::<ParsedStreamEvent>(line.trim()) {
                        turn_events.push(event);
                    }
                }
            }
            historical_turns.push(turn_events);
        }

        // Load the latest turn's events with offset tracking
        let mut active_events = Vec::new();
        let mut active_file_offset = 0u64;
        if let Some(last_cid) = turn_ids.last() {
            let path = dir.join(format!("{last_cid}.events.jsonl"));
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
                            active_file_offset += bytes_read as u64;
                            if let Ok(event) = serde_json::from_str::<ParsedStreamEvent>(buf.trim())
                            {
                                active_events.push(event);
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }

        let turn_count = turn_ids.len();
        let total_duration_ms: u64 = thread_records.iter().map(|r| r.duration_ms).sum();
        let models: Vec<String> = thread_records
            .iter()
            .map(|r| r.model.clone())
            .collect::<Vec<_>>();
        let backends: Vec<String> = thread_records
            .iter()
            .map(|r| r.backend.clone())
            .collect::<Vec<_>>();
        let success = thread_records.last().map(|r| r.success);
        let project = Some(thread_records[0].project.clone());

        // Check if the latest turn is still active
        let is_active = turn_ids
            .last()
            .is_some_and(|cid| self.is_consultation_active(cid));

        self.mode = AppMode::ThreadDetail(ThreadDetailState {
            thread_id,
            turn_ids,
            historical_turns,
            active_events,
            active_file_offset,
            turn_line_offsets: Vec::new(), // computed during rendering
            selected_turn: turn_count.saturating_sub(1),
            scroll: if is_active { usize::MAX } else { 0 },
            auto_scroll: is_active,
            models,
            backends,
            project,
            total_duration_ms,
            turn_count,
            success,
        });
    }

    fn lookup_consult_metadata(&self, consultation_id: &str) -> DetailMetadata {
        // Check active consults
        for server in self.servers.values() {
            if let Some(ac) = server.active_consults.get(consultation_id) {
                return DetailMetadata {
                    model: Some(ac.model.clone()),
                    backend: Some(ac.backend.clone()),
                    started_at: Some(ac.started_at),
                    duration_ms: None,
                    success: None,
                    project: server.project.clone(),
                    task_mode: ac.task_mode.clone(),
                    reasoning_effort: ac.reasoning_effort.clone(),
                };
            }
        }
        // Check completed consults
        for server in self.servers.values() {
            if let Some(cc) = server
                .completed_consults
                .iter()
                .find(|c| c.id == consultation_id)
            {
                return DetailMetadata {
                    model: Some(cc.model.clone()),
                    backend: Some(cc.backend.clone()),
                    started_at: None,
                    duration_ms: Some(cc.duration_ms),
                    success: Some(cc.success),
                    project: server.project.clone(),
                    task_mode: cc.task_mode.clone(),
                    reasoning_effort: None,
                };
            }
        }
        // Check history
        if let Some(hr) = self
            .history
            .iter()
            .find(|h| h.consultation_id.as_deref() == Some(consultation_id))
        {
            let started_at = DateTime::parse_from_rfc3339(&hr.ts)
                .map(|dt| dt.with_timezone(&Utc))
                .ok();
            return DetailMetadata {
                model: Some(hr.model.clone()),
                backend: Some(hr.backend.clone()),
                started_at,
                duration_ms: Some(hr.duration_ms),
                success: Some(hr.success),
                project: Some(hr.project.clone()),
                task_mode: None,
                reasoning_effort: hr.reasoning_effort.clone(),
            };
        }
        DetailMetadata {
            model: None,
            backend: None,
            started_at: None,
            duration_ms: None,
            success: None,
            project: None,
            task_mode: None,
            reasoning_effort: None,
        }
    }

    /// Check if a consultation is still active (running) in any server.
    pub(crate) fn is_consultation_active(&self, consultation_id: &str) -> bool {
        self.servers
            .values()
            .any(|s| s.active_consults.contains_key(consultation_id))
    }

    /// Apply a scroll mutation to whichever detail mode is active.
    fn mutate_scroll(&mut self, f: impl Fn(&mut usize, &mut bool, usize)) {
        let height = self.detail_inner_height;
        match &mut self.mode {
            AppMode::Detail(d) => f(&mut d.scroll, &mut d.auto_scroll, height),
            AppMode::ThreadDetail(d) => f(&mut d.scroll, &mut d.auto_scroll, height),
            _ => {}
        }
    }

    /// Clamp history_selected to the filtered list length.
    fn clamp_history_selection(&mut self) {
        let count = self.build_history_display_rows().len();
        if count == 0 {
            self.history_selected = 0;
        } else if self.history_selected >= count {
            self.history_selected = count - 1;
        }
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
                0 // active consultations
            } else if !server.completed_consults.is_empty() {
                1 // recently completed consultations
            } else if !server.stopped && !server.dead {
                2 // idle, no consultations
            } else {
                3 // stopped/dead
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

    /// Apply a state update received from the background poller.
    pub(crate) fn apply_poll_update(&mut self, update: PollUpdate) {
        match update {
            PollUpdate::Events(events) => {
                for (server_id, envelope) in &events {
                    self.process_event(server_id, envelope);
                }
            }
            PollUpdate::HistoryRecords(records) => {
                if !records.is_empty() {
                    self.invalidate_filter_cache();
                }
                for record in records {
                    self.history.push_front(record);
                    if !self.history.is_empty() {
                        self.history_selected =
                            (self.history_selected + 1).min(self.history.len() - 1);
                    }
                    if self.history.len() > 100 {
                        self.history.pop_back();
                        if self.history_selected >= self.history.len() {
                            self.history_selected = self.history.len().saturating_sub(1);
                        }
                    }
                }
            }
            PollUpdate::Deaths(deaths) => {
                for server_id in &deaths {
                    if let Some(server) = self.servers.get_mut(server_id) {
                        server.dead = true;
                        server.active_consults.clear();
                    }
                }
            }
            PollUpdate::Pruned(pruned_ids) => {
                for id in &pruned_ids {
                    self.servers.remove(id);
                }
                self.server_order.retain(|id| self.servers.contains_key(id));
            }
            PollUpdate::DetailEvents {
                consultation_id,
                events,
            } => {
                match &mut self.mode {
                    AppMode::Detail(detail)
                        if detail.consultation_id == consultation_id && !events.is_empty() =>
                    {
                        detail.events.extend(events);
                        if detail.auto_scroll {
                            detail.scroll = usize::MAX;
                        }
                    }
                    AppMode::ThreadDetail(detail) if !events.is_empty() => {
                        // Only accept events for the latest turn
                        if let Some(last_turn) = detail.turn_ids.last()
                            && *last_turn == consultation_id
                        {
                            detail.active_events.extend(events);
                            if detail.auto_scroll {
                                detail.scroll = usize::MAX;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
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
                // Deduplicate completed consults by backend (last per backend),
                // matching the table renderer's display logic
                let mut seen_backends = std::collections::HashSet::new();
                let deduped: Vec<_> = server
                    .completed_consults
                    .iter()
                    .rev()
                    .filter(|cc| seen_backends.insert(&cc.backend))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                for cc in deduped {
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
