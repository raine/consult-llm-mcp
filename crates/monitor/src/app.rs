use std::collections::HashSet;

use chrono::{DateTime, Utc};

use arboard::Clipboard;
use consult_llm_core::jsonl::read_jsonl_from_offset;
use consult_llm_core::monitoring::{
    HistoryRecord, RunEvent, RunEventKind, RunMeta, active_dir, append_history,
};
use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::action::Action;
use crate::meta::load_run_meta;
use crate::poller::PollUpdate;
use crate::state::{
    ActiveRun, AppMode, AppState, DetailState, Focus, RowInfo, ThreadDetailState, parse_rfc3339_utc,
};

impl AppState {
    pub(crate) fn apply(&mut self, action: Action) {
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
            Action::EnterDetail(run_id) => {
                let from_history = matches!(self.focus, Focus::History);
                self.enter_detail(run_id);
                if from_history && let AppMode::Detail(ref mut detail) = self.mode {
                    detail.scroll = 0;
                    detail.auto_scroll = false;
                }
                self.populate_detail_siblings();
            }
            Action::EnterThreadDetail(thread_id) => {
                self.enter_thread_detail(thread_id);
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
            Action::NextSibling | Action::PrevSibling => {
                let forward = matches!(action, Action::NextSibling);
                if let AppMode::Detail(ref detail) = self.mode {
                    let siblings = detail.siblings.clone();
                    let current_idx = detail.sibling_index;
                    if siblings.len() > 1 {
                        let next_idx = if forward {
                            (current_idx + 1) % siblings.len()
                        } else {
                            (current_idx + siblings.len() - 1) % siblings.len()
                        };
                        let next_id = siblings[next_idx].clone();
                        self.enter_detail(next_id);
                        if let AppMode::Detail(ref mut detail) = self.mode {
                            detail.siblings = siblings;
                            detail.sibling_index = next_idx;
                        }
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
                *scroll = scroll.saturating_add((height / 2).max(1));
            }),
            Action::HalfPageUp => self.mutate_scroll(|scroll, auto_scroll, height| {
                *scroll = scroll.saturating_sub((height / 2).max(1));
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
            Action::ScrollToTop => self.mutate_scroll(|scroll, auto_scroll, _| {
                *scroll = 0;
                *auto_scroll = false;
            }),
            Action::ScrollToResponse => {
                let offset = match &self.mode {
                    AppMode::Detail(detail) => detail.response_line_offset,
                    AppMode::ThreadDetail(detail) => detail.response_line_offset,
                    _ => None,
                };
                if let Some(offset) = offset {
                    self.mutate_scroll(|scroll, auto_scroll, _| {
                        *scroll = offset;
                        *auto_scroll = false;
                    });
                }
            }
            Action::PromptClearHistory => {
                self.mode = AppMode::ConfirmClearHistory;
            }
            Action::ClearHistory => {
                self.history.clear();
                self.history_selected = 0;
                self.invalidate_filter_cache();
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
            Action::ToggleSystemPrompt => {
                if let AppMode::Detail(ref mut detail) = self.mode {
                    detail.show_system_prompt = !detail.show_system_prompt;
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
                    let last_text = events.iter().rev().find_map(|event| match event {
                        ParsedStreamEvent::AssistantText { text } if !text.is_empty() => {
                            Some(text.clone())
                        }
                        _ => None,
                    });
                    match last_text {
                        Some(text) => match Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
                            Ok(()) => self.flash = Some(("Copied to clipboard".into(), 20)),
                            Err(e) => self.flash = Some((format!("Clipboard error: {e}"), 20)),
                        },
                        None => self.flash = Some(("No assistant response to copy".into(), 20)),
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
            }
            Action::FilterCancel => {
                self.filter_editing = false;
                self.filter_text.clear();
                self.invalidate_filter_cache();
                self.clamp_history_selection();
            }
            Action::PromptKillProcess(pid) => {
                self.mode = AppMode::ConfirmKillProcess(pid);
            }
            Action::KillProcess(pid) => {
                use std::process::Command;
                let result = Command::new("kill").arg(pid.to_string()).status();
                match result {
                    Ok(status) if status.success() => {
                        self.flash = Some((format!("Sent SIGTERM to PID {pid}"), 20));
                    }
                    _ => {
                        self.flash = Some((format!("Failed to kill PID {pid}"), 20));
                    }
                }
                self.mode = AppMode::Table;
            }
            Action::CancelKill => {
                self.mode = AppMode::Table;
                self.flash = None;
            }
        }
    }

    pub(crate) fn enter_detail(&mut self, run_id: String) {
        let (events, offset) = load_run_events(&run_id);
        let is_active = self.is_run_active(&run_id);

        let mut detail = DetailState {
            run_id: run_id.clone(),
            events: Vec::new(),
            file_offset: offset,
            scroll: if is_active { usize::MAX } else { 0 },
            auto_scroll: is_active,
            model: None,
            backend: None,
            reasoning_effort: None,
            started_at: None,
            duration_ms: None,
            success: None,
            project: None,
            task_mode: None,
            error: None,
            last_stage: None,
            cached_lines: None,
            cached_event_count: 0,
            cached_width: 0,
            cached_has_active_tools: false,
            show_system_prompt: false,
            response_line_offset: None,
            siblings: Vec::new(),
            sibling_index: 0,
        };

        if let Some(meta) = load_run_meta(&run_id) {
            apply_run_meta_to_detail(&mut detail, &meta);
        }
        if let Some(active) = self.active_runs.get(&run_id) {
            apply_active_run_to_detail(&mut detail, active);
        }
        if let Some(record) = self
            .history
            .iter()
            .find(|record| record.run_id.as_deref() == Some(run_id.as_str()))
        {
            apply_history_record_to_detail(&mut detail, record);
        }
        for event in events {
            apply_run_event_to_detail(&mut detail, event);
        }

        self.mode = AppMode::Detail(detail);
    }

    fn populate_detail_siblings(&mut self) {
        if let AppMode::Detail(ref detail) = self.mode {
            let project = detail.project.clone();
            let started_at = detail.started_at;
            let run_id = detail.run_id.clone();
            let siblings = self.find_siblings(project.as_deref(), started_at);
            let idx = siblings.iter().position(|id| *id == run_id).unwrap_or(0);
            if let AppMode::Detail(ref mut detail) = self.mode {
                detail.siblings = siblings;
                detail.sibling_index = idx;
            }
        }
    }

    pub(crate) fn enter_thread_detail(&mut self, thread_id: String) {
        let mut thread_records: Vec<_> = self
            .history
            .iter()
            .filter(|record| record.thread_id.as_deref() == Some(thread_id.as_str()))
            .filter(|record| record.run_id.is_some())
            .collect();
        thread_records.sort_by(|a, b| a.ts.cmp(&b.ts));

        if thread_records.is_empty() {
            self.flash = Some(("No history records for thread".into(), 20));
            return;
        }

        let active_run = self
            .active_runs
            .values()
            .filter(|run| run.thread_id.as_deref() == Some(thread_id.as_str()))
            .max_by_key(|run| run.started_at);

        let mut turn_ids: Vec<String> = Vec::new();
        let mut turn_events: Vec<Vec<ParsedStreamEvent>> = Vec::new();
        let mut turn_offsets: Vec<u64> = Vec::new();
        let mut models: Vec<String> = Vec::new();
        let mut backends: Vec<String> = Vec::new();

        for record in &thread_records {
            let Some(run_id) = record.run_id.clone() else {
                continue;
            };
            let (events, offset) = load_stream_events(&run_id);
            turn_ids.push(run_id);
            turn_events.push(events);
            turn_offsets.push(offset);
            models.push(record.model.clone());
            backends.push(record.backend.clone());
        }

        if let Some(active_run) = active_run {
            let (events, offset) = load_stream_events(&active_run.run_id);
            turn_ids.push(active_run.run_id.clone());
            turn_events.push(events);
            turn_offsets.push(offset);
            models.push(active_run.model.clone());
            backends.push(active_run.backend.clone());
        }

        let Some(active_events) = turn_events.pop() else {
            self.flash = Some(("No run events for thread".into(), 20));
            return;
        };
        let active_file_offset = turn_offsets.pop().unwrap_or(0);
        let historical_turns = turn_events;
        let turn_count = turn_ids.len();
        let total_duration_ms: u64 = thread_records.iter().map(|record| record.duration_ms).sum();
        let success = if active_run.is_some() {
            None
        } else {
            thread_records.last().map(|record| record.success)
        };
        let project = thread_records
            .first()
            .map(|record| record.project.clone())
            .or_else(|| active_run.map(|run| run.project.clone()));

        self.mode = AppMode::ThreadDetail(ThreadDetailState {
            thread_id,
            turn_ids,
            historical_turns,
            active_events,
            active_file_offset,
            turn_line_offsets: Vec::new(),
            selected_turn: turn_count.saturating_sub(1),
            scroll: if active_run.is_some() { usize::MAX } else { 0 },
            auto_scroll: active_run.is_some(),
            models,
            backends,
            project,
            total_duration_ms,
            turn_count,
            success,
            response_line_offset: None,
        });
    }

    pub(crate) fn is_run_active(&self, run_id: &str) -> bool {
        self.active_runs.contains_key(run_id)
    }

    fn find_siblings(
        &self,
        project: Option<&str>,
        reference_time: Option<DateTime<Utc>>,
    ) -> Vec<String> {
        let Some(project) = project else {
            return Vec::new();
        };

        let window_secs = 60;
        let mut seen = HashSet::new();
        let mut candidates: Vec<(String, DateTime<Utc>, bool)> = Vec::new();

        for run in self.active_runs.values() {
            if run.project == project && seen.insert(run.run_id.clone()) {
                candidates.push((run.run_id.clone(), run.started_at, true));
            }
        }

        for record in &self.history {
            if record.project != project {
                continue;
            }
            let Some(run_id) = record.run_id.clone() else {
                continue;
            };
            if !seen.insert(run_id.clone()) {
                continue;
            }
            if let Some(started_at) = history_started_at(record) {
                candidates.push((run_id, started_at, false));
            }
        }

        if let Some(reference_time) = reference_time {
            candidates.retain(|(_, started_at, is_active)| {
                *is_active
                    || reference_time
                        .signed_duration_since(*started_at)
                        .num_seconds()
                        .unsigned_abs()
                        <= window_secs
            });
        }

        candidates.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.1.cmp(&a.1)));
        candidates
            .into_iter()
            .map(|(run_id, _, _)| run_id)
            .collect()
    }

    fn mutate_scroll(&mut self, f: impl Fn(&mut usize, &mut bool, usize)) {
        let height = self.detail_inner_height;
        match &mut self.mode {
            AppMode::Detail(detail) => f(&mut detail.scroll, &mut detail.auto_scroll, height),
            AppMode::ThreadDetail(detail) => f(&mut detail.scroll, &mut detail.auto_scroll, height),
            _ => {}
        }
    }

    fn clamp_history_selection(&mut self) {
        let count = self.build_history_display_rows().len();
        if count == 0 {
            self.history_selected = 0;
        } else if self.history_selected >= count {
            self.history_selected = count - 1;
        }
    }

    pub(crate) fn apply_poll_update(&mut self, update: PollUpdate) {
        match update {
            PollUpdate::ActiveRunAdded(snapshot) | PollUpdate::ActiveRunUpdated(snapshot) => {
                let run = ActiveRun::from(snapshot);
                self.active_runs.insert(run.run_id.clone(), run);
                self.sort_active_order();
            }
            PollUpdate::ActiveRunRemoved(run_id) => {
                self.active_runs.remove(&run_id);
                self.active_order.retain(|id| id != &run_id);
            }
            PollUpdate::OrphanDetected(run_id) => {
                if let Some(run) = self.active_runs.get_mut(&run_id) {
                    run.orphaned = true;
                    run.last_stage = Some("orphaned".into());
                }
                if let AppMode::Detail(ref mut detail) = self.mode
                    && detail.run_id == run_id
                {
                    detail.last_stage = Some("orphaned".into());
                    detail.success = Some(false);
                    detail.error = Some("process died without completing".into());
                }
                self.persist_orphan(&run_id);
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
            PollUpdate::DetailMetadata(meta) => {
                if let AppMode::Detail(ref mut detail) = self.mode
                    && detail.run_id == meta.run_id
                {
                    apply_run_meta_to_detail(detail, &meta);
                }
            }
            PollUpdate::DetailEvents { run_id, events } => match &mut self.mode {
                AppMode::Detail(detail) if detail.run_id == run_id => {
                    if !events.is_empty() {
                        for event in events {
                            apply_run_event_to_detail(detail, event);
                        }
                        detail.cached_lines = None;
                        if detail.auto_scroll {
                            detail.scroll = usize::MAX;
                        }
                    }
                }
                AppMode::ThreadDetail(detail)
                    if detail.turn_ids.last().is_some_and(|last| *last == run_id) =>
                {
                    if !events.is_empty() {
                        for event in events {
                            apply_run_event_to_thread_detail(detail, event);
                        }
                        if detail.auto_scroll {
                            detail.scroll = usize::MAX;
                        }
                    }
                }
                _ => {}
            },
        }
    }

    pub(crate) fn build_row_infos(&self) -> Vec<RowInfo> {
        self.active_order
            .iter()
            .cloned()
            .map(|run_id| RowInfo { run_id })
            .collect()
    }

    fn persist_orphan(&mut self, run_id: &str) {
        if self
            .history
            .iter()
            .any(|record| record.run_id.as_deref() == Some(run_id))
        {
            let _ = std::fs::remove_file(active_dir().join(format!("{run_id}.json")));
            return;
        }

        let meta = load_run_meta(run_id).or_else(|| {
            self.active_runs.get(run_id).map(|run| RunMeta {
                v: 1,
                run_id: run.run_id.clone(),
                pid: run.pid,
                started_at: run
                    .started_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                project: run.project.clone(),
                cwd: String::new(),
                model: run.model.clone(),
                backend: run.backend.clone(),
                thread_id: run.thread_id.clone(),
                task_mode: run.task_mode.clone(),
                reasoning_effort: run.reasoning_effort.clone(),
            })
        });

        let Some(meta) = meta else {
            return;
        };

        let finished_at = Utc::now();
        let duration_ms = parse_rfc3339_utc(&meta.started_at)
            .map(|started_at| {
                finished_at
                    .signed_duration_since(started_at)
                    .num_milliseconds()
                    .max(0) as u64
            })
            .unwrap_or(0);

        let record = HistoryRecord {
            ts: finished_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            run_id: Some(run_id.to_string()),
            project: meta.project,
            model: meta.model,
            backend: meta.backend,
            duration_ms,
            success: false,
            error: Some("process died without completing".into()),
            tokens_in: None,
            tokens_out: None,
            parsed_ts: Some(finished_at),
            thread_id: meta.thread_id,
            reasoning_effort: meta.reasoning_effort,
            task_mode: meta.task_mode,
        };
        append_history(&record);
        let _ = std::fs::remove_file(active_dir().join(format!("{run_id}.json")));
    }
}

fn load_run_events(run_id: &str) -> (Vec<RunEvent>, u64) {
    let mut offset = 0u64;
    let path = consult_llm_core::monitoring::runs_dir().join(format!("{run_id}.events.jsonl"));
    let events = read_jsonl_from_offset(&path, &mut offset);
    (events, offset)
}

fn load_stream_events(run_id: &str) -> (Vec<ParsedStreamEvent>, u64) {
    let (events, offset) = load_run_events(run_id);
    (
        events
            .into_iter()
            .filter_map(|event| match event.kind {
                RunEventKind::Stream { event } => Some(event),
                _ => None,
            })
            .collect(),
        offset,
    )
}

fn history_started_at(record: &HistoryRecord) -> Option<DateTime<Utc>> {
    let finished_at = record.parsed_ts.or_else(|| parse_rfc3339_utc(&record.ts))?;
    Some(finished_at - chrono::Duration::milliseconds(record.duration_ms as i64))
}

fn apply_active_run_to_detail(detail: &mut DetailState, run: &ActiveRun) {
    detail.model.get_or_insert(run.model.clone());
    detail.backend.get_or_insert(run.backend.clone());
    detail.reasoning_effort = detail
        .reasoning_effort
        .clone()
        .or_else(|| run.reasoning_effort.clone());
    detail.started_at.get_or_insert(run.started_at);
    detail.project.get_or_insert(run.project.clone());
    detail.task_mode = detail.task_mode.clone().or_else(|| run.task_mode.clone());
    detail.last_stage = detail.last_stage.clone().or_else(|| run.last_stage.clone());
    if run.orphaned {
        detail.last_stage = Some("orphaned".into());
        detail.error = Some("process died without completing".into());
    }
}

fn apply_run_meta_to_detail(detail: &mut DetailState, meta: &RunMeta) {
    detail.model = Some(meta.model.clone());
    detail.backend = Some(meta.backend.clone());
    detail.reasoning_effort = meta.reasoning_effort.clone();
    detail.project = Some(meta.project.clone());
    detail.task_mode = meta.task_mode.clone();
    detail.started_at = parse_rfc3339_utc(&meta.started_at);
}

fn apply_history_record_to_detail(detail: &mut DetailState, record: &HistoryRecord) {
    detail.model.get_or_insert(record.model.clone());
    detail.backend.get_or_insert(record.backend.clone());
    detail.reasoning_effort = detail
        .reasoning_effort
        .clone()
        .or_else(|| record.reasoning_effort.clone());
    detail.project.get_or_insert(record.project.clone());
    detail.task_mode = detail
        .task_mode
        .clone()
        .or_else(|| record.task_mode.clone());
    detail.started_at = detail.started_at.or_else(|| history_started_at(record));
    detail.duration_ms = detail.duration_ms.or(Some(record.duration_ms));
    detail.success = detail.success.or(Some(record.success));
    detail.error = detail.error.clone().or_else(|| record.error.clone());
}

fn apply_run_event_to_detail(detail: &mut DetailState, event: RunEvent) {
    match event.kind {
        RunEventKind::RunStarted => {
            detail.started_at = detail.started_at.or_else(|| parse_rfc3339_utc(&event.ts));
        }
        RunEventKind::Progress { stage } => {
            detail.last_stage = Some(stage.to_string());
        }
        RunEventKind::Stream { event } => {
            detail.events.push(event);
        }
        RunEventKind::RunFinished {
            duration_ms,
            success,
            error,
        } => {
            detail.duration_ms = Some(duration_ms);
            detail.success = Some(success);
            detail.error = error;
        }
    }
}

fn apply_run_event_to_thread_detail(detail: &mut ThreadDetailState, event: RunEvent) {
    match event.kind {
        RunEventKind::Stream { event } => detail.active_events.push(event),
        RunEventKind::RunFinished {
            duration_ms,
            success,
            ..
        } => {
            detail.total_duration_ms = detail.total_duration_ms.saturating_add(duration_ms);
            detail.success = Some(success);
        }
        RunEventKind::RunStarted | RunEventKind::Progress { .. } => {}
    }
}
