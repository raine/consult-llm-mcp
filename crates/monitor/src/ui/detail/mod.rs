mod blocks;
mod run_view;
mod thread_view;

pub(super) use run_view::render_detail_view;
pub(super) use thread_view::render_thread_detail_view;

use ratatui::layout::{Constraint, Layout, Rect};

/// Three-way vertical split used by both detail views: header (3) / body (min 3) / status (1).
fn compute_detail_layout(area: Rect) -> [Rect; 3] {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

#[cfg(test)]
mod tests {
    use super::compute_detail_layout;
    use ratatui::layout::Rect;

    #[test]
    fn detail_layout_splits_80x24_into_header_body_status() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let [header, body, status] = compute_detail_layout(area);
        assert_eq!(header.height, 3);
        assert_eq!(body.height, 20);
        assert_eq!(status.height, 1);
        assert_eq!(
            header.height + body.height + status.height,
            area.height,
            "chunks should fully cover the area height",
        );
        assert_eq!(header.width, 80);
        assert_eq!(header.y, 0);
        assert_eq!(body.y, 3);
        assert_eq!(status.y, 23);
    }

    #[test]
    fn detail_layout_does_not_panic_on_tight_height() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 4,
        };
        let [_h, _b, _s] = compute_detail_layout(area);
    }
}
