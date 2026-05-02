use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::format::{format_cost_value, format_duration_friendly};
use crate::state::{
    AppMode, AppState, BG, DIM, DIM_WHITE, GREEN, RED, SEPARATOR, SPINNER_FRAMES, TEAL, WHITE,
    YELLOW,
};

use super::blocks::{live_spinner_label, normalize_events, render_blocks};
use super::compute_detail_layout;

pub(in crate::ui) fn render_thread_detail_view(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &mut AppState,
) {
    let AppMode::ThreadDetail(ref detail) = state.mode else {
        return;
    };

    let tick = state.tick;

    // ── Layout: header / content / status bar ───────────────────────
    let chunks = compute_detail_layout(area);

    let inner_width = chunks[1].width.saturating_sub(2) as usize;

    // ── Header ──────────────────────────────────────────────────────
    let thread_id_short = if detail.thread_id.len() > 16 {
        &detail.thread_id[..16]
    } else {
        &detail.thread_id
    };
    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            format!(" thread:{thread_id_short}… "),
            Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
        )]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SEPARATOR));

    let mut header_spans: Vec<Span> = Vec::new();
    header_spans.push(Span::styled(" ", Style::default()));

    // Turn count
    header_spans.push(Span::styled(
        format!("{} turns", detail.turn_count),
        Style::default().fg(YELLOW),
    ));

    // Model(s)
    let unique_models: Vec<&str> = {
        let mut seen = std::collections::HashSet::new();
        detail
            .models
            .iter()
            .filter(|m| seen.insert(m.as_str()))
            .map(|m| m.as_str())
            .collect()
    };
    let model_display = if unique_models.len() == 1 {
        unique_models[0].to_string()
    } else {
        format!("{}*", unique_models.last().unwrap_or(&""))
    };
    header_spans.push(Span::styled(
        format!("  {model_display}"),
        Style::default().fg(WHITE),
    ));

    // Backend(s)
    let unique_backends: Vec<&str> = {
        let mut seen = std::collections::HashSet::new();
        detail
            .backends
            .iter()
            .filter(|b| seen.insert(b.as_str()))
            .map(|b| b.as_str())
            .collect()
    };
    if let Some(backend) = unique_backends.first() {
        header_spans.push(Span::styled(
            format!("  {backend}"),
            Style::default().fg(DIM),
        ));
    }

    // Token totals and cost across all turns
    {
        let mut total_cost = 0.0f64;
        let mut has_cost = false;
        let all_turns = detail
            .historical_turns
            .iter()
            .enumerate()
            .map(|(i, events)| (i, events.as_slice()))
            .chain(std::iter::once((
                detail.historical_turns.len(),
                detail.active_events.as_slice(),
            )));
        for (i, events) in all_turns {
            let (ti, to) = events.iter().fold((0u64, 0u64), |(ai, ao), e| {
                if let ParsedStreamEvent::Usage {
                    prompt_tokens,
                    completion_tokens,
                } = e
                {
                    (ai + prompt_tokens, ao + completion_tokens)
                } else {
                    (ai, ao)
                }
            });
            if (ti > 0 || to > 0)
                && detail.backends.get(i).map(|b| b.as_str()) == Some("api")
                && let Some(m) = detail.models.get(i)
            {
                let c = consult_llm_core::llm_cost::calculate_cost(ti, to, m);
                if c.total_cost > 0.0 {
                    total_cost += c.total_cost;
                    has_cost = true;
                }
            }
        }
        if has_cost {
            header_spans.push(Span::styled(
                format!("  {}", format_cost_value(total_cost)),
                Style::default().fg(DIM_WHITE),
            ));
        }
    }

    // Total duration
    header_spans.push(Span::styled(
        format!("  {}", format_duration_friendly(detail.total_duration_ms)),
        Style::default().fg(DIM_WHITE),
    ));

    // Success/failure indicator
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

    // ── Content: render all turns ────────────────────────────────────
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut turn_line_offsets: Vec<usize> = Vec::new();

    // Render historical turns (all except the last)
    for (i, turn_events) in detail.historical_turns.iter().enumerate() {
        // Turn separator
        turn_line_offsets.push(lines.len());
        let turn_num = i + 1;
        let model = detail.models.get(i).map(|m| m.as_str()).unwrap_or("?");
        let turn_header = format!(" Turn {turn_num} \u{b7} {model} ");
        let dashes_left = "\u{2500}\u{2500}\u{2500}";
        let right_len = inner_width.saturating_sub(4 + turn_header.chars().count());
        let dashes_right = "\u{2500}".repeat(right_len);

        lines.push(Line::from(vec![
            Span::styled(format!("  {dashes_left}"), Style::default().fg(TEAL)),
            Span::styled(turn_header, Style::default().fg(TEAL)),
            Span::styled(dashes_right, Style::default().fg(TEAL)),
        ]));
        lines.push(Line::default());

        let turn_model = detail.models.get(i).map(|m| m.as_str());
        let turn_backend = detail.backends.get(i).map(|b| b.as_str());
        let blocks = normalize_events(turn_events);
        let (turn_lines, _) =
            render_blocks(&blocks, inner_width, tick, false, turn_model, turn_backend);
        lines.extend(turn_lines);
        lines.push(Line::default());
    }

    let historical_turn_count = detail.historical_turns.len();

    // Render the active (latest) turn
    let active_turn_idx = detail.turn_ids.len().saturating_sub(1);
    turn_line_offsets.push(lines.len());
    let turn_num = historical_turn_count + 1;
    let model = detail
        .models
        .get(active_turn_idx)
        .map(|m| m.as_str())
        .unwrap_or("?");
    let turn_header = format!(" Turn {turn_num} \u{b7} {model} ");
    let dashes_left = "\u{2500}\u{2500}\u{2500}";
    let right_len = inner_width.saturating_sub(4 + turn_header.chars().count());
    let dashes_right = "\u{2500}".repeat(right_len);

    lines.push(Line::from(vec![
        Span::styled(format!("  {dashes_left}"), Style::default().fg(TEAL)),
        Span::styled(turn_header, Style::default().fg(TEAL)),
        Span::styled(dashes_right, Style::default().fg(TEAL)),
    ]));
    lines.push(Line::default());

    let active_model = detail.models.get(active_turn_idx).map(|m| m.as_str());
    let active_backend = detail.backends.get(active_turn_idx).map(|b| b.as_str());
    let active_blocks = normalize_events(&detail.active_events);
    let active_turn_base = lines.len();
    let (active_lines, active_response_offset) = render_blocks(
        &active_blocks,
        inner_width,
        tick,
        false,
        active_model,
        active_backend,
    );
    lines.extend(active_lines);
    let response_line_offset = active_response_offset.map(|off| active_turn_base + off);

    // Append spinner if latest turn is still live
    let is_live = detail
        .turn_ids
        .last()
        .is_some_and(|run_id| state.is_run_active(run_id));
    if is_live {
        let spinner = SPINNER_FRAMES[tick % SPINNER_FRAMES.len()];
        let label = live_spinner_label(&detail.active_events);
        lines.push(Line::default());
        lines.push(Line::from(vec![Span::styled(
            format!("  {spinner} {label}"),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )]));
    }

    // Store turn_line_offsets and response offset back into state
    if let AppMode::ThreadDetail(ref mut detail) = state.mode {
        detail.turn_line_offsets = turn_line_offsets;
        detail.response_line_offset = response_line_offset;
    }

    // ── Scroll / viewport ───────────────────────────────────────────
    let inner_height = chunks[1].height.saturating_sub(2) as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(inner_height);
    state.detail_inner_height = inner_height;

    let effective_scroll = if let AppMode::ThreadDetail(ref mut detail) = state.mode {
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
    let follow_on = matches!(state.mode, AppMode::ThreadDetail(ref d) if d.auto_scroll);

    let mut bar_spans = vec![
        Span::styled(" q/Esc", Style::default().fg(TEAL)),
        Span::styled(" back  ", Style::default().fg(DIM_WHITE)),
        Span::styled("j/k", Style::default().fg(TEAL)),
        Span::styled(" scroll  ", Style::default().fg(DIM_WHITE)),
        Span::styled("d/u", Style::default().fg(TEAL)),
        Span::styled(" half-page  ", Style::default().fg(DIM_WHITE)),
        Span::styled("[/]", Style::default().fg(TEAL)),
        Span::styled(" prev/next turn", Style::default().fg(DIM_WHITE)),
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
    let bar = Line::from(bar_spans);
    frame.render_widget(
        Paragraph::new(bar).style(Style::default().bg(BG)),
        chunks[2],
    );
}
