use std::collections::HashSet;
use std::sync::Arc;

use crate::cli::run_spec::RunSpec;
use crate::cli::{Cli, input, output};
use crate::config;
use crate::llm::ExecutorProvider;
use crate::schema::{ConsultLlmArgs, GitDiffArgs, ModelSelector};
use crate::service::{ConsultJob, ConsultOutcome, ConsultService};

fn build_git_diff_args(cli: &Cli) -> Option<GitDiffArgs> {
    if cli.diff_files.is_empty() {
        return None;
    }
    Some(GitDiffArgs {
        repo_path: cli.diff_repo.clone(),
        files: cli.diff_files.clone(),
        base_ref: cli.diff_base.clone().unwrap_or_else(|| "HEAD".into()),
    })
}

pub fn build_args(cli: &Cli, prompt: String) -> ConsultLlmArgs {
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
        git_diff: build_git_diff_args(cli),
    }
}

fn validate_run_threads(specs: &[RunSpec]) -> Result<(), String> {
    let mut seen_threads = HashSet::new();
    for spec in specs {
        let Some(thread_id) = &spec.thread_id else {
            continue;
        };
        if crate::group_thread_store::is_group_id(thread_id) {
            return Err(format!(
                "--run: thread= expects a per-model thread id, got group thread '{thread_id}'"
            ));
        }
        if !seen_threads.insert(thread_id) {
            return Err(format!("--run: duplicate explicit thread '{thread_id}'"));
        }
    }
    Ok(())
}

pub fn run_ask(cli: Cli) -> Result<(), input::CliError> {
    if !cli.runs.is_empty() {
        if !cli.model.is_empty() || cli.thread_id.is_some() || cli.prompt_file.is_some() || cli.web
        {
            return Err(input::CliError::Generic(
                "--run cannot be combined with -m, -t, --prompt-file, or --web".into(),
            ));
        }
        if cli.runs.len() > 5 {
            return Err(input::CliError::Generic(
                "--run: max 5 runs per call".into(),
            ));
        }

        let specs: Vec<RunSpec> = cli
            .runs
            .iter()
            .map(|s| s.parse::<RunSpec>())
            .collect::<anyhow::Result<_>>()
            .map_err(|e| input::CliError::Generic(e.to_string()))?;

        let (cfg, registry) =
            config::init_config().map_err(|e| input::CliError::Config(e.to_string()))?;

        validate_run_threads(&specs).map_err(input::CliError::Generic)?;

        let mut jobs: Vec<ConsultJob> = Vec::with_capacity(specs.len());
        for spec in &specs {
            let model = registry
                .resolve_model(Some(&spec.model))
                .map_err(|e| input::CliError::Generic(e.to_string()))?;
            let prompt = std::fs::read_to_string(&spec.prompt_file).map_err(|e| {
                input::CliError::Generic(format!(
                    "--run: cannot read prompt-file {:?}: {e}",
                    spec.prompt_file
                ))
            })?;
            jobs.push(ConsultJob {
                model,
                prompt,
                thread_id: spec.thread_id.clone(),
                entry_index: None,
            });
        }

        let git_diff_args = build_git_diff_args(&cli);
        let executor_provider = Arc::new(ExecutorProvider::new(Arc::clone(&cfg)));
        let service = ConsultService::new(cfg, registry, executor_provider);

        let outcome = service
            .consult_jobs(
                jobs,
                &cli.files,
                git_diff_args.as_ref(),
                cli.task.into(),
                None,
            )
            .map_err(|e| input::CliError::Generic(format!("{e:#}")))?;

        match outcome {
            ConsultOutcome::GroupResponse { body, .. } => println!("{body}"),
            ConsultOutcome::Response {
                body,
                model,
                thread_id,
                ..
            } => {
                output::print_response(&model, thread_id.as_deref(), &body);
            }
            ConsultOutcome::WebPrompt { .. } => unreachable!(),
        }
        return Ok(());
    }

    let (cfg, registry) =
        config::init_config().map_err(|e| input::CliError::Config(e.to_string()))?;
    let prompt = input::read_prompt(cli.prompt_file.as_deref())?;
    let args = build_args(&cli, prompt);

    let executor_provider = Arc::new(ExecutorProvider::new(Arc::clone(&cfg)));
    let service = ConsultService::new(cfg, registry, executor_provider);

    let outcome = service
        .consult(args)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(model: &str, thread_id: Option<&str>) -> RunSpec {
        RunSpec {
            model: model.into(),
            thread_id: thread_id.map(str::to_string),
            prompt_file: "/tmp/prompt.md".into(),
        }
    }

    #[test]
    fn validate_run_threads_allows_duplicate_models_without_threads() {
        validate_run_threads(&[spec("openai", None), spec("openai", None)]).unwrap();
    }

    #[test]
    fn validate_run_threads_allows_duplicate_models_with_distinct_threads() {
        validate_run_threads(&[spec("openai", Some("api_1")), spec("openai", Some("api_2"))])
            .unwrap();
    }

    #[test]
    fn validate_run_threads_rejects_duplicate_explicit_thread() {
        let err =
            validate_run_threads(&[spec("openai", Some("api_1")), spec("openai", Some("api_1"))])
                .unwrap_err();
        assert!(err.contains("duplicate explicit thread"));
    }

    #[test]
    fn validate_run_threads_rejects_group_thread() {
        let err = validate_run_threads(&[spec("openai", Some("group_abc"))]).unwrap_err();
        assert!(err.contains("per-model thread id"));
    }
}
