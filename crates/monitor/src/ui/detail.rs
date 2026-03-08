use std::collections::HashMap;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::format::format_token_count;
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

    frame.render_widget(
        Paragraph::new(Line::from(header_spans)).block(block),
        chunks[0],
    );

    // ── Pass 1: normalize events → blocks ───────────────────────────
    let blocks = normalize_events(&detail.events);

    // ── Pass 2: blocks → ratatui lines ──────────────────────────────
    let lines = render_blocks(&blocks, inner_width, tick);

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

    for block in blocks {
        let next_phase = block.phase();

        // Insert blank line on phase transitions (but not from Start or Prompt)
        if next_phase != current_phase
            && current_phase != Phase::Start
            && current_phase != Phase::Prompt
        {
            lines.push(Line::default());
        }
        current_phase = next_phase;

        match block {
            RenderedBlock::Prompt(text) => {
                lines.push(Line::from(vec![Span::styled(
                    "  Prompt:",
                    Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
                )]));
                let indent = 4;
                let wrap_width = inner_width.saturating_sub(indent);
                for line in text.lines() {
                    for wrapped in wrap_line(line, wrap_width) {
                        lines.push(Line::from(vec![Span::styled(
                            format!("    {wrapped}"),
                            Style::default().fg(DIM_WHITE),
                        )]));
                    }
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

// ── Word wrapping ───────────────────────────────────────────────────────

fn wrap_line(line: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![line.to_string()];
    }
    if line.chars().count() <= max_width {
        return vec![line.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut col = 0;

    for word in line.split_whitespace() {
        let wlen = word.chars().count();
        if col == 0 {
            current.push_str(word);
            col = wlen;
        } else if col + 1 + wlen <= max_width {
            current.push(' ');
            current.push_str(word);
            col += 1 + wlen;
        } else {
            lines.push(current);
            current = word.to_string();
            col = wlen;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
