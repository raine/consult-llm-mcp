use std::io::{IsTerminal, Read};

pub fn read_prompt(prompt_file: Option<&str>) -> Result<String, CliError> {
    if let Some(path) = prompt_file {
        return std::fs::read_to_string(path)
            .map_err(|e| CliError::Usage(format!("--prompt-file: {e}")));
    }
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(CliError::Usage(
            "no prompt provided\n\n  Usage:  cat <<'EOF' | consult-llm [-m <model>] [-f <file>]\n          Your question here.\n          EOF\n\n  From your agent: /consult, /debate, /collab\n  Run 'consult-llm --help' for full options."
                .into(),
        ));
    }
    let mut buf = String::new();
    stdin
        .lock()
        .read_to_string(&mut buf)
        .map_err(|e| CliError::Generic(format!("stdin read failed: {e}")))?;
    Ok(buf)
}

pub enum CliError {
    Usage(String),
    Config(String),
    Generic(String),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Generic(_) => 1,
            Self::Usage(_) => 2,
            Self::Config(_) => 3,
        }
    }
    pub fn message(&self) -> &str {
        match self {
            Self::Usage(s) | Self::Config(s) | Self::Generic(s) => s,
        }
    }
}
