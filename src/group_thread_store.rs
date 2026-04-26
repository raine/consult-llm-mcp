use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredGroup {
    pub id: String,
    /// model → per-model thread ID
    pub members: BTreeMap<String, String>,
    /// Ordered list of model IDs preserving original selection order.
    /// Missing in groups written by older versions; falls back to BTreeMap key order.
    #[serde(default)]
    pub member_order: Vec<String>,
}

fn groups_dir() -> PathBuf {
    consult_llm_core::paths::state_home().join("consult-llm/groups")
}

pub fn generate_group_id() -> String {
    format!("group_{}", uuid::Uuid::new_v4().simple())
}

pub fn is_group_id(s: &str) -> bool {
    s.starts_with("group_")
}

pub fn load(group_id: &str) -> anyhow::Result<Option<StoredGroup>> {
    let path = groups_dir().join(format!("{group_id}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&data)?))
}

pub fn save(group: &StoredGroup) -> anyhow::Result<()> {
    let dir = groups_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", group.id));
    let tmp = tempfile::NamedTempFile::new_in(&dir)?;
    serde_json::to_writer(&tmp, group)?;
    tmp.persist(path)?;
    Ok(())
}

/// Acquire an exclusive lock on this group's lockfile for the duration of
/// `f`. Used by callers that need to load → modify → save atomically; two
/// concurrent runs of the same group otherwise both load the old state and
/// the second persist clobbers the first.
pub fn with_lock<R, F>(group_id: &str, f: F) -> anyhow::Result<R>
where
    F: FnOnce() -> anyhow::Result<R>,
{
    let dir = groups_dir();
    fs::create_dir_all(&dir)?;
    let lock_path = dir.join(format!("{group_id}.lock"));
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    lock_file.lock_exclusive()?;
    let result = f();
    let _ = FileExt::unlock(&lock_file);
    result
}

pub fn cleanup_expired(ttl_days: u64) -> anyhow::Result<()> {
    let dir = groups_dir();
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
    fn test_is_group_id() {
        assert!(is_group_id("group_abc"));
        assert!(!is_group_id("api_abc"));
        assert!(!is_group_id("abc"));
    }

    #[test]
    fn test_generate_group_id_prefix() {
        let id = generate_group_id();
        assert!(id.starts_with("group_"));
        assert!(is_group_id(&id));
    }

    #[test]
    fn test_save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", tmp.path());
        }
        let id = generate_group_id();
        let mut members = BTreeMap::new();
        members.insert("gemini-2.5-pro".to_string(), "api_aaa".to_string());
        members.insert("gpt-5.2".to_string(), "api_bbb".to_string());
        let member_order = vec!["gemini-2.5-pro".to_string(), "gpt-5.2".to_string()];
        let group = StoredGroup {
            id: id.clone(),
            members: members.clone(),
            member_order: member_order.clone(),
        };
        save(&group).unwrap();
        let loaded = load(&id).unwrap().expect("group should load");
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.members, members);
        assert_eq!(loaded.member_order, member_order);
    }
}
