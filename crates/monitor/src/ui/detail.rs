use std::collections::HashMap;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use consult_llm_core::stream_events::ParsedStreamEvent;

use chrono::Utc;

use crate::format::{format_duration_friendly, format_token_count};
use crate::state::{
    AppMode, AppState, BG, DIM, DIM_WHITE, GREEN, RED, SEPARATOR, SPINNER_FRAMES, TEAL, WHITE,
};

// ── Intermediate representation ─────────────────────────────────────────

enum RenderedBlock {
    Prompt(String),
    Thinking,
    Tool {
        label: String,
        success: Option<bool>,
    },
    Text(String),
    Usage {
        prompt_tokens: u64,
        completion_tokens: u64,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Start,
    Prompt,
    Thinking,
    Tool,
    Text,
    Usage,
}

impl RenderedBlock {
    fn phase(&self) -> Phase {
        match self {
            RenderedBlock::Prompt(_) => Phase::Prompt,
            RenderedBlock::Thinking => Phase::Thinking,
            RenderedBlock::Tool { .. } => Phase::Tool,
            RenderedBlock::Text(_) => Phase::Text,
            RenderedBlock::Usage { .. } => Phase::Usage,
        }
    }
}

// ── Rendering ───────────────────────────────────────────────────────────

pub(super) fn render_detail_view(frame: &mut ratatui::Frame, area: Rect, state: &mut AppState) {
    let AppMode::Detail(ref detail) = state.mode else {
        return;
    };

    let consultation_id = detail.consultation_id.clone();
    let tick = state.tick;

    // ── Layout: header / content / status bar ───────────────────────
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    let inner_width = chunks[1].width.saturating_sub(2) as usize;

    // ── Header ──────────────────────────────────────────────────────
    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            format!(" {consultation_id} "),
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
    }

    // Duration or live elapsed
    let is_live = state.is_consultation_active(&consultation_id);
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

    let mut lines = if cache_valid {
        detail.cached_lines.clone().unwrap()
    } else {
        let blocks = normalize_events(&detail.events);

        render_blocks(&blocks, inner_width, tick)
    };

    // Append a spinner when the consultation is still live
    if is_live {
        let spinner = SPINNER_FRAMES[tick % SPINNER_FRAMES.len()];
        lines.push(Line::default());
        lines.push(Line::from(vec![Span::styled(
            format!("  {spinner} Generating..."),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )]));
    }

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
    }

    // ── Scroll / viewport ───────────────────────────────────────────
    let inner_height = chunks[1].height.saturating_sub(2) as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(inner_height);
    state.detail_inner_height = inner_height;

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

    // ── Status bar ──────────────────────────────────────────────────
    let is_live = state.is_consultation_active(&consultation_id);
    let follow_on = matches!(state.mode, AppMode::Detail(ref d) if d.auto_scroll);

    let mut bar_spans = vec![
        Span::styled(" q/Esc", Style::default().fg(TEAL)),
        Span::styled(" back  ", Style::default().fg(DIM_WHITE)),
        Span::styled("j/k", Style::default().fg(TEAL)),
        Span::styled(" scroll  ", Style::default().fg(DIM_WHITE)),
        Span::styled("d/u", Style::default().fg(TEAL)),
        Span::styled(" half-page", Style::default().fg(DIM_WHITE)),
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

// ── Pass 1: events → RenderedBlock ──────────────────────────────────────

fn normalize_events(events: &[ParsedStreamEvent]) -> Vec<RenderedBlock> {
    let mut blocks: Vec<RenderedBlock> = Vec::new();
    let mut tool_indices: HashMap<&str, usize> = HashMap::new();

    for event in events {
        match event {
            ParsedStreamEvent::SessionStarted { .. } => {}
            ParsedStreamEvent::Prompt { text } => {
                blocks.push(RenderedBlock::Prompt(text.clone()));
            }
            ParsedStreamEvent::Thinking => {
                blocks.push(RenderedBlock::Thinking);
            }
            ParsedStreamEvent::ToolStarted { call_id, label } => {
                let idx = blocks.len();
                blocks.push(RenderedBlock::Tool {
                    label: label.clone(),
                    success: None,
                });
                tool_indices.insert(call_id.as_str(), idx);
            }
            ParsedStreamEvent::ToolFinished { call_id, success } => {
                if let Some(&idx) = tool_indices.get(call_id.as_str())
                    && let Some(RenderedBlock::Tool { success: s, .. }) = blocks.get_mut(idx)
                {
                    *s = Some(*success);
                }
            }
            ParsedStreamEvent::AssistantText { text } if !text.is_empty() => {
                // Merge consecutive text chunks into a single block so
                // markdown renders correctly across streaming boundaries.
                if let Some(RenderedBlock::Text(prev)) = blocks.last_mut() {
                    prev.push_str(text);
                } else {
                    blocks.push(RenderedBlock::Text(text.clone()));
                }
            }
            ParsedStreamEvent::Usage {
                prompt_tokens,
                completion_tokens,
            } => {
                blocks.push(RenderedBlock::Usage {
                    prompt_tokens: *prompt_tokens,
                    completion_tokens: *completion_tokens,
                });
            }
            _ => {}
        }
    }

    blocks
}

// ── Pass 2: RenderedBlock → Line ────────────────────────────────────────

fn render_blocks(blocks: &[RenderedBlock], inner_width: usize, tick: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    let mut current_phase = Phase::Start;
    let mut response_header_shown = false;

    for block in blocks {
        let next_phase = block.phase();

        // Insert blank line on phase transitions (but not from Start or Prompt)
        let phase_changed = next_phase != current_phase;
        if phase_changed && current_phase != Phase::Start && current_phase != Phase::Prompt {
            lines.push(Line::default());
        }
        current_phase = next_phase;

        match block {
            RenderedBlock::Prompt(text) => {
                lines.push(Line::from(vec![Span::styled(
                    "  Prompt:",
                    Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
                )]));
                let indent = "    ";
                let wrap_width = inner_width.saturating_sub(indent.len());
                let md_lines = super::markdown::render_markdown(text, wrap_width);
                for line in md_lines {
                    let mut indented = vec![Span::raw(indent.to_string())];
                    indented.extend(line.spans);
                    lines.push(Line::from(indented));
                }
                lines.push(Line::default());
            }
            RenderedBlock::Thinking => {
                lines.push(Line::from(vec![Span::styled(
                    "  Thinking...",
                    Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                )]));
            }
            RenderedBlock::Tool { label, success } => {
                lines.push(render_tool_line(label, *success, inner_width, tick));
            }
            RenderedBlock::Text(text) => {
                if !response_header_shown {
                    response_header_shown = true;
                    lines.push(Line::from(vec![Span::styled(
                        "  Response:",
                        Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
                    )]));
                }
                let indent = "    ";
                let wrap_width = inner_width.saturating_sub(indent.len());
                let md_lines = super::markdown::render_markdown(text, wrap_width);
                for line in md_lines {
                    let mut indented = vec![Span::raw(indent.to_string())];
                    indented.extend(line.spans);
                    lines.push(Line::from(indented));
                }
            }
            RenderedBlock::Usage {
                prompt_tokens,
                completion_tokens,
            } => {
                lines.push(render_usage_line(
                    *prompt_tokens,
                    *completion_tokens,
                    inner_width,
                ));
            }
        }
    }

    lines
}

// ── Tool line ───────────────────────────────────────────────────────────

fn render_tool_line(
    label: &str,
    success: Option<bool>,
    inner_width: usize,
    tick: usize,
) -> Line<'static> {
    match success {
        Some(ok) => {
            // Completed: "  ▶ {label} ··· ✓"
            let icon_char = if ok { "\u{2713}" } else { "\u{2717}" };
            let icon_color = if ok { GREEN } else { RED };
            let suffix = format!(" {icon_char} ");
            let suffix_len = suffix.chars().count();

            // Truncate label if it would push the icon off-screen
            // Reserve: 4 (indent+"› ") + 1 (space) + suffix + 3 (min dots)
            let overhead = 5 + suffix_len + 3;
            let max_label = inner_width.saturating_sub(overhead);
            let display_label = if label.chars().count() > max_label && max_label > 0 {
                format!(
                    "{}…",
                    label
                        .chars()
                        .take(max_label.saturating_sub(1))
                        .collect::<String>()
                )
            } else {
                label.to_string()
            };

            let prefix = format!("  \u{203a} {display_label} ");
            let prefix_len = prefix.chars().count();
            let dots_count = inner_width.saturating_sub(prefix_len + suffix_len);
            let dots = "\u{b7}".repeat(dots_count);

            Line::from(vec![
                Span::styled(prefix, Style::default().fg(DIM_WHITE)),
                Span::styled(dots, Style::default().fg(DIM)),
                Span::styled(suffix, Style::default().fg(icon_color)),
            ])
        }
        None => {
            // In-progress: "  {spinner} {label}"
            let spinner = SPINNER_FRAMES[tick % SPINNER_FRAMES.len()];
            Line::from(vec![Span::styled(
                format!("  {spinner} {label}"),
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            )])
        }
    }
}

// ── Usage separator line ────────────────────────────────────────────────

fn render_usage_line(
    prompt_tokens: u64,
    completion_tokens: u64,
    inner_width: usize,
) -> Line<'static> {
    let label = format!(
        " tokens: {} in / {} out ",
        format_token_count(prompt_tokens),
        format_token_count(completion_tokens),
    );
    let prefix = "  \u{2500}\u{2500}\u{2500}";
    let prefix_len = prefix.chars().count();
    let label_len = label.chars().count();
    let right_len = inner_width.saturating_sub(prefix_len + label_len);
    let right_dashes = "\u{2500}".repeat(right_len);

    let dim = Style::default().fg(DIM);
    Line::from(vec![
        Span::styled(prefix.to_string(), dim),
        Span::styled(label, dim),
        Span::styled(right_dashes, dim),
    ])
}
