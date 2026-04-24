use std::collections::{HashMap, HashSet, VecDeque};

use chrono::{DateTime, Utc};
use ratatui::style::Color;
use ratatui::text::Line as RatatuiLine;
use ratatui::widgets::TableState;

use consult_llm_core::llm_cost::calculate_cost;
use consult_llm_core::monitoring::{ActiveSnapshot, HistoryRecord};
use consult_llm_core::stream_events::ParsedStreamEvent;

pub(crate) const TEAL: Color = Color::Rgb(78, 201, 176);
pub(crate) const WHITE: Color = Color::Rgb(255, 255, 255);
pub(crate) const DIM_WHITE: Color = Color::Rgb(180, 190, 200);
pub(crate) const SEPARATOR: Color = Color::Rgb(80, 80, 80);
pub(crate) const BG: Color = Color::Rgb(18, 18, 22);
pub(crate) const GREEN: Color = Color::Rgb(120, 200, 120);
pub(crate) const RED: Color = Color::Rgb(220, 120, 120);
pub(crate) const YELLOW: Color = Color::Rgb(220, 200, 100);
pub(crate) const DIM: Color = Color::Rgb(100, 100, 110);
pub(crate) const SELECTED_BG: Color = Color::Rgb(40, 40, 50);

pub(crate) fn task_mode_color(mode: Option<&str>) -> Color {
    match mode {
        Some("review") => Color::Rgb(130, 180, 230),
        Some("debug") => Color::Rgb(220, 150, 120),
        Some("plan") => Color::Rgb(170, 140, 210),
        Some("create") => Color::Rgb(120, 200, 160),
        _ => DIM_WHITE,
    }
}

pub(crate) const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

#[allow(clippy::large_enum_variant)]
pub(crate) enum AppMode {
    Table,
    Detail(DetailState),
    ThreadDetail(ThreadDetailState),
    ConfirmClearHistory,
    ConfirmKillProcess(u32),
}

pub(crate) struct ThreadDetailState {
    pub(crate) thread_id: String,
    pub(crate) turn_ids: Vec<String>,
    pub(crate) historical_turns: Vec<Vec<ParsedStreamEvent>>,
    pub(crate) active_events: Vec<ParsedStreamEvent>,
    pub(crate) active_file_offset: u64,
    pub(crate) turn_line_offsets: Vec<usize>,
    pub(crate) selected_turn: usize,
    pub(crate) scroll: usize,
    pub(crate) auto_scroll: bool,
    pub(crate) models: Vec<String>,
    pub(crate) backends: Vec<String>,
    pub(crate) project: Option<String>,
    pub(crate) total_duration_ms: u64,
    pub(crate) turn_count: usize,
    pub(crate) success: Option<bool>,
    pub(crate) response_line_offset: Option<usize>,
}

pub(crate) struct DetailState {
    pub(crate) run_id: String,
    pub(crate) events: Vec<ParsedStreamEvent>,
    pub(crate) file_offset: u64,
    pub(crate) scroll: usize,
    pub(crate) auto_scroll: bool,
    pub(crate) model: Option<String>,
    pub(crate) backend: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) success: Option<bool>,
    pub(crate) project: Option<String>,
    pub(crate) task_mode: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) last_stage: Option<String>,
    pub(crate) cached_lines: Option<Vec<RatatuiLine<'static>>>,
    pub(crate) cached_event_count: usize,
    pub(crate) cached_width: usize,
    pub(crate) cached_has_active_tools: bool,
    pub(crate) show_system_prompt: bool,
    pub(crate) response_line_offset: Option<usize>,
    pub(crate) siblings: Vec<String>,
    pub(crate) sibling_index: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Focus {
    Active,
    History,
}

pub(crate) struct ActiveRun {
    pub(crate) run_id: String,
    pub(crate) pid: u32,
    pub(crate) project: String,
    pub(crate) model: String,
    pub(crate) backend: String,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) last_stage: Option<String>,
    pub(crate) thread_id: Option<String>,
    pub(crate) task_mode: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) last_seq: u64,
    pub(crate) last_event_at: DateTime<Utc>,
    pub(crate) orphaned: bool,
}

impl From<ActiveSnapshot> for ActiveRun {
    fn from(snapshot: ActiveSnapshot) -> Self {
        Self {
            run_id: snapshot.run_id,
            pid: snapshot.pid,
            project: snapshot.project,
            model: snapshot.model,
            backend: snapshot.backend,
            started_at: parse_rfc3339_utc(&snapshot.started_at).unwrap_or_else(Utc::now),
            last_stage: snapshot.stage.map(|stage| stage.to_string()),
            thread_id: snapshot.thread_id,
            task_mode: snapshot.task_mode,
            reasoning_effort: snapshot.reasoning_effort,
            last_seq: snapshot.last_seq,
            last_event_at: parse_rfc3339_utc(&snapshot.last_event_at).unwrap_or_else(Utc::now),
            orphaned: false,
        }
    }
}

pub(crate) struct AppState {
    pub(crate) active_runs: HashMap<String, ActiveRun>,
    pub(crate) active_order: Vec<String>,
    pub(crate) selected: usize,
    pub(crate) row_count: usize,
    pub(crate) tick: usize,
    pub(crate) table_state: TableState,
    pub(crate) focus: Focus,
    pub(crate) history_selected: usize,
    pub(crate) history_table_state: TableState,
    pub(crate) mode: AppMode,
    pub(crate) history: VecDeque<HistoryRecord>,
    pub(crate) flash: Option<(String, u8)>,
    pub(crate) detail_inner_height: usize,
    pub(crate) show_help: bool,
    pub(crate) filter_text: String,
    pub(crate) filter_editing: bool,
    pub(crate) cached_filter_indices: Option<Vec<usize>>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct RowInfo {
    pub(crate) run_id: String,
}

pub(crate) enum HistoryDisplayRow {
    Single(usize),
    ThreadSummary {
        thread_id: String,
        latest_parsed_ts: Option<DateTime<Utc>>,
        model: String,
        backend: String,
        total_duration_ms: u64,
        total_tokens_in: Option<u64>,
        total_tokens_out: Option<u64>,
        total_cost: Option<f64>,
        turn_count: usize,
        success: bool,
        mixed_model: bool,
        project: String,
    },
}

impl AppState {
    pub(crate) fn new() -> Self {
        Self {
            active_runs: HashMap::new(),
            active_order: Vec::new(),
            selected: 0,
            row_count: 0,
            tick: 0,
            table_state: TableState::default(),
            focus: Focus::Active,
            history_selected: 0,
            history_table_state: TableState::default(),
            mode: AppMode::Table,
            history: VecDeque::new(),
            flash: None,
            detail_inner_height: 0,
            show_help: false,
            filter_text: String::new(),
            filter_editing: false,
            cached_filter_indices: None,
        }
    }

    pub(crate) fn filtered_history_indices(&self) -> &[usize] {
        self.cached_filter_indices.as_deref().unwrap_or(&[])
    }

    pub(crate) fn ensure_filter_cache(&mut self) {
        if self.cached_filter_indices.is_some() {
            return;
        }

        let indices = if self.filter_text.is_empty() {
            (0..self.history.len()).collect()
        } else {
            let needle = self.filter_text.to_lowercase();
            self.history
                .iter()
                .enumerate()
                .filter(|(_, record)| {
                    record.project.to_lowercase().contains(&needle)
                        || record.model.to_lowercase().contains(&needle)
                        || record.backend.to_lowercase().contains(&needle)
                        || record
                            .thread_id
                            .as_deref()
                            .is_some_and(|thread| thread.to_lowercase().contains(&needle))
                        || record
                            .consultation_id
                            .as_deref()
                            .is_some_and(|run_id| run_id.to_lowercase().contains(&needle))
                        || (record.success && "success".contains(&needle))
                        || (!record.success && "failed".contains(&needle))
                })
                .map(|(index, _)| index)
                .collect()
        };

        self.cached_filter_indices = Some(indices);
    }

    pub(crate) fn invalidate_filter_cache(&mut self) {
        self.cached_filter_indices = None;
    }

    pub(crate) fn sort_active_order(&mut self) {
        let mut ids: Vec<_> = self.active_runs.keys().cloned().collect();
        ids.sort_by(|a, b| {
            let left = self.active_runs.get(a).unwrap();
            let right = self.active_runs.get(b).unwrap();
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| right.last_event_at.cmp(&left.last_event_at))
                .then_with(|| right.last_seq.cmp(&left.last_seq))
                .then_with(|| a.cmp(b))
        });
        self.active_order = ids;
    }

    pub(crate) fn build_history_display_rows(&self) -> Vec<HistoryDisplayRow> {
        let filtered = self.filtered_history_indices();

        let mut thread_groups: HashMap<String, Vec<usize>> = HashMap::new();
        let mut seen_threads: Vec<String> = Vec::new();
        let mut single_rows: Vec<(usize, usize)> = Vec::new();

        for (position, &idx) in filtered.iter().enumerate() {
            let record = &self.history[idx];
            if let Some(ref thread_id) = record.thread_id {
                if !thread_groups.contains_key(thread_id) {
                    seen_threads.push(thread_id.clone());
                }
                thread_groups
                    .entry(thread_id.clone())
                    .or_default()
                    .push(idx);
            } else {
                single_rows.push((position, idx));
            }
        }

        let mut rows_with_pos: Vec<(usize, HistoryDisplayRow)> = Vec::new();

        for (position, idx) in single_rows {
            rows_with_pos.push((position, HistoryDisplayRow::Single(idx)));
        }

        for thread_id in &seen_threads {
            let indices = &thread_groups[thread_id];
            if indices.len() == 1 {
                let pos = filtered.iter().position(|&i| i == indices[0]).unwrap_or(0);
                rows_with_pos.push((pos, HistoryDisplayRow::Single(indices[0])));
                continue;
            }

            let position = indices
                .iter()
                .filter_map(|&i| filtered.iter().position(|&filtered_idx| filtered_idx == i))
                .min()
                .unwrap_or(0);

            let mut sorted_indices = indices.clone();
            sorted_indices.sort_by(|&a, &b| self.history[a].ts.cmp(&self.history[b].ts));

            let latest = &self.history[sorted_indices[sorted_indices.len() - 1]];
            let first = &self.history[sorted_indices[0]];
            let models: HashSet<&str> = sorted_indices
                .iter()
                .map(|&i| self.history[i].model.as_str())
                .collect();

            let total_tokens_in = sorted_indices
                .iter()
                .filter_map(|&i| self.history[i].tokens_in)
                .reduce(|a, b| a + b);
            let total_tokens_out = sorted_indices
                .iter()
                .filter_map(|&i| self.history[i].tokens_out)
                .reduce(|a, b| a + b);

            let total_cost = {
                let mut sum = 0.0;
                let mut any = false;
                for &i in &sorted_indices {
                    let record = &self.history[i];
                    if record.backend == "api"
                        && let (Some(tokens_in), Some(tokens_out)) =
                            (record.tokens_in, record.tokens_out)
                    {
                        let cost = calculate_cost(tokens_in, tokens_out, &record.model);
                        if cost.total_cost > 0.0 {
                            sum += cost.total_cost;
                            any = true;
                        }
                    }
                }
                if any { Some(sum) } else { None }
            };

            rows_with_pos.push((
                position,
                HistoryDisplayRow::ThreadSummary {
                    thread_id: thread_id.clone(),
                    latest_parsed_ts: latest.parsed_ts,
                    model: latest.model.clone(),
                    backend: latest.backend.clone(),
                    total_duration_ms: indices.iter().map(|&i| self.history[i].duration_ms).sum(),
                    total_tokens_in,
                    total_tokens_out,
                    total_cost,
                    turn_count: indices.len(),
                    success: latest.success,
                    mixed_model: models.len() > 1,
                    project: first.project.clone(),
                },
            ));
        }

        rows_with_pos.sort_by_key(|(position, _)| *position);
        rows_with_pos.into_iter().map(|(_, row)| row).collect()
    }
}

pub(crate) fn parse_rfc3339_utc(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_snapshot_converts_to_active_run() {
        let run = ActiveRun::from(ActiveSnapshot {
            v: 1,
            run_id: "run-1".into(),
            pid: 42,
            started_at: "2026-04-24T01:02:03.000Z".into(),
            model: "gpt-5".into(),
            backend: "api".into(),
            project: "proj".into(),
            thread_id: Some("thread-1".into()),
            task_mode: Some("review".into()),
            reasoning_effort: Some("high".into()),
            last_seq: 7,
            last_event_at: "2026-04-24T01:02:04.000Z".into(),
            stage: None,
        });

        assert_eq!(run.run_id, "run-1");
        assert_eq!(run.pid, 42);
        assert_eq!(run.project, "proj");
        assert_eq!(run.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(run.task_mode.as_deref(), Some("review"));
        assert_eq!(run.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(run.last_seq, 7);
        assert!(!run.orphaned);
    }
}
