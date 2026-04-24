use consult_llm_core::monitoring::{active_dir, runs_dir, sessions_dir};

use crate::models::PROVIDER_SPECS;

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

fn all_config_keys() -> Vec<&'static str> {
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
        keys.push(spec.api_key_env);
    }
    keys
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

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let home = dirs::home_dir();
    let paths = crate::config_discovery::discover(&cwd, home.as_deref());
    match crate::config_loader::LayeredEnv::load(&paths) {
        Ok(env) => {
            println!("\nResolved config:");
            for key in all_config_keys() {
                match env.lookup(key) {
                    Some((v, src)) => println!("  {key:40} = {v:20} [{src}]"),
                    None => println!("  {key:40} = (unset)              [default]"),
                }
            }
        }
        Err(e) => println!("\nConfig file error: {}: {}", e.path.display(), e.message),
    }

    Ok(())
}
