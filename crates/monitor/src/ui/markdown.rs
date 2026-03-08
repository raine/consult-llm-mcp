//! Markdown to ratatui rendering.
//!
//! Parses markdown with pulldown-cmark and produces ratatui `Line`/`Span` output
//! directly, without going through ANSI escape codes.
//!
//! Adapted from claude-history's TUI markdown renderer.

use std::sync::OnceLock;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

const HEADING_COLOR: Color = Color::Rgb(180, 190, 200);
const CODE_COLOR: Color = Color::Rgb(147, 161, 199);
const GREEN: Color = Color::Rgb(0, 255, 0);
const BLUE: Color = Color::Rgb(100, 149, 237);

/// Render markdown text to ratatui lines.
pub fn render_markdown(input: &str, max_width: usize) -> Vec<Line<'static>> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(input, options);
    let mut renderer = MarkdownRenderer::new(max_width);

    for event in parser {
        renderer.handle_event(event);
    }

    renderer.finish()
}

// ── Renderer state ──────────────────────────────────────────────────────

struct MarkdownRenderer {
    lines: Vec<Vec<StyledSpan>>,
    current_line: Vec<StyledSpan>,
    max_width: usize,
    current_width: usize,
    style_stack: Vec<MdStyle>,
    list_stack: Vec<ListContext>,
    in_code_block: bool,
    code_block_content: String,
    code_block_lang: String,
    in_list_item_start: bool,
    table_state: Option<TableState>,
}

#[derive(Clone)]
struct StyledSpan {
    text: String,
    fg: Option<Color>,
    bold: bool,
    dimmed: bool,
    italic: bool,
}

#[derive(Clone)]
enum MdStyle {
    Bold,
    Italic,
    Strikethrough,
    Quote,
    Link,
    Heading,
}

#[derive(Clone)]
struct ListContext {
    index: Option<u64>,
    depth: usize,
}

struct TableState {
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
}

impl TableState {
    fn new() -> Self {
        Self {
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
        }
    }
}

impl MarkdownRenderer {
    fn new(max_width: usize) -> Self {
        Self {
            lines: Vec::new(),
            current_line: Vec::new(),
            max_width,
            current_width: 0,
            style_stack: Vec::new(),
            list_stack: Vec::new(),
            in_code_block: false,
            code_block_content: String::new(),
            code_block_lang: String::new(),
            in_list_item_start: false,
            table_state: None,
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => self.inline_code(&code),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.flush_line(),
            Event::Rule => self.rule(),
            Event::Html(html) | Event::InlineHtml(html) => self.text(&html),
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                if !self.in_list_item_start
                    && (!self.lines.is_empty() || !self.current_line.is_empty())
                {
                    self.ensure_blank_line();
                }
                self.in_list_item_start = false;
            }
            Tag::Heading { .. } => {
                self.ensure_blank_line();
                self.style_stack.push(MdStyle::Heading);
            }
            Tag::CodeBlock(kind) => {
                self.ensure_blank_line();
                self.in_code_block = true;
                self.code_block_content.clear();
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code_block_lang = lang.clone();
                let fence = if lang.is_empty() {
                    "```".to_string()
                } else {
                    format!("```{lang}")
                };
                self.push_span(&fence, true, false, false, None);
                self.flush_line();
            }
            Tag::List(start) => {
                if self.list_stack.is_empty() {
                    self.ensure_blank_line();
                } else {
                    self.flush_line();
                }
                let depth = self.list_stack.len();
                self.list_stack.push(ListContext {
                    index: start,
                    depth,
                });
            }
            Tag::Item => {
                self.flush_line();
                let (bullet_text, is_numbered) = if let Some(ctx) = self.list_stack.last_mut() {
                    let indent = "  ".repeat(ctx.depth);
                    match &mut ctx.index {
                        None => (format!("{indent}- "), false),
                        Some(n) => {
                            let b = format!("{indent}{}. ", n);
                            *n += 1;
                            (b, true)
                        }
                    }
                } else {
                    ("- ".to_string(), false)
                };
                self.push_span(&bullet_text, is_numbered, false, false, None);
                self.in_list_item_start = true;
            }
            Tag::Emphasis => self.style_stack.push(MdStyle::Italic),
            Tag::Strong => self.style_stack.push(MdStyle::Bold),
            Tag::Strikethrough => self.style_stack.push(MdStyle::Strikethrough),
            Tag::BlockQuote(_) => {
                self.ensure_blank_line();
                self.push_span("> ", false, false, false, Some(GREEN));
                self.style_stack.push(MdStyle::Quote);
            }
            Tag::Link { .. } => {
                self.style_stack.push(MdStyle::Link);
            }
            Tag::Table(_) => {
                self.ensure_blank_line();
                self.table_state = Some(TableState::new());
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(state) = &mut self.table_state {
                    state.current_row = Vec::new();
                }
            }
            Tag::TableCell => {
                if let Some(state) = &mut self.table_state {
                    state.current_cell = String::new();
                }
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_line();
            }
            TagEnd::Heading(_) => {
                self.style_stack.pop();
                self.flush_line();
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                let content = std::mem::take(&mut self.code_block_content);
                let content = wrap_code_lines(&content, self.max_width);

                if let Some(highlighted) = highlight_code(&content, &self.code_block_lang) {
                    for line_tokens in highlighted {
                        for (text, fg, bold, italic) in line_tokens {
                            let text = text.trim_end_matches('\n');
                            self.push_span(text, false, bold, italic, Some(fg));
                        }
                        self.flush_line();
                    }
                } else {
                    for code_line in content.lines() {
                        self.push_span(code_line, false, false, false, Some(CODE_COLOR));
                        self.flush_line();
                    }
                }

                self.push_span("```", true, false, false, None);
                self.flush_line();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.in_list_item_start = false;
                // Add blank line after top-level lists
                if self.list_stack.is_empty() {
                    self.ensure_blank_line();
                }
            }
            TagEnd::Item => {
                self.flush_line();
                self.in_list_item_start = false;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.style_stack.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.style_stack.pop();
                self.flush_line();
            }
            TagEnd::Table => {
                if let Some(state) = self.table_state.take() {
                    let table_lines = render_table(&state.rows);
                    self.lines.extend(table_lines);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(state) = &mut self.table_state {
                    let row = std::mem::take(&mut state.current_row);
                    state.rows.push(row);
                }
            }
            TagEnd::TableCell => {
                if let Some(state) = &mut self.table_state {
                    let cell = std::mem::take(&mut state.current_cell);
                    state.current_row.push(cell);
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if let Some(state) = &mut self.table_state {
            state.current_cell.push_str(&text.replace('\n', " "));
            return;
        }

        if self.in_code_block {
            self.code_block_content.push_str(text);
            return;
        }

        let (fg, bold, dimmed, italic) = self.current_style();

        for word in text.split_inclusive(char::is_whitespace) {
            let word_width = word.chars().count();

            if self.current_width + word_width > self.max_width && self.current_width > 0 {
                self.flush_line();
                // Add list indent on continuation
                if let Some(ctx) = self.list_stack.last() {
                    let indent = "  ".repeat(ctx.depth + 1);
                    self.push_span(&indent, false, false, false, None);
                }
            }

            self.push_span(word, dimmed, bold, italic, fg);
        }
    }

    fn inline_code(&mut self, code: &str) {
        if let Some(state) = &mut self.table_state {
            state.current_cell.push_str(code);
            return;
        }
        // Wrap to next line if the code won't fit on the current line
        let code_width = code.chars().count();
        if self.current_width + code_width > self.max_width && self.current_width > 0 {
            self.flush_line();
            if let Some(ctx) = self.list_stack.last() {
                let indent = "  ".repeat(ctx.depth + 1);
                self.push_span(&indent, false, false, false, None);
            }
        }
        self.push_span(code, false, false, false, Some(CODE_COLOR));
    }

    fn rule(&mut self) {
        self.ensure_blank_line();
        let rule = "─".repeat(self.max_width.min(40));
        self.push_span(&rule, true, false, false, None);
        self.flush_line();
    }

    fn push_span(&mut self, text: &str, dimmed: bool, bold: bool, italic: bool, fg: Option<Color>) {
        if !text.is_empty() {
            self.current_line.push(StyledSpan {
                text: text.to_string(),
                fg,
                bold,
                dimmed,
                italic,
            });
            self.current_width += text.chars().count();
        }
    }

    fn flush_line(&mut self) {
        if !self.current_line.is_empty() {
            self.lines.push(std::mem::take(&mut self.current_line));
        }
        self.current_width = 0;
    }

    fn ensure_blank_line(&mut self) {
        self.flush_line();
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(Vec::new());
        }
    }

    fn current_style(&self) -> (Option<Color>, bool, bool, bool) {
        let mut fg = None;
        let mut bold = false;
        let mut dimmed = false;
        let mut italic = false;

        for s in &self.style_stack {
            match s {
                MdStyle::Bold => bold = true,
                MdStyle::Italic => italic = true,
                MdStyle::Strikethrough => dimmed = true,
                MdStyle::Quote => fg = Some(GREEN),
                MdStyle::Link => fg = Some(BLUE),
                MdStyle::Heading => {
                    bold = true;
                    fg = Some(HEADING_COLOR);
                }
            }
        }

        (fg, bold, dimmed, italic)
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_line();
        // Remove trailing empty lines
        while self.lines.last().is_some_and(|l| l.is_empty()) {
            self.lines.pop();
        }
        self.lines.into_iter().map(spans_to_line).collect()
    }
}

// ── Conversion to ratatui types ─────────────────────────────────────────

fn spans_to_line(spans: Vec<StyledSpan>) -> Line<'static> {
    Line::from(
        spans
            .into_iter()
            .map(|s| {
                let mut style = Style::default();
                if let Some(fg) = s.fg {
                    style = style.fg(fg);
                }
                if s.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if s.dimmed {
                    style = style.add_modifier(Modifier::DIM);
                }
                if s.italic {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                Span::styled(s.text, style)
            })
            .collect::<Vec<_>>(),
    )
}

// ── Table rendering ─────────────────────────────────────────────────────

fn render_table(rows: &[Vec<String>]) -> Vec<Vec<StyledSpan>> {
    if rows.is_empty() {
        return Vec::new();
    }

    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths = vec![0usize; num_cols];

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < num_cols {
                col_widths[i] = col_widths[i].max(cell.trim().chars().count());
            }
        }
    }

    let mut lines = Vec::new();

    let build_border = |left: char, mid: char, right: char| -> Vec<StyledSpan> {
        let mut s = String::new();
        s.push(left);
        for (i, &width) in col_widths.iter().enumerate() {
            s.extend(std::iter::repeat_n('─', width + 2));
            if i < col_widths.len() - 1 {
                s.push(mid);
            }
        }
        s.push(right);
        vec![StyledSpan {
            text: s,
            fg: None,
            bold: false,
            dimmed: true,
            italic: false,
        }]
    };

    lines.push(build_border('┌', '┬', '┐'));

    for (row_idx, row) in rows.iter().enumerate() {
        let mut spans = Vec::new();
        for (i, width) in col_widths.iter().enumerate() {
            spans.push(StyledSpan {
                text: "│ ".to_string(),
                fg: None,
                bold: false,
                dimmed: true,
                italic: false,
            });
            let cell = row.get(i).map(|s| s.trim()).unwrap_or("");
            let padding = width.saturating_sub(cell.chars().count());
            spans.push(StyledSpan {
                text: cell.to_string(),
                fg: None,
                bold: false,
                dimmed: false,
                italic: false,
            });
            spans.push(StyledSpan {
                text: format!("{} ", " ".repeat(padding)),
                fg: None,
                bold: false,
                dimmed: true,
                italic: false,
            });
        }
        spans.push(StyledSpan {
            text: "│".to_string(),
            fg: None,
            bold: false,
            dimmed: true,
            italic: false,
        });
        lines.push(spans);

        if row_idx < rows.len() - 1 {
            lines.push(build_border('├', '┼', '┤'));
        }
    }

    lines.push(build_border('└', '┴', '┘'));
    lines
}

// ── Syntax highlighting ─────────────────────────────────────────────────

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn get_syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn get_theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

fn normalize_language(lang: &str) -> &str {
    let lang = lang.split([',', ' ']).next().unwrap_or(lang).trim();
    match lang.to_lowercase().as_str() {
        "js" => "javascript",
        "ts" => "typescript",
        "sh" | "shell" => "bash",
        "yml" => "yaml",
        "py" => "python",
        "rb" => "ruby",
        "md" => "markdown",
        "dockerfile" => "Dockerfile",
        _ => lang,
    }
}

type HighlightedLine = Vec<(String, Color, bool, bool)>;

/// Returns highlighted tokens per line: Vec<(text, fg_color, bold, italic)>
fn highlight_code(code: &str, lang: &str) -> Option<Vec<HighlightedLine>> {
    let ps = get_syntax_set();
    let ts = get_theme_set();

    let lang = normalize_language(lang);
    let syntax = ps.find_syntax_by_token(lang)?;
    let theme = ts.themes.get("base16-ocean.dark")?;

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for line in LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(line, ps).ok()?;
        let tokens: Vec<_> = ranges
            .into_iter()
            .map(|(style, text)| {
                let fg = style.foreground;
                (
                    text.to_string(),
                    Color::Rgb(fg.r, fg.g, fg.b),
                    style.font_style.contains(FontStyle::BOLD),
                    style.font_style.contains(FontStyle::ITALIC),
                )
            })
            .collect();
        lines.push(tokens);
    }

    Some(lines)
}

// ── Code wrapping ───────────────────────────────────────────────────────

fn wrap_code_lines(code: &str, max_width: usize) -> String {
    if max_width == 0 {
        return code.to_string();
    }

    let mut result = String::new();
    for line in code.lines() {
        let line_width = line.chars().count();
        if line_width <= max_width {
            result.push_str(line);
            result.push('\n');
        } else {
            let mut current_width = 0;
            for ch in line.chars() {
                let ch_width = 1; // simplified from UnicodeWidthChar
                if current_width + ch_width > max_width && current_width > 0 {
                    result.push('\n');
                    current_width = 0;
                }
                result.push(ch);
                current_width += ch_width;
            }
            result.push('\n');
        }
    }
    result
}
