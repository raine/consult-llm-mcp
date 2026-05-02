use std::collections::HashSet;

use chrono::{DateTime, Utc};

use consult_llm_core::jsonl::read_jsonl_from_offset;
use consult_llm_core::monitoring::{HistoryRecord, RunEvent, RunEventKind, RunMeta};
use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::meta::load_run_meta;
use crate::state::{
    ActiveRun, AppMode, AppState, DetailState, ThreadDetailState, parse_rfc3339_utc,
};

impl AppState {
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

    pub(super) fn populate_detail_siblings(&mut self) {
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

pub(super) fn history_started_at(record: &HistoryRecord) -> Option<DateTime<Utc>> {
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

pub(super) fn apply_run_meta_to_detail(detail: &mut DetailState, meta: &RunMeta) {
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

pub(super) fn apply_run_event_to_detail(detail: &mut DetailState, event: RunEvent) {
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

pub(super) fn apply_run_event_to_thread_detail(detail: &mut ThreadDetailState, event: RunEvent) {
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
