use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const TTL_SECS: u64 = 24 * 60 * 60;
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CachedModels {
    models: Vec<String>,
    fetched_at: u64,
}

#[derive(Debug, Clone)]
pub enum ModelList {
    Fresh(Vec<String>),
    Stale(Vec<String>),
    Unavailable,
}

impl ModelList {
    pub fn as_slice(&self) -> &[String] {
        match self {
            ModelList::Fresh(v) | ModelList::Stale(v) => v.as_slice(),
            ModelList::Unavailable => &[],
        }
    }
}

#[derive(Debug)]
pub struct ResolveError {
    pub candidate: String,
    pub base: String,
    pub available: Vec<String>,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let avail = if self.available.is_empty() {
            "none".to_string()
        } else {
            self.available.join(", ")
        };
        write!(
            f,
            "cursor model '{}' is not available; cursor-agent supports for '{}': {}",
            self.candidate, self.base, avail
        )
    }
}

impl std::error::Error for ResolveError {}

pub fn parse_list_models_output(s: &str) -> Vec<String> {
    s.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        // Drop the "Tip: ..." trailer up front — it can legitimately contain
        // " - " and would otherwise be partially captured as a model id.
        .filter(|l| !l.to_ascii_lowercase().starts_with("tip:"))
        .filter_map(|l| l.split_once(" - ").map(|(id, _)| id.trim()))
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
        .collect()
}

pub fn resolve_model(
    candidate: &str,
    base: &str,
    list: &ModelList,
) -> Result<String, ResolveError> {
    let strict = matches!(list, ModelList::Fresh(_));
    let models = list.as_slice();

    if models.iter().any(|m| m == candidate) {
        return Ok(candidate.to_string());
    }

    let effort = candidate
        .strip_prefix(base)
        .and_then(|s| s.strip_prefix('-'))
        .unwrap_or("");
    for v in variants_for(effort) {
        let try_id = format!("{base}-{v}");
        if models.iter().any(|m| m == &try_id) {
            return Ok(try_id);
        }
    }

    if strict {
        let available: Vec<String> = models
            .iter()
            .filter(|m| is_effort_variant_of(m, base))
            .cloned()
            .collect();
        Err(ResolveError {
            candidate: candidate.into(),
            base: base.into(),
            available,
        })
    } else {
        Ok(candidate.to_string())
    }
}

/// True if `model` is exactly `base` or `base-<known-effort>` (optionally with
/// a `-fast` suffix). Used to scope the "available" list reported in resolver
/// errors to actual effort variants — unrelated families that happen to share
/// the textual prefix (e.g. `gpt-5.5-mini` vs base `gpt-5.5`) are excluded so
/// the error message stays focused on the user's effort-misconfiguration.
fn is_effort_variant_of(model: &str, base: &str) -> bool {
    if model == base {
        return true;
    }
    let Some(rest) = model.strip_prefix(base).and_then(|s| s.strip_prefix('-')) else {
        return false;
    };
    is_known_effort_token(rest)
}

fn is_known_effort_token(s: &str) -> bool {
    // The `-fast` axis is orthogonal to effort and may co-occur (e.g.
    // `gpt-5.4-high-fast`), so accept either form.
    let core = s.strip_suffix("-fast").unwrap_or(s);
    matches!(
        core,
        "low" | "medium" | "high" | "xhigh" | "extra-high" | "none" | "minimal"
    )
}

fn variants_for(effort: &str) -> &'static [&'static str] {
    match effort {
        "none" | "minimal" | "low" => &["medium", "high", "extra-high"],
        "medium" => &["high", "extra-high"],
        "high" => &["medium", "extra-high"],
        "xhigh" => &["extra-high", "high", "medium"],
        "extra-high" => &["high", "medium"],
        _ => &[],
    }
}

fn cache_path() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("consult-llm");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("cursor_models.json"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn load_cached(path: &Path) -> Option<CachedModels> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn save_cached(path: &Path, c: &CachedModels) {
    if let Ok(json) = serde_json::to_string(c) {
        let _ = std::fs::write(path, json);
    }
}

fn fetch_from_cli() -> Option<Vec<String>> {
    use crate::executors::child_guard::ChildGuard;

    let mut cmd = std::process::Command::new("cursor-agent");
    cmd.arg("--list-models")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut guard = ChildGuard::spawn(&mut cmd).ok()?;

    // Drain stdout/stderr in worker threads so they cannot fill the OS pipe
    // and deadlock try_wait. We hold the child in the main thread so we can
    // kill it on timeout.
    let stdout_pipe = guard.child_mut().stdout.take()?;
    let stderr_pipe = guard.child_mut().stderr.take()?;
    let stdout_handle = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut pipe = stdout_pipe;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut pipe, &mut buf)?;
        Ok(buf)
    });
    let stderr_handle = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut pipe = stderr_pipe;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut pipe, &mut buf)?;
        Ok(buf)
    });

    let deadline = std::time::Instant::now() + FETCH_TIMEOUT;
    let exited = loop {
        match guard.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };

    let Some(status) = exited else {
        // Timeout: kill the child, then join the drain threads so they
        // don't outlive this function as detached threads holding the
        // (now-EOF) pipe ends.
        drop(guard);
        let _ = stdout_handle.join();
        let _ = stderr_handle.join();
        return None;
    };
    if !status.success() {
        let _ = guard.wait();
        let _ = stdout_handle.join();
        let _ = stderr_handle.join();
        return None;
    }
    // Reap the guard normally (already exited).
    let _ = guard.wait();
    let stdout_bytes = stdout_handle.join().ok()?.ok()?;
    let _ = stderr_handle.join();
    let text = String::from_utf8_lossy(&stdout_bytes);
    let models = parse_list_models_output(&text);
    if models.is_empty() {
        None
    } else {
        Some(models)
    }
}

pub fn available_models() -> ModelList {
    let path = cache_path();
    let now = now_secs();
    let cached = path.as_deref().and_then(load_cached);

    if let Some(c) = &cached
        && c.fetched_at <= now
        && now - c.fetched_at < TTL_SECS
    {
        return ModelList::Fresh(c.models.clone());
    }

    if let Some(models) = fetch_from_cli() {
        if let Some(p) = path.as_deref() {
            save_cached(
                p,
                &CachedModels {
                    models: models.clone(),
                    fetched_at: now,
                },
            );
        }
        return ModelList::Fresh(models);
    }

    match cached {
        Some(c) => ModelList::Stale(c.models),
        None => ModelList::Unavailable,
    }
}

/// Pure decision logic mirroring `available_models`, used by tests so they
/// don't have to spawn `cursor-agent`. Production code calls
/// `available_models()` which composes this with `fetch_from_cli()`.
#[cfg(test)]
fn available_models_with(
    now: u64,
    path: Option<&Path>,
    freshly_fetched: Option<Vec<String>>,
) -> ModelList {
    let cached = path.and_then(load_cached);

    if let Some(c) = &cached
        && c.fetched_at <= now
        && now - c.fetched_at < TTL_SECS
    {
        return ModelList::Fresh(c.models.clone());
    }

    if let Some(models) = freshly_fetched {
        if let Some(p) = path {
            save_cached(
                p,
                &CachedModels {
                    models: models.clone(),
                    fetched_at: now,
                },
            );
        }
        return ModelList::Fresh(models);
    }

    match cached {
        Some(c) => ModelList::Stale(c.models),
        None => ModelList::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_models() -> Vec<String> {
        vec![
            "gpt-5.5-medium".into(),
            "gpt-5.5-high".into(),
            "gpt-5.5-extra-high".into(),
            "gpt-5.4-low".into(),
            "gpt-5.4-medium".into(),
            "claude-4.5-sonnet".into(),
        ]
    }

    #[test]
    fn parse_real_world_output() {
        let s = "Available models\n\
                 \n\
                 auto - Auto\n\
                 gpt-5.5-medium - GPT-5.5 1M\n\
                 gpt-5.5-extra-high - GPT-5.5 Extra High\n\
                 claude-4.5-sonnet - Sonnet 4.5\n\
                 \n\
                 Tip: use --model <id> (or /model <id> in interactive mode) to switch.\n";
        let ids = parse_list_models_output(s);
        assert_eq!(
            ids,
            vec![
                "auto",
                "gpt-5.5-medium",
                "gpt-5.5-extra-high",
                "claude-4.5-sonnet",
            ]
        );
    }

    #[test]
    fn parse_header_with_colon_excluded() {
        // A trailing colon on the header still has no " - " separator → dropped.
        let s = "Available models:\n\nfoo - Foo\n";
        assert_eq!(parse_list_models_output(s), vec!["foo"]);
    }

    #[test]
    fn parse_empty() {
        assert!(parse_list_models_output("").is_empty());
    }

    #[test]
    fn parse_drops_tip_line_containing_separator() {
        // The Tip line could legitimately contain " - "; ensure we drop it
        // before splitting rather than capturing its left-hand side.
        let s = "Tip: use --model foo - switch\nfoo - Foo\n";
        assert_eq!(parse_list_models_output(s), vec!["foo"]);
    }

    #[test]
    fn parse_drops_lines_without_separator() {
        let s = "warning something happened\nfoo - Foo\nrandom noise\nbar - Bar\n";
        assert_eq!(parse_list_models_output(s), vec!["foo", "bar"]);
    }

    #[test]
    fn resolve_literal_present() {
        let list = ModelList::Fresh(fixture_models());
        assert_eq!(
            resolve_model("gpt-5.5-high", "gpt-5.5", &list).unwrap(),
            "gpt-5.5-high"
        );
    }

    #[test]
    fn resolve_xhigh_to_extra_high() {
        let list = ModelList::Fresh(fixture_models());
        assert_eq!(
            resolve_model("gpt-5.5-xhigh", "gpt-5.5", &list).unwrap(),
            "gpt-5.5-extra-high"
        );
    }

    #[test]
    fn resolve_low_to_medium() {
        let list = ModelList::Fresh(fixture_models());
        assert_eq!(
            resolve_model("gpt-5.5-low", "gpt-5.5", &list).unwrap(),
            "gpt-5.5-medium"
        );
    }

    #[test]
    fn resolve_none_to_medium() {
        let list = ModelList::Fresh(fixture_models());
        assert_eq!(
            resolve_model("gpt-5.5-none", "gpt-5.5", &list).unwrap(),
            "gpt-5.5-medium"
        );
    }

    #[test]
    fn resolve_minimal_to_medium() {
        let list = ModelList::Fresh(fixture_models());
        assert_eq!(
            resolve_model("gpt-5.5-minimal", "gpt-5.5", &list).unwrap(),
            "gpt-5.5-medium"
        );
    }

    #[test]
    fn resolve_fresh_no_match_errors_with_available_subset() {
        let list = ModelList::Fresh(vec![
            "gpt-5.5".into(),
            "gpt-5.5-medium".into(),
            "gpt-5.5-mini".into(), // distinct base, must NOT appear in `available`
            "gpt-5.50-foo".into(), // distinct base, must NOT appear in `available`
            "claude-4.5-sonnet".into(),
        ]);
        let err = resolve_model("gpt-5.5-bogus", "gpt-5.5", &list).unwrap_err();
        assert_eq!(err.available, vec!["gpt-5.5", "gpt-5.5-medium"]);
    }

    #[test]
    fn resolve_fresh_no_match_no_base_entries() {
        let list = ModelList::Fresh(vec!["claude-4.5-sonnet".into()]);
        let err = resolve_model("gpt-5.5-xhigh", "gpt-5.5", &list).unwrap_err();
        assert!(err.available.is_empty());
    }

    #[test]
    fn resolve_stale_passes_through_on_miss() {
        let list = ModelList::Stale(vec!["claude-4.5-sonnet".into()]);
        assert_eq!(
            resolve_model("gpt-5.5-xhigh", "gpt-5.5", &list).unwrap(),
            "gpt-5.5-xhigh"
        );
    }

    #[test]
    fn resolve_stale_uses_variant_on_match() {
        let list = ModelList::Stale(fixture_models());
        assert_eq!(
            resolve_model("gpt-5.5-xhigh", "gpt-5.5", &list).unwrap(),
            "gpt-5.5-extra-high"
        );
    }

    #[test]
    fn resolve_unavailable_passes_through() {
        let list = ModelList::Unavailable;
        assert_eq!(
            resolve_model("gpt-5.5-xhigh", "gpt-5.5", &list).unwrap(),
            "gpt-5.5-xhigh"
        );
    }

    fn write_cache(path: &Path, fetched_at: u64, models: &[&str]) {
        let c = CachedModels {
            models: models.iter().map(|s| s.to_string()).collect(),
            fetched_at,
        };
        save_cached(path, &c);
    }

    #[test]
    fn available_fresh_cache_no_fetch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cursor_models.json");
        write_cache(&path, 1_000_000, &["foo", "bar"]);
        // Now is 1h after fetched_at — well within TTL.
        let list = available_models_with(1_000_000 + 3600, Some(&path), None);
        match list {
            ModelList::Fresh(v) => assert_eq!(v, vec!["foo", "bar"]),
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    #[test]
    fn available_stale_cache_refresh_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cursor_models.json");
        write_cache(&path, 1_000_000, &["old"]);
        // 25h later → stale.
        let now = 1_000_000 + 25 * 3600;
        let list = available_models_with(now, Some(&path), Some(vec!["new".into()]));
        match list {
            ModelList::Fresh(v) => assert_eq!(v, vec!["new"]),
            other => panic!("expected Fresh, got {other:?}"),
        }
        // File overwritten with new contents.
        let after = load_cached(&path).unwrap();
        assert_eq!(after.models, vec!["new"]);
        assert_eq!(after.fetched_at, now);
    }

    #[test]
    fn available_stale_cache_refresh_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cursor_models.json");
        write_cache(&path, 1_000_000, &["old"]);
        let now = 1_000_000 + 25 * 3600;
        let list = available_models_with(now, Some(&path), None);
        match list {
            ModelList::Stale(v) => assert_eq!(v, vec!["old"]),
            other => panic!("expected Stale, got {other:?}"),
        }
        // File NOT rewritten.
        let after = load_cached(&path).unwrap();
        assert_eq!(after.fetched_at, 1_000_000);
    }

    #[test]
    fn available_no_cache_no_fetch_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cursor_models.json");
        let list = available_models_with(1_000_000, Some(&path), None);
        assert!(matches!(list, ModelList::Unavailable));
    }

    #[test]
    fn available_future_timestamp_treated_as_stale() {
        // A cache file with a fetched_at in the future (e.g. clock rollback)
        // must not be trusted as fresh. With no fetch result available, we
        // expect Stale fallback.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cursor_models.json");
        write_cache(&path, 2_000_000, &["future"]);
        let list = available_models_with(1_000_000, Some(&path), None);
        match list {
            ModelList::Stale(v) => assert_eq!(v, vec!["future"]),
            other => panic!("expected Stale, got {other:?}"),
        }
    }

    #[test]
    fn available_no_cache_with_fetch_fresh_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cursor_models.json");
        let list = available_models_with(1_000_000, Some(&path), Some(vec!["a".into()]));
        match list {
            ModelList::Fresh(v) => assert_eq!(v, vec!["a"]),
            other => panic!("expected Fresh, got {other:?}"),
        }
        let after = load_cached(&path).unwrap();
        assert_eq!(after.models, vec!["a"]);
    }
}
