//! Screen Lighting Control (SLC) — entry point.
#![cfg_attr(not(test), windows_subsystem = "windows")]
// Modules are wired up across the v1.0 milestone PRs; remove before tagging v1.0.
#![allow(dead_code)]

mod app;
mod cli;
mod icon;
mod log;
mod tray;
mod win;

use cli::Command;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            console();
            eprintln!("slc: {e}\n\n{}", cli::HELP);
            std::process::exit(2);
        }
    };
    if let Some(f) = &cli.log_file {
        log::enable_file(std::path::Path::new(f));
    }
    info!("SLC {VERSION} starting: {:?}", cli.command);
    let code = match cli.command {
        Command::Run => run(),
        Command::Help => {
            console();
            println!("{}", cli::HELP);
            0
        }
        Command::Version => {
            console();
            println!("SLC {VERSION}");
            0
        }
        other => {
            console();
            eprintln!("slc: {other:?} is not implemented yet");
            1
        }
    };
    std::process::exit(code);
}

/// Attaches to the parent console so CLI output is visible (we are a GUI-subsystem exe).
fn console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn run() -> i32 {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // Single instance: the mutex lives until the process exits.
        let _mutex = CreateMutexW(None, false, win::INSTANCE_MUTEX);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(win::CONTROLLER_CLASS, None) {
                win::post(h, win::WM_APP_ACTIVATE, 0, 0);
            }
            return 0;
        }
    }
    let app = match app::App::create() {
        Ok(a) => a,
        Err(e) => {
            info!("startup failed: {e}");
            return 1;
        }
    };
    let code = app::run_loop();
    drop(app);
    info!("exit {code}");
    code
}
