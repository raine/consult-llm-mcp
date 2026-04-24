use std::sync::Arc;

use rmcp::ServiceExt;

mod clipboard;
mod config;
mod errors;
mod executors;
mod external_dirs;
mod file;
mod git;
mod git_worktree;
mod group_thread_store;
mod llm;
mod llm_query;
mod logger;
mod logging_reader;
mod models;
mod prompt_builder;
mod schema;
mod server;
mod service;
mod system_prompt;
mod update;

use consult_llm_core::monitoring;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("GIT_HASH");

#[tokio::main]
async fn main() {
    // Set panic hook for logging
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        logger::log_to_file(&format!("PANIC: {info}"));
        default_hook(info);
    }));

    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("{VERSION}+{GIT_HASH}");
        std::process::exit(0);
    }

    if args.iter().any(|a| a == "init-prompt") {
        if let Err(e) = server::init_system_prompt() {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }

    if args.get(1).is_some_and(|a| a == "update") {
        if let Err(e) = update::run() {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        return;
    }

    if args.get(1).is_some_and(|a| a == "_check-update") {
        if let Err(e) = update::run_background_check() {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        return;
    }

    let registry = match config::init_config() {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            logger::log_to_file(&format!("FATAL ERROR:\n{msg}"));
            eprintln!("\u{274c} {msg}");
            std::process::exit(1);
        }
    };

    monitoring::init();
    update::check_and_notify();
    let project = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
    monitoring::emit(monitoring::MonitorEvent::ServerStarted {
        version: format!("{VERSION}+{GIT_HASH}"),
        pid: std::process::id(),
        project,
    });

    let server_version = format!("{VERSION}+{GIT_HASH}");
    logger::log_server_start(&server_version);

    // Log configuration
    let cfg = config::config();
    let mut config_map = std::collections::HashMap::new();
    for (provider, provider_cfg) in cfg.iter_providers() {
        let name = provider.spec().id;
        config_map.insert(
            format!("{name}ApiKey"),
            provider_cfg.api_key.clone().unwrap_or_default(),
        );
        config_map.insert(
            format!("{name}Backend"),
            provider_cfg.backend.as_str().to_string(),
        );
    }
    if let Some(ref dm) = cfg.default_model {
        config_map.insert("defaultModel".to_string(), dm.clone());
    }
    config_map.insert(
        "codexReasoningEffort".to_string(),
        cfg.codex_reasoning_effort.clone(),
    );
    config_map.insert("allowedModels".to_string(), cfg.allowed_models.join(", "));
    logger::log_configuration(&config_map);

    // Log available models with their backends
    let model_lines: Vec<String> = cfg
        .allowed_models
        .iter()
        .map(|m| {
            let backend = models::Provider::from_model(m)
                .map(|p| cfg.backend_for(p).as_str())
                .unwrap_or("unknown");
            format!("  {m} ({backend})")
        })
        .collect();
    logger::log_to_file(&format!(
        "AVAILABLE MODELS:\n{}\n{}",
        model_lines.join("\n"),
        "=".repeat(80)
    ));

    let executor_provider = Arc::new(llm::ExecutorProvider::new());
    let consult_service = Arc::new(service::ConsultService::new(registry, executor_provider));
    let server = server::ConsultServer::new(consult_service);

    let mcp_service = if std::env::var("MCP_DEBUG_STDIN").is_ok() {
        logger::log_to_file("MCP_DEBUG_STDIN enabled");
        let stdin = logging_reader::LoggingReader::new(tokio::io::stdin());
        let stdout = tokio::io::stdout();
        server
            .serve((stdin, stdout))
            .await
            .expect("Failed to start MCP server")
    } else {
        let transport = rmcp::transport::io::stdio();
        server
            .serve(transport)
            .await
            .expect("Failed to start MCP server")
    };

    tokio::select! {
        res = mcp_service.waiting() => {
            if let Err(e) = res {
                logger::log_to_file(&format!("MCP server error: {e}"));
            }
        }
        _ = tokio::signal::ctrl_c() => {
            logger::log_to_file("Received SIGINT, shutting down");
        }
    }

    monitoring::emit(monitoring::MonitorEvent::ServerStopped);
    monitoring::cleanup();
}
