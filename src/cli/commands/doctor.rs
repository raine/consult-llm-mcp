use std::io::IsTerminal;

use consult_llm_core::monitoring::{active_dir, runs_dir, sessions_dir};

use crate::models::PROVIDER_SPECS;

fn path_has(bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        if dir.join(bin).is_file() {
            return true;
        }
    }
    false
}

// Config keys shown in "Resolved config" — excludes API key vars (covered in "API keys" section).
fn config_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = vec![
        "CONSULT_LLM_DEFAULT_MODEL",
        "CONSULT_LLM_ALLOWED_MODELS",
        "CONSULT_LLM_EXTRA_MODELS",
        "CONSULT_LLM_CODEX_REASONING_EFFORT",
        "CONSULT_LLM_SYSTEM_PROMPT_PATH",
        "CONSULT_LLM_NO_UPDATE_CHECK",
        "CONSULT_LLM_OPENCODE_PROVIDER",
    ];
    for spec in PROVIDER_SPECS {
        keys.push(spec.backend_env);
        keys.push(spec.opencode_env);
    }
    keys
}

fn use_color() -> bool {
    std::env::var("NO_COLOR").is_err() && std::io::stdout().is_terminal()
}

fn ok_mark(color: bool) -> &'static str {
    if color { "\x1b[32m✓\x1b[0m" } else { "✓" }
}

fn missing_mark(color: bool) -> &'static str {
    if color { "\x1b[31m✗\x1b[0m" } else { "✗" }
}

pub fn run(verbose: bool) -> anyhow::Result<()> {
    let color = use_color();
    let ok = ok_mark(color);
    let missing = missing_mark(color);

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
        let mark = if std::env::var(var).is_ok() {
            ok
        } else {
            missing
        };
        println!("  {var:<22} {mark}");
    }

    println!("\nCLI backends on PATH:");
    for bin in ["codex", "gemini", "cursor-agent", "opencode"] {
        let mark = if path_has(bin) { ok } else { missing };
        println!("  {bin:<16} {mark}");
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let home = dirs::home_dir();
    let paths = crate::config_discovery::discover(&cwd, home.as_deref());
    match crate::config_loader::LayeredEnv::load(&paths) {
        Ok(env) => {
            println!("\nResolved config:");
            let keys = config_keys();
            if verbose {
                for key in keys {
                    match env.lookup(key) {
                        Some((v, src)) => println!("  {key:40} = {v:<20} [{src}]"),
                        None => println!("  {key:40} = (unset)              [default]"),
                    }
                }
            } else {
                let set: Vec<_> = keys
                    .into_iter()
                    .filter_map(|key| env.lookup(key).map(|(v, src)| (key, v, src)))
                    .collect();
                if set.is_empty() {
                    println!("  (all defaults)");
                } else {
                    for (key, v, src) in set {
                        println!("  {key:40} = {v:<20} [{src}]");
                    }
                }
            }
        }
        Err(e) => eprintln!("config error: {}: {}", e.path.display(), e.message),
    }

    Ok(())
}
