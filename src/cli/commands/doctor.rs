use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use consult_llm_core::monitoring::{active_dir, runs_dir, sessions_dir};

use crate::models::PROVIDER_SPECS;

// ---- terminal helpers -------------------------------------------------------

fn use_color() -> bool {
    std::env::var("NO_COLOR").is_err() && std::io::stdout().is_terminal()
}

fn ok_mark(color: bool) -> &'static str {
    if color { "\x1b[32m✓\x1b[0m" } else { "✓" }
}

fn err_mark(color: bool) -> &'static str {
    if color { "\x1b[31m✗\x1b[0m" } else { "✗" }
}

fn dim(color: bool, s: &str) -> String {
    if color {
        format!("\x1b[2m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

// ---- path helpers -----------------------------------------------------------

fn shorten(path: &Path, home: Option<&Path>) -> String {
    if let Some(h) = home
        && let Ok(rel) = path.strip_prefix(h)
    {
        return format!("~/{}", rel.display());
    }
    path.display().to_string()
}

fn shorten_str(s: &str, home: Option<&Path>) -> String {
    if let Some(h) = home {
        let h_str = h.to_string_lossy();
        if let Some(rest) = s.strip_prefix(h_str.as_ref()) {
            return format!("~{rest}");
        }
    }
    s.to_string()
}

fn check_writable(path: &Path) -> bool {
    let test = path.join(".consult-llm-write-test");
    match std::fs::File::create(&test) {
        Ok(_) => {
            let _ = std::fs::remove_file(&test);
            true
        }
        Err(_) => false,
    }
}

// ---- backend helpers --------------------------------------------------------

fn api_key_set(var: &str) -> bool {
    std::env::var(var).map(|v| !v.is_empty()).unwrap_or(false)
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn backend_binary(backend: &str) -> Option<&'static str> {
    match backend {
        "codex-cli" => Some("codex"),
        "gemini-cli" => Some("gemini"),
        "cursor-cli" => Some("cursor-agent"),
        "opencode" => Some("opencode"),
        _ => None,
    }
}

// ---- config key helpers -----------------------------------------------------

fn config_keys() -> Vec<&'static str> {
    let mut keys = vec![
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

fn semantic_name(env_key: &str) -> String {
    match env_key {
        "CONSULT_LLM_DEFAULT_MODEL" => "default_model".into(),
        "CONSULT_LLM_ALLOWED_MODELS" => "allowed_models".into(),
        "CONSULT_LLM_EXTRA_MODELS" => "extra_models".into(),
        "CONSULT_LLM_CODEX_REASONING_EFFORT" => "codex.reasoning_effort".into(),
        "CONSULT_LLM_SYSTEM_PROMPT_PATH" => "system_prompt_path".into(),
        "CONSULT_LLM_NO_UPDATE_CHECK" => "no_update_check".into(),
        "CONSULT_LLM_OPENCODE_PROVIDER" => "opencode.provider".into(),
        k => {
            for spec in PROVIDER_SPECS {
                if k == spec.backend_env {
                    return format!("{}.backend", spec.id);
                }
                if k == spec.opencode_env {
                    return format!("opencode.{}.provider", spec.id);
                }
            }
            k.into()
        }
    }
}

// ---- data collection --------------------------------------------------------

enum ProvStatus {
    Ok,
    Err,
    Skip, // not in allowed_models scope
}

struct ProvRow {
    id: &'static str,
    model: String,
    backend: String,
    status: ProvStatus,
    detail: String,
}

struct PathRow {
    name: &'static str,
    path: PathBuf,
    exists: bool,
    writable: bool,
}

// ---- main -------------------------------------------------------------------

pub fn run(verbose: bool) -> anyhow::Result<()> {
    let color = use_color();
    let ok = ok_mark(color);
    let err = err_mark(color);

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = dirs::home_dir();
    let disc = crate::config_discovery::discover(&cwd, home.as_deref());
    let env = match crate::config_loader::LayeredEnv::load(&disc) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("config error: {}: {}", e.path.display(), e.message);
            return Ok(());
        }
    };

    let allowed: Option<Vec<String>> = env.lookup("CONSULT_LLM_ALLOWED_MODELS").map(|(v, _)| {
        v.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });

    // ---- Collect provider rows
    let mut prov_rows: Vec<ProvRow> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut has_warn = false;

    for spec in PROVIDER_SPECS {
        let backend = env
            .lookup(spec.backend_env)
            .map(|(v, _)| v)
            .unwrap_or_else(|| "api".to_string());

        let in_scope = match &allowed {
            None => true,
            Some(models) => models
                .iter()
                .any(|m| spec.model_prefixes.iter().any(|p| m.starts_with(p))),
        };

        if !in_scope {
            prov_rows.push(ProvRow {
                id: spec.id,
                model: String::new(),
                backend: String::new(),
                status: ProvStatus::Skip,
                detail: "not in allowed_models".into(),
            });
            continue;
        }

        let model = match &allowed {
            Some(models) => models
                .iter()
                .find(|m| spec.model_prefixes.iter().any(|p| m.starts_with(p)))
                .cloned()
                .unwrap_or_else(|| {
                    spec.builtin_models
                        .first()
                        .copied()
                        .unwrap_or("")
                        .to_string()
                }),
            None => spec
                .builtin_models
                .first()
                .copied()
                .unwrap_or("")
                .to_string(),
        };

        let (dep_ok, detail) = if backend == "api" {
            if api_key_set(spec.api_key_env) {
                (true, format!("{} set", spec.api_key_env))
            } else {
                (false, format!("{} unset", spec.api_key_env))
            }
        } else if let Some(bin) = backend_binary(&backend) {
            match which(bin) {
                Some(path) => (true, format!("{bin} ({})", path.display())),
                None => (false, format!("{bin} not found on PATH")),
            }
        } else {
            (false, format!("unknown backend '{backend}'"))
        };

        if !dep_ok {
            has_warn = true;
            warnings.push(format!("{}: {}", spec.id, detail));
        }

        prov_rows.push(ProvRow {
            id: spec.id,
            model,
            backend,
            status: if dep_ok {
                ProvStatus::Ok
            } else {
                ProvStatus::Err
            },
            detail,
        });
    }

    // ---- Collect path rows
    let path_entries: &[(&'static str, PathBuf)] = &[
        ("sessions", sessions_dir()),
        ("active", active_dir()),
        ("runs", runs_dir()),
    ];

    let mut path_rows: Vec<PathRow> = Vec::new();
    for (name, path) in path_entries {
        let exists = path.exists();
        let writable = exists && check_writable(path);
        if !exists || !writable {
            has_warn = true;
            warnings.push(format!(
                "{name}: {}",
                if !exists { "not found" } else { "not writable" }
            ));
        }
        path_rows.push(PathRow {
            name,
            path: path.clone(),
            exists,
            writable,
        });
    }

    // ---- Header
    let version = env!("CARGO_PKG_VERSION");
    let status = if has_warn {
        if color { "\x1b[33mWARN\x1b[0m" } else { "WARN" }
    } else if color {
        "\x1b[32mOK\x1b[0m"
    } else {
        "OK"
    };
    println!("consult-llm v{version} doctor: {status}");

    // ---- Providers
    println!("\nProviders:");
    let id_w = prov_rows.iter().map(|r| r.id.len()).max().unwrap_or(8);
    let model_w = prov_rows.iter().map(|r| r.model.len()).max().unwrap_or(20);
    let backend_w = prov_rows.iter().map(|r| r.backend.len()).max().unwrap_or(7);

    for row in &prov_rows {
        match row.status {
            ProvStatus::Skip => {
                let dash = dim(color, "-");
                let detail = dim(color, &row.detail);
                println!(
                    "  {:<id_w$}  {:<model_w$}  {:<backend_w$}  {dash}   {detail}",
                    row.id, "", "",
                );
            }
            ProvStatus::Ok => {
                println!(
                    "  {:<id_w$}  {:<model_w$}  {:<backend_w$}  {ok}   {}",
                    row.id, row.model, row.backend, row.detail,
                );
            }
            ProvStatus::Err => {
                println!(
                    "  {:<id_w$}  {:<model_w$}  {:<backend_w$}  {err}   {}",
                    row.id, row.model, row.backend, row.detail,
                );
            }
        }
    }

    // ---- Config
    println!("\nConfig:");
    let keys = config_keys();
    if verbose {
        let key_w = keys.iter().map(|k| k.len()).max().unwrap_or(30);
        for key in &keys {
            match env.lookup(key) {
                Some((v, src)) => {
                    let src_str = dim(color, &format!("[{src}]"));
                    println!("  {key:<key_w$}  {v:<24}  {src_str}");
                }
                None => {
                    let def = dim(color, "[default]");
                    println!("  {key:<key_w$}  (unset)                   {def}");
                }
            }
        }
    } else {
        let set: Vec<_> = keys
            .iter()
            .filter_map(|&k| env.lookup(k).map(|(v, src)| (k, v, src)))
            .collect();
        if set.is_empty() {
            println!("  {}", dim(color, "(all defaults)"));
        } else {
            let name_w = set
                .iter()
                .map(|(k, _, _)| semantic_name(k).len())
                .max()
                .unwrap_or(20);
            for (key, v, src) in &set {
                let name = semantic_name(key);
                let src_str = dim(
                    color,
                    &format!("[{}]", shorten_str(&src.to_string(), home.as_deref())),
                );
                println!("  {name:<name_w$}  {v:<24}  {src_str}");
            }
        }
    }

    // ---- Config files
    println!("\nConfig files:");
    let user_config = home
        .as_deref()
        .map(|h| h.join(".consult-llm").join("config.yaml"));
    struct FileEntry {
        label: &'static str,
        display: String,
        loaded: bool,
    }
    let file_entries = [
        FileEntry {
            label: "user",
            display: user_config
                .as_deref()
                .map(|p| shorten(p, home.as_deref()))
                .unwrap_or_else(|| "~/.consult-llm/config.yaml".into()),
            loaded: disc.user.is_some(),
        },
        FileEntry {
            label: "project",
            display: disc
                .project
                .as_deref()
                .map(|p| shorten(p, home.as_deref()))
                .unwrap_or_else(|| ".consult-llm.yaml".into()),
            loaded: disc.project.is_some(),
        },
        FileEntry {
            label: "project-local",
            display: disc
                .project_local
                .as_deref()
                .map(|p| shorten(p, home.as_deref()))
                .unwrap_or_else(|| ".consult-llm.local.yaml".into()),
            loaded: disc.project_local.is_some(),
        },
    ];
    let label_w = file_entries
        .iter()
        .map(|e| e.label.len())
        .max()
        .unwrap_or(12);
    let path_w = file_entries
        .iter()
        .map(|e| e.display.len())
        .max()
        .unwrap_or(40);
    for entry in &file_entries {
        if entry.loaded {
            println!(
                "  {:<label_w$}  {:<path_w$}  {ok}",
                entry.label, entry.display
            );
        } else {
            let status = dim(color, "not found");
            println!(
                "  {:<label_w$}  {:<path_w$}  {status}",
                entry.label, entry.display
            );
        }
    }

    // ---- State
    println!("\nState:");
    let name_w = path_rows.iter().map(|r| r.name.len()).max().unwrap_or(8);
    let spath_w = path_rows
        .iter()
        .map(|r| shorten(&r.path, home.as_deref()).len())
        .max()
        .unwrap_or(40);
    for row in &path_rows {
        let path_str = shorten(&row.path, home.as_deref());
        let status = if !row.exists {
            format!("{err} not found")
        } else if !row.writable {
            format!("{err} not writable")
        } else {
            format!("{ok} writable")
        };
        println!("  {:<name_w$}  {:<spath_w$}  {status}", row.name, path_str);
    }

    // ---- Warnings
    if !warnings.is_empty() {
        println!("\nWarnings:");
        for w in &warnings {
            println!("  {err} {w}");
        }
    }

    Ok(())
}
