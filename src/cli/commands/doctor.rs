use consult_llm_core::monitoring::{active_dir, runs_dir, sessions_dir};

fn path_has(bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return true;
        }
    }
    false
}

pub fn run() -> anyhow::Result<()> {
    println!("Paths:");
    println!("  sessions_dir : {}", sessions_dir().display());
    println!("  active_dir   : {}", active_dir().display());
    println!("  runs_dir     : {}", runs_dir().display());

    println!("\nAPI keys:");
    for var in [
        "OPENAI_API_KEY",
        "GEMINI_API_KEY",
        "ANTHROPIC_API_KEY",
        "DEEPSEEK_API_KEY",
        "MINIMAX_API_KEY",
    ] {
        let state = if std::env::var(var).is_ok() {
            "OK"
        } else {
            "MISSING"
        };
        println!("  {var:<22} {state}");
    }

    println!("\nCLI backends on PATH:");
    for bin in ["codex", "gemini", "cursor-agent", "opencode"] {
        let state = if path_has(bin) { "OK" } else { "MISSING" };
        println!("  {bin:<16} {state}");
    }
    Ok(())
}
