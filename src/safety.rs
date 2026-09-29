//! The safety net (PRODUCT §12): never leave the user in the dark.
//!
//! - A session flag in `state.ini` detects a previous crash; the next start resets gamma first.
//! - Panic hook and unhandled-exception filter restore neutral gamma before the process dies.
//!   (Overlays and the Magnification color effect vanish with the process on their own.)

use crate::config::Ini;
use crate::{engine, info, monitors};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows::Win32::System::Diagnostics::Debug::{
    SetUnhandledExceptionFilter, EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS,
};

static STATE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Restores neutral gamma on every monitor. Safe to call from crash handlers.
pub fn emergency_reset() {
    let n = engine::reset_all(&monitors::enumerate());
    info!("emergency reset: {n} monitor(s)");
}

unsafe extern "system" fn on_exception(_: *const EXCEPTION_POINTERS) -> i32 {
    emergency_reset();
    mark(false);
    EXCEPTION_CONTINUE_SEARCH
}

/// Installs the panic hook and exception filter; remembers where the state file lives.
pub fn install(state_path: &Path) {
    let _ = STATE_PATH.set(state_path.to_path_buf());
    std::panic::set_hook(Box::new(|p| {
        info!("panic: {p}");
        emergency_reset();
        mark(false);
    }));
    unsafe {
        SetUnhandledExceptionFilter(Some(on_exception));
    }
}

fn mark(running: bool) {
    let Some(path) = STATE_PATH.get() else { return };
    let mut ini = Ini::load(path).unwrap_or_default();
    ini.set("session", "status", if running { "running" } else { "clean" });
    ini.set("session", "pid", std::process::id());
    let _ = ini.save(path);
}

/// Marks the session as running. Returns true if the previous session did not end cleanly.
pub fn begin_session() -> bool {
    let dirty = STATE_PATH
        .get()
        .and_then(|p| Ini::load(p))
        .is_some_and(|ini| ini.get("session", "status") == Some("running"));
    mark(true);
    dirty
}

pub fn end_session() {
    mark(false);
}
