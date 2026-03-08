use chrono::Utc;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};

use crate::format::{
    PROJECT_COL_WIDTH, format_duration_friendly, format_relative_time, format_tokens,
    truncate_project,
};
use crate::state::{
    AppState, BG, DIM, DIM_WHITE, Focus, GREEN, RED, SELECTED_BG, SEPARATOR, SPINNER_FRAMES, TEAL,
    WHITE, YELLOW,
};

pub(super) fn render_table_view(frame: &mut ratatui::Frame, area: Rect, state: &mut AppState) {
    // Dynamic active table height: +3 for borders (2) + header row (1), minimum 4
    let active_height = (state.row_count as u16 + 3).min(area.height / 2).max(4);

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(active_height),
        Constraint::Min(8),
        Constraint::Length(1),
    ])
    .split(area);

    render_header(frame, chunks[0], state);
    render_table(frame, chunks[1], state);
    render_history_table(frame, chunks[2], state);
    render_status_bar(frame, chunks[3], &state.flash);
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

    let line = Line::from(vec![
        Span::styled(
            " consult-llm-monitor  ",
            Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{active}"), Style::default().fg(GREEN)),
        Span::styled(" servers  ", Style::default().fg(DIM_WHITE)),
        Span::styled(
            format!("{consulting}"),
            Style::default().fg(if consulting > 0 { YELLOW } else { DIM }),
        ),
        Span::styled(" active", Style::default().fg(DIM_WHITE)),
    ]);

    frame.render_widget(Paragraph::new(line).style(Style::default().bg(BG)), area);
}

fn render_table(frame: &mut ratatui::Frame, area: Rect, state: &mut AppState) {
    let elapsed_col_width: u16 = 10;
    let header = Row::new(vec![
        Line::from("Project"),
        Line::from("PID"),
        Line::from("Consultation"),
        Line::from(Span::raw("Elapsed")).alignment(Alignment::Right),
    ])
    .style(Style::default().fg(TEAL).add_modifier(Modifier::BOLD));

    let now = Utc::now();
    let mut rows: Vec<Row> = Vec::new();

    for server_id in state.display_server_ids() {
        let Some(server) = state.servers.get(server_id) else {
            continue;
        };

        let display_name = server
            .project
            .as_deref()
            .unwrap_or(&server.server_id[..8.min(server.server_id.len())]);
        let pid = server.pid.to_string();

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
            rows.push(Row::new(vec![
                Line::from(Span::styled(
                    truncate_project(display_name),
                    Style::default().fg(DIM_WHITE),
                )),
                Line::from(Span::styled(pid.clone(), Style::default().fg(DIM_WHITE))),
                Line::from(Span::styled(hist, Style::default().fg(DIM_WHITE))),
                Line::from(Span::styled(
                    format!("{:>width$}", "\u{2014}", width = elapsed_col_width as usize),
                    Style::default().fg(DIM),
                )),
            ]));
        } else {
            let is_first_row = true;
            for (i, (_, consult)) in AppState::sorted_active_consults(server)
                .into_iter()
                .enumerate()
            {
                let elapsed_ms = now
                    .signed_duration_since(consult.started_at)
                    .num_milliseconds()
                    .max(0) as u64;
                let elapsed_str = format_duration_friendly(elapsed_ms);
                let show_server = is_first_row && i == 0;
                let spinner = SPINNER_FRAMES[state.tick % SPINNER_FRAMES.len()];
                let consult_text = match &consult.last_progress {
                    Some(progress) => format!("{} ({})", consult.model, progress),
                    None => format!("{} ({})", consult.model, consult.backend),
                };
                let consult_cell = Line::from(vec![
                    Span::styled(format!("{spinner} "), Style::default().fg(TEAL)),
                    Span::styled(consult_text, Style::default().fg(WHITE)),
                ]);
                rows.push(Row::new(vec![
                    Line::from(Span::styled(
                        if show_server {
                            truncate_project(display_name)
                        } else {
                            String::new()
                        },
                        Style::default().fg(DIM_WHITE),
                    )),
                    Line::from(Span::styled(
                        if show_server {
                            pid.clone()
                        } else {
                            String::new()
                        },
                        Style::default().fg(DIM_WHITE),
                    )),
                    consult_cell,
                    Line::from(Span::styled(
                        format!(
                            "{:>width$}",
                            elapsed_str,
                            width = elapsed_col_width as usize
                        ),
                        Style::default().fg(DIM_WHITE),
                    )),
                ]));
            }

            // Render completed consultations with dimmed styling
            for (i, cc) in server.completed_consults.iter().enumerate() {
                let show_server = is_first_row && server.active_consults.is_empty() && i == 0;
                let duration_str = format_duration_friendly(cc.duration_ms);
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
                rows.push(Row::new(vec![
                    Line::from(Span::styled(
                        if show_server {
                            truncate_project(display_name)
                        } else {
                            String::new()
                        },
                        Style::default().fg(DIM),
                    )),
                    Line::from(Span::styled(
                        if show_server {
                            pid.clone()
                        } else {
                            String::new()
                        },
                        Style::default().fg(DIM),
                    )),
                    Line::from(Span::styled(consult_text, Style::default().fg(DIM))),
                    Line::from(Span::styled(
                        format!(
                            "{:>width$}",
                            duration_str,
                            width = elapsed_col_width as usize
                        ),
                        Style::default().fg(DIM),
                    )),
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
            Constraint::Length(PROJECT_COL_WIDTH),
            Constraint::Length(7),
            Constraint::Min(20),
            Constraint::Length(elapsed_col_width),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().bg(SELECTED_BG))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(SEPARATOR)),
    );

    if state.focus == Focus::Active {
        state.table_state.select(Some(state.selected));
    } else {
        state.table_state.select(None);
    }
    frame.render_stateful_widget(table, area, &mut state.table_state);
}

fn render_history_table(frame: &mut ratatui::Frame, area: Rect, state: &mut AppState) {
    let duration_col_width: u16 = 10;
    let tokens_col_width: u16 = 13;
    let header = Row::new(vec![
        Line::from("Time"),
        Line::from("Project"),
        Line::from("Model"),
        Line::from("Backend"),
        Line::from(Span::raw("Duration")).alignment(Alignment::Right),
        Line::from(Span::raw("Tokens")).alignment(Alignment::Right),
        Line::from("✓"),
    ])
    .style(Style::default().fg(TEAL).add_modifier(Modifier::BOLD));

    let now = Utc::now();
    let rows: Vec<Row> = state
        .history
        .iter()
        .map(|record| {
            let status_icon = if record.success {
                "\u{2713}"
            } else {
                "\u{2717}"
            };
            let status_color = if record.success { GREEN } else { RED };
            let duration_str = format_duration_friendly(record.duration_ms);
            let tokens_str = format_tokens(record.tokens_in, record.tokens_out);

            Row::new(vec![
                Line::from(Span::styled(
                    format_relative_time(&record.ts, now),
                    Style::default().fg(DIM),
                )),
                Line::from(Span::styled(
                    record.project.clone(),
                    Style::default().fg(DIM_WHITE),
                )),
                Line::from(Span::styled(
                    record.model.clone(),
                    Style::default().fg(DIM_WHITE),
                )),
                Line::from(Span::styled(
                    record.backend.clone(),
                    Style::default().fg(DIM),
                )),
                Line::from(Span::styled(
                    format!(
                        "{:>width$}",
                        duration_str,
                        width = duration_col_width as usize
                    ),
                    Style::default().fg(DIM_WHITE),
                )),
                Line::from(Span::styled(
                    format!("{:>width$}", tokens_str, width = tokens_col_width as usize),
                    Style::default().fg(DIM),
                )),
                Line::from(Span::styled(
                    status_icon.to_string(),
                    Style::default().fg(status_color),
                )),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Fill(1),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(duration_col_width),
            Constraint::Length(tokens_col_width),
            Constraint::Length(3),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().bg(SELECTED_BG))
    .block(
        Block::default()
            .title(Line::from(vec![Span::styled(
                " History ",
                Style::default().fg(if state.focus == Focus::History {
                    TEAL
                } else {
                    DIM_WHITE
                }),
            )]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(SEPARATOR)),
    );

    if state.focus == Focus::History && !state.history.is_empty() {
        state
            .history_table_state
            .select(Some(state.history_selected));
    } else {
        state.history_table_state.select(None);
    }
    frame.render_stateful_widget(table, area, &mut state.history_table_state);
}

fn render_status_bar(frame: &mut ratatui::Frame, area: Rect, flash: &Option<(String, u8)>) {
    if let Some((msg, _)) = flash {
        let bar = Line::from(vec![Span::styled(
            format!(" {msg}"),
            Style::default().fg(DIM),
        )]);
        frame.render_widget(Paragraph::new(bar).style(Style::default().bg(BG)), area);
        return;
    }
    let bar = Line::from(vec![
        Span::styled(" j/k", Style::default().fg(TEAL)),
        Span::styled(" navigate  ", Style::default().fg(DIM_WHITE)),
        Span::styled("Tab", Style::default().fg(TEAL)),
        Span::styled(" switch  ", Style::default().fg(DIM_WHITE)),
        Span::styled("Enter", Style::default().fg(TEAL)),
        Span::styled(" detail  ", Style::default().fg(DIM_WHITE)),
        Span::styled("q", Style::default().fg(TEAL)),
        Span::styled(" quit", Style::default().fg(DIM_WHITE)),
    ]);
    frame.render_widget(Paragraph::new(bar).style(Style::default().bg(BG)), area);
}
