use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskMode {
    Review,
    Debug,
    Plan,
    Create,
    General,
}

impl Default for TaskMode {
    fn default() -> Self {
        Self::General
    }
}

pub const ALL_MODELS: &[&str] = &[
    "gemini-2.5-pro",
    "gemini-3-pro-preview",
    "gemini-3.1-pro-preview",
    "deepseek-reasoner",
    "gpt-5.2",
    "gpt-5.4",
    "gpt-5.3-codex",
    "gpt-5.2-codex",
];
