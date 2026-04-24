use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;
use serde_json::{Map, Value};

pub use crate::models::TaskMode;

/// Accepts either a single model identifier or an array of identifiers.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ModelSelector {
    /// Array of model identifiers.
    Many(Vec<String>),
    /// Single model identifier.
    One(String),
}

impl ModelSelector {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v,
        }
    }
}

fn model_selector_schema(generator: &mut SchemaGenerator) -> Schema {
    let string_schema = generator.subschema_for::<String>();
    json_schema!({
        "oneOf": [
            string_schema,
            {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "maxItems": 5
            }
        ]
    })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GitDiffArgs {
    /// Path to git repository (defaults to current working directory)
    pub repo_path: Option<String>,
    /// Specific files to include in diff
    #[schemars(length(min = 1))]
    pub files: Vec<String>,
    /// Git reference to compare against (e.g., "HEAD", "main", commit hash)
    #[serde(default = "default_base_ref")]
    #[schemars(default = "default_base_ref")]
    pub base_ref: String,
}

fn default_base_ref() -> String {
    "HEAD".to_string()
}

fn default_task_mode_str() -> &'static str {
    "general"
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ConsultLlmArgs {
    /// Your question or request for the consultant LLM. Ask neutral, open-ended questions without suggesting specific solutions to avoid biasing the analysis.
    pub prompt: String,
    /// Array of file paths to include as context. All files are added as context with file paths and code blocks.
    pub files: Option<Vec<String>>,
    /// Optional model selector. Pass a single string (e.g. "gemini") for one model, or an array (e.g. ["gemini", "openai"]) to consult multiple models in parallel in one call. Usually omit this to use the server's configured default. Use 'gemini', 'openai', 'anthropic', 'deepseek', or 'minimax' to pick a provider family. Exact model IDs are also accepted as an advanced override. Ignored when `web_mode` is `true`. Max 5 models per call.
    #[schemars(schema_with = "model_selector_schema")]
    pub model: Option<ModelSelector>,
    /// Controls the system prompt persona. Choose based on the task: "review": critical code reviewer for finding bugs, security issues, and quality problems. "debug": focused troubleshooter for root cause analysis from errors, logs, and stack traces — ignores style issues. "plan": constructive architect for exploring trade-offs and designing solutions — always includes a final recommendation. "create": generative writer for producing documentation, content, or designs. "general" (default): neutral prompt that defers to your instructions in the prompt field.
    #[serde(default)]
    #[schemars(default = "default_task_mode_str")]
    pub task_mode: TaskMode,
    /// If true, copy the formatted prompt to the clipboard instead of querying an LLM. When true, the `model` parameter is ignored. Use this to paste the prompt into browser-based LLM services. IMPORTANT: Only use this when the user specifically requests it. When true, wait for the user to provide the external LLM's response before proceeding with any implementation.
    #[serde(default)]
    pub web_mode: bool,
    /// Thread/session ID for resuming a conversation. Works with all backends. CLI backends maintain native sessions; API backends replay conversation history from disk. Returned in the response prefix as [thread_id:xxx]. When multiple models are consulted in one call, the response returns a group thread id (`group_<uuid>`) on the first line; passing that back as `thread_id` resumes all the same models together.
    pub thread_id: Option<String>,
    /// Generate git diff output to include as context. Shows uncommitted changes by default.
    pub git_diff: Option<GitDiffArgs>,
}

/// Build the MCP tool input schema from the ConsultLlmArgs struct.
/// Inlines all `$ref`/`$defs` so the schema is self-contained for MCP clients.
pub fn consult_llm_schema() -> Map<String, Value> {
    let schema = schemars::schema_for!(ConsultLlmArgs);
    let mut value = serde_json::to_value(schema).expect("schema serialization");

    // Extract $defs before inlining, then remove them from the root
    let defs = value
        .get("$defs")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    inline_refs(&mut value, &defs);

    match value {
        Value::Object(mut map) => {
            map.remove("$schema");
            map.remove("$defs");
            map.remove("definitions");
            map.remove("title");
            map
        }
        _ => unreachable!("schema is always an object"),
    }
}

/// Recursively replace `{"$ref": "#/$defs/Foo"}` with the inlined definition.
/// When a node has both `$ref` and sibling keys (e.g. `description`), the
/// definition is merged in and the `$ref` is removed.
fn inline_refs(value: &mut Value, defs: &Value) {
    match value {
        Value::Object(map) => {
            // If this object has a $ref, resolve it
            if let Some(Value::String(ref_path)) = map.get("$ref").cloned()
                && let Some(resolved) = resolve_ref(&ref_path, defs)
            {
                // Remove the $ref key
                map.remove("$ref");
                // Merge resolved definition into this object
                // (preserves sibling keys like `description`)
                if let Value::Object(resolved_map) = resolved {
                    for (k, v) in resolved_map {
                        map.entry(k).or_insert(v);
                    }
                }
            }
            // Recurse into all values
            for v in map.values_mut() {
                inline_refs(v, defs);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                inline_refs(v, defs);
            }
        }
        _ => {}
    }
}

fn resolve_ref(ref_path: &str, defs: &Value) -> Option<Value> {
    // Handle "#/$defs/Name" format
    let name = ref_path.strip_prefix("#/$defs/")?;
    defs.get(name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_required_prompt() {
        let schema = consult_llm_schema();
        let required = schema
            .get("required")
            .and_then(|v| v.as_array())
            .expect("required field");
        assert!(required.iter().any(|v| v.as_str() == Some("prompt")));
    }

    #[test]
    fn schema_has_all_struct_fields() {
        let schema = consult_llm_schema();
        let props = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("properties");
        let expected = [
            "prompt",
            "files",
            "model",
            "task_mode",
            "web_mode",
            "thread_id",
            "git_diff",
        ];
        for field in expected {
            assert!(
                props.contains_key(field),
                "missing schema property: {field}"
            );
        }
    }

    #[test]
    fn schema_no_refs_remain() {
        let schema = consult_llm_schema();
        let json = serde_json::to_string(&schema).unwrap();
        assert!(
            !json.contains("$ref"),
            "schema should not contain $ref after inlining"
        );
        assert!(
            !json.contains("$defs"),
            "schema should not contain $defs after inlining"
        );
    }

    #[test]
    fn schema_task_mode_inlined() {
        let schema = consult_llm_schema();
        let task_mode = schema
            .get("properties")
            .and_then(|v| v.get("task_mode"))
            .and_then(|v| v.as_object())
            .expect("task_mode property");
        // Should have enum values inlined
        assert!(task_mode.contains_key("enum"), "task_mode should have enum");
        // Should still have description
        assert!(
            task_mode.contains_key("description"),
            "task_mode should have description"
        );
        // Should advertise the default value
        assert_eq!(
            task_mode.get("default").and_then(|v| v.as_str()),
            Some("general"),
            "task_mode should have default 'general'"
        );
    }

    #[test]
    fn schema_git_diff_files_min_items() {
        let schema = consult_llm_schema();
        let json = serde_json::to_string(&schema).unwrap();
        // The git_diff.files array should require at least 1 item
        assert!(
            json.contains("\"minItems\":1") || json.contains("\"minItems\": 1"),
            "git_diff.files should have minItems: 1"
        );
    }

    #[test]
    fn schema_model_is_oneof() {
        let schema = consult_llm_schema();
        let model = schema
            .get("properties")
            .and_then(|v| v.get("model"))
            .and_then(|v| v.as_object())
            .expect("model property");
        let variants = model
            .get("oneOf")
            .and_then(|v| v.as_array())
            .expect("model.oneOf");
        assert_eq!(variants.len(), 2);
    }

    #[test]
    fn schema_model_max_items_5() {
        let schema = consult_llm_schema();
        let model = schema
            .get("properties")
            .and_then(|v| v.get("model"))
            .and_then(|v| v.as_object())
            .expect("model property");
        let variants = model
            .get("oneOf")
            .and_then(|v| v.as_array())
            .expect("oneOf");
        let array_variant = variants
            .iter()
            .find(|v| v.get("type").and_then(|t| t.as_str()) == Some("array"))
            .expect("array variant");
        assert_eq!(
            array_variant.get("maxItems").and_then(|v| v.as_u64()),
            Some(5)
        );
        assert_eq!(
            array_variant.get("minItems").and_then(|v| v.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn schema_git_diff_inlined() {
        let schema = consult_llm_schema();
        let git_diff = schema
            .get("properties")
            .and_then(|v| v.get("git_diff"))
            .and_then(|v| v.as_object())
            .expect("git_diff property");

        // Should have description
        assert!(
            git_diff.contains_key("description"),
            "git_diff should have description"
        );

        // Find the object variant (may be in anyOf for Option)
        let has_properties = git_diff.contains_key("properties")
            || git_diff
                .get("anyOf")
                .and_then(|a| a.as_array())
                .is_some_and(|arr| arr.iter().any(|item| item.get("properties").is_some()));
        assert!(has_properties, "git_diff should have inlined properties");
    }
}
