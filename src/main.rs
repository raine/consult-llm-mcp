mod cli;
mod clipboard;
mod config;
mod config_discovery;
mod config_file;
mod config_loader;
mod errors;
mod executors;
mod external_dirs;
mod file;
mod git;
mod git_worktree;
mod llm;
mod llm_query;
mod logger;
mod models;
mod prompt_builder;
mod schema;
mod service;
mod system_prompt;
mod update;

#[tokio::main]
async fn main() {
    // Set panic hook for logging
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        logger::log_to_file(&format!("PANIC: {info}"));
        default_hook(info);
    }));

    consult_llm_core::path_migrate::migrate_if_needed();

    use clap::Parser;
    let cli = cli::Cli::parse();
    if !matches!(
        cli.cmd,
        Some(cli::Command::CheckUpdate | cli::Command::Update)
    ) {
        update::check_and_notify();
    }

    let result: Result<(), cli::input::CliError> =
        match cli.cmd {
            None => cli::run::run_ask(cli).await,
            Some(cli::Command::Models) => cli::commands::models::run()
                .map_err(|e| cli::input::CliError::Generic(e.to_string())),
            Some(cli::Command::Doctor { verbose }) => cli::commands::doctor::run(verbose)
                .map_err(|e| cli::input::CliError::Generic(e.to_string())),
            Some(cli::Command::InitPrompt) => cli::commands::init_prompt::run()
                .map_err(|e| cli::input::CliError::Generic(e.to_string())),
            Some(cli::Command::InitConfig) => cli::commands::init_config::run()
                .map_err(|e| cli::input::CliError::Generic(e.to_string())),
            Some(cli::Command::Update) => cli::commands::update::run()
                .map_err(|e| cli::input::CliError::Generic(e.to_string())),
            Some(cli::Command::CheckUpdate) => update::run_background_check()
                .map_err(|e| cli::input::CliError::Generic(e.to_string())),
        };

    if let Err(e) = result {
        eprintln!("error: {}", e.message());
        std::process::exit(e.exit_code());
    }
}
