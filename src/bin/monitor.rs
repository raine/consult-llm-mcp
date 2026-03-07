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

use consult_llm_mcp::monitoring::{EventEnvelope, MonitorEvent, is_pid_alive, sessions_dir};

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

// ── State ───────────────────────────────────────────────────────────────

struct AppState {
    servers: HashMap<String, ServerState>,
    server_order: Vec<String>,
    /// Server IDs that have been pruned — skip on next poll
    pruned: HashSet<String>,
}

struct ServerState {
    server_id: String,
    pid: u32,
    _version: String,
    stopped: bool,
    dead: bool,
    active_consults: HashMap<String, ActiveConsult>,
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

// ── Event processing ────────────────────────────────────────────────────

impl AppState {
    fn new() -> Self {
        Self {
            servers: HashMap::new(),
            server_order: Vec::new(),
            pruned: HashSet::new(),
        }
    }

    fn process_event(&mut self, server_id: &str, envelope: &EventEnvelope) {
        match &envelope.event {
            MonitorEvent::ServerStarted { version, pid } => {
                if !self.server_order.contains(&server_id.to_string()) {
                    self.server_order.push(server_id.to_string());
                }
                self.servers.insert(
                    server_id.to_string(),
                    ServerState {
                        server_id: server_id.to_string(),
                        pid: *pid,
                        _version: version.clone(),
                        stopped: false,
                        dead: false,
                        active_consults: HashMap::new(),
                        completed_count: 0,
                        failed_count: 0,
                        file_offset: 0,
                    },
                );
            }
            MonitorEvent::ConsultStarted { id, model, backend } => {
                if let Some(server) = self.servers.get_mut(server_id) {
                    // Parse the event timestamp for real elapsed time
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
            MonitorEvent::ConsultFinished { id, success, .. } => {
                if let Some(server) = self.servers.get_mut(server_id) {
                    server.active_consults.remove(id);
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
            // Delete the session file so it doesn't reappear on next poll
            let path = dir.join(format!("{id}.jsonl"));
            let _ = fs::remove_file(&path);
            self.servers.remove(id);
            self.pruned.insert(id.clone());
        }
        self.server_order.retain(|id| self.servers.contains_key(id));
    }
}

// ── Rendering ───────────────────────────────────────────────────────────

fn render(frame: &mut ratatui::Frame, state: &AppState) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

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
    let header = Row::new(vec!["Server", "PID", "Status", "Consultation", "Elapsed"])
        .style(Style::default().fg(TEAL).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let now = Utc::now();
    let mut rows: Vec<Row> = Vec::new();

    for server_id in &state.server_order {
        let Some(server) = state.servers.get(server_id) else {
            continue;
        };

        let short_id = &server.server_id[..8.min(server.server_id.len())];
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

        if server.active_consults.is_empty() {
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
            rows.push(Row::new(vec![
                Span::styled(short_id.to_string(), Style::default().fg(DIM_WHITE)),
                Span::styled(pid.clone(), Style::default().fg(DIM_WHITE)),
                Span::styled(status.to_string(), Style::default().fg(status_color)),
                Span::styled(hist, Style::default().fg(DIM_WHITE)),
                Span::styled(String::new(), Style::default().fg(DIM)),
            ]));
        } else {
            for (i, consult) in server.active_consults.values().enumerate() {
                let elapsed = now
                    .signed_duration_since(consult.started_at)
                    .num_milliseconds()
                    .max(0) as f64
                    / 1000.0;
                let elapsed_str = format!("{elapsed:.1}s");
                rows.push(Row::new(vec![
                    Span::styled(
                        if i == 0 {
                            short_id.to_string()
                        } else {
                            String::new()
                        },
                        Style::default().fg(DIM_WHITE),
                    ),
                    Span::styled(
                        if i == 0 { pid.clone() } else { String::new() },
                        Style::default().fg(DIM_WHITE),
                    ),
                    Span::styled(status.to_string(), Style::default().fg(status_color)),
                    Span::styled(
                        match &consult.last_progress {
                            Some(progress) => format!("{} ({})", consult.model, progress),
                            None => format!("{} ({})", consult.model, consult.backend),
                        },
                        Style::default().fg(WHITE),
                    ),
                    Span::styled(elapsed_str, Style::default().fg(YELLOW)),
                ]));
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

fn render_status_bar(frame: &mut ratatui::Frame, area: Rect) {
    let bar = Line::from(vec![
        Span::styled(" q", Style::default().fg(TEAL)),
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
        guard.terminal.draw(|frame| render(frame, &state))?;

        if event::poll(render_interval)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                _ => {}
            }
        }

        if last_poll.elapsed() >= poll_interval {
            state.poll_files(&dir);
            state.check_liveness();
            state.prune_finished(&dir);
            last_poll = std::time::Instant::now();
        }
    }

    Ok(())
}
