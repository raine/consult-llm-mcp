use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

pub use crate::catalog::ModelRegistry;
pub use types::{Backend, Config, ConfigError};

mod migrate;
pub mod parse;
pub mod types;

use crate::config_discovery::discover;
use crate::config_loader::LayeredEnv;

static CONFIG: OnceLock<Config> = OnceLock::new();
static LAYERED_ENV: OnceLock<LayeredEnv> = OnceLock::new();

pub fn config() -> &'static Config {
    CONFIG.get().expect("config not initialized")
}

#[allow(dead_code)]
pub fn layered_env() -> &'static LayeredEnv {
    LAYERED_ENV.get().expect("config not initialized")
}

/// Initialize config and model registry from environment variables and config files.
/// Must be called before consult requests start.
/// Returns the ModelRegistry for explicit dependency injection.
pub fn init_config() -> Result<Arc<ModelRegistry>, ConfigError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = dirs::home_dir();
    let paths = discover(&cwd, home.as_deref());
    let layered = LayeredEnv::load(&paths).map_err(|e| ConfigError::ConfigFile {
        path: e.path,
        message: e.message,
    })?;

    let (config, registry) = parse::parse_config(layered.as_env_fn())?;

    let _ = CONFIG.set(config);
    let _ = LAYERED_ENV.set(layered);

    Ok(registry)
}
