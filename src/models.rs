use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, JsonSchema)]
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

/// Known LLM provider families, determined by model ID prefix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Provider {
    OpenAI,
    Gemini,
    DeepSeek,
}

impl Provider {
    /// Determine the provider for a model ID based on its prefix.
    pub fn from_model(model: &str) -> Option<Self> {
        if model.starts_with("gpt-") {
            Some(Provider::OpenAI)
        } else if model.starts_with("gemini-") {
            Some(Provider::Gemini)
        } else if model.starts_with("deepseek-") {
            Some(Provider::DeepSeek)
        } else {
            None
        }
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

/// Abstract selectors mapped to ordered lists of concrete model IDs (best first).
/// When a user passes e.g. "gemini", the server picks the first available model from the list.
pub const SELECTOR_PRIORITIES: &[(&str, &[&str])] = &[
    (
        "gemini",
        &[
            "gemini-3.1-pro-preview",
            "gemini-3-pro-preview",
            "gemini-2.5-pro",
        ],
    ),
    (
        "openai",
        &["gpt-5.4", "gpt-5.3-codex", "gpt-5.2", "gpt-5.2-codex"],
    ),
    ("deepseek", &["deepseek-reasoner"]),
];
