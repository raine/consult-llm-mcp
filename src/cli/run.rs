use std::sync::Arc;

use crate::cli::{Cli, input, output};
use crate::config;
use crate::llm::ExecutorProvider;
use crate::schema::{ConsultLlmArgs, GitDiffArgs, ModelSelector};
use crate::service::{ConsultOutcome, ConsultService};

pub fn build_args(cli: &Cli, prompt: String) -> ConsultLlmArgs {
    let git_diff = if !cli.diff_files.is_empty() {
        Some(GitDiffArgs {
            repo_path: cli.diff_repo.clone(),
            files: cli.diff_files.clone(),
            base_ref: cli.diff_base.clone().unwrap_or_else(|| "HEAD".into()),
        })
    } else {
        None
    };
    let model = match cli.model.len() {
        0 => None,
        1 => Some(ModelSelector::One(cli.model[0].clone())),
        _ => Some(ModelSelector::Many(cli.model.clone())),
    };
    ConsultLlmArgs {
        prompt,
        files: if cli.files.is_empty() {
            None
        } else {
            Some(cli.files.clone())
        },
        model,
        task_mode: cli.task.into(),
        web_mode: cli.web,
        thread_id: cli.thread_id.clone(),
        git_diff,
    }
}

pub async fn run_ask(cli: Cli) -> Result<(), input::CliError> {
    let registry = config::init_config().map_err(|e| input::CliError::Config(e.to_string()))?;
    let prompt = input::read_prompt(cli.prompt_file.as_deref())?;
    let args = build_args(&cli, prompt);

    let executor_provider = Arc::new(ExecutorProvider::new());
    let service = ConsultService::new(registry, executor_provider);

    let outcome = service
        .consult(args)
        .await
        .map_err(|e| input::CliError::Generic(format!("{e:#}")))?;

    match outcome {
        ConsultOutcome::Response {
            body,
            model,
            thread_id,
            ..
        } => {
            output::print_response(&model, thread_id.as_deref(), &body);
        }
        ConsultOutcome::GroupResponse { body, .. } => {
            println!("{body}");
        }
        ConsultOutcome::WebPrompt {
            clipboard_text,
            file_count,
        } => {
            crate::clipboard::copy_to_clipboard(&clipboard_text)
                .map_err(|e| input::CliError::Generic(format!("{e:#}")))?;
            output::print_web_confirmation(file_count);
        }
    }
    Ok(())
}
