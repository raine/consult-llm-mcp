//! Shared test-time coordination primitives.
//!
//! Multiple tests in this crate mutate `XDG_STATE_HOME` to redirect on-disk
//! state to a tempdir. Cargo runs tests in parallel by default, and
//! `std::env::set_var` is process-global — without coordination one test's
//! tempdir can be torn down (TempDir drop) while another test is mid-write,
//! producing flaky `Invalid argument` rename failures. Tests that touch
//! `XDG_STATE_HOME` should hold this lock for their entire body.

use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub static XDG_STATE_LOCK: Mutex<()> = Mutex::new(());

pub struct XdgStateGuard {
    _lock: MutexGuard<'static, ()>,
    previous: Option<OsString>,
    _temp: Option<tempfile::TempDir>,
}

impl XdgStateGuard {
    pub fn new(path: &Path) -> Self {
        let lock = XDG_STATE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let previous = std::env::var_os("XDG_STATE_HOME");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", path);
        }
        Self {
            _lock: lock,
            previous,
            _temp: None,
        }
    }

    pub fn temp() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut guard = Self::new(temp.path());
        guard._temp = Some(temp);
        guard
    }
}

impl Drop for XdgStateGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(previous) => std::env::set_var("XDG_STATE_HOME", previous),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
        }
    }
}
