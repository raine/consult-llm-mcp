use std::collections::{HashMap, HashSet, VecDeque};

use chrono::{DateTime, Utc};
use ratatui::style::Color;
use ratatui::widgets::TableState;

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

pub(crate) enum AppMode {
    Table,
    Detail(DetailState),
}

pub(crate) struct DetailState {
    pub(crate) consultation_id: String,
    pub(crate) events: Vec<ParsedStreamEvent>,
    pub(crate) file_offset: u64,
    pub(crate) scroll: usize,
    pub(crate) auto_scroll: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Focus {
    Active,
    History,
}

pub(crate) struct AppState {
    pub(crate) servers: HashMap<String, ServerState>,
    pub(crate) server_order: Vec<String>,
    /// Server IDs that have been pruned — skip on next poll
    pub(crate) pruned: HashSet<String>,
    pub(crate) selected: usize,
    pub(crate) row_count: usize,
    pub(crate) tick: usize,
    pub(crate) table_state: TableState,
    pub(crate) focus: Focus,
    pub(crate) history_selected: usize,
    pub(crate) history_table_state: TableState,
    pub(crate) mode: AppMode,
    pub(crate) history: VecDeque<HistoryRecord>,
    pub(crate) history_offset: u64,
    /// Transient message shown in status bar, cleared after a few renders
    pub(crate) flash: Option<(String, u8)>,
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
    pub(crate) file_offset: u64,
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
            pruned: HashSet::new(),
            selected: 0,
            row_count: 0,
            tick: 0,
            table_state: TableState::default(),
            focus: Focus::Active,
            history_selected: 0,
            history_table_state: TableState::default(),
            mode: AppMode::Table,
            history: VecDeque::new(),
            history_offset: 0,
            flash: None,
        }
    }
}
