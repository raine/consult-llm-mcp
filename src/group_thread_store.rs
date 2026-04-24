use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredGroup {
    pub id: String,
    pub members: BTreeMap<String, String>,
}

fn groups_dir() -> PathBuf {
    let state_home = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(".local")
            .join("state")
            .to_string_lossy()
            .to_string()
    });
    PathBuf::from(state_home).join("consult-llm-mcp/groups")
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
        // SAFETY: test is single-threaded; we set XDG_STATE_HOME for isolation.
        unsafe {
            std::env::set_var("XDG_STATE_HOME", tmp.path());
        }
        let id = generate_group_id();
        let mut members = BTreeMap::new();
        members.insert("gemini-2.5-pro".to_string(), "api_aaa".to_string());
        members.insert("gpt-5.2".to_string(), "api_bbb".to_string());
        let group = StoredGroup {
            id: id.clone(),
            members: members.clone(),
        };
        save(&group).unwrap();
        let loaded = load(&id).unwrap().expect("group should load");
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.members, members);
    }
}
