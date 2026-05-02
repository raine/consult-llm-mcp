use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use chrono::Utc;
use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::format::{format_cost, format_duration_friendly, format_token_count};
use crate::state::{
    AppMode, AppState, BG, DIM, DIM_WHITE, GREEN, RED, SEPARATOR, SPINNER_FRAMES, TEAL, WHITE,
    task_mode_color,
};

use super::blocks::{live_spinner_label, normalize_events, render_blocks, split_at_width};
use super::compute_detail_layout;

pub(in crate::ui) fn render_detail_view(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &mut AppState,
) {
    let AppMode::Detail(ref detail) = state.mode else {
        return;
    };

    let run_id = detail.run_id.clone();
    let tick = state.tick;

    // ── Layout: header / content / status bar ───────────────────────
    let chunks = compute_detail_layout(area);

    let inner_width = chunks[1].width.saturating_sub(2) as usize;

    // ── Header ──────────────────────────────────────────────────────
    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            format!(" {run_id} "),
            Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
        )]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SEPARATOR));

    let mut header_spans: Vec<Span> = Vec::new();
    header_spans.push(Span::styled(" ", Style::default()));

    if let Some(ref model) = detail.model {
        header_spans.push(Span::styled(model.clone(), Style::default().fg(WHITE)));
    }
    if let Some(ref backend) = detail.backend {
        header_spans.push(Span::styled(
            format!("  {backend}"),
            Style::default().fg(DIM),
        ));
    }
    {
        let mode = detail.task_mode.as_deref();
        header_spans.push(Span::styled(
            format!("  {}", mode.unwrap_or("general")),
            Style::default().fg(task_mode_color(mode)),
        ));
    }
    if let Some(ref stage) = detail.last_stage {
        header_spans.push(Span::styled(
            format!("  {stage}"),
            Style::default().fg(DIM_WHITE),
        ));
    }
    if let Some(ref effort) = detail.reasoning_effort {
        header_spans.push(Span::styled(
            format!("  reasoning:{effort}"),
            Style::default().fg(DIM),
        ));
    }

    // Show token totals from Usage events
    let (total_in, total_out) = detail.events.iter().fold((0u64, 0u64), |(i, o), e| {
        if let ParsedStreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
        } = e
        {
            (i + prompt_tokens, o + completion_tokens)
        } else {
            (i, o)
        }
    });
    if total_in > 0 || total_out > 0 {
        header_spans.push(Span::styled(
            format!(
                "  {}/{}",
                format_token_count(total_in),
                format_token_count(total_out)
            ),
            Style::default().fg(DIM_WHITE),
        ));
        if let (Some(model), Some(backend)) = (&detail.model, &detail.backend) {
            let cost_str = format_cost(Some(total_in), Some(total_out), model, backend);
            if cost_str != "\u{2014}" {
                header_spans.push(Span::styled(
                    format!("  {cost_str}"),
                    Style::default().fg(DIM_WHITE),
                ));
            }
        }
    }

    // Duration or live elapsed
    let is_live = state.is_run_active(&run_id);
    if is_live {
        if let Some(started_at) = detail.started_at {
            let elapsed_ms = Utc::now()
                .signed_duration_since(started_at)
                .num_milliseconds()
                .max(0) as u64;
            header_spans.push(Span::styled(
                format!("  {}", format_duration_friendly(elapsed_ms)),
                Style::default().fg(DIM_WHITE),
            ));
        }
    } else if let Some(duration_ms) = detail.duration_ms {
        header_spans.push(Span::styled(
            format!("  {}", format_duration_friendly(duration_ms)),
            Style::default().fg(DIM_WHITE),
        ));
    }

    // Relative timestamp
    if let Some(started_at) = detail.started_at {
        let secs = Utc::now()
            .signed_duration_since(started_at)
            .num_seconds()
            .max(0);
        let relative = if secs < 10 {
            "just now".to_string()
        } else if secs < 60 {
            format!("{}s ago", secs)
        } else if secs < 3600 {
            format!("{}m ago", secs / 60)
        } else if secs < 86400 {
            format!("{}h ago", secs / 3600)
        } else {
            format!("{}d ago", secs / 86400)
        };
        header_spans.push(Span::styled(
            format!("  {relative}"),
            Style::default().fg(DIM),
        ));
    }

    // Success/failure indicator (completed only)
    if let Some(success) = detail.success {
        let (icon, color) = if success {
            ("\u{2713}", GREEN)
        } else {
            ("\u{2717}", RED)
        };
        header_spans.push(Span::styled(
            format!("  {icon}"),
            Style::default().fg(color),
        ));
    }

    // Project
    if let Some(ref project) = detail.project {
        header_spans.push(Span::styled(
            format!("  {project}"),
            Style::default().fg(DIM),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(header_spans)).block(block),
        chunks[0],
    );

    // ── Cached content rendering ────────────────────────────────────
    // normalize_events + render_blocks (with markdown/syntect) is expensive.
    // Cache the result and only recompute when events change, width changes,
    // or in-progress tool spinners need animation.
    let event_count = detail.events.len();
    let has_active_tools = detail.events.iter().any(|e| {
        matches!(e, ParsedStreamEvent::ToolStarted { call_id, .. }
            if !detail.events.iter().any(|e2| matches!(e2, ParsedStreamEvent::ToolFinished { call_id: cid, .. } if cid == call_id)))
    });

    let cache_valid = detail.cached_lines.is_some()
        && detail.cached_event_count == event_count
        && detail.cached_width == inner_width
        && !has_active_tools
        && !detail.cached_has_active_tools;

    let (mut lines, response_offset) = if cache_valid {
        (
            detail.cached_lines.clone().unwrap(),
            detail.response_line_offset,
        )
    } else {
        let blocks = normalize_events(&detail.events);

        render_blocks(
            &blocks,
            inner_width,
            tick,
            detail.show_system_prompt,
            detail.model.as_deref(),
            detail.backend.as_deref(),
        )
    };

    // Append a spinner when the run is still live
    if is_live {
        let spinner = SPINNER_FRAMES[tick % SPINNER_FRAMES.len()];
        let label = live_spinner_label(&detail.events);
        lines.push(Line::default());
        lines.push(Line::from(vec![Span::styled(
            format!("  {spinner} {label}"),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )]));
    }

    // Clone error before mutable borrow for cache update
    let detail_error = detail.error.clone();

    // Update cache (store lines before the live spinner was appended)
    if !cache_valid && let AppMode::Detail(ref mut detail) = state.mode {
        let cache_lines = if is_live {
            // Remove the 2 spinner lines we just appended
            lines[..lines.len().saturating_sub(2)].to_vec()
        } else {
            lines.clone()
        };
        detail.cached_lines = Some(cache_lines);
        detail.cached_event_count = event_count;
        detail.cached_width = inner_width;
        detail.cached_has_active_tools = has_active_tools;
        detail.response_line_offset = response_offset;
    }

    // Show error message when the run failed (after cache, like spinner)
    if let Some(ref error) = detail_error {
        let prefix = "  Error: ";
        let cont = "         ";
        let wrap_width = inner_width.saturating_sub(cont.len());
        lines.push(Line::default());
        if wrap_width > 0 {
            let mut remaining = error.as_str();
            let mut first = true;
            while !remaining.is_empty() {
                let indent = if first { prefix } else { cont };
                first = false;
                let (chunk, rest) = split_at_width(remaining, wrap_width);
                lines.push(Line::from(vec![Span::styled(
                    format!("{indent}{chunk}"),
                    Style::default().fg(RED),
                )]));
                remaining = rest;
            }
        } else {
            lines.push(Line::from(vec![Span::styled(
                format!("{prefix}{error}"),
                Style::default().fg(RED),
            )]));
        }
    }

    // ── Scroll / viewport ───────────────────────────────────────────
    let inner_height = chunks[1].height.saturating_sub(2) as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(inner_height);
    state.detail_inner_height = inner_height;

    let effective_scroll = if let AppMode::Detail(ref mut detail) = state.mode {
        detail.scroll = detail.scroll.min(max_scroll);
        // Auto-enable follow when scrolled to bottom of a live run
        if is_live && detail.scroll >= max_scroll {
            detail.auto_scroll = true;
        }
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

    // ── Status bar ──────────────────────────────────────────────────
    let is_live = state.is_run_active(&run_id);
    let follow_on = matches!(state.mode, AppMode::Detail(ref d) if d.auto_scroll);

    let mut bar_spans = vec![
        Span::styled(" q/Esc", Style::default().fg(TEAL)),
        Span::styled(" back  ", Style::default().fg(DIM_WHITE)),
        Span::styled("j/k", Style::default().fg(TEAL)),
        Span::styled(" scroll  ", Style::default().fg(DIM_WHITE)),
        Span::styled("d/u", Style::default().fg(TEAL)),
        Span::styled(" half-page  ", Style::default().fg(DIM_WHITE)),
        Span::styled("r", Style::default().fg(TEAL)),
        Span::styled(" response  ", Style::default().fg(DIM_WHITE)),
        Span::styled("s", Style::default().fg(TEAL)),
        Span::styled(" sys prompt", Style::default().fg(DIM_WHITE)),
    ];
    if is_live {
        bar_spans.push(Span::styled("  G", Style::default().fg(TEAL)));
        bar_spans.push(Span::styled(" follow  ", Style::default().fg(DIM_WHITE)));
        if follow_on {
            bar_spans.push(Span::styled(
                " FOLLOW ",
                Style::default()
                    .fg(BG)
                    .bg(TEAL)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }

    // Sibling indicator (right-aligned)
    let sibling_indicator = if let AppMode::Detail(ref d) = state.mode {
        if d.siblings.len() > 1 {
            Some(format!(" {}/{} ", d.sibling_index + 1, d.siblings.len()))
        } else {
            None
        }
    } else {
        None
    };

    let bar = if let Some(ref indicator) = sibling_indicator {
        // Use chars().count() not .len() — the arrows ◂▸ are multi-byte but single-width
        let left_len: usize = bar_spans.iter().map(|s| s.content.chars().count()).sum();
        let tab_hint = "Tab ◂▸  ";
        let total_content = left_len + tab_hint.chars().count() + indicator.chars().count();
        let padding = (chunks[2].width as usize).saturating_sub(total_content);
        bar_spans.push(Span::styled(" ".repeat(padding), Style::default()));
        bar_spans.push(Span::styled(tab_hint, Style::default().fg(TEAL)));
        bar_spans.push(Span::styled(
            indicator.clone(),
            Style::default()
                .fg(BG)
                .bg(TEAL)
                .add_modifier(Modifier::BOLD),
        ));
        Line::from(bar_spans)
    } else {
        Line::from(bar_spans)
    };

    frame.render_widget(
        Paragraph::new(bar).style(Style::default().bg(BG)),
        chunks[2],
    );
}
