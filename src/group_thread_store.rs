use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use fs2::FileExt;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupEntry {
    pub model: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoredGroup {
    pub id: String,
    pub entries: Vec<GroupEntry>,
}

impl<'de> Deserialize<'de> for StoredGroup {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawStoredGroup {
            id: String,
            entries: Option<Vec<GroupEntry>>,
            members: Option<BTreeMap<String, String>>,
            #[serde(default)]
            member_order: Vec<String>,
        }

        let raw = RawStoredGroup::deserialize(deserializer)?;
        if raw.entries.is_some() && (raw.members.is_some() || !raw.member_order.is_empty()) {
            return Err(serde::de::Error::custom(
                "group JSON cannot contain both entries and legacy members/member_order",
            ));
        }

        if let Some(entries) = raw.entries {
            return Ok(StoredGroup {
                id: raw.id,
                entries,
            });
        }

        let members = raw.members.unwrap_or_default();
        let order = if raw.member_order.is_empty() {
            members.keys().cloned().collect()
        } else {
            let mut seen = std::collections::HashSet::new();
            for model in &raw.member_order {
                if !seen.insert(model) {
                    return Err(serde::de::Error::custom(format!(
                        "legacy member_order contains duplicate model {model:?}"
                    )));
                }
            }
            raw.member_order
        };

        let mut entries = Vec::with_capacity(order.len());
        for model in order {
            let Some(thread_id) = members.get(&model) else {
                return Err(serde::de::Error::custom(format!(
                    "legacy member_order references missing member {model:?}"
                )));
            };
            entries.push(GroupEntry {
                model,
                thread_id: thread_id.clone(),
            });
        }

        Ok(StoredGroup {
            id: raw.id,
            entries,
        })
    }
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
        let entries = vec![
            GroupEntry {
                model: "gemini-2.5-pro".to_string(),
                thread_id: "api_aaa".to_string(),
            },
            GroupEntry {
                model: "gpt-5.2".to_string(),
                thread_id: "api_bbb".to_string(),
            },
        ];
        let group = StoredGroup {
            id: id.clone(),
            entries: entries.clone(),
        };
        save(&group).unwrap();
        let loaded = load(&id).unwrap().expect("group should load");
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.entries, entries);
    }

    #[test]
    fn current_schema_json_byte_for_byte_roundtrip() {
        // Pin the on-disk write format. If a refactor changes the
        // serialization shape (field order, naming, structure), this test
        // fails before any persisted group file silently shifts.
        let fixture = r#"{"id":"group_abc","entries":[{"model":"gpt-5.4","thread_id":"api_x"},{"model":"gemini-2.5-pro","thread_id":"api_y"}]}"#;
        let group: StoredGroup = serde_json::from_str(fixture).unwrap();
        let serialized = serde_json::to_string(&group).unwrap();
        assert_eq!(serialized, fixture);
    }

    #[test]
    fn legacy_group_loads_from_member_order() {
        let json = r#"{
            "id":"group_abc",
            "members":{"gpt-5.2":"api_x","gemini-2.5-pro":"api_y"},
            "member_order":["gemini-2.5-pro","gpt-5.2"]
        }"#;
        let group: StoredGroup = serde_json::from_str(json).unwrap();
        assert_eq!(
            group.entries,
            vec![
                GroupEntry {
                    model: "gemini-2.5-pro".into(),
                    thread_id: "api_y".into(),
                },
                GroupEntry {
                    model: "gpt-5.2".into(),
                    thread_id: "api_x".into(),
                },
            ]
        );
    }

    #[test]
    fn legacy_group_loads_from_member_key_order() {
        let json = r#"{
            "id":"group_abc",
            "members":{"gpt-5.2":"api_x","gemini-2.5-pro":"api_y"}
        }"#;
        let group: StoredGroup = serde_json::from_str(json).unwrap();
        assert_eq!(
            group.entries,
            vec![
                GroupEntry {
                    model: "gemini-2.5-pro".into(),
                    thread_id: "api_y".into(),
                },
                GroupEntry {
                    model: "gpt-5.2".into(),
                    thread_id: "api_x".into(),
                },
            ]
        );
    }

    #[test]
    fn legacy_group_errors_on_missing_order_member() {
        let json = r#"{
            "id":"group_abc",
            "members":{"gpt-5.2":"api_x"},
            "member_order":["gpt-5.2","gemini-2.5-pro"]
        }"#;
        let err = serde_json::from_str::<StoredGroup>(json).unwrap_err();
        assert!(err.to_string().contains("missing member"));
    }

    #[test]
    fn legacy_group_errors_on_duplicate_order_member() {
        let json = r#"{
            "id":"group_abc",
            "members":{"gpt-5.2":"api_x"},
            "member_order":["gpt-5.2","gpt-5.2"]
        }"#;
        let err = serde_json::from_str::<StoredGroup>(json).unwrap_err();
        assert!(err.to_string().contains("duplicate model"));
    }

    #[test]
    fn group_errors_on_mixed_schema() {
        let json = r#"{
            "id":"group_abc",
            "entries":[{"model":"gpt-5.2","thread_id":"api_x"}],
            "members":{"gpt-5.2":"api_x"}
        }"#;
        let err = serde_json::from_str::<StoredGroup>(json).unwrap_err();
        assert!(err.to_string().contains("both entries and legacy"));
    }
}
