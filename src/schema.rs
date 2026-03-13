use serde::Deserialize;
use serde_json::{Map, Value, json};

pub use crate::models::TaskMode;

#[derive(Debug, Clone, Deserialize)]
pub struct GitDiffArgs {
    pub repo_path: Option<String>,
    pub files: Vec<String>,
    #[serde(default = "default_base_ref")]
    pub base_ref: String,
}

fn default_base_ref() -> String {
    "HEAD".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConsultLlmArgs {
    pub prompt: String,
    pub files: Option<Vec<String>>,
    pub model: Option<String>,
    #[serde(default)]
    pub task_mode: TaskMode,
    #[serde(default)]
    pub web_mode: bool,
    pub thread_id: Option<String>,
    pub git_diff: Option<GitDiffArgs>,
}

/// Build the MCP tool input schema.
pub fn consult_llm_schema() -> Map<String, Value> {
    serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "prompt": {
                "type": "string",
                "description": "Your question or request for the consultant LLM. Ask neutral, open-ended questions without suggesting specific solutions to avoid biasing the analysis."
            },
            "files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Array of file paths to include as context. All files are added as context with file paths and code blocks."
            },
            "model": {
                "type": "string",
                "examples": ["gemini", "openai", "deepseek"],
                "description": "Optional model selector. Usually omit this to use the server's configured default. Use 'gemini', 'openai', or 'deepseek' to pick a provider family. Exact model IDs are also accepted as an advanced override. Ignored when `web_mode` is `true`."
            },
            "task_mode": {
                "type": "string",
                "enum": ["review", "debug", "plan", "create", "general"],
                "default": "general",
                "description": "Controls the system prompt persona. Choose based on the task: \"review\": critical code reviewer for finding bugs, security issues, and quality problems. \"debug\": focused troubleshooter for root cause analysis from errors, logs, and stack traces — ignores style issues. \"plan\": constructive architect for exploring trade-offs and designing solutions — always includes a final recommendation. \"create\": generative writer for producing documentation, content, or designs. \"general\" (default): neutral prompt that defers to your instructions in the prompt field."
            },
            "web_mode": {
                "type": "boolean",
                "default": false,
                "description": "If true, copy the formatted prompt to the clipboard instead of querying an LLM. When true, the `model` parameter is ignored. Use this to paste the prompt into browser-based LLM services. IMPORTANT: Only use this when the user specifically requests it. When true, wait for the user to provide the external LLM's response before proceeding with any implementation."
            },
            "thread_id": {
                "type": "string",
                "description": "Thread/session ID for resuming a conversation. Works with CLI backends (Codex, Gemini CLI, Cursor CLI). Returned in the response prefix as [thread_id:xxx]."
            },
            "git_diff": {
                "type": "object",
                "properties": {
                    "repo_path": {
                        "type": "string",
                        "description": "Path to git repository (defaults to current working directory)"
                    },
                    "files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "description": "Specific files to include in diff"
                    },
                    "base_ref": {
                        "type": "string",
                        "default": "HEAD",
                        "description": "Git reference to compare against (e.g., \"HEAD\", \"main\", commit hash)"
                    }
                },
                "required": ["files"],
                "description": "Generate git diff output to include as context. Shows uncommitted changes by default."
            }
        },
        "required": ["prompt"]
    }))
    .expect("valid schema JSON")
}
