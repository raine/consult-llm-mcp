use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Normalized event from any CLI's JSON stream.
/// Each CLI adapter maps its raw JSON lines to these.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParsedStreamEvent {
    SessionStarted {
        id: String,
    },
    Thinking,
    AssistantText {
        text: String,
    },
    ToolStarted {
        call_id: String,
        label: String,
    },
    ToolFinished {
        call_id: String,
        success: bool,
    },
    Prompt {
        text: String,
    },
    #[serde(rename = "usage")]
    Usage {
        prompt_tokens: u64,
        completion_tokens: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_all_variants() {
        let events = vec![
            ParsedStreamEvent::SessionStarted {
                id: "sess-1".to_string(),
            },
            ParsedStreamEvent::Thinking,
            ParsedStreamEvent::AssistantText {
                text: "hello world".to_string(),
            },
            ParsedStreamEvent::ToolStarted {
                call_id: "c1".to_string(),
                label: "read src/main.rs".to_string(),
            },
            ParsedStreamEvent::ToolFinished {
                call_id: "c1".to_string(),
                success: true,
            },
            ParsedStreamEvent::ToolFinished {
                call_id: "c2".to_string(),
                success: false,
            },
            ParsedStreamEvent::Prompt {
                text: "What is Rust?".to_string(),
            },
            ParsedStreamEvent::Usage {
                prompt_tokens: 1000,
                completion_tokens: 200,
            },
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let parsed: ParsedStreamEvent = serde_json::from_str(&json).unwrap();
            // Re-serialize to verify round-trip
            let json2 = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn deserialize_from_json_lines() {
        let lines = vec![
            r#"{"type":"session_started","id":"abc"}"#,
            r#"{"type":"thinking"}"#,
            r#"{"type":"assistant_text","text":"hi"}"#,
            r#"{"type":"tool_started","call_id":"c1","label":"read foo.rs"}"#,
            r#"{"type":"tool_finished","call_id":"c1","success":true}"#,
            r#"{"type":"prompt","text":"What is Rust?"}"#,
            r#"{"type":"usage","prompt_tokens":500,"completion_tokens":100}"#,
        ];
        for line in lines {
            let result = serde_json::from_str::<ParsedStreamEvent>(line);
            assert!(
                result.is_ok(),
                "Failed to parse '{line}': {:?}",
                result.err()
            );
        }
    }
}
