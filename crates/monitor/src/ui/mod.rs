mod detail;
mod table;

use crate::state::{AppMode, AppState, BG};

use ratatui::style::Style;
use ratatui::widgets::Block;

pub(crate) fn render(frame: &mut ratatui::Frame, state: &mut AppState) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    match &state.mode {
        AppMode::Table => table::render_table_view(frame, area, state),
        AppMode::Detail(_) => {
            detail::render_detail_view(frame, area, state);
        }
    }
}
