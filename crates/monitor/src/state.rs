use std::collections::{HashMap, HashSet, VecDeque};

use chrono::{DateTime, Utc};
use ratatui::style::Color;
use ratatui::widgets::TableState;

use ratatui::text::Line as RatatuiLine;

use consult_llm_core::monitoring::HistoryRecord;
use consult_llm_core::stream_events::ParsedStreamEvent;

// ── Colors ───────────────────────────────────────────────────────────────

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

/// Per-task-mode colors for visual distinction in tables and detail views.
pub(crate) fn task_mode_color(mode: Option<&str>) -> Color {
    match mode {
        Some("review") => Color::Rgb(130, 180, 230), // soft blue
        Some("debug") => Color::Rgb(220, 150, 120),  // warm orange
        Some("plan") => Color::Rgb(170, 140, 210),   // muted purple
        Some("create") => Color::Rgb(120, 200, 160), // soft green
        _ => DIM_WHITE,
    }
}

pub(crate) const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

// ── Types ────────────────────────────────────────────────────────────────

#[allow(clippy::large_enum_variant)]
pub(crate) enum AppMode {
    Table,
    Detail(DetailState),
    ThreadDetail(ThreadDetailState),
    ConfirmClearHistory,
}

pub(crate) struct ThreadDetailState {
    pub(crate) thread_id: String,
    /// Consultation IDs in chronological order
    pub(crate) turn_ids: Vec<String>,
    /// Events per completed turn (immutable), one Vec per turn
    pub(crate) historical_turns: Vec<Vec<ParsedStreamEvent>>,
    /// Events from the latest/active turn (may still be streaming)
    pub(crate) active_events: Vec<ParsedStreamEvent>,
    /// Byte offset for polling the active turn's .events.jsonl
    pub(crate) active_file_offset: u64,
    /// Line index where each turn starts (for jump-to-turn navigation)
    pub(crate) turn_line_offsets: Vec<usize>,
    /// Index of the currently focused turn
    pub(crate) selected_turn: usize,
    pub(crate) scroll: usize,
    pub(crate) auto_scroll: bool,
    // Metadata for header
    pub(crate) models: Vec<String>,
    pub(crate) backends: Vec<String>,
    pub(crate) project: Option<String>,
    pub(crate) total_duration_ms: u64,
    pub(crate) turn_count: usize,
    pub(crate) success: Option<bool>,
}

pub(crate) struct DetailState {
    pub(crate) consultation_id: String,
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
    /// Cached rendered lines from normalize_events + render_blocks.
    pub(crate) cached_lines: Option<Vec<RatatuiLine<'static>>>,
    /// Event count when cache was built (invalidate when events arrive).
    pub(crate) cached_event_count: usize,
    /// Inner width when cache was built (invalidate on resize).
    pub(crate) cached_width: usize,
    /// Whether any in-progress tools existed at cache time (spinners need re-render).
    pub(crate) cached_has_active_tools: bool,
    /// Whether the system prompt overlay is visible.
    pub(crate) show_system_prompt: bool,
    /// Sibling consultation IDs (same project, similar start time) including self.
    pub(crate) siblings: Vec<String>,
    /// Index of the current consultation within `siblings`.
    pub(crate) sibling_index: usize,
}

pub(crate) struct DetailMetadata {
    pub(crate) model: Option<String>,
    pub(crate) backend: Option<String>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) success: Option<bool>,
    pub(crate) project: Option<String>,
    pub(crate) task_mode: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Focus {
    Active,
    History,
}

pub(crate) struct AppState {
    pub(crate) servers: HashMap<String, ServerState>,
    pub(crate) server_order: Vec<String>,
    pub(crate) selected: usize,
    pub(crate) row_count: usize,
    pub(crate) tick: usize,
    pub(crate) table_state: TableState,
    pub(crate) focus: Focus,
    pub(crate) history_selected: usize,
    pub(crate) history_table_state: TableState,
    pub(crate) mode: AppMode,
    pub(crate) history: VecDeque<HistoryRecord>,
    /// Transient message shown in status bar, cleared after a few renders
    pub(crate) flash: Option<(String, u8)>,
    /// Last known inner height of the detail view (for half-page scroll)
    pub(crate) detail_inner_height: usize,
    /// Whether the help/shortcuts overlay is visible
    pub(crate) show_help: bool,
    /// Current filter text (always present, empty = no filter)
    pub(crate) filter_text: String,
    /// Whether the filter input is currently being edited
    pub(crate) filter_editing: bool,
    /// Cached filtered history indices, invalidated by filter/history changes
    pub(crate) cached_filter_indices: Option<Vec<usize>>,
}

pub(crate) struct ServerState {
    pub(crate) server_id: String,
    pub(crate) pid: u32,
    pub(crate) _version: String,
    pub(crate) project: Option<String>,
    pub(crate) stopped: bool,
    pub(crate) dead: bool,
    pub(crate) active_consults: HashMap<String, ActiveConsult>,
    pub(crate) completed_consults: Vec<CompletedConsult>,
    pub(crate) completed_count: u32,
    pub(crate) failed_count: u32,
    /// Timestamp of the most recent consultation activity (start or finish)
    pub(crate) last_consult_at: Option<DateTime<Utc>>,
}

pub(crate) struct ActiveConsult {
    pub(crate) model: String,
    pub(crate) backend: String,
    /// Real start time from the event timestamp (survives TUI restart)
    pub(crate) started_at: DateTime<Utc>,
    /// Latest progress stage from ConsultProgress events
    pub(crate) last_progress: Option<String>,
    /// Thread ID if this is a resumed thread consultation
    pub(crate) thread_id: Option<String>,
    /// Task mode (review, debug, plan, create) — None means general
    pub(crate) task_mode: Option<String>,
    /// Reasoning effort level (e.g. for Codex models)
    pub(crate) reasoning_effort: Option<String>,
}

pub(crate) struct CompletedConsult {
    pub(crate) id: String,
    pub(crate) model: String,
    pub(crate) backend: String,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) duration_ms: u64,
    pub(crate) success: bool,
    pub(crate) error: Option<String>,
    pub(crate) task_mode: Option<String>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct RowInfo {
    pub(crate) server_id: String,
    pub(crate) consultation_id: String,
}

/// A display row in the history table — either a single consultation or a thread summary.
pub(crate) enum HistoryDisplayRow {
    /// Non-threaded consultation — index into `self.history`
    Single(usize),
    /// Thread summary row (collapsed)
    ThreadSummary {
        thread_id: String,
        latest_parsed_ts: Option<DateTime<Utc>>,
        model: String,
        backend: String,
        total_duration_ms: u64,
        total_tokens_in: Option<u64>,
        total_tokens_out: Option<u64>,
        turn_count: usize,
        success: bool,
        /// True if models differ across turns
        mixed_model: bool,
        project: String,
    },
}

impl AppState {
    pub(crate) fn new() -> Self {
        Self {
            servers: HashMap::new(),
            server_order: Vec::new(),
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

    /// Return indices of history rows matching the current filter.
    /// Uses a cache that is invalidated by `invalidate_filter_cache()`.
    pub(crate) fn filtered_history_indices(&self) -> &[usize] {
        // Cache is always populated before read via ensure_filter_cache()
        self.cached_filter_indices.as_deref().unwrap_or(&[])
    }

    /// Recompute filter cache if invalidated.
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
                .filter(|(_, r)| {
                    r.project.to_lowercase().contains(&needle)
                        || r.model.to_lowercase().contains(&needle)
                        || r.backend.to_lowercase().contains(&needle)
                        || (r.success && "success".contains(&needle))
                        || (!r.success && "failed".contains(&needle))
                })
                .map(|(i, _)| i)
                .collect()
        };
        self.cached_filter_indices = Some(indices);
    }

    /// Invalidate the cached filter indices.
    pub(crate) fn invalidate_filter_cache(&mut self) {
        self.cached_filter_indices = None;
    }

    /// Build display rows for the history table, grouping threaded consultations.
    /// Returns rows in newest-first order (matching history VecDeque order).
    pub(crate) fn build_history_display_rows(&self) -> Vec<HistoryDisplayRow> {
        let filtered = self.filtered_history_indices();

        // Group filtered indices by thread_id
        // Key: thread_id, Value: indices into self.history (in VecDeque order = newest first)
        let mut thread_groups: HashMap<String, Vec<usize>> = HashMap::new();
        let mut seen_threads: Vec<String> = Vec::new(); // preserve first-seen order
        let mut single_rows: Vec<(usize, usize)> = Vec::new(); // (position, history_index)

        for (position, &idx) in filtered.iter().enumerate() {
            let record = &self.history[idx];
            if let Some(ref tid) = record.thread_id {
                if !thread_groups.contains_key(tid) {
                    seen_threads.push(tid.clone());
                }
                thread_groups.entry(tid.clone()).or_default().push(idx);
            } else {
                single_rows.push((position, idx));
            }
        }

        // Build display rows, maintaining position order
        // We'll collect all rows with their "sort position" (position of latest entry)
        let mut rows_with_pos: Vec<(usize, HistoryDisplayRow)> = Vec::new();

        for (position, idx) in single_rows {
            rows_with_pos.push((position, HistoryDisplayRow::Single(idx)));
        }

        for tid in &seen_threads {
            let indices = &thread_groups[tid];
            if indices.len() == 1 {
                // Single-turn thread — show as regular row
                let pos = filtered.iter().position(|&i| i == indices[0]).unwrap_or(0);
                rows_with_pos.push((pos, HistoryDisplayRow::Single(indices[0])));
                continue;
            }

            // Position = earliest position in filtered list (= newest entry, since newest-first)
            let position = indices
                .iter()
                .filter_map(|&i| filtered.iter().position(|&fi| fi == i))
                .min()
                .unwrap_or(0);

            // Sort indices chronologically (oldest first) by timestamp for aggregation
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

            rows_with_pos.push((
                position,
                HistoryDisplayRow::ThreadSummary {
                    thread_id: tid.clone(),
                    latest_parsed_ts: latest.parsed_ts,
                    model: latest.model.clone(),
                    backend: latest.backend.clone(),
                    total_duration_ms: indices.iter().map(|&i| self.history[i].duration_ms).sum(),
                    total_tokens_in,
                    total_tokens_out,
                    turn_count: indices.len(),
                    success: latest.success,
                    mixed_model: models.len() > 1,
                    project: first.project.clone(),
                },
            ));
        }

        // Sort by position (preserves original order)
        rows_with_pos.sort_by_key(|(pos, _)| *pos);
        rows_with_pos.into_iter().map(|(_, row)| row).collect()
    }
}
