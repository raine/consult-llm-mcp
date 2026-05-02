use std::sync::OnceLock;

use ratatui::style::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

/// Pre-initialize syntax and theme sets to avoid UI stutter on first code block.
pub fn init_syntax() {
    get_syntax_set();
    get_theme_set();
}

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

pub(super) type HighlightedLine = Vec<(String, Color, bool, bool)>;

/// Returns highlighted tokens per line: Vec<(text, fg_color, bold, italic)>
pub(super) fn highlight_code(code: &str, lang: &str) -> Option<Vec<HighlightedLine>> {
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
