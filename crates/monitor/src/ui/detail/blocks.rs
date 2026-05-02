use std::collections::HashMap;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::format::{format_cost, format_token_count};
use crate::state::{DIM, DIM_WHITE, GREEN, RED, SPINNER_FRAMES, TEAL, WHITE};

/// Split off the longest prefix whose display width fits in `max_cols`.
/// Returns at least one char so the caller's loop always makes progress
/// even when a single wide char is wider than the budget.
pub(super) fn split_at_width(text: &str, max_cols: usize) -> (&str, &str) {
    let mut used = 0usize;
    let mut byte_end = 0usize;
    for (i, ch) in text.char_indices() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if byte_end > 0 && used + w > max_cols {
            break;
        }
        byte_end = i + ch.len_utf8();
        used += w;
    }
    text.split_at(byte_end)
}

// ── Intermediate representation ─────────────────────────────────────────

pub(super) enum RenderedBlock {
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

// ── Pass 1: events → RenderedBlock ──────────────────────────────────────

pub(super) fn normalize_events(events: &[ParsedStreamEvent]) -> Vec<RenderedBlock> {
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

pub(super) fn render_blocks(
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
                            let (chunk, rest) = split_at_width(remaining, wrap_width);
                            lines.push(Line::from(vec![
                                Span::raw(indent.to_string()),
                                Span::styled(chunk.to_string(), dim_style),
                            ]));
                            remaining = rest;
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
                let md_lines = crate::ui::markdown::render_markdown(text, wrap_width);
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
                    let md_lines = crate::ui::markdown::render_markdown(text, wrap_width);
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
                let md_lines = crate::ui::markdown::render_markdown(text, wrap_width);
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
pub(super) fn live_spinner_label(events: &[ParsedStreamEvent]) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::split_at_width;

    #[test]
    fn split_at_width_does_not_panic_inside_multibyte_char() {
        // "café" is 5 bytes. Byte-level slicing at offset 4 panics ("byte
        // index 4 is not a char boundary; it is inside 'é'"). The
        // width-aware splitter must land on a char boundary.
        let (chunk, rest) = split_at_width("café", 4);
        assert_eq!(chunk, "café");
        assert_eq!(rest, "");
    }

    #[test]
    fn split_at_width_respects_display_width() {
        let (chunk, rest) = split_at_width("hello world", 5);
        assert_eq!(chunk, "hello");
        assert_eq!(rest, " world");
    }

    #[test]
    fn split_at_width_advances_on_oversized_char() {
        // A wide char (CJK = 2 cols) wider than the budget still consumes
        // one char so callers don't loop forever.
        let (chunk, rest) = split_at_width("日本語", 1);
        assert_eq!(chunk, "日");
        assert_eq!(rest, "本語");
    }
}
