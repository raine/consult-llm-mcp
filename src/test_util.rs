//! Shared test-time coordination primitives.
//!
//! Multiple tests in this crate mutate `XDG_STATE_HOME` to redirect on-disk
//! state to a tempdir. Cargo runs tests in parallel by default, and
//! `std::env::set_var` is process-global — without coordination one test's
//! tempdir can be torn down (TempDir drop) while another test is mid-write,
//! producing flaky `Invalid argument` rename failures. Tests that touch
//! `XDG_STATE_HOME` should hold this lock for their entire body.

use std::sync::Mutex;

pub static XDG_STATE_LOCK: Mutex<()> = Mutex::new(());
