use std::sync::Arc;

use crate::cli::{Cli, input, output};
use crate::config;
use crate::llm::ExecutorProvider;
use crate::schema::{ConsultLlmArgs, GitDiffArgs, ModelSelector};
use crate::service::{ConsultJob, ConsultOutcome, ConsultService};

#[derive(Debug)]
pub struct RunSpec {
    pub model: String,
    pub thread_id: Option<String>,
    pub prompt_file: String,
}

impl std::str::FromStr for RunSpec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let mut model: Option<String> = None;
        let mut thread_id: Option<String> = None;
        let mut prompt_file: Option<String> = None;

        let mut remaining = s;
        while !remaining.is_empty() {
            let (key, rest) = remaining
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("expected key=value in --run {:?}", s))?;
            // Value ends at the next comma or end of string.
            // This means prompt-file paths containing commas are not supported.
            let (value, next) = match rest.find(',') {
                Some(i) => (&rest[..i], &rest[i + 1..]),
                None => (rest, ""),
            };
            match key.trim() {
                "model" => {
                    anyhow::ensure!(model.is_none(), "duplicate 'model' key in --run {:?}", s);
                    anyhow::ensure!(!value.is_empty(), "'model' value is empty in --run {:?}", s);
                    model = Some(value.to_string());
                }
                "thread" => {
                    anyhow::ensure!(
                        thread_id.is_none(),
                        "duplicate 'thread' key in --run {:?}",
                        s
                    );
                    anyhow::ensure!(
                        !value.trim().is_empty(),
                        "'thread' value is empty in --run {:?}",
                        s
                    );
                    thread_id = Some(value.to_string());
                }
                "prompt-file" => {
                    anyhow::ensure!(
                        prompt_file.is_none(),
                        "duplicate 'prompt-file' key in --run {:?}",
                        s
                    );
                    anyhow::ensure!(
                        !value.is_empty(),
                        "'prompt-file' value is empty in --run {:?}",
                        s
                    );
                    prompt_file = Some(value.to_string());
                }
                other => anyhow::bail!("unknown key {:?} in --run {:?}", other, s),
            }
            remaining = next;
        }

        Ok(RunSpec {
            model: model.ok_or_else(|| anyhow::anyhow!("--run missing 'model=' in {:?}", s))?,
            prompt_file: prompt_file
                .ok_or_else(|| anyhow::anyhow!("--run missing 'prompt-file=' in {:?}", s))?,
            thread_id,
        })
    }
}

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

pub async fn run_ask(cli: Cli) -> Result<(), input::CliError> {
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

        let registry = config::init_config().map_err(|e| input::CliError::Config(e.to_string()))?;

        let mut jobs: Vec<ConsultJob> = Vec::with_capacity(specs.len());
        let mut seen_models = std::collections::HashSet::new();
        for spec in &specs {
            let model = registry
                .resolve_model(Some(&spec.model))
                .map_err(|e| input::CliError::Generic(e.to_string()))?;
            if !seen_models.insert(model.clone()) {
                return Err(input::CliError::Generic(format!(
                    "--run: duplicate resolved model '{model}'"
                )));
            }
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
            });
        }

        let git_diff_args = build_git_diff_args(&cli);
        let executor_provider = Arc::new(ExecutorProvider::new());
        let service = ConsultService::new(registry, executor_provider);

        let outcome = service
            .consult_jobs(
                jobs,
                &cli.files,
                git_diff_args.as_ref(),
                cli.task.into(),
                None,
            )
            .await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_spec_full() {
        let s: RunSpec = "model=gemini,thread=abc123,prompt-file=/tmp/g.md"
            .parse()
            .unwrap();
        assert_eq!(s.model, "gemini");
        assert_eq!(s.thread_id.as_deref(), Some("abc123"));
        assert_eq!(s.prompt_file, "/tmp/g.md");
    }

    #[test]
    fn run_spec_no_thread() {
        let s: RunSpec = "model=openai,prompt-file=/tmp/p.md".parse().unwrap();
        assert_eq!(s.model, "openai");
        assert!(s.thread_id.is_none());
        assert_eq!(s.prompt_file, "/tmp/p.md");
    }

    #[test]
    fn run_spec_duplicate_model_key() {
        let err = "model=gemini,model=openai,prompt-file=/tmp/p.md"
            .parse::<RunSpec>()
            .unwrap_err();
        assert!(err.to_string().contains("duplicate 'model'"));
    }

    #[test]
    fn run_spec_missing_model() {
        let err = "prompt-file=/tmp/p.md".parse::<RunSpec>().unwrap_err();
        assert!(err.to_string().contains("missing 'model='"));
    }

    #[test]
    fn run_spec_missing_prompt_file() {
        let err = "model=gemini".parse::<RunSpec>().unwrap_err();
        assert!(err.to_string().contains("missing 'prompt-file='"));
    }

    #[test]
    fn run_spec_unknown_key() {
        let err = "model=gemini,foo=bar,prompt-file=/tmp/p.md"
            .parse::<RunSpec>()
            .unwrap_err();
        assert!(err.to_string().contains("unknown key"));
    }

    #[test]
    fn run_spec_empty_model_value() {
        let err = "model=,prompt-file=/tmp/p.md"
            .parse::<RunSpec>()
            .unwrap_err();
        assert!(err.to_string().contains("'model' value is empty"));
    }

    #[test]
    fn run_spec_empty_prompt_file_value() {
        let err = "model=gemini,prompt-file=".parse::<RunSpec>().unwrap_err();
        assert!(err.to_string().contains("'prompt-file' value is empty"));
    }

    #[test]
    fn run_spec_empty_thread_value() {
        let err = "model=gemini,thread=,prompt-file=/tmp/p.md"
            .parse::<RunSpec>()
            .unwrap_err();
        assert!(err.to_string().contains("'thread' value is empty"));
    }
}
