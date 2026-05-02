use std::collections::HashMap;

use crate::group_thread_store::{GroupEntry, StoredGroup};

use super::ConsultJob;
use super::runner::SingleResult;

pub(super) fn merge_group_entries(
    existing_group: Option<&StoredGroup>,
    results: &[SingleResult],
) -> anyhow::Result<Vec<GroupEntry>> {
    let mut entries = existing_group
        .map(|g| g.entries.clone())
        .unwrap_or_default();

    for r in results {
        if r.failed {
            continue;
        }
        let Some(tid) = &r.thread_id else {
            continue;
        };
        let entry = GroupEntry {
            model: r.model.clone(),
            thread_id: tid.clone(),
        };
        if let Some(idx) = r.entry_index {
            let Some(slot) = entries.get_mut(idx) else {
                anyhow::bail!("matched group entry index {idx} is out of bounds");
            };
            *slot = entry;
        } else {
            entries.push(entry);
        }
    }

    Ok(entries)
}

pub(super) fn assemble_group_markdown(group_id: &str, results: &[SingleResult]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for r in results {
        *counts.entry(&r.model).or_default() += 1;
    }

    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut out = format!("[thread_id:{group_id}]");
    for (idx, r) in results.iter().enumerate() {
        if idx == 0 {
            out.push_str("\n\n");
        } else {
            out.push_str("\n\n---\n\n");
        }
        let label = if counts[&r.model.as_str()] > 1 {
            let n = seen.entry(&r.model).or_default();
            *n += 1;
            format!("{}#{}", r.model, *n)
        } else {
            r.model.clone()
        };
        out.push_str(&format!("## Model: {label}\n[model:{label}]"));
        if let Some(tid) = &r.thread_id {
            out.push_str(&format!(" [thread_id:{tid}]"));
        }
        out.push_str("\n\n");
        out.push_str(r.body.trim_end());
    }
    out
}

pub(super) fn collect_group_results(
    jobs: &[ConsultJob],
    outcomes: Vec<anyhow::Result<SingleResult>>,
) -> anyhow::Result<Vec<SingleResult>> {
    let mut results: Vec<SingleResult> = Vec::with_capacity(outcomes.len());
    for (job, outcome) in jobs.iter().zip(outcomes) {
        match outcome {
            Ok(r) => results.push(r),
            Err(e) => results.push(SingleResult {
                model: job.model.clone(),
                body: format!("**Error:** {e:#}"),
                usage: None,
                thread_id: None,
                entry_index: job.entry_index,
                failed: true,
            }),
        }
    }
    if results.iter().all(|r| r.failed) {
        let details = results
            .iter()
            .map(|r| format!("{}: {}", r.model, r.body))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::bail!("all model consultations failed\n{details}");
    }
    Ok(results)
}
