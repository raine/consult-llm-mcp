use crate::config::{self, config};
use crate::models::{ALL_PROVIDERS, Provider};

pub fn run() -> anyhow::Result<()> {
    let registry = config::init_config().map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let cfg = config();
    println!("Selectors:");
    for p in ALL_PROVIDERS {
        let spec = p.spec();
        let backend = cfg.backend_for(*p).as_str();
        let resolved = registry
            .resolve_model(Some(spec.id))
            .unwrap_or_else(|_| "-".into());
        println!("  {:<10} -> {resolved} ({backend})", spec.id);
    }
    println!("\nAllowed models:");
    for m in &cfg.allowed_models {
        let backend = Provider::from_model(m)
            .map(|p| cfg.backend_for(p).as_str())
            .unwrap_or("unknown");
        println!("  {m} ({backend})");
    }
    Ok(())
}
