use chrono::Utc;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};

use crate::format::{
    PROJECT_COL_WIDTH, format_cost, format_cost_value, format_duration_friendly,
    format_relative_time, format_tokens, truncate_project,
};
use crate::state::{
    AppState, BG, DIM, DIM_WHITE, Focus, GREEN, HistoryDisplayRow, RED, SELECTED_BG, SEPARATOR,
    SPINNER_FRAMES, TEAL, WHITE, YELLOW, task_mode_color,
};

pub(super) fn render_table_view(frame: &mut ratatui::Frame, area: Rect, state: &mut AppState) {
    // Dynamic active table height: +3 for borders (2) + header row (1), minimum 4
    let active_height = (state.row_count as u16 + 3).min(area.height / 2).max(4);

    let filter_height = if state.filter_editing { 1 } else { 0 };

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(active_height),
        Constraint::Min(8),
        Constraint::Length(filter_height),
        Constraint::Length(1),
    ])
    .split(area);

    render_header(frame, chunks[0], state);
    render_table(frame, chunks[1], state);
    render_history_table(frame, chunks[2], state);
    if state.filter_editing {
        render_filter_input(frame, chunks[3], &state.filter_text);
    }
    render_status_bar(frame, chunks[4], state);
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
            " consult-llm-monitor",
            Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(DIM)),
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
    let task_col_width: u16 = 8;
    let show_task_col = area.width >= 100;
    let mut header_cells = vec![Line::from("Project"), Line::from("PID")];
    if show_task_col {
        header_cells.push(Line::from("Task"));
    }
    header_cells.push(Line::from("Consultation"));
    header_cells.push(Line::from(Span::raw("Elapsed")).alignment(Alignment::Right));
    let header =
        Row::new(header_cells).style(Style::default().fg(TEAL).add_modifier(Modifier::BOLD));

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
            let (hist, hist_color) = if server.completed_count > 0 || server.failed_count > 0 {
                (
                    format!(
                        "{} done{}",
                        server.completed_count,
                        if server.failed_count > 0 {
                            format!(", {} failed", server.failed_count)
                        } else {
                            String::new()
                        }
                    ),
                    DIM_WHITE,
                )
            } else {
                ("\u{2014}".to_string(), DIM)
            };
            let mut cells = vec![
                Line::from(Span::styled(
                    truncate_project(display_name),
                    Style::default().fg(DIM_WHITE),
                )),
                Line::from(Span::styled(pid.clone(), Style::default().fg(DIM_WHITE))),
            ];
            if show_task_col {
                cells.push(Line::from(Span::styled("", Style::default().fg(DIM))));
            }
            cells.push(Line::from(Span::styled(
                hist,
                Style::default().fg(hist_color),
            )));
            cells.push(Line::from(Span::styled(
                format!("{:>width$}", "\u{2014}", width = elapsed_col_width as usize),
                Style::default().fg(DIM),
            )));
            rows.push(Row::new(cells));
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
                let mut consult_spans = vec![
                    Span::styled(format!("{spinner} "), Style::default().fg(TEAL)),
                    Span::styled(consult_text, Style::default().fg(WHITE)),
                ];
                if let Some(ref tid) = consult.thread_id {
                    let turn_num = state
                        .history
                        .iter()
                        .filter(|h| h.thread_id.as_deref() == Some(tid))
                        .count()
                        + 1;
                    consult_spans.push(Span::styled(
                        format!("  \u{21b3}{turn_num}"),
                        Style::default().fg(DIM),
                    ));
                }
                let consult_cell = Line::from(consult_spans);
                let mut cells = vec![
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
                ];
                if show_task_col {
                    let mode = consult.task_mode.as_deref();
                    cells.push(Line::from(Span::styled(
                        mode.unwrap_or("general"),
                        Style::default().fg(task_mode_color(mode)),
                    )));
                }
                cells.push(consult_cell);
                cells.push(Line::from(Span::styled(
                    format!(
                        "{:>width$}",
                        elapsed_str,
                        width = elapsed_col_width as usize
                    ),
                    Style::default().fg(DIM_WHITE),
                )));
                rows.push(Row::new(cells));
            }

            // Render completed consultations (last per backend only)
            let mut seen_backends = std::collections::HashSet::new();
            let deduped_completed: Vec<_> = server
                .completed_consults
                .iter()
                .rev()
                .filter(|cc| seen_backends.insert(&cc.backend))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            for (i, cc) in deduped_completed.iter().enumerate() {
                let show_server = is_first_row && server.active_consults.is_empty() && i == 0;
                let duration_str = format_duration_friendly(cc.duration_ms);
                let (indicator, indicator_color) = if cc.success {
                    ("\u{2713}", GREEN) // ✓
                } else {
                    ("\u{2717}", RED) // ✗
                };
                let rest = match &cc.error {
                    Some(err) => format!(" {} ({}) {}", cc.model, cc.backend, err),
                    None => format!(" {} ({})", cc.model, cc.backend),
                };
                let mut cells = vec![
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
                ];
                if show_task_col {
                    let mode = cc.task_mode.as_deref();
                    cells.push(Line::from(Span::styled(
                        mode.unwrap_or("general"),
                        Style::default().fg(task_mode_color(mode)),
                    )));
                }
                cells.push(Line::from(vec![
                    Span::styled(indicator, Style::default().fg(indicator_color)),
                    Span::styled(rest, Style::default().fg(DIM_WHITE)),
                ]));
                cells.push(Line::from(Span::styled(
                    format!(
                        "{:>width$}",
                        duration_str,
                        width = elapsed_col_width as usize
                    ),
                    Style::default().fg(DIM_WHITE),
                )));
                rows.push(Row::new(cells));
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

    let mut constraints = vec![Constraint::Length(PROJECT_COL_WIDTH), Constraint::Length(7)];
    if show_task_col {
        constraints.push(Constraint::Length(task_col_width));
    }
    constraints.push(Constraint::Min(20));
    constraints.push(Constraint::Length(elapsed_col_width));

    let table = Table::new(rows, constraints)
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
    let cost_col_width: u16 = 7;
    let task_col_width: u16 = 8;
    let show_task_col = area.width >= 100;
    let mut header_cells = vec![Line::from("Time"), Line::from("Project")];
    if show_task_col {
        header_cells.push(Line::from("Task"));
    }
    header_cells.push(Line::from("Model"));
    header_cells.push(Line::from("Backend"));
    header_cells.push(Line::from(Span::raw("Duration")).alignment(Alignment::Right));
    header_cells.push(Line::from(Span::raw("Tokens")).alignment(Alignment::Right));
    header_cells.push(Line::from(Span::raw("Cost")).alignment(Alignment::Right));
    header_cells.push(Line::from("✓"));
    let header =
        Row::new(header_cells).style(Style::default().fg(TEAL).add_modifier(Modifier::BOLD));

    let now = Utc::now();
    let display_rows = state.build_history_display_rows();
    let rows: Vec<Row> = display_rows
        .iter()
        .map(|display_row| match display_row {
            HistoryDisplayRow::Single(idx) => {
                let record = &state.history[*idx];
                let status_icon = if record.success {
                    "\u{2713}"
                } else {
                    "\u{2717}"
                };
                let status_color = if record.success { GREEN } else { RED };
                let duration_str = format_duration_friendly(record.duration_ms);
                let tokens_str = format_tokens(record.tokens_in, record.tokens_out);
                let cost_str = format_cost(record.tokens_in, record.tokens_out, &record.model);

                let mut cells = vec![
                    Line::from(Span::styled(
                        format_relative_time(record.parsed_ts, now),
                        Style::default().fg(DIM),
                    )),
                    Line::from(Span::styled(
                        record.project.clone(),
                        Style::default().fg(DIM_WHITE),
                    )),
                ];
                if show_task_col {
                    let mode = record.task_mode.as_deref();
                    cells.push(Line::from(Span::styled(
                        mode.unwrap_or("general"),
                        Style::default().fg(task_mode_color(mode)),
                    )));
                }
                cells.push(Line::from(Span::styled(
                    record.model.clone(),
                    Style::default().fg(DIM_WHITE),
                )));
                cells.push(Line::from(Span::styled(
                    record.backend.clone(),
                    Style::default().fg(DIM),
                )));
                cells.push(Line::from(Span::styled(
                    format!(
                        "{:>width$}",
                        duration_str,
                        width = duration_col_width as usize
                    ),
                    Style::default().fg(DIM_WHITE),
                )));
                cells.push(Line::from(Span::styled(
                    format!("{:>width$}", tokens_str, width = tokens_col_width as usize),
                    Style::default().fg(DIM),
                )));
                cells.push(Line::from(Span::styled(
                    format!("{:>width$}", cost_str, width = cost_col_width as usize),
                    Style::default().fg(DIM),
                )));
                cells.push(Line::from(Span::styled(
                    status_icon.to_string(),
                    Style::default().fg(status_color),
                )));
                Row::new(cells)
            }
            HistoryDisplayRow::ThreadSummary {
                latest_parsed_ts,
                model,
                backend,
                total_duration_ms,
                total_tokens_in,
                total_tokens_out,
                total_cost,
                turn_count,
                success,
                mixed_model,
                project,
                ..
            } => {
                let status_icon = if *success { "\u{2713}" } else { "\u{2717}" };
                let status_color = if *success { GREEN } else { RED };
                let duration_str = format_duration_friendly(*total_duration_ms);
                let tokens_str = format_tokens(*total_tokens_in, *total_tokens_out);
                let cost_str = match total_cost {
                    Some(c) => format_cost_value(*c),
                    None => "\u{2014}".to_string(),
                };
                let model_display = if *mixed_model {
                    format!("{model}*")
                } else {
                    model.clone()
                };

                let turns_suffix = if area.width >= 100 {
                    format!(" ({turn_count} turns)")
                } else {
                    format!(" \u{21b3}{turn_count}")
                };

                let mut cells = vec![
                    Line::from(Span::styled(
                        format_relative_time(*latest_parsed_ts, now),
                        Style::default().fg(DIM),
                    )),
                    Line::from(vec![
                        Span::styled(project.clone(), Style::default().fg(DIM_WHITE)),
                        Span::styled(turns_suffix, Style::default().fg(DIM)),
                    ]),
                ];
                if show_task_col {
                    cells.push(Line::from(Span::styled("", Style::default().fg(DIM))));
                }
                cells.push(Line::from(Span::styled(
                    model_display,
                    Style::default().fg(DIM_WHITE),
                )));
                cells.push(Line::from(Span::styled(
                    backend.clone(),
                    Style::default().fg(DIM),
                )));
                cells.push(Line::from(Span::styled(
                    format!(
                        "{:>width$}",
                        duration_str,
                        width = duration_col_width as usize
                    ),
                    Style::default().fg(DIM_WHITE),
                )));
                cells.push(Line::from(Span::styled(
                    format!("{:>width$}", tokens_str, width = tokens_col_width as usize),
                    Style::default().fg(DIM),
                )));
                cells.push(Line::from(Span::styled(
                    format!("{:>width$}", cost_str, width = cost_col_width as usize),
                    Style::default().fg(DIM),
                )));
                cells.push(Line::from(Span::styled(
                    status_icon,
                    Style::default().fg(status_color),
                )));
                Row::new(cells)
            }
        })
        .collect();

    let mut constraints = vec![Constraint::Length(10), Constraint::Fill(1)];
    if show_task_col {
        constraints.push(Constraint::Length(task_col_width));
    }
    constraints.push(Constraint::Length(14));
    constraints.push(Constraint::Length(10));
    constraints.push(Constraint::Length(duration_col_width));
    constraints.push(Constraint::Length(tokens_col_width));
    constraints.push(Constraint::Length(cost_col_width));
    constraints.push(Constraint::Length(2));

    let table = Table::new(rows, constraints)
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

    if state.focus == Focus::History && !display_rows.is_empty() {
        state
            .history_table_state
            .select(Some(state.history_selected));
    } else {
        state.history_table_state.select(None);
    }
    frame.render_stateful_widget(table, area, &mut state.history_table_state);
}

fn render_filter_input(frame: &mut ratatui::Frame, area: Rect, text: &str) {
    let bar = Line::from(vec![
        Span::styled(" /", Style::default().fg(TEAL)),
        Span::styled(text, Style::default().fg(WHITE)),
        Span::styled("▎", Style::default().fg(TEAL)),
    ]);
    frame.render_widget(Paragraph::new(bar).style(Style::default().bg(BG)), area);
}

fn render_status_bar(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if let Some((msg, _)) = &state.flash {
        let bar = Line::from(vec![Span::styled(
            format!(" {msg}"),
            Style::default().fg(DIM),
        )]);
        frame.render_widget(Paragraph::new(bar).style(Style::default().bg(BG)), area);
        return;
    }

    let mut spans = vec![
        Span::styled(" j/k", Style::default().fg(TEAL)),
        Span::styled(" navigate  ", Style::default().fg(DIM_WHITE)),
        Span::styled("Tab", Style::default().fg(TEAL)),
        Span::styled(" switch  ", Style::default().fg(DIM_WHITE)),
        Span::styled("/", Style::default().fg(TEAL)),
        Span::styled(" filter  ", Style::default().fg(DIM_WHITE)),
        Span::styled("Enter", Style::default().fg(TEAL)),
        Span::styled(" detail  ", Style::default().fg(DIM_WHITE)),
        Span::styled("q", Style::default().fg(TEAL)),
        Span::styled(" quit", Style::default().fg(DIM_WHITE)),
    ];

    if !state.filter_text.is_empty() && !state.filter_editing {
        spans.push(Span::styled("  filter: ", Style::default().fg(DIM)));
        spans.push(Span::styled(&state.filter_text, Style::default().fg(TEAL)));
    }

    let bar = Line::from(spans);
    frame.render_widget(Paragraph::new(bar).style(Style::default().bg(BG)), area);
}
