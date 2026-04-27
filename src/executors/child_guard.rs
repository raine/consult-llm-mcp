use std::collections::HashSet;
use std::process::{Child, Command, ExitStatus};
use std::sync::{Mutex, OnceLock};

/// PID set of every live child this process has spawned via `ChildGuard::spawn`.
/// Used by the SIGINT handler to terminate the process tree before exit.
fn tracked() -> &'static Mutex<HashSet<u32>> {
    static TRACKED: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    TRACKED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Install a Ctrl-C handler that signals every tracked child (TERM, brief
/// grace, then KILL) and exits the process. Drop bypass on exit is acceptable
/// here — see `Watched Risks: drop-bypass-on-sigint` in the plan.
///
/// Idempotent: calling twice is harmless because `ctrlc::set_handler` is the
/// inner `OnceLock` guard.
pub fn install_signal_handler() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let _ = ctrlc::set_handler(|| {
            shutdown_children();
            std::process::exit(130);
        });
    });
}

fn shutdown_children() {
    let pids: Vec<u32> = {
        let g = tracked().lock().unwrap_or_else(|e| e.into_inner());
        g.iter().copied().collect()
    };
    for pid in &pids {
        send_signal(*pid, libc::SIGTERM);
    }
    if pids.is_empty() {
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    // Re-snapshot before SIGKILL: any child that exited cleanly (and was
    // untracked by ChildGuard::wait) is no longer in the set, so we won't
    // SIGKILL a recycled PID. Closes the PID-reuse race flagged in review.
    let alive: Vec<u32> = {
        let g = tracked().lock().unwrap_or_else(|e| e.into_inner());
        g.iter().copied().collect()
    };
    for pid in &alive {
        send_signal(*pid, libc::SIGKILL);
    }
}

fn send_signal(pid: u32, sig: libc::c_int) {
    // SAFETY: kill(2) with a pid we previously spawned. errno is ignored —
    // the child may already have exited.
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// RAII wrapper around a `std::process::Child`. On drop, the child is killed
/// and reaped (with a brief grace) and its PID is removed from the tracked
/// set. Use `wait()` to consume the guard normally and avoid the kill on
/// drop.
pub struct ChildGuard {
    child: Option<Child>,
    pid: u32,
}

impl ChildGuard {
    pub fn spawn(cmd: &mut Command) -> std::io::Result<Self> {
        let child = cmd.spawn()?;
        let pid = child.id();
        tracked()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(pid);
        Ok(Self {
            child: Some(child),
            pid,
        })
    }

    pub fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child after spawn")
    }

    pub fn id(&self) -> u32 {
        self.pid
    }

    /// Wait for the child to exit, consuming the guard. Untracks the PID
    /// BEFORE the OS reaps it so a concurrent SIGINT handler can't snapshot
    /// the PID, then later SIGKILL a recycled value.
    pub fn wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self.child.take().expect("child after spawn");
        tracked()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.pid);
        child.wait()
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child_mut().try_wait()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            tracked()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.pid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn drop_kills_running_child() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let guard = ChildGuard::spawn(&mut cmd).expect("spawn sleep");
        let pid = guard.id();
        assert!(tracked().lock().unwrap().contains(&pid));
        let start = Instant::now();
        drop(guard);
        // Drop must complete promptly — we shouldn't wait the full 30s.
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(!tracked().lock().unwrap().contains(&pid));
    }

    #[test]
    fn wait_consumes_and_untracks() {
        let mut cmd = Command::new("true");
        let guard = ChildGuard::spawn(&mut cmd).expect("spawn true");
        let pid = guard.id();
        let status = guard.wait().expect("wait");
        assert!(status.success());
        assert!(!tracked().lock().unwrap().contains(&pid));
    }

    #[test]
    fn try_wait_polls() {
        let mut cmd = Command::new("sleep");
        cmd.arg("0.05");
        let mut guard = ChildGuard::spawn(&mut cmd).expect("spawn sleep");
        // First poll: still running
        assert!(guard.try_wait().expect("try_wait").is_none());
        std::thread::sleep(Duration::from_millis(150));
        let s = guard.try_wait().expect("try_wait").expect("exited");
        assert!(s.success());
    }
}
