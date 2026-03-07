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
mod llm;
mod llm_cost;
mod llm_query;
mod logger;
mod logging_reader;
mod models;
mod prompt_builder;
mod schema;
mod server;
mod system_prompt;

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
        server::init_system_prompt();
        return;
    }

    config::init_config();

    let server_version = format!("{VERSION}+{GIT_HASH}");
    logger::log_server_start(&server_version);

    // Log configuration
    let cfg = config::config();
    let mut config_map = std::collections::HashMap::new();
    config_map.insert(
        "openaiApiKey".to_string(),
        cfg.openai_api_key.clone().unwrap_or_default(),
    );
    config_map.insert(
        "geminiApiKey".to_string(),
        cfg.gemini_api_key.clone().unwrap_or_default(),
    );
    config_map.insert(
        "deepseekApiKey".to_string(),
        cfg.deepseek_api_key.clone().unwrap_or_default(),
    );
    config_map.insert(
        "geminiBackend".to_string(),
        format!("{:?}", cfg.gemini_backend).to_lowercase(),
    );
    config_map.insert(
        "openaiBackend".to_string(),
        format!("{:?}", cfg.openai_backend).to_lowercase(),
    );
    if let Some(ref dm) = cfg.default_model {
        config_map.insert("defaultModel".to_string(), dm.clone());
    }
    if let Some(ref effort) = cfg.codex_reasoning_effort {
        config_map.insert("codexReasoningEffort".to_string(), effort.clone());
    }
    config_map.insert("allowedModels".to_string(), cfg.allowed_models.join(", "));
    logger::log_configuration(&config_map);

    let executor_provider = Arc::new(llm::ExecutorProvider::new());
    let server = server::ConsultServer::new(executor_provider);

    let service = if std::env::var("MCP_DEBUG_STDIN").is_ok() {
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
    service.waiting().await.expect("MCP server error");
}
