use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::state::{
    AppMode, AppState, BG, DIM, DIM_WHITE, GREEN, RED, SEPARATOR, TEAL, WHITE, YELLOW,
};

pub(super) fn render_detail_view(frame: &mut ratatui::Frame, area: Rect, state: &mut AppState) {
    let AppMode::Detail(ref detail) = state.mode else {
        return;
    };

    let consultation_id = detail.consultation_id.clone();
    let events: Vec<&ParsedStreamEvent> = detail.events.iter().collect();

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
    for event in events.iter() {
        match event {
            ParsedStreamEvent::Prompt { text } => {
                lines.push(Line::from(vec![Span::styled(
                    "  Prompt:",
                    Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
                )]));
                for line in text.lines() {
                    lines.push(Line::from(vec![Span::styled(
                        format!("    {line}"),
                        Style::default().fg(DIM_WHITE),
                    )]));
                }
                lines.push(Line::from(""));
            }
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
    state.detail_inner_height = inner_height;

    // Clamp scroll and persist so j/k/d/u operate on real values next frame
    let effective_scroll = if let AppMode::Detail(ref mut detail) = state.mode {
        detail.scroll = detail.scroll.min(max_scroll);
        detail.scroll
    } else {
        0
    };

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
        Span::styled(" q/Esc", Style::default().fg(TEAL)),
        Span::styled(" back  ", Style::default().fg(DIM_WHITE)),
        Span::styled("j/k", Style::default().fg(TEAL)),
        Span::styled(" scroll  ", Style::default().fg(DIM_WHITE)),
        Span::styled("d/u", Style::default().fg(TEAL)),
        Span::styled(" half-page", Style::default().fg(DIM_WHITE)),
    ]);
    frame.render_widget(
        Paragraph::new(bar).style(Style::default().bg(BG)),
        chunks[2],
    );
}
