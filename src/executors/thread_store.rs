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
    let state_home = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(".local")
            .join("state")
            .to_string_lossy()
            .to_string()
    });
    PathBuf::from(state_home).join("consult-llm/threads")
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
        let mut thread = load(thread_id)?.unwrap_or_else(|| StoredThread {
            id: thread_id.to_string(),
            turns: Vec::new(),
        });
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
