use std::collections::{HashMap, VecDeque};

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

pub(crate) const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

// ── Types ────────────────────────────────────────────────────────────────

#[allow(clippy::large_enum_variant)]
pub(crate) enum AppMode {
    Table,
    Detail(DetailState),
    ConfirmClearHistory,
}

pub(crate) struct DetailState {
    pub(crate) consultation_id: String,
    pub(crate) events: Vec<ParsedStreamEvent>,
    pub(crate) file_offset: u64,
    pub(crate) scroll: usize,
    pub(crate) auto_scroll: bool,
    pub(crate) model: Option<String>,
    pub(crate) backend: Option<String>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) success: Option<bool>,
    pub(crate) project: Option<String>,
    /// Cached rendered lines from normalize_events + render_blocks.
    pub(crate) cached_lines: Option<Vec<RatatuiLine<'static>>>,
    /// Event count when cache was built (invalidate when events arrive).
    pub(crate) cached_event_count: usize,
    /// Inner width when cache was built (invalidate on resize).
    pub(crate) cached_width: usize,
    /// Whether any in-progress tools existed at cache time (spinners need re-render).
    pub(crate) cached_has_active_tools: bool,
}

pub(crate) struct DetailMetadata {
    pub(crate) model: Option<String>,
    pub(crate) backend: Option<String>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) success: Option<bool>,
    pub(crate) project: Option<String>,
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
}

pub(crate) struct ActiveConsult {
    pub(crate) model: String,
    pub(crate) backend: String,
    /// Real start time from the event timestamp (survives TUI restart)
    pub(crate) started_at: DateTime<Utc>,
    /// Latest progress stage from ConsultProgress events
    pub(crate) last_progress: Option<String>,
}

pub(crate) struct CompletedConsult {
    pub(crate) id: String,
    pub(crate) model: String,
    pub(crate) backend: String,
    pub(crate) duration_ms: u64,
    pub(crate) success: bool,
    pub(crate) error: Option<String>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct RowInfo {
    pub(crate) server_id: String,
    pub(crate) consultation_id: String,
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
}
