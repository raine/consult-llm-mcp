use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Stdout};
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};

use consult_llm_core::monitoring::{EventEnvelope, MonitorEvent, is_pid_alive, sessions_dir};
use consult_llm_core::stream_events::ParsedStreamEvent;

// ── Colors (matching claude-history aesthetic) ──────────────────────────

const TEAL: Color = Color::Rgb(78, 201, 176);
const WHITE: Color = Color::Rgb(255, 255, 255);
const DIM_WHITE: Color = Color::Rgb(180, 190, 200);
const SEPARATOR: Color = Color::Rgb(80, 80, 80);
const BG: Color = Color::Rgb(18, 18, 22);
const GREEN: Color = Color::Rgb(120, 200, 120);
const RED: Color = Color::Rgb(220, 120, 120);
const YELLOW: Color = Color::Rgb(220, 200, 100);
const DIM: Color = Color::Rgb(100, 100, 110);
const SELECTED_BG: Color = Color::Rgb(40, 40, 50);

const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

// ── State ───────────────────────────────────────────────────────────────

enum AppMode {
    Table,
    Detail {
        consultation_id: String,
        events: Vec<ParsedStreamEvent>,
        file_offset: u64,
        scroll: usize,
        auto_scroll: bool,
    },
}

struct AppState {
    servers: HashMap<String, ServerState>,
    server_order: Vec<String>,
    /// Server IDs that have been pruned — skip on next poll
    pruned: HashSet<String>,
    selected: usize,
    row_count: usize,
    tick: usize,
    mode: AppMode,
}

struct ServerState {
    server_id: String,
    pid: u32,
    _version: String,
    project: Option<String>,
    stopped: bool,
    dead: bool,
    active_consults: HashMap<String, ActiveConsult>,
    completed_consults: Vec<CompletedConsult>,
    completed_count: u32,
    failed_count: u32,
    file_offset: u64,
}

struct ActiveConsult {
    model: String,
    backend: String,
    /// Real start time from the event timestamp (survives TUI restart)
    started_at: DateTime<Utc>,
    /// Latest progress stage from ConsultProgress events
    last_progress: Option<String>,
}

struct CompletedConsult {
    id: String,
    model: String,
    backend: String,
    duration_ms: u64,
    success: bool,
    error: Option<String>,
}

// ── Terminal guard (RAII cleanup) ───────────────────────────────────────

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        crossterm::execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = crossterm::execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
    }
}

// ── Row identity — maps table row index to a consultation ID ────────────

struct RowInfo {
    consultation_id: String,
}

// ── Event processing ────────────────────────────────────────────────────

impl AppState {
    fn new() -> Self {
        Self {
            servers: HashMap::new(),
            server_order: Vec::new(),
            pruned: HashSet::new(),
            selected: 0,
            row_count: 0,
            tick: 0,
            mode: AppMode::Table,
        }
    }

    fn process_event(&mut self, server_id: &str, envelope: &EventEnvelope) {
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

    /// Scan sessions dir, read new lines from each file using read_line()
    /// to correctly handle partial writes and track byte offsets.
    fn poll_files(&mut self, dir: &Path) {
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
            // Skip sidecar event files
            if stem.ends_with(".events") {
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

    fn check_liveness(&mut self) {
        for server in self.servers.values_mut() {
            if !server.stopped && !server.dead && !is_pid_alive(server.pid) {
                server.dead = true;
                server.active_consults.clear();
            }
        }
    }

    /// Remove stopped/dead servers from the view and delete their files.
    fn prune_finished(&mut self, dir: &Path) {
        let to_prune: Vec<String> = self
            .servers
            .iter()
            .filter(|(_, s)| s.stopped || s.dead)
            .map(|(id, _)| id.clone())
            .collect();

        for id in &to_prune {
            // Collect consultation IDs before removing the server
            if let Some(server) = self.servers.get(id) {
                for cid in server.active_consults.keys() {
                    let sidecar = dir.join(format!("{cid}.events.jsonl"));
                    let _ = fs::remove_file(&sidecar);
                }
                for cc in &server.completed_consults {
                    let sidecar = dir.join(format!("{}.events.jsonl", cc.id));
                    let _ = fs::remove_file(&sidecar);
                }
            }
            // Delete the session file so it doesn't reappear on next poll
            let path = dir.join(format!("{id}.jsonl"));
            let _ = fs::remove_file(&path);
            self.servers.remove(id);
            self.pruned.insert(id.clone());
        }
        self.server_order.retain(|id| self.servers.contains_key(id));
    }

    /// Poll the sidecar file for new events in detail mode.
    fn poll_detail_events(&mut self, dir: &Path) {
        let AppMode::Detail {
            consultation_id,
            events,
            file_offset,
            scroll,
            auto_scroll,
        } = &mut self.mode
        else {
            return;
        };

        let path = dir.join(format!("{consultation_id}.events.jsonl"));
        let Ok(file) = File::open(&path) else {
            return;
        };

        let mut reader = BufReader::new(file);
        let _ = reader.seek(SeekFrom::Start(*file_offset));

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
                    *file_offset += bytes_read as u64;
                    if let Ok(event) = serde_json::from_str::<ParsedStreamEvent>(buf.trim()) {
                        events.push(event);
                        got_new = true;
                    }
                }
                Err(_) => break,
            }
        }

        if got_new && *auto_scroll {
            // Will be clamped during render
            *scroll = usize::MAX;
        }
    }

    /// Enter detail mode for a consultation ID.
    fn enter_detail(&mut self, consultation_id: String, dir: &Path) {
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

        self.mode = AppMode::Detail {
            consultation_id,
            events,
            file_offset: offset,
            scroll: usize::MAX, // start at bottom
            auto_scroll: true,
        };
    }

    /// Build a list of RowInfo for the current table rows.
    fn build_row_infos(&self) -> Vec<RowInfo> {
        let mut infos = Vec::new();
        for server_id in &self.server_order {
            let Some(server) = self.servers.get(server_id) else {
                continue;
            };

            if server.active_consults.is_empty() && server.completed_consults.is_empty() {
                // Server row with no consultations — not selectable for detail
                infos.push(RowInfo {
                    consultation_id: String::new(),
                });
            } else {
                for cid in server.active_consults.keys() {
                    infos.push(RowInfo {
                        consultation_id: cid.clone(),
                    });
                }
                for cc in &server.completed_consults {
                    infos.push(RowInfo {
                        consultation_id: cc.id.clone(),
                    });
                }
            }
        }
        infos
    }
}

// ── Rendering ───────────────────────────────────────────────────────────

fn render(frame: &mut ratatui::Frame, state: &AppState) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    match &state.mode {
        AppMode::Table => render_table_view(frame, area, state),
        AppMode::Detail {
            consultation_id,
            events,
            scroll,
            ..
        } => render_detail_view(frame, area, consultation_id, events, *scroll),
    }
}

fn render_table_view(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    render_header(frame, chunks[0], state);
    render_table(frame, chunks[1], state);
    render_status_bar(frame, chunks[2]);
}

fn render_header(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state
        .servers
        .values()
        .filter(|s| !s.stopped && !s.dead)
        .count();
    let consulting: usize = state
        .servers
        .values()
        .map(|s| s.active_consults.len())
        .sum();

    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            " consult-llm-monitor ",
            Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
        )]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SEPARATOR));

    let text = Line::from(vec![
        Span::styled(format!(" {active}"), Style::default().fg(GREEN)),
        Span::styled(" servers  ", Style::default().fg(DIM_WHITE)),
        Span::styled(
            format!("{consulting}"),
            Style::default().fg(if consulting > 0 { YELLOW } else { DIM }),
        ),
        Span::styled(" active consultations", Style::default().fg(DIM_WHITE)),
    ]);

    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn render_table(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let header = Row::new(vec!["Project", "PID", "Status", "Consultation", "Elapsed"])
        .style(Style::default().fg(TEAL).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let now = Utc::now();
    let mut rows: Vec<Row> = Vec::new();
    let mut row_idx = 0usize;

    for server_id in &state.server_order {
        let Some(server) = state.servers.get(server_id) else {
            continue;
        };

        let display_name = server
            .project
            .as_deref()
            .unwrap_or(&server.server_id[..8.min(server.server_id.len())]);
        let pid = server.pid.to_string();

        let (status, status_color) = if server.dead {
            ("dead", RED)
        } else if !server.active_consults.is_empty() {
            ("active", GREEN)
        } else if server.stopped {
            ("stopped", DIM)
        } else {
            ("idle", DIM)
        };

        if server.active_consults.is_empty() && server.completed_consults.is_empty() {
            let hist = if server.completed_count > 0 || server.failed_count > 0 {
                format!(
                    "{} done{}",
                    server.completed_count,
                    if server.failed_count > 0 {
                        format!(", {} failed", server.failed_count)
                    } else {
                        String::new()
                    }
                )
            } else {
                "\u{2014}".to_string()
            };
            let bg = if row_idx == state.selected {
                Style::default().bg(SELECTED_BG)
            } else {
                Style::default()
            };
            rows.push(
                Row::new(vec![
                    Span::styled(display_name.to_string(), Style::default().fg(DIM_WHITE)),
                    Span::styled(pid.clone(), Style::default().fg(DIM_WHITE)),
                    Span::styled(status.to_string(), Style::default().fg(status_color)),
                    Span::styled(hist, Style::default().fg(DIM_WHITE)),
                    Span::styled(String::new(), Style::default().fg(DIM)),
                ])
                .style(bg),
            );
            row_idx += 1;
        } else {
            let is_first_row = true;
            for (i, consult) in server.active_consults.values().enumerate() {
                let elapsed = now
                    .signed_duration_since(consult.started_at)
                    .num_milliseconds()
                    .max(0) as f64
                    / 1000.0;
                let elapsed_str = format!("{elapsed:.1}s");
                let show_server = is_first_row && i == 0;
                let bg = if row_idx == state.selected {
                    Style::default().bg(SELECTED_BG)
                } else {
                    Style::default()
                };
                let spinner = SPINNER_FRAMES[state.tick % SPINNER_FRAMES.len()];
                let consult_text = match &consult.last_progress {
                    Some(progress) => format!("{} ({})", consult.model, progress),
                    None => format!("{} ({})", consult.model, consult.backend),
                };
                let consult_cell = Line::from(vec![
                    Span::styled(format!("{spinner} "), Style::default().fg(TEAL)),
                    Span::styled(consult_text, Style::default().fg(WHITE)),
                ]);
                rows.push(
                    Row::new([
                        Line::styled(
                            if show_server {
                                display_name.to_string()
                            } else {
                                String::new()
                            },
                            Style::default().fg(DIM_WHITE),
                        ),
                        Line::styled(
                            if show_server {
                                pid.clone()
                            } else {
                                String::new()
                            },
                            Style::default().fg(DIM_WHITE),
                        ),
                        Line::styled(status.to_string(), Style::default().fg(status_color)),
                        consult_cell,
                        Line::styled(elapsed_str, Style::default().fg(YELLOW)),
                    ])
                    .style(bg),
                );
                row_idx += 1;
            }

            // Render completed consultations with dimmed styling
            for (i, cc) in server.completed_consults.iter().enumerate() {
                let show_server = is_first_row && server.active_consults.is_empty() && i == 0;
                let duration_str = format!("{:.1}s", cc.duration_ms as f64 / 1000.0);
                let result_indicator = if cc.success {
                    "\u{2713}" // ✓
                } else {
                    "\u{2717}" // ✗
                };
                let consult_text = match &cc.error {
                    Some(err) => {
                        format!("{} {} ({}) {}", result_indicator, cc.model, cc.backend, err)
                    }
                    None => format!("{} {} ({})", result_indicator, cc.model, cc.backend),
                };
                let bg = if row_idx == state.selected {
                    Style::default().bg(SELECTED_BG)
                } else {
                    Style::default()
                };
                rows.push(
                    Row::new(vec![
                        Span::styled(
                            if show_server {
                                display_name.to_string()
                            } else {
                                String::new()
                            },
                            Style::default().fg(DIM),
                        ),
                        Span::styled(
                            if show_server {
                                pid.clone()
                            } else {
                                String::new()
                            },
                            Style::default().fg(DIM),
                        ),
                        Span::styled("done", Style::default().fg(DIM)),
                        Span::styled(consult_text, Style::default().fg(DIM)),
                        Span::styled(duration_str, Style::default().fg(DIM)),
                    ])
                    .style(bg),
                );
                row_idx += 1;
            }
        }
    }

    if rows.is_empty() {
        let msg = Paragraph::new(Line::from(vec![Span::styled(
            "  No active servers. Waiting...",
            Style::default().fg(DIM),
        )]))
        .style(Style::default().bg(BG));
        frame.render_widget(msg, area);
        return;
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Min(20),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(SEPARATOR)),
    );

    frame.render_widget(table, area);
}

fn render_detail_view(
    frame: &mut ratatui::Frame,
    area: Rect,
    consultation_id: &str,
    events: &[ParsedStreamEvent],
    scroll: usize,
) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    // Header
    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            format!(" {consultation_id} "),
            Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
        )]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SEPARATOR));

    let header_text = Line::from(vec![Span::styled(
        format!(" {} events", events.len()),
        Style::default().fg(DIM_WHITE),
    )]);
    frame.render_widget(Paragraph::new(header_text).block(block), chunks[0]);

    // Event timeline
    let mut lines: Vec<Line> = Vec::new();
    for event in events {
        match event {
            ParsedStreamEvent::SessionStarted { id } => {
                lines.push(Line::from(vec![Span::styled(
                    format!("  session: {id}"),
                    Style::default().fg(DIM),
                )]));
            }
            ParsedStreamEvent::Thinking => {
                lines.push(Line::from(vec![Span::styled(
                    "  Thinking...",
                    Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                )]));
            }
            ParsedStreamEvent::ToolStarted { label, .. } => {
                lines.push(Line::from(vec![Span::styled(
                    format!("  \u{25b6} {label}"),
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                )]));
            }
            ParsedStreamEvent::ToolFinished { success, .. } => {
                if *success {
                    lines.push(Line::from(vec![Span::styled(
                        "  \u{2713}",
                        Style::default().fg(GREEN),
                    )]));
                } else {
                    lines.push(Line::from(vec![Span::styled(
                        "  \u{2717}",
                        Style::default().fg(RED),
                    )]));
                }
            }
            ParsedStreamEvent::AssistantText { text } => {
                for line in text.lines() {
                    lines.push(Line::from(vec![Span::styled(
                        format!("    {line}"),
                        Style::default().fg(WHITE),
                    )]));
                }
            }
            ParsedStreamEvent::Usage {
                prompt_tokens,
                completion_tokens,
            } => {
                lines.push(Line::from(vec![Span::styled(
                    format!("  tokens: {prompt_tokens} in / {completion_tokens} out"),
                    Style::default().fg(DIM),
                )]));
            }
        }
    }

    // Calculate visible area height (inside border)
    let inner_height = chunks[1].height.saturating_sub(2) as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(inner_height);
    let effective_scroll = scroll.min(max_scroll);

    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(effective_scroll)
        .take(inner_height)
        .collect();

    let content = Paragraph::new(visible_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(SEPARATOR)),
    );
    frame.render_widget(content, chunks[1]);

    // Status bar
    let bar = Line::from(vec![
        Span::styled(" Esc", Style::default().fg(TEAL)),
        Span::styled(" back  ", Style::default().fg(DIM_WHITE)),
        Span::styled("j/k", Style::default().fg(TEAL)),
        Span::styled(" scroll  ", Style::default().fg(DIM_WHITE)),
        Span::styled("q", Style::default().fg(TEAL)),
        Span::styled(" quit", Style::default().fg(DIM_WHITE)),
    ]);
    frame.render_widget(
        Paragraph::new(bar).style(Style::default().bg(BG)),
        chunks[2],
    );
}

fn render_status_bar(frame: &mut ratatui::Frame, area: Rect) {
    let bar = Line::from(vec![
        Span::styled(" j/k", Style::default().fg(TEAL)),
        Span::styled(" navigate  ", Style::default().fg(DIM_WHITE)),
        Span::styled("Enter", Style::default().fg(TEAL)),
        Span::styled(" detail  ", Style::default().fg(DIM_WHITE)),
        Span::styled("q", Style::default().fg(TEAL)),
        Span::styled(" quit", Style::default().fg(DIM_WHITE)),
    ]);
    frame.render_widget(Paragraph::new(bar).style(Style::default().bg(BG)), area);
}

// ── Main ────────────────────────────────────────────────────────────────

fn main() -> io::Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    let mut guard = TerminalGuard::new()?;
    let mut state = AppState::new();
    let dir = sessions_dir();
    let _ = fs::create_dir_all(&dir);

    state.poll_files(&dir);
    state.check_liveness();
    state.prune_finished(&dir);

    let poll_interval = Duration::from_millis(500);
    let mut last_poll = std::time::Instant::now();
    let render_interval = Duration::from_millis(100);

    loop {
        // Update row_count before render for selection clamping
        let row_infos = state.build_row_infos();
        state.row_count = row_infos.len();
        if state.row_count > 0 && state.selected >= state.row_count {
            state.selected = state.row_count - 1;
        }

        guard.terminal.draw(|frame| render(frame, &state))?;
        state.tick += 1;

        if event::poll(render_interval)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match &state.mode {
                AppMode::Table => match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('j') | KeyCode::Down => {
                        if state.row_count > 0 {
                            state.selected = (state.selected + 1).min(state.row_count - 1);
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        state.selected = state.selected.saturating_sub(1);
                    }
                    KeyCode::Enter => {
                        if let Some(info) = row_infos.get(state.selected)
                            && !info.consultation_id.is_empty()
                        {
                            let cid = info.consultation_id.clone();
                            state.enter_detail(cid, &dir);
                        }
                    }
                    _ => {}
                },
                AppMode::Detail { .. } => match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Esc => {
                        state.mode = AppMode::Table;
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        if let AppMode::Detail {
                            scroll,
                            auto_scroll,
                            ..
                        } = &mut state.mode
                        {
                            *scroll = scroll.saturating_add(1);
                            *auto_scroll = false;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        if let AppMode::Detail {
                            scroll,
                            auto_scroll,
                            ..
                        } = &mut state.mode
                        {
                            *scroll = scroll.saturating_sub(1);
                            *auto_scroll = false;
                        }
                    }
                    KeyCode::Char('G') => {
                        if let AppMode::Detail {
                            scroll,
                            auto_scroll,
                            ..
                        } = &mut state.mode
                        {
                            *scroll = usize::MAX;
                            *auto_scroll = true;
                        }
                    }
                    _ => {}
                },
            }
        }

        if last_poll.elapsed() >= poll_interval {
            state.poll_files(&dir);
            state.check_liveness();
            state.prune_finished(&dir);
            state.poll_detail_events(&dir);
            last_poll = std::time::Instant::now();
        }
    }

    Ok(())
}
