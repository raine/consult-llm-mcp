use std::sync::Arc;

use crate::config::parse::parse_config;
use crate::group_thread_store::{GroupEntry, StoredGroup};
use crate::llm::ExecutorProvider;

use super::group;
use super::runner::SingleResult;
use super::{ConsultJob, ConsultOutcome, ConsultService, single_outcome};

fn build_service(system_prompt_path: &str) -> ConsultService {
    let path = system_prompt_path.to_string();
    let env = move |key: &str| match key {
        "CONSULT_LLM_SYSTEM_PROMPT_PATH" => Some(path.clone()),
        "OPENAI_API_KEY" => Some("sk-test".into()),
        _ => None,
    };
    let (config, registry) = parse_config(env).expect("parse_config");
    let config = Arc::new(config);
    let executor_provider = Arc::new(ExecutorProvider::new(Arc::clone(&config)));
    ConsultService::new(config, registry, executor_provider)
}

#[test]
fn two_services_hold_distinct_configs() {
    let a = build_service("/tmp/prompt-a.md");
    let b = build_service("/tmp/prompt-b.md");
    assert_eq!(
        a.config().system_prompt_path.as_deref(),
        Some("/tmp/prompt-a.md")
    );
    assert_eq!(
        b.config().system_prompt_path.as_deref(),
        Some("/tmp/prompt-b.md")
    );
}

fn result(
    model: &str,
    thread_id: Option<&str>,
    entry_index: Option<usize>,
    failed: bool,
) -> SingleResult {
    SingleResult {
        model: model.into(),
        body: format!("body for {model}"),
        usage: None,
        thread_id: thread_id.map(str::to_string),
        entry_index,
        failed,
    }
}

fn job(model: &str) -> ConsultJob {
    ConsultJob {
        model: model.into(),
        prompt: "p".into(),
        thread_id: None,
        entry_index: None,
    }
}

#[test]
fn group_markdown_suffixes_only_duplicate_models() {
    let out = group::assemble_group_markdown(
        "group_abc",
        &[
            result("gpt-5.2", Some("api_1"), None, false),
            result("gemini-2.5-pro", Some("api_g"), None, false),
            result("gpt-5.2", Some("api_2"), None, false),
        ],
    );
    assert_eq!(
        out,
        "[thread_id:group_abc]\n\n## Model: gpt-5.2#1\n[model:gpt-5.2#1] [thread_id:api_1]\n\nbody for gpt-5.2\n\n---\n\n## Model: gemini-2.5-pro\n[model:gemini-2.5-pro] [thread_id:api_g]\n\nbody for gemini-2.5-pro\n\n---\n\n## Model: gpt-5.2#2\n[model:gpt-5.2#2] [thread_id:api_2]\n\nbody for gpt-5.2"
    );
}

#[test]
fn group_markdown_distinct_models_stays_plain() {
    let out = group::assemble_group_markdown(
        "group_abc",
        &[
            result("gpt-5.2", Some("api_1"), None, false),
            result("gemini-2.5-pro", Some("api_g"), None, false),
        ],
    );
    assert!(out.contains("## Model: gpt-5.2\n[model:gpt-5.2] [thread_id:api_1]"));
    assert!(out.contains("## Model: gemini-2.5-pro\n[model:gemini-2.5-pro] [thread_id:api_g]"));
    assert!(!out.contains("#1"));
}

#[test]
fn merge_group_entries_preserves_failed_resume_position() {
    let existing = StoredGroup {
        id: "group_abc".into(),
        entries: vec![
            GroupEntry {
                model: "gpt-5.2".into(),
                thread_id: "api_old_1".into(),
            },
            GroupEntry {
                model: "gpt-5.2".into(),
                thread_id: "api_old_2".into(),
            },
        ],
    };
    let entries = group::merge_group_entries(
        Some(&existing),
        &[
            result("gpt-5.2", None, Some(0), true),
            result("gpt-5.2", Some("api_new_2"), Some(1), false),
        ],
    )
    .unwrap();
    assert_eq!(
        entries,
        vec![
            GroupEntry {
                model: "gpt-5.2".into(),
                thread_id: "api_old_1".into(),
            },
            GroupEntry {
                model: "gpt-5.2".into(),
                thread_id: "api_new_2".into(),
            },
        ]
    );
}

#[test]
fn merge_group_entries_appends_first_turn_successes() {
    let entries = group::merge_group_entries(
        None,
        &[
            result("gpt-5.2", Some("api_1"), None, false),
            result("gpt-5.2", None, None, true),
            result("gpt-5.2", Some("api_3"), None, false),
        ],
    )
    .unwrap();
    assert_eq!(
        entries,
        vec![
            GroupEntry {
                model: "gpt-5.2".into(),
                thread_id: "api_1".into(),
            },
            GroupEntry {
                model: "gpt-5.2".into(),
                thread_id: "api_3".into(),
            },
        ]
    );
}

#[test]
fn single_outcome_propagates_worker_error() {
    let err = match single_outcome(vec![Err(anyhow::anyhow!("boom"))]) {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("boom"));
}

#[test]
fn single_outcome_returns_response_on_success() {
    let outcome = single_outcome(vec![Ok(result("gpt-5.2", Some("api_1"), None, false))]).unwrap();
    match outcome {
        ConsultOutcome::Response {
            model, thread_id, ..
        } => {
            assert_eq!(model, "gpt-5.2");
            assert_eq!(thread_id.as_deref(), Some("api_1"));
        }
        _ => panic!("expected Response"),
    }
}

#[test]
fn multi_outcome_renders_partial_failures_inline() {
    let jobs = vec![job("gpt-5.2"), job("gemini-2.5-pro")];
    let outcomes = vec![
        Ok(result("gpt-5.2", Some("api_1"), None, false)),
        Err(anyhow::anyhow!("network kaput")),
    ];
    let results = group::collect_group_results(&jobs, outcomes).unwrap();
    assert_eq!(results.len(), 2);
    assert!(!results[0].failed);
    assert!(results[1].failed);
    assert!(results[1].body.contains("network kaput"));
    assert_eq!(results[1].model, "gemini-2.5-pro");
}

#[test]
fn multi_outcome_bails_when_all_fail() {
    let jobs = vec![job("gpt-5.2"), job("gemini-2.5-pro")];
    let outcomes = vec![
        Err(anyhow::anyhow!("first failed")),
        Err(anyhow::anyhow!("second failed")),
    ];
    let err = match group::collect_group_results(&jobs, outcomes) {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(msg.contains("all model consultations failed"));
    assert!(msg.contains("first failed"));
    assert!(msg.contains("second failed"));
}
