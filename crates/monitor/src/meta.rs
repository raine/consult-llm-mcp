use std::fs;

use consult_llm_core::monitoring::{RunMeta, runs_dir};

pub(crate) fn load_run_meta(run_id: &str) -> Option<RunMeta> {
    let path = runs_dir().join(format!("{run_id}.meta.json"));
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}
