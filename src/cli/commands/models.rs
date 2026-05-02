use crate::config;
use crate::models::{PROVIDERS, Provider};

pub fn run() -> anyhow::Result<()> {
    let (cfg, registry) = config::init_config().map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("Selectors:");
    for spec in PROVIDERS {
        let Ok(resolved) = registry.resolve_model(Some(spec.id)) else {
            continue;
        };
        let backend = cfg.backend_for(spec.provider).as_str();
        println!("  {:<10} -> {resolved} ({backend})", spec.id);
    }
    println!("\nAllowed models:");
    for m in &cfg.allowed_models {
        let backend = Provider::from_model(m)
            .map(|p| cfg.backend_for(p).as_str())
            .unwrap_or("unknown");
        println!("  {m} ({backend})");
    }
    println!("\nDefault models (ordered; duplicates are intentional):");
    if cfg.default_models.is_empty() {
        println!("  (none)");
    } else {
        for m in &cfg.default_models {
            println!("  {m}");
        }
    }
    println!("\nDefault -m args:");
    if cfg.default_models.is_empty() {
        println!("  (none)");
    } else {
        println!(
            "  {}",
            cfg.default_models
                .iter()
                .map(|m| format!("-m {m}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    Ok(())
}
