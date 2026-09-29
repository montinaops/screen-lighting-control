//! Tiny in-memory log (ring buffer) with an optional file mirror (`--log`).

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

const CAPACITY: usize = 200;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static FILE: Mutex<Option<File>> = Mutex::new(None);

/// Mirrors every log line to `path` from now on.
pub fn enable_file(path: &std::path::Path) {
    if let Ok(f) = File::create(path) {
        *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }
}

pub fn push(msg: String) {
    let line = format!("{} {}", timestamp(), msg);
    if let Some(f) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = writeln!(f, "{line}");
    }
    let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if lines.len() == CAPACITY {
        lines.pop_front();
    }
    lines.push_back(line);
}

/// All buffered lines, oldest first (used by "Copy diagnostics").
pub fn snapshot() -> String {
    let lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = String::new();
    for l in lines.iter() {
        out.push_str(l);
        out.push_str("\r\n");
    }
    out
}

#[cfg(windows)]
fn timestamp() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:02}:{:02}:{:02}.{:03}", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds)
}

#[cfg(not(windows))]
fn timestamp() -> String {
    String::new()
}

#[macro_export]
macro_rules! info {
    ($($t:tt)*) => { $crate::log::push(format!($($t)*)) };
}

#[cfg(test)]
mod tests {
    #[test]
    fn ring_buffer_is_bounded() {
        for i in 0..(super::CAPACITY + 50) {
            super::push(format!("line {i}"));
        }
        let snap = super::snapshot();
        assert_eq!(snap.lines().count(), super::CAPACITY);
        assert!(snap.contains(&format!("line {}", super::CAPACITY + 49)));
    }
}
