use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::state::{BG, DIM, DIM_WHITE, GREEN, RED, SEPARATOR, TEAL, WHITE, YELLOW};

pub(super) fn render_detail_view(
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
