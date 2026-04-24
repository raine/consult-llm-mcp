use clap::{Parser, Subcommand};

pub mod commands;
pub mod input;
pub mod output;
pub mod run;

#[cfg(test)]
mod tests;

#[derive(Parser, Debug)]
#[command(name = "consult-llm", version, about = "Consult an external LLM")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Command>,

    /// Model selector ("gemini", "openai", "anthropic", "deepseek", "minimax") or exact ID
    #[arg(short = 'm', long = "model")]
    pub model: Option<String>,

    /// File context (repeatable)
    #[arg(short = 'f', long = "file")]
    pub files: Vec<String>,

    /// Resume a multi-turn conversation
    #[arg(short = 't', long = "thread-id")]
    pub thread_id: Option<String>,

    /// Task persona: general (default), review, debug, plan, create
    #[arg(long = "task", default_value = "general")]
    pub task: TaskArg,

    /// Copy formatted prompt to clipboard, exit 0
    #[arg(long = "web")]
    pub web: bool,

    /// Read prompt from this file (alternative to stdin)
    #[arg(long = "prompt-file")]
    pub prompt_file: Option<String>,

    /// Include git diff for these files (repeatable)
    #[arg(long = "diff-files")]
    pub diff_files: Vec<String>,

    /// Base ref for git diff (default HEAD)
    #[arg(long = "diff-base")]
    pub diff_base: Option<String>,

    /// Repo path for git diff (default cwd)
    #[arg(long = "diff-repo")]
    pub diff_repo: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List available models and resolved selectors
    Models,
    /// Self-update the binary
    Update,
    /// Diagnose backend auth, paths, and env vars
    Doctor {
        /// Show all config keys including unset defaults
        #[arg(long = "verbose")]
        verbose: bool,
    },
    /// Scaffold ~/.consult-llm/SYSTEM_PROMPT.md
    InitPrompt,
    /// Scaffold ~/.consult-llm/config.yaml
    InitConfig,
    /// Internal: background update check (self-spawned by update.rs).
    #[command(hide = true)]
    CheckUpdate,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum TaskArg {
    General,
    Review,
    Debug,
    Plan,
    Create,
}

impl From<TaskArg> for crate::schema::TaskMode {
    fn from(t: TaskArg) -> Self {
        match t {
            TaskArg::General => Self::General,
            TaskArg::Review => Self::Review,
            TaskArg::Debug => Self::Debug,
            TaskArg::Plan => Self::Plan,
            TaskArg::Create => Self::Create,
        }
    }
}
