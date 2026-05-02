use chrono::Utc;

use consult_llm_core::monitoring::{HistoryRecord, RunMeta, active_dir, append_history};

use crate::meta::load_run_meta;
use crate::poller::PollUpdate;
use crate::state::{ActiveRun, AppMode, AppState, parse_rfc3339_utc};

use super::detail_load::{
    apply_run_event_to_detail, apply_run_event_to_thread_detail, apply_run_meta_to_detail,
};

impl AppState {
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
