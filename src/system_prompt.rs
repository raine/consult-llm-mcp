use std::fs;
use std::path::Path;

use crate::config::config;
use crate::logger::log_to_file;
use crate::schema::TaskMode;

const BASE_SYSTEM_PROMPT: &str = "You are an expert software engineering consultant. You are communicating with another AI system, not a human.\n\nCommunication style:\n- Skip pleasantries and praise\n- Be direct and specific\n- Respond in Markdown\n\nMindset:\n- Do not restrict yourself to minimal or conservative changes\n- Always strive for the best possible architecture and long-term maintainability\n- Recommend large-scale refactorings or rewrites if the current approach is suboptimal\n- When a better architecture requires significant changes, say so — don't default to minimal patches that preserve existing design flaws";

const CLI_MODE_SUFFIX: &str =
    "\n\nIMPORTANT: Do not edit files yourself, only provide recommendations and code examples";

fn mode_overlay(mode: TaskMode) -> &'static str {
    match mode {
        TaskMode::Review => {
            "Your role is to:\n- Identify bugs, inefficiencies, and architectural problems\n- Provide specific solutions with code examples\n- Point out edge cases and risks\n- Challenge foundational design decisions aggressively; suggest structural rewrites if the current architecture is poor\n- Focus on what needs improvement, regardless of diff size\n\nWhen reviewing code changes, prioritize:\n- Optimal architecture over minimal changes\n- Bugs and correctness issues\n- Performance problems\n- Security vulnerabilities\n- Code smell and anti-patterns\n- Inconsistencies with codebase conventions\n\nBe critical and thorough. Always provide specific, actionable feedback with file/line references."
        }
        TaskMode::Debug => {
            "Your role is to:\n- Analyze error messages, stack traces, and logs to identify root causes\n- Trace execution flow and state to pinpoint failures\n- Rank hypotheses by likelihood with supporting evidence\n- Propose specific, targeted fixes\n- Suggest debugging steps or instrumentation when evidence is insufficient\n\nFocus on correctness and functionality. Ignore style, naming, and non-causal code quality issues."
        }
        TaskMode::Plan => {
            "Your role is to:\n- Explore multiple approaches and evaluate trade-offs\n- Favor optimal architectural solutions over minimal-change band-aids, even if they require significant refactoring\n- Assume backward compatibility can be broken unless explicitly constrained\n- Consider scalability, maintainability, and simplicity\n- Think about edge cases and failure modes\n- Suggest incremental implementation strategies for complex rewrites\n\nChallenge the status quo. Present your recommendation as the ideal path, then optionally note minimal alternatives. Always conclude with a specific recommendation and rationale."
        }
        TaskMode::Create => {
            "Your role is to:\n- Generate clear, well-structured content\n- Match the appropriate tone and level of detail for the audience\n- Provide complete, ready-to-use output\n- Include relevant examples where helpful\n- Focus on clarity and correctness\n\nBe helpful and thorough. Produce polished, high-quality output."
        }
        TaskMode::General => "",
    }
}

/// The default system prompt written by `init-prompt`. Contains only the
/// mode-neutral base — task_mode overlays are appended at runtime.
pub const DEFAULT_SYSTEM_PROMPT: &str = BASE_SYSTEM_PROMPT;

pub fn get_system_prompt(is_cli: bool, task_mode: TaskMode) -> String {
    let cfg = config();
    let custom_path = cfg.system_prompt_path.clone().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".consult-llm")
            .join("SYSTEM_PROMPT.md")
            .to_string_lossy()
            .to_string()
    });

    let path = Path::new(&custom_path);
    let base = if path.exists() {
        match fs::read_to_string(path) {
            Ok(custom) => custom.trim().to_string(),
            Err(e) => {
                let msg = format!("Failed to read custom system prompt from {custom_path}: {e}");
                log_to_file(&format!("WARNING: {msg}"));
                eprintln!("Warning: {msg}");
                BASE_SYSTEM_PROMPT.to_string()
            }
        }
    } else {
        BASE_SYSTEM_PROMPT.to_string()
    };

    let overlay = mode_overlay(task_mode);
    let prompt = if overlay.is_empty() {
        base
    } else {
        format!("{base}\n\n{overlay}")
    };

    if is_cli {
        format!("{prompt}{CLI_MODE_SUFFIX}")
    } else {
        prompt
    }
}
