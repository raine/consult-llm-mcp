use std::collections::HashMap;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use consult_llm_core::stream_events::ParsedStreamEvent;

use chrono::Utc;

use crate::format::{format_cost, format_cost_value, format_duration_friendly, format_token_count};
use crate::state::{
    AppMode, AppState, BG, DIM, DIM_WHITE, GREEN, RED, SEPARATOR, SPINNER_FRAMES, TEAL, WHITE,
    YELLOW, task_mode_color,
};

// ── Intermediate representation ─────────────────────────────────────────

enum RenderedBlock {
    SystemPrompt(String),
    Prompt(String),
    Thinking(String),
    Tool {
        label: String,
        success: Option<bool>,
        error: Option<String>,
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
            RenderedBlock::SystemPrompt(_) => Phase::Prompt,
            RenderedBlock::Prompt(_) => Phase::Prompt,
            RenderedBlock::Thinking(_) => Phase::Thinking,
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

    let run_id = detail.run_id.clone();
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

    // Append a spinner when the consultation is still live
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

    // Show error message when the consultation failed (after cache, like spinner)
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
                let chunk_len = remaining.len().min(wrap_width);
                let (chunk, rest) = remaining.split_at(chunk_len);
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
        // Auto-enable follow when scrolled to bottom of a live consultation
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

// ── Pass 1: events → RenderedBlock ──────────────────────────────────────

fn normalize_events(events: &[ParsedStreamEvent]) -> Vec<RenderedBlock> {
    let mut blocks: Vec<RenderedBlock> = Vec::new();
    let mut tool_indices: HashMap<&str, usize> = HashMap::new();
    let mut pending_files: Option<Vec<String>> = None;

    for event in events {
        match event {
            ParsedStreamEvent::SessionStarted { .. } => {}
            ParsedStreamEvent::FilesContext { files } => {
                pending_files = Some(files.clone());
            }
            ParsedStreamEvent::SystemPrompt { text } => {
                blocks.push(RenderedBlock::SystemPrompt(text.clone()));
            }
            ParsedStreamEvent::Prompt { text } => {
                let text = if let Some(files) = pending_files.take() {
                    let stripped = strip_inlined_files(text);
                    let mut result = String::from("## Relevant Files\n\n");
                    for f in &files {
                        result.push_str(&format!("- `{f}`\n"));
                    }
                    result.push('\n');
                    result.push_str(&stripped);
                    result
                } else {
                    text.clone()
                };
                blocks.push(RenderedBlock::Prompt(text));
            }
            ParsedStreamEvent::Thinking { text } => {
                // Merge consecutive Thinking events into a single block
                if let Some(RenderedBlock::Thinking(prev)) = blocks.last_mut() {
                    prev.push_str(text);
                } else {
                    blocks.push(RenderedBlock::Thinking(text.clone()));
                }
            }
            ParsedStreamEvent::ToolStarted { call_id, label } => {
                let idx = blocks.len();
                blocks.push(RenderedBlock::Tool {
                    label: label.clone(),
                    success: None,
                    error: None,
                });
                tool_indices.insert(call_id.as_str(), idx);
            }
            ParsedStreamEvent::ToolFinished {
                call_id,
                success,
                error,
            } => {
                if let Some(&idx) = tool_indices.get(call_id.as_str())
                    && let Some(RenderedBlock::Tool {
                        success: s,
                        error: e,
                        ..
                    }) = blocks.get_mut(idx)
                {
                    *s = Some(*success);
                    *e = error.clone();
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

/// Strip the "## Relevant Files\n\n### File: ...\n```\n...\n```\n\n" section
/// from a prompt built by `build_prompt()`, preserving everything else.
fn strip_inlined_files(text: &str) -> String {
    let Some(start) = text.find("## Relevant Files\n") else {
        return text.to_string();
    };

    let before = &text[..start];
    let after_header = &text[start..];

    // Skip past "## Relevant Files\n\n" then all "### File: ...\n```\n...\n```\n\n" blocks
    const FENCE: &str = "\n```\n";
    let mut pos = "## Relevant Files\n".len();
    // Skip optional blank line after header
    if after_header[pos..].starts_with('\n') {
        pos += 1;
    }

    // Skip each "### File:" block
    while after_header[pos..].starts_with("### File:") {
        // Find the opening ``` after the "### File: ..." line
        if let Some(rel) = after_header[pos..].find(FENCE) {
            let content_start = pos + rel + FENCE.len();
            // Find the closing ```
            if let Some(rel_end) = after_header[content_start..].find(FENCE) {
                pos = content_start + rel_end + FENCE.len();
                // Skip trailing blank lines
                while after_header[pos..].starts_with('\n') {
                    pos += 1;
                }
            } else {
                // No closing fence found, bail
                break;
            }
        } else {
            break;
        }
    }

    let remaining = &after_header[pos..];
    let mut result = before.to_string();
    result.push_str(remaining);
    result
}

// ── Pass 2: RenderedBlock → Line ────────────────────────────────────────

fn render_blocks(
    blocks: &[RenderedBlock],
    inner_width: usize,
    tick: usize,
    show_system_prompt: bool,
    model: Option<&str>,
    backend: Option<&str>,
) -> (Vec<Line<'static>>, Option<usize>) {
    let mut lines: Vec<Line> = Vec::new();
    let mut current_phase = Phase::Start;
    let mut response_header_shown = false;
    let mut response_line_offset: Option<usize> = None;
    let last_idx = blocks.len().saturating_sub(1);

    for (i, block) in blocks.iter().enumerate() {
        let next_phase = block.phase();

        // Insert blank line on phase transitions (but not from Start or Prompt)
        let phase_changed = next_phase != current_phase;
        if phase_changed && current_phase != Phase::Start && current_phase != Phase::Prompt {
            lines.push(Line::default());
        }
        current_phase = next_phase;

        match block {
            RenderedBlock::SystemPrompt(text) => {
                if !show_system_prompt {
                    continue;
                }
                lines.push(Line::from(vec![Span::styled(
                    "  System Prompt:",
                    Style::default().fg(DIM_WHITE).add_modifier(Modifier::BOLD),
                )]));
                let indent = "    ";
                let wrap_width = inner_width.saturating_sub(indent.len());
                let dim_style = Style::default().fg(DIM);
                for raw_line in text.lines() {
                    if raw_line.is_empty() {
                        lines.push(Line::default());
                    } else {
                        // Simple word-wrap for system prompt (no markdown)
                        let mut remaining = raw_line;
                        while !remaining.is_empty() {
                            let chunk_len = remaining.len().min(wrap_width);
                            let chunk = &remaining[..chunk_len];
                            lines.push(Line::from(vec![
                                Span::raw(indent.to_string()),
                                Span::styled(chunk.to_string(), dim_style),
                            ]));
                            remaining = &remaining[chunk_len..];
                        }
                    }
                }
                lines.push(Line::default());
            }
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
            RenderedBlock::Thinking(text) => {
                if text.is_empty() {
                    // Skip trailing empty thinking block – the live spinner
                    // already shows "Thinking…" so rendering it here too would
                    // duplicate the label.
                    if i == last_idx {
                        continue;
                    }
                    lines.push(Line::from(vec![Span::styled(
                        "  Thinking...",
                        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                    )]));
                } else {
                    lines.push(Line::from(vec![Span::styled(
                        "  Thinking:",
                        Style::default().fg(DIM).add_modifier(Modifier::BOLD),
                    )]));
                    let indent = "    ";
                    let wrap_width = inner_width.saturating_sub(indent.len());
                    let md_lines = super::markdown::render_markdown(text, wrap_width);
                    let dim_italic = Style::default().fg(DIM).add_modifier(Modifier::ITALIC);
                    for line in md_lines {
                        let mut indented: Vec<Span<'static>> = vec![Span::raw(indent.to_string())];
                        for span in line.spans {
                            indented.push(Span::styled(
                                span.content.into_owned(),
                                span.style.patch(dim_italic),
                            ));
                        }
                        lines.push(Line::from(indented));
                    }
                }
            }
            RenderedBlock::Tool {
                label,
                success,
                error,
            } => {
                lines.push(render_tool_line(label, *success, inner_width, tick));
                if let Some(err) = error {
                    lines.push(Line::from(vec![
                        Span::raw("      "),
                        Span::styled(err.clone(), Style::default().fg(RED)),
                    ]));
                }
            }
            RenderedBlock::Text(text) => {
                if !response_header_shown {
                    response_header_shown = true;
                    response_line_offset = Some(lines.len());
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
                    model,
                    backend,
                ));
            }
        }
    }

    (lines, response_line_offset)
}

/// Pick the live spinner label based on what the last event was.
fn live_spinner_label(events: &[ParsedStreamEvent]) -> &'static str {
    match events.last() {
        Some(ParsedStreamEvent::Thinking { .. }) => "Thinking...",
        _ => "Generating...",
    }
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

// ── Thread detail view ──────────────────────────────────────────────────

pub(super) fn render_thread_detail_view(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &mut AppState,
) {
    let AppMode::ThreadDetail(ref detail) = state.mode else {
        return;
    };

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
        // Auto-enable follow when scrolled to bottom of a live consultation
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

// ── Usage separator line ────────────────────────────────────────────────

fn render_usage_line(
    prompt_tokens: u64,
    completion_tokens: u64,
    inner_width: usize,
    model: Option<&str>,
    backend: Option<&str>,
) -> Line<'static> {
    let cost_suffix = model
        .zip(backend)
        .map(|(m, b)| {
            let s = format_cost(Some(prompt_tokens), Some(completion_tokens), m, b);
            if s == "\u{2014}" {
                String::new()
            } else {
                format!(" \u{b7} {s}")
            }
        })
        .unwrap_or_default();
    let label = format!(
        " tokens: {} in / {} out{cost_suffix} ",
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
