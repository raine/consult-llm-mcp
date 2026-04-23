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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    OpenAI,
    Gemini,
    DeepSeek,
    MiniMax,
    Anthropic,
}

/// HTTP wire-format family used when calling the provider's native API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiProtocol {
    /// OpenAI-compatible `/chat/completions` — used by OpenAI, Gemini (OpenAI-compat
    /// endpoint), DeepSeek, MiniMax.
    OpenAiCompat,
    /// Anthropic `/v1/messages` — `x-api-key` auth, top-level `system`, content-block
    /// responses, distinct usage shape.
    AnthropicMessages,
}

/// All known providers in deterministic order.
/// Adding a provider means: add a variant to `Provider`, add it here, and add a `ProviderSpec`.
pub const ALL_PROVIDERS: &[Provider] = &[
    Provider::Gemini,
    Provider::DeepSeek,
    Provider::OpenAI,
    Provider::MiniMax,
    Provider::Anthropic,
];

/// Static metadata for a provider — the single place to define provider-specific constants.
pub struct ProviderSpec {
    pub provider: Provider,
    /// Lowercase identifier used for logging and config key generation (e.g. "openai").
    pub id: &'static str,
    /// Prefixes that identify this provider's models (e.g. &["gpt-", "o3-"]).
    pub model_prefixes: &'static [&'static str],
    /// API base URL override (None = default OpenAI-compatible URL).
    pub api_base_url: Option<&'static str>,
    /// API wire format — picks which executor to instantiate in `Backend::Api` mode.
    pub api_protocol: ApiProtocol,
    /// Built-in model IDs shipped with this provider.
    pub builtin_models: &'static [&'static str],
    /// Env var for the API key (e.g. "OPENAI_API_KEY").
    pub api_key_env: &'static str,
    /// Prefixed backend env var (e.g. "CONSULT_LLM_OPENAI_BACKEND").
    pub backend_env: &'static str,
    /// Legacy unprefixed backend env var, if any (e.g. "OPENAI_BACKEND").
    pub legacy_backend_env: Option<&'static str>,
    /// Legacy mode env var, if any (e.g. "OPENAI_MODE").
    pub legacy_mode_env: Option<&'static str>,
    /// CLI backend value used when migrating legacy mode env (e.g. "codex-cli").
    pub cli_backend_value: Option<&'static str>,
    /// Allowed backend string values for validation.
    pub allowed_backends: &'static [&'static str],
    /// Per-provider opencode provider env var (e.g. "CONSULT_LLM_OPENCODE_OPENAI_PROVIDER").
    pub opencode_env: &'static str,
    /// Default opencode provider prefix (e.g. "openai").
    pub default_opencode_provider: &'static str,
}

/// Order matters: `all_builtin_models()` flattens in this order, which determines the
/// fallback model when gpt-5.2 is not available (first enabled model wins).
pub static PROVIDER_SPECS: &[ProviderSpec] = &[
    ProviderSpec {
        provider: Provider::Gemini,
        id: "gemini",
        model_prefixes: &["gemini-"],
        api_base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai/"),
        api_protocol: ApiProtocol::OpenAiCompat,
        builtin_models: &[
            "gemini-2.5-pro",
            "gemini-3-pro-preview",
            "gemini-3.1-pro-preview",
        ],
        api_key_env: "GEMINI_API_KEY",
        backend_env: "CONSULT_LLM_GEMINI_BACKEND",
        legacy_backend_env: Some("GEMINI_BACKEND"),
        legacy_mode_env: Some("GEMINI_MODE"),
        cli_backend_value: Some("gemini-cli"),
        allowed_backends: &["api", "gemini-cli", "cursor-cli", "opencode"],
        opencode_env: "CONSULT_LLM_OPENCODE_GEMINI_PROVIDER",
        default_opencode_provider: "google",
    },
    ProviderSpec {
        provider: Provider::DeepSeek,
        id: "deepseek",
        model_prefixes: &["deepseek-"],
        api_base_url: Some("https://api.deepseek.com"),
        api_protocol: ApiProtocol::OpenAiCompat,
        builtin_models: &["deepseek-reasoner"],
        api_key_env: "DEEPSEEK_API_KEY",
        backend_env: "CONSULT_LLM_DEEPSEEK_BACKEND",
        legacy_backend_env: None,
        legacy_mode_env: None,
        cli_backend_value: None,
        allowed_backends: &["api", "opencode"],
        opencode_env: "CONSULT_LLM_OPENCODE_DEEPSEEK_PROVIDER",
        default_opencode_provider: "deepseek",
    },
    ProviderSpec {
        provider: Provider::OpenAI,
        id: "openai",
        model_prefixes: &["gpt-"],
        api_base_url: None,
        api_protocol: ApiProtocol::OpenAiCompat,
        builtin_models: &[
            "gpt-5.2",
            "gpt-5.4",
            "gpt-5.5",
            "gpt-5.3-codex",
            "gpt-5.2-codex",
        ],
        api_key_env: "OPENAI_API_KEY",
        backend_env: "CONSULT_LLM_OPENAI_BACKEND",
        legacy_backend_env: Some("OPENAI_BACKEND"),
        legacy_mode_env: Some("OPENAI_MODE"),
        cli_backend_value: Some("codex-cli"),
        allowed_backends: &["api", "codex-cli", "cursor-cli", "opencode"],
        opencode_env: "CONSULT_LLM_OPENCODE_OPENAI_PROVIDER",
        default_opencode_provider: "openai",
    },
    ProviderSpec {
        provider: Provider::MiniMax,
        id: "minimax",
        model_prefixes: &["MiniMax-"],
        api_base_url: Some("https://api.minimax.io/v1"),
        api_protocol: ApiProtocol::OpenAiCompat,
        builtin_models: &["MiniMax-M2.7"],
        api_key_env: "MINIMAX_API_KEY",
        backend_env: "CONSULT_LLM_MINIMAX_BACKEND",
        legacy_backend_env: None,
        legacy_mode_env: None,
        cli_backend_value: None,
        allowed_backends: &["api", "opencode"],
        opencode_env: "CONSULT_LLM_OPENCODE_MINIMAX_PROVIDER",
        default_opencode_provider: "minimax",
    },
    ProviderSpec {
        provider: Provider::Anthropic,
        id: "anthropic",
        model_prefixes: &["claude-"],
        api_base_url: Some("https://api.anthropic.com"),
        api_protocol: ApiProtocol::AnthropicMessages,
        builtin_models: &["claude-opus-4-7"],
        api_key_env: "ANTHROPIC_API_KEY",
        backend_env: "CONSULT_LLM_ANTHROPIC_BACKEND",
        legacy_backend_env: None,
        legacy_mode_env: None,
        cli_backend_value: None,
        allowed_backends: &["api"],
        opencode_env: "CONSULT_LLM_OPENCODE_ANTHROPIC_PROVIDER",
        default_opencode_provider: "anthropic",
    },
];

impl Provider {
    /// Look up the static spec for this provider.
    pub fn spec(&self) -> &'static ProviderSpec {
        PROVIDER_SPECS
            .iter()
            .find(|s| s.provider == *self)
            .expect("every Provider variant must have a ProviderSpec entry")
    }

    /// Determine the provider for a model ID based on its prefix.
    pub fn from_model(model: &str) -> Option<Self> {
        PROVIDER_SPECS
            .iter()
            .find(|spec| spec.model_prefixes.iter().any(|p| model.starts_with(p)))
            .map(|spec| spec.provider)
    }

    /// API base URL for this provider (when using API backend).
    pub fn api_base_url(&self) -> Option<&'static str> {
        self.spec().api_base_url
    }

    /// API wire format for this provider (when using API backend).
    pub fn api_protocol(&self) -> ApiProtocol {
        self.spec().api_protocol
    }
}

/// Collect all builtin model IDs from the provider registry, in deterministic order.
pub fn all_builtin_models() -> Vec<&'static str> {
    PROVIDER_SPECS
        .iter()
        .flat_map(|spec| spec.builtin_models.iter().copied())
        .collect()
}

/// Abstract selectors mapped to ordered lists of concrete model IDs (best first).
/// When a user passes e.g. "gemini", the server picks the first available model from the list.
/// Kept separate from the provider registry — selectors are a routing concept that may
/// eventually span multiple providers (e.g. "reasoning" -> models from different providers).
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
        &[
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.3-codex",
            "gpt-5.2",
            "gpt-5.2-codex",
        ],
    ),
    ("deepseek", &["deepseek-reasoner"]),
    ("minimax", &["MiniMax-M2.7"]),
    ("anthropic", &["claude-opus-4-7"]),
];
