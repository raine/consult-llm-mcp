use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const REPO: &str = "raine/consult-llm-mcp";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const NOTIFY_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Map OS/arch to the release artifact suffix used in GitHub releases.
fn platform_suffix() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("darwin-arm64"),
        ("macos", "x86_64") => Ok("darwin-x64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        ("linux", "aarch64") => Ok("linux-arm64"),
        (os, arch) => bail!("Unsupported platform: {os}/{arch}"),
    }
}

/// Check if the binary is managed by npm (installed via node_modules).
fn is_npm_install(exe_path: &std::path::Path) -> bool {
    let path_str = exe_path.to_string_lossy();
    path_str.contains("/node_modules/") || path_str.contains("\\node_modules\\")
}

/// Fetch the latest release tag from GitHub API using curl.
fn fetch_latest_version() -> Result<String> {
    let output = Command::new("curl")
        .args([
            "-sSf",
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        ])
        .output()
        .context("Failed to run curl. Is curl installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to fetch latest release: {}", stderr.trim());
    }

    let body: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse GitHub API response")?;

    let tag = body["tag_name"]
        .as_str()
        .context("No tag_name in GitHub API response")?;

    Ok(tag.strip_prefix('v').unwrap_or(tag).to_string())
}

/// Download a URL to a file path using curl.
fn download(url: &str, dest: &std::path::Path) -> Result<()> {
    let status = Command::new("curl")
        .args(["-sSLf", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .context("Failed to run curl")?;

    if !status.success() {
        bail!("Download failed: {url}");
    }
    Ok(())
}

/// Extract a tar.gz archive into a directory.
fn extract_tar(archive: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .status()
        .context("Failed to run tar")?;

    if !status.success() {
        bail!("Failed to extract archive");
    }
    Ok(())
}

/// Compute SHA-256 hash of a file using system tools.
fn sha256_of(path: &std::path::Path) -> Result<String> {
    // Try sha256sum first (common on Linux)
    if let Ok(output) = Command::new("sha256sum").arg(path).output()
        && output.status.success()
    {
        let out = String::from_utf8_lossy(&output.stdout);
        if let Some(hash) = out.split_whitespace().next() {
            return Ok(hash.to_string());
        }
    }

    // Fall back to shasum -a 256 (macOS)
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .context("Neither sha256sum nor shasum found. Cannot verify checksum.")?;

    if !output.status.success() {
        bail!("Checksum command failed");
    }

    let out = String::from_utf8_lossy(&output.stdout);
    out.split_whitespace()
        .next()
        .map(|s| s.to_string())
        .context("Could not parse checksum output")
}

/// Verify SHA-256 checksum of a file against the expected checksum line.
fn verify_checksum(file: &std::path::Path, expected_line: &str) -> Result<()> {
    let expected_hash = expected_line
        .split_whitespace()
        .next()
        .context("Invalid checksum file format")?;

    let actual_hash = sha256_of(file)?;
    if actual_hash != expected_hash {
        bail!("Checksum mismatch!\n  Expected: {expected_hash}\n  Got:      {actual_hash}");
    }
    Ok(())
}

/// Replace a binary with a new one, with rollback on failure.
fn replace_binary(
    new_binary: &std::path::Path,
    current_exe: &std::path::Path,
    name: &str,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let exe_dir = current_exe
        .parent()
        .context("Could not determine binary directory")?;

    let staged = exe_dir.join(format!(".{name}.new"));
    std::fs::copy(new_binary, &staged).context("Failed to copy new binary to install directory")?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;

    let backup = exe_dir.join(format!(".{name}.old"));
    std::fs::rename(current_exe, &backup).context("Failed to move current binary aside")?;

    if let Err(e) = std::fs::rename(&staged, current_exe) {
        let _ = std::fs::rename(&backup, current_exe);
        return Err(e).context("Failed to install new binary (rolled back)");
    }

    let _ = std::fs::remove_file(&backup);
    Ok(())
}

fn do_update(artifact_name: &str, current_exe: &std::path::Path) -> Result<String> {
    let latest_version = fetch_latest_version()?;

    if latest_version == CURRENT_VERSION {
        return Ok(format!("Already up to date (v{CURRENT_VERSION})"));
    }

    eprintln!("Downloading v{latest_version}...");

    let tmp = tempfile::tempdir().context("Failed to create temp directory")?;
    let tar_path = tmp.path().join(format!("{artifact_name}.tar.gz"));
    let sha_path = tmp.path().join(format!("{artifact_name}.sha256"));

    let base_url = format!("https://github.com/{REPO}/releases/download/v{latest_version}");

    download(&format!("{base_url}/{artifact_name}.tar.gz"), &tar_path)?;
    download(&format!("{base_url}/{artifact_name}.sha256"), &sha_path)?;

    eprintln!("Verifying checksum...");
    let sha_content = std::fs::read_to_string(&sha_path).context("Failed to read checksum file")?;
    verify_checksum(&tar_path, &sha_content)?;

    eprintln!("Installing...");
    let extract_dir = tmp.path().join("extract");
    std::fs::create_dir(&extract_dir)?;
    extract_tar(&tar_path, &extract_dir)?;

    // Update main binary
    let new_binary = extract_dir.join("consult-llm-mcp");
    if !new_binary.exists() {
        bail!("Extracted archive does not contain 'consult-llm-mcp' binary");
    }
    replace_binary(&new_binary, current_exe, "consult-llm-mcp")?;

    // Try to update monitor binary if it exists alongside the main binary
    let exe_dir = current_exe.parent();
    let new_monitor = extract_dir.join("consult-llm-monitor");
    if let Some(dir) = exe_dir {
        let monitor_path = dir.join("consult-llm-monitor");
        if monitor_path.exists()
            && new_monitor.exists()
            && let Err(e) = replace_binary(&new_monitor, &monitor_path, "consult-llm-monitor")
        {
            eprintln!("Warning: failed to update consult-llm-monitor: {e}");
        }
    }

    Ok(format!(
        "Updated consult-llm-mcp v{CURRENT_VERSION} -> v{latest_version}"
    ))
}

pub fn run() -> Result<()> {
    let current_exe =
        std::env::current_exe().context("Could not determine current executable path")?;

    let canonical_exe = std::fs::canonicalize(&current_exe).unwrap_or(current_exe.clone());

    if is_npm_install(&canonical_exe) {
        bail!("consult-llm-mcp is managed by npm. Run `npm update consult-llm-mcp` instead.");
    }

    let platform = platform_suffix()?;
    let artifact_name = format!("consult-llm-mcp-{platform}");

    eprintln!("Checking for updates...");

    match do_update(&artifact_name, &current_exe) {
        Ok(msg) => {
            eprintln!("✔ {msg}");
            Ok(())
        }
        Err(e) => {
            eprintln!("✘ Update failed");
            Err(e)
        }
    }
}

// --- Auto-update check ---

#[derive(Debug, Serialize, Deserialize, Default)]
struct UpdateCache {
    latest_version: Option<String>,
    last_checked: Option<u64>,
    last_notified: Option<u64>,
}

fn update_cache_path() -> Option<std::path::PathBuf> {
    let cache_dir = dirs::cache_dir()?;
    let dir = cache_dir.join("consult-llm-mcp");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("update_check.json"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn load_cache(path: &std::path::Path) -> UpdateCache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_cache(path: &std::path::Path, cache: &UpdateCache) {
    if let Ok(json) = serde_json::to_string(cache) {
        let _ = std::fs::write(path, json);
    }
}

/// Compare two version strings as numeric tuples (e.g. "0.1.10" > "0.1.9").
fn is_newer_version(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.split('.').filter_map(|s| s.parse().ok()).collect() };
    let l = parse(latest);
    let c = parse(current);
    l > c
}

/// Fetch latest version with a timeout (for background checks).
fn fetch_latest_version_with_timeout() -> Result<String> {
    let output = Command::new("curl")
        .args([
            "-sSf",
            "--connect-timeout",
            "5",
            "--max-time",
            "10",
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        ])
        .output()
        .context("Failed to run curl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to fetch latest release: {}", stderr.trim());
    }

    let body: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse GitHub API response")?;

    let tag = body["tag_name"]
        .as_str()
        .context("No tag_name in GitHub API response")?;

    Ok(tag.strip_prefix('v').unwrap_or(tag).to_string())
}

/// Hidden subcommand handler: fetch the latest version and update the cache.
pub fn run_background_check() -> Result<()> {
    let latest = fetch_latest_version_with_timeout()?;
    let now = now_secs();

    let cache_path = update_cache_path().context("Could not determine cache path")?;
    let mut cache = load_cache(&cache_path);

    if cache.latest_version.as_deref() != Some(&latest) {
        cache.last_notified = Some(0);
    }

    cache.latest_version = Some(latest);
    cache.last_checked = Some(now);
    save_cache(&cache_path, &cache);

    Ok(())
}

/// Called on startup to check for updates in the background and log if one is available.
/// Designed to be completely non-blocking and fail-silent.
pub fn check_and_notify() {
    if std::env::var("CONSULT_LLM_NO_UPDATE_CHECK").is_ok() {
        return;
    }

    // Skip for package-manager-managed installs
    let is_managed = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(&p).ok())
        .is_some_and(|p| is_npm_install(&p));
    if is_managed {
        return;
    }

    let cache_path = match update_cache_path() {
        Some(p) => p,
        None => return,
    };

    let mut cache = load_cache(&cache_path);
    let now = now_secs();

    // Spawn background check if cache is stale
    if now.saturating_sub(cache.last_checked.unwrap_or(0)) > CHECK_INTERVAL_SECS {
        let spawned = std::env::current_exe().ok().and_then(|exe| {
            Command::new(exe)
                .arg("_check-update")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
        });

        if let Some(mut child) = spawned {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            cache.last_checked = Some(now);
            save_cache(&cache_path, &cache);
        }
    }

    // Log notice if a newer version is available
    if let Some(ref latest) = cache.latest_version
        && is_newer_version(latest, CURRENT_VERSION)
        && now.saturating_sub(cache.last_notified.unwrap_or(0)) > NOTIFY_INTERVAL_SECS
    {
        crate::logger::log_to_file(&format!(
            "Update available: consult-llm-mcp v{CURRENT_VERSION} -> v{latest} (run `consult-llm-mcp update`)"
        ));

        cache.last_notified = Some(now);
        save_cache(&cache_path, &cache);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_suffix_current() {
        let suffix = platform_suffix().unwrap();
        assert!(["darwin-arm64", "darwin-x64", "linux-x64", "linux-arm64"].contains(&suffix));
    }

    #[test]
    fn test_is_npm_install_node_modules() {
        assert!(is_npm_install(std::path::Path::new(
            "/home/user/project/node_modules/consult-llm-mcp-linux-x64/consult-llm-mcp"
        )));
    }

    #[test]
    fn test_is_not_npm_install_local_bin() {
        assert!(!is_npm_install(std::path::Path::new(
            "/usr/local/bin/consult-llm-mcp"
        )));
    }

    #[test]
    fn test_is_newer_version_patch() {
        assert!(is_newer_version("2.8.1", "2.8.0"));
    }

    #[test]
    fn test_is_newer_version_minor() {
        assert!(is_newer_version("2.9.0", "2.8.0"));
    }

    #[test]
    fn test_is_newer_version_major() {
        assert!(is_newer_version("3.0.0", "2.99.99"));
    }

    #[test]
    fn test_is_not_newer_same() {
        assert!(!is_newer_version("2.8.0", "2.8.0"));
    }

    #[test]
    fn test_is_not_newer_older() {
        assert!(!is_newer_version("2.7.0", "2.8.0"));
    }
}
