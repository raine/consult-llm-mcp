use std::collections::{HashMap, HashSet};

use crate::catalog::ModelRegistry;
use crate::group_thread_store::{StoredGroup, is_group_id};
use crate::schema::ModelSelector;

#[derive(Debug)]
pub struct ResumePlan {
    pub threads: HashMap<String, Option<String>>,
    pub group_id: Option<String>,
    pub unwrap_single: bool,
}

pub fn normalize_models(
    registry: &ModelRegistry,
    selector: Option<ModelSelector>,
    group_fallback: Option<&StoredGroup>,
) -> anyhow::Result<Vec<String>> {
    let raw = match selector {
        Some(s) => s.into_vec(),
        None => match group_fallback {
            Some(g) => {
                if !g.member_order.is_empty() {
                    g.member_order.clone()
                } else {
                    g.members.keys().cloned().collect()
                }
            }
            None => vec![registry.resolve_model(None)?],
        },
    };
    if raw.is_empty() {
        anyhow::bail!("`model` array must contain at least one entry");
    }
    let mut resolved: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for item in raw {
        let concrete = registry.resolve_model(Some(&item))?;
        if seen.insert(concrete.clone()) {
            resolved.push(concrete);
        }
    }
    if resolved.len() > 5 {
        anyhow::bail!("max 5 models per call (got {})", resolved.len());
    }
    Ok(resolved)
}

pub fn plan_resume(
    thread_id: Option<&str>,
    resolved_models: &[String],
    loaded_group: Option<StoredGroup>,
) -> anyhow::Result<ResumePlan> {
    match (thread_id, loaded_group) {
        (None, _) => Ok(ResumePlan {
            threads: resolved_models.iter().map(|m| (m.clone(), None)).collect(),
            group_id: None,
            unwrap_single: resolved_models.len() == 1,
        }),
        (Some(tid), Some(group)) if is_group_id(tid) => {
            let mut threads = HashMap::new();
            for m in resolved_models {
                let Some(member_tid) = group.members.get(m) else {
                    anyhow::bail!(
                        "group {tid} has no member for model {m}; group members: {:?}",
                        group.members.keys().collect::<Vec<_>>()
                    );
                };
                threads.insert(m.clone(), Some(member_tid.clone()));
            }
            Ok(ResumePlan {
                threads,
                group_id: Some(tid.to_string()),
                // Always use the group output path when resuming a group thread,
                // even if only one model is selected, so the group ID is preserved.
                unwrap_single: false,
            })
        }
        (Some(tid), _) if is_group_id(tid) => {
            anyhow::bail!("group thread not found: {tid}")
        }
        (Some(tid), _) => {
            if resolved_models.len() > 1 {
                anyhow::bail!(
                    "per-model thread_id cannot be used with multiple models; pass a group thread id or omit thread_id"
                );
            }
            let mut threads = HashMap::new();
            threads.insert(resolved_models[0].clone(), Some(tid.to_string()));
            Ok(ResumePlan {
                threads,
                group_id: None,
                unwrap_single: true,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::catalog::ModelRegistry;

    fn reg() -> ModelRegistry {
        ModelRegistry {
            allowed_models: vec![
                "gpt-5.2".into(),
                "gpt-5.4".into(),
                "gemini-2.5-pro".into(),
                "gemini-3.1-pro-preview".into(),
            ],
            fallback_model: "gpt-5.2".into(),
            default_model: None,
        }
    }

    #[test]
    fn normalize_none_with_no_group_uses_default() {
        let out = normalize_models(&reg(), None, None).unwrap();
        assert_eq!(out, vec!["gpt-5.2"]);
    }

    #[test]
    fn normalize_single_string() {
        let out =
            normalize_models(&reg(), Some(ModelSelector::One("gemini".into())), None).unwrap();
        assert_eq!(out, vec!["gemini-3.1-pro-preview"]);
    }

    #[test]
    fn normalize_dedupes_after_resolution() {
        let out = normalize_models(
            &reg(),
            Some(ModelSelector::Many(vec![
                "gemini".into(),
                "gemini-3.1-pro-preview".into(),
                "openai".into(),
            ])),
            None,
        )
        .unwrap();
        assert_eq!(out, vec!["gemini-3.1-pro-preview", "gpt-5.4"]);
    }

    #[test]
    fn normalize_caps_at_5() {
        // Need 6 distinct resolvable models — build a registry with 6 allowed models.
        let big_reg = ModelRegistry {
            allowed_models: vec![
                "m1".into(),
                "m2".into(),
                "m3".into(),
                "m4".into(),
                "m5".into(),
                "m6".into(),
            ],
            fallback_model: "m1".into(),
            default_model: None,
        };
        let out = normalize_models(
            &big_reg,
            Some(ModelSelector::Many(vec![
                "m1".into(),
                "m2".into(),
                "m3".into(),
                "m4".into(),
                "m5".into(),
                "m6".into(),
            ])),
            None,
        );
        assert!(out.is_err());
        assert!(out.unwrap_err().to_string().contains("max 5"));
    }

    #[test]
    fn normalize_empty_array_errors() {
        let out = normalize_models(&reg(), Some(ModelSelector::Many(vec![])), None);
        assert!(out.is_err());
    }

    #[test]
    fn normalize_falls_back_to_group_members() {
        let mut members = BTreeMap::new();
        members.insert("gpt-5.2".to_string(), "api_x".to_string());
        members.insert("gemini-2.5-pro".to_string(), "api_y".to_string());
        let group = StoredGroup {
            id: "group_abc".into(),
            members,
            member_order: vec![],
        };
        let out = normalize_models(&reg(), None, Some(&group)).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out.contains(&"gpt-5.2".to_string()));
        assert!(out.contains(&"gemini-2.5-pro".to_string()));
    }

    #[test]
    fn plan_resume_no_tid_single_unwraps() {
        let plan = plan_resume(None, &["gpt-5.2".into()], None).unwrap();
        assert!(plan.unwrap_single);
        assert!(plan.group_id.is_none());
    }

    #[test]
    fn plan_resume_no_tid_multi_no_unwrap() {
        let plan = plan_resume(None, &["gpt-5.2".into(), "gemini-2.5-pro".into()], None).unwrap();
        assert!(!plan.unwrap_single);
        assert!(plan.group_id.is_none());
    }

    #[test]
    fn plan_resume_per_model_tid_with_multi_errors() {
        let err = plan_resume(
            Some("api_xxx"),
            &["gpt-5.2".into(), "gemini-2.5-pro".into()],
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("per-model thread_id"));
    }

    #[test]
    fn plan_resume_per_model_tid_single() {
        let plan = plan_resume(Some("api_xxx"), &["gpt-5.2".into()], None).unwrap();
        assert!(plan.unwrap_single);
        assert_eq!(
            plan.threads.get("gpt-5.2").unwrap(),
            &Some("api_xxx".to_string())
        );
    }

    #[test]
    fn plan_resume_group_tid_member_subset() {
        let mut members = BTreeMap::new();
        members.insert("gpt-5.2".to_string(), "api_x".to_string());
        members.insert("gemini-2.5-pro".to_string(), "api_y".to_string());
        let group = StoredGroup {
            id: "group_abc".into(),
            members,
            member_order: vec![],
        };
        let plan = plan_resume(
            Some("group_abc"),
            &["gpt-5.2".into(), "gemini-2.5-pro".into()],
            Some(group),
        )
        .unwrap();
        assert_eq!(plan.group_id.as_deref(), Some("group_abc"));
        assert_eq!(
            plan.threads.get("gpt-5.2").unwrap(),
            &Some("api_x".to_string())
        );
        assert_eq!(
            plan.threads.get("gemini-2.5-pro").unwrap(),
            &Some("api_y".to_string())
        );
    }

    #[test]
    fn plan_resume_group_tid_single_member_unwraps() {
        let mut members = BTreeMap::new();
        members.insert("gpt-5.2".to_string(), "api_x".to_string());
        members.insert("gemini-2.5-pro".to_string(), "api_y".to_string());
        let group = StoredGroup {
            id: "group_abc".into(),
            members,
            member_order: vec![],
        };
        let plan = plan_resume(Some("group_abc"), &["gpt-5.2".into()], Some(group)).unwrap();
        assert!(
            !plan.unwrap_single,
            "group resumes always use the group path"
        );
        assert_eq!(plan.group_id.as_deref(), Some("group_abc"));
    }

    #[test]
    fn plan_resume_group_tid_model_not_in_group_errors() {
        let mut members = BTreeMap::new();
        members.insert("gpt-5.2".to_string(), "api_x".to_string());
        let group = StoredGroup {
            id: "group_abc".into(),
            members,
            member_order: vec![],
        };
        let err =
            plan_resume(Some("group_abc"), &["gemini-2.5-pro".into()], Some(group)).unwrap_err();
        assert!(err.to_string().contains("no member for model"));
    }

    #[test]
    fn plan_resume_group_tid_not_found_errors() {
        let err = plan_resume(Some("group_missing"), &["gpt-5.2".into()], None).unwrap_err();
        assert!(err.to_string().contains("group thread not found"));
    }
}
