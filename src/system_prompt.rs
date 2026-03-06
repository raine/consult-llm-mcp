use std::fs;
use std::path::Path;

use crate::config::config;
use crate::logger::log_to_file;
use crate::schema::TaskMode;

const BASE_SYSTEM_PROMPT: &str = "You are an expert software engineering consultant. You are communicating with another AI system, not a human.\n\nCommunication style:\n- Skip pleasantries and praise\n- Be direct and specific\n- Respond in Markdown";

const CLI_MODE_SUFFIX: &str =
    "\n\nIMPORTANT: Do not edit files yourself, only provide recommendations and code examples";

fn mode_overlay(mode: TaskMode) -> &'static str {
    match mode {
        TaskMode::Review => {
            "Your role is to:\n- Identify bugs, inefficiencies, and architectural problems\n- Provide specific solutions with code examples\n- Point out edge cases and risks\n- Challenge design decisions when suboptimal\n- Focus on what needs improvement\n\nWhen reviewing code changes, prioritize:\n- Bugs and correctness issues\n- Performance problems\n- Security vulnerabilities\n- Code smell and anti-patterns\n- Inconsistencies with codebase conventions\n\nBe critical and thorough. Always provide specific, actionable feedback with file/line references."
        }
        TaskMode::Debug => {
            "Your role is to:\n- Analyze error messages, stack traces, and logs to identify root causes\n- Trace execution flow and state to pinpoint failures\n- Rank hypotheses by likelihood with supporting evidence\n- Propose specific, targeted fixes\n- Suggest debugging steps or instrumentation when evidence is insufficient\n\nFocus on correctness and functionality. Ignore style, naming, and non-causal code quality issues."
        }
        TaskMode::Plan => {
            "Your role is to:\n- Explore multiple approaches and evaluate trade-offs\n- Consider scalability, maintainability, and simplicity\n- Think about edge cases and failure modes\n- Suggest incremental implementation strategies\n\nBe constructive and thorough. Present options clearly with pros and cons, and always conclude with a specific recommendation and rationale."
        }
        TaskMode::Create => {
            "Your role is to:\n- Generate clear, well-structured content\n- Match the appropriate tone and level of detail for the audience\n- Provide complete, ready-to-use output\n- Include relevant examples where helpful\n- Focus on clarity and correctness\n\nBe helpful and thorough. Produce polished, high-quality output."
        }
        TaskMode::General => "",
    }
}

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are an expert software engineering consultant. You are communicating with another AI system, not a human.\n\nCommunication style:\n- Skip pleasantries and praise\n- Be direct and specific\n- Respond in Markdown\n\nYour role is to:\n- Identify bugs, inefficiencies, and architectural problems\n- Provide specific solutions with code examples\n- Point out edge cases and risks\n- Challenge design decisions when suboptimal\n- Focus on what needs improvement\n\nWhen reviewing code changes, prioritize:\n- Bugs and correctness issues\n- Performance problems\n- Security vulnerabilities\n- Code smell and anti-patterns\n- Inconsistencies with codebase conventions\n\nBe critical and thorough. Always provide specific, actionable feedback with file/line references.";

pub fn get_system_prompt(is_cli: bool, task_mode: TaskMode) -> String {
    let cfg = config();
    let custom_path = cfg.system_prompt_path.clone().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".consult-llm-mcp")
            .join("SYSTEM_PROMPT.md")
            .to_string_lossy()
            .to_string()
    });

    let path = Path::new(&custom_path);
    if path.exists() {
        match fs::read_to_string(path) {
            Ok(custom) => {
                let trimmed = custom.trim().to_string();
                return if is_cli {
                    format!("{trimmed}{CLI_MODE_SUFFIX}")
                } else {
                    trimmed
                };
            }
            Err(e) => {
                let msg = format!("Failed to read custom system prompt from {custom_path}: {e}");
                log_to_file(&format!("WARNING: {msg}"));
                eprintln!("Warning: {msg}");
            }
        }
    }

    let overlay = mode_overlay(task_mode);
    let prompt = if overlay.is_empty() {
        BASE_SYSTEM_PROMPT.to_string()
    } else {
        format!("{BASE_SYSTEM_PROMPT}\n\n{overlay}")
    };

    if is_cli {
        format!("{prompt}{CLI_MODE_SUFFIX}")
    } else {
        prompt
    }
}
