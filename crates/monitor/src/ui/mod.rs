mod detail;
mod table;

use crate::state::{AppMode, AppState, BG, DIM_WHITE, SEPARATOR, TEAL};

use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

pub(crate) fn render(frame: &mut ratatui::Frame, state: &mut AppState) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    match &state.mode {
        AppMode::Table => {
            table::render_table_view(frame, area, state);
        }
        AppMode::ConfirmClearHistory => {
            table::render_table_view(frame, area, state);
            render_confirm_dialog(frame, area);
        }
        AppMode::Detail(_) => {
            detail::render_detail_view(frame, area, state);
        }
    }
}

fn render_confirm_dialog(frame: &mut ratatui::Frame, area: Rect) {
    let popup_width = 36;
    let popup_height = 5;

    let [popup_area] = Layout::horizontal([Constraint::Length(popup_width)])
        .flex(Flex::Center)
        .areas(area);
    let [popup_area] = Layout::vertical([Constraint::Length(popup_height)])
        .flex(Flex::Center)
        .areas(popup_area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SEPARATOR))
        .style(Style::default().bg(BG));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let lines = vec![
        Line::from(Span::styled(
            "Clear all history?",
            Style::default().fg(DIM_WHITE).add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(""),
        Line::from(vec![
            Span::styled("y", Style::default().fg(TEAL)),
            Span::styled(" confirm  ", Style::default().fg(DIM_WHITE)),
            Span::styled("n", Style::default().fg(TEAL)),
            Span::styled(" cancel", Style::default().fg(DIM_WHITE)),
        ])
        .alignment(Alignment::Center),
    ];

    frame.render_widget(Paragraph::new(lines), inner);
}
