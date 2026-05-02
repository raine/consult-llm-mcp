use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::types::Usage;

#[derive(Serialize, Deserialize)]
pub struct StoredThread {
    pub id: String,
    pub turns: Vec<StoredTurn>,
}

#[derive(Serialize, Deserialize)]
pub struct StoredTurn {
    pub user_prompt: String,
    pub assistant_response: String,
    pub model: String,
    pub usage: Option<Usage>,
}

fn threads_dir() -> PathBuf {
    consult_llm_core::paths::state_home().join("consult-llm/threads")
}

pub fn generate_thread_id() -> String {
    format!("api_{}", uuid::Uuid::new_v4().simple())
}

pub fn load(thread_id: &str) -> anyhow::Result<Option<StoredThread>> {
    let path = threads_dir().join(format!("{thread_id}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&path)?;
    let thread: StoredThread = serde_json::from_str(&data)?;
    Ok(Some(thread))
}

pub fn save(thread: &StoredThread) -> anyhow::Result<()> {
    let dir = threads_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", thread.id));
    let tmp = tempfile::NamedTempFile::new_in(&dir)?;
    serde_json::to_writer(&tmp, thread)?;
    tmp.persist(path)?;
    Ok(())
}

pub fn append_turn(thread_id: &str, turn: StoredTurn, is_new_thread: bool) -> anyhow::Result<()> {
    let dir = threads_dir();
    fs::create_dir_all(&dir)?;
    let lock_path = dir.join(format!("{thread_id}.lock"));
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    lock_file.lock_exclusive()?;

    let result = (|| -> anyhow::Result<()> {
        let mut thread = match load(thread_id)? {
            Some(t) => t,
            None if is_new_thread => StoredThread {
                id: thread_id.to_string(),
                turns: Vec::new(),
            },
            // Resume case: the file existed when start() loaded history but
            // is gone now (e.g. cleanup_expired raced with us). Recreating it
            // here would persist a thread containing only the new turn,
            // silently losing every prior turn the model just saw. Bail.
            None => anyhow::bail!(
                "Thread '{thread_id}' disappeared during the call (likely cleaned up); refusing to recreate with only the new turn"
            ),
        };
        thread.turns.push(turn);
        save(&thread)
    })();

    let _ = FileExt::unlock(&lock_file);

    result?;

    if is_new_thread {
        // Fire-and-forget cleanup of expired threads
        std::thread::spawn(|| {
            let _ = cleanup_expired(7);
        });
    }

    Ok(())
}

pub fn cleanup_expired(ttl_days: u64) -> anyhow::Result<()> {
    let dir = threads_dir();
    if !dir.exists() {
        return Ok(());
    }
    let cutoff = SystemTime::now() - Duration::from_secs(ttl_days * 86400);
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json")
            && let Ok(meta) = entry.metadata()
            && let Ok(modified) = meta.modified()
            && modified < cutoff
        {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_schema_json_byte_for_byte_roundtrip() {
        // Pin the on-disk write format for API thread files. NOTE: the phase
        // brief calls this "JSONL" but the actual format is a single JSON
        // object per file; this test encodes the current behaviour.
        // `unify-api-executors` should fail this test before changing the
        // wire shape.
        // XXX: phase brief mismatch — see plan note. No fix here (test-only phase).
        let fixture = r#"{"id":"api_abc","turns":[{"user_prompt":"hi","assistant_response":"hello","model":"gpt-5.4","usage":{"prompt_tokens":5,"completion_tokens":3}},{"user_prompt":"again","assistant_response":"sure","model":"gpt-5.4","usage":null}]}"#;
        let thread: StoredThread = serde_json::from_str(fixture).unwrap();
        let serialized = serde_json::to_string(&thread).unwrap();
        assert_eq!(serialized, fixture);
    }
}
