use std::path::PathBuf;
use std::sync::Arc;

pub use crate::catalog::ModelRegistry;
pub use types::{Backend, Config, ConfigError};

pub mod discovery;
pub mod file;
pub mod loader;
mod migrate;
pub mod parse;
pub mod types;

use discovery::discover;
use loader::LayeredEnv;

/// Load config and model registry from environment variables and config files.
/// Returned values are passed explicitly to ConsultService and ExecutorProvider.
pub fn init_config() -> Result<(Arc<Config>, Arc<ModelRegistry>), ConfigError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = dirs::home_dir();
    let paths = discover(&cwd, home.as_deref());
    let layered = LayeredEnv::load(&paths).map_err(|e| ConfigError::ConfigFile {
        path: e.path,
        message: e.message,
    })?;

    let (config, registry) = parse::parse_config(layered.as_env_fn())?;
    Ok((Arc::new(config), registry))
}
