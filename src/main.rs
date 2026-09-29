//! Screen Lighting Control (SLC) — entry point.
#![cfg_attr(not(test), windows_subsystem = "windows")]
// Modules are wired up across the v1.0 milestone PRs; remove before tagging v1.0.
#![allow(dead_code)]

mod app;
mod cities;
mod cli;
mod color;
mod config;
mod engine;
mod glyph;
mod hotkeys;
mod icon;
mod install;
mod log;
mod model;
mod monitors;
mod safety;
mod schedule;
mod solar;
mod tray;
mod ui;
mod win;
mod worker;

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
        Command::Reset => {
            console();
            let n = engine::reset_all(&monitors::enumerate());
            println!("SLC: reset {n} monitor(s)");
            0
        }
        Command::ExpandRange => {
            console();
            match install::expand_gamma_range() {
                Ok(()) => {
                    println!("SLC: color range expanded; sign out and back in to apply");
                    if !cli.quiet {
                        install::ask(
                            "Done. Windows will allow SLC's full color range after you sign out and back in (or restart).",
                            false,
                        );
                    }
                    0
                }
                Err(e) => {
                    eprintln!("slc: {e}");
                    if !cli.quiet {
                        install::ask(&format!("Could not expand the color range: {e}"), false);
                    }
                    1
                }
            }
        }
        Command::Install => {
            console();
            cmd_install()
        }
        Command::Uninstall => {
            console();
            cmd_uninstall(cli.quiet, cli.yes)
        }
        Command::Forward(line) => {
            console();
            forward(&line)
        }
        Command::SelfTest => {
            console();
            self_test()
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
            // The other instance may still be starting: give its window a moment to appear.
            for _ in 0..20 {
                if let Ok(h) = FindWindowW(win::CONTROLLER_CLASS, None) {
                    win::post(h, win::WM_APP_ACTIVATE, 0, 0);
                    return 0;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            // A mutex without a window means a hung or dying process: start anyway.
            info!("instance mutex held but no running window; starting anyway");
        }
    }
    let paths = config::resolve();
    safety::install(&paths.state);
    let recovered = safety::begin_session();
    if recovered {
        info!("previous session did not end cleanly; resetting gamma");
        safety::emergency_reset();
    }
    let app = match app::App::create(paths, recovered) {
        Ok(a) => a,
        Err(e) => {
            info!("startup failed: {e}");
            return 1;
        }
    };
    let code = app::run_loop();
    drop(app);
    info!("exit {code}");
    if let Some((exe, args)) = app::take_relaunch() {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        info!("relaunching {} {:?}", exe.display(), args);
        install::launch(&exe, &args);
    }
    code
}

/// Asks a running instance to exit and waits (up to 3 s) for it to go away.
fn stop_running_instance() -> bool {
    let Ok(hwnd) = (unsafe { FindWindowW(win::CONTROLLER_CLASS, None) }) else { return false };
    forward("--exit");
    for _ in 0..30 {
        if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)) }.as_bool() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        } else {
            break;
        }
    }
    true
}

fn cmd_install() -> i32 {
    stop_running_instance();
    match install::install() {
        Ok(target) => {
            println!("SLC: installed to {}", target.display());
            install::launch(&target, &[]);
            0
        }
        Err(e) => {
            eprintln!("slc: install failed: {e}");
            install::ask(&format!("Installation failed: {e}"), false);
            1
        }
    }
}

fn cmd_uninstall(quiet: bool, confirmed: bool) -> i32 {
    if !quiet && !confirmed && !install::ask("Remove Screen Lighting Control from this computer?", true) {
        return 1;
    }
    stop_running_instance();
    // Put the screens back the way they were before SLC.
    let mons = monitors::enumerate();
    engine::reset_all(&mons);
    let settings = config::Ini::load(&config::resolve().settings).map(|i| model::Settings::from_ini(&i));
    if let Some(s) = &settings {
        for m in &mons {
            if let Some(level) = s.monitor(&monitors::settings_key(&m.id)).and_then(|ms| ms.original_hw) {
                engine::hardware::write(m.hmon, m.internal, level);
            }
        }
    }
    let remove_settings = !quiet && install::ask("Also delete your SLC settings?", true);
    match install::uninstall(remove_settings) {
        Ok(()) => {
            println!("SLC: uninstalled");
            if !quiet {
                install::ask("Screen Lighting Control was removed.", false);
            }
            0
        }
        Err(e) => {
            eprintln!("slc: uninstall failed: {e}");
            1
        }
    }
}

/// Sends a request line to the running instance (WM_COPYDATA).
fn forward(line: &str) -> i32 {
    if let Err(e) = cli::parse_forward(line) {
        eprintln!("slc: {e}");
        return 2;
    }
    let Ok(hwnd) = (unsafe { FindWindowW(win::CONTROLLER_CLASS, None) }) else {
        eprintln!("slc: SLC is not running");
        return 1;
    };
    let bytes = line.as_bytes();
    let cds = windows::Win32::System::DataExchange::COPYDATASTRUCT {
        dwData: win::COPYDATA_FORWARD,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr() as *mut _,
    };
    let r = unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::WM_COPYDATA,
            Some(windows::Win32::Foundation::WPARAM(0)),
            Some(windows::Win32::Foundation::LPARAM(&cds as *const _ as isize)),
        )
    };
    if r.0 == 1 {
        0
    } else {
        eprintln!("slc: the running instance rejected the request");
        1
    }
}

/// Non-interactive checks (CI and diagnostics): prints what SLC can see and control.
fn self_test() -> i32 {
    let mons = monitors::enumerate();
    println!("SLC {VERSION} self-test: {} monitor(s)", mons.len());
    for (i, m) in mons.iter().enumerate() {
        let ramp = engine::gamma::read(&m.device);
        let hw = engine::hardware::read(m.hmon, m.internal);
        println!(
            "  #{} {} [{}] {}x{} primary={} internal={} hdr={} gamma={} hardware={}",
            i + 1,
            m.name,
            m.device,
            m.width(),
            m.height(),
            m.primary,
            m.internal,
            m.hdr,
            if ramp.is_some() { "readable" } else { "unavailable" },
            match hw {
                Some(c) => format!("{:?} {:.0}%", c.kind, c.current),
                None => "none".into(),
            }
        );
    }
    // Gamma probe: apply a strong warm + dim ramp for a moment, report what Windows accepted, restore.
    for m in &mons {
        let probes = [(2700, 0.5), (1200, 0.2), (3400, 1.0)];
        let mut st = engine::gamma::State::default();
        for (k, scale) in probes {
            let t0 = std::time::Instant::now();
            let a = engine::gamma::apply(&m.device, k, scale, &mut st);
            let ms = t0.elapsed().as_secs_f32() * 1000.0;
            println!(
                "  probe {} {k}K x{scale}: scale={:.2} warmth={:.2} limited={} failed={} ({ms:.1} ms, bound {:.3})",
                m.device, a.scale, a.warmth, a.limited, a.failed, st.bound
            );
        }
        engine::gamma::reset(&m.device);
    }
    // Schedule (from the settings next to the exe or the installed copy).
    let settings = config::Ini::load(&config::resolve().settings)
        .map(|i| model::Settings::from_ini(&i))
        .unwrap_or_default();
    let sc = &settings.schedule;
    let (y, doy, t, off) = schedule::now_local();
    let ev = schedule::events(sc, y, doy, off);
    let tg = schedule::target_at(sc, &ev, t);
    println!(
        "  schedule: enabled={} location={} day_start={} sunset={} night_start={} now={} -> {}K {:?} (UTC{:+}h)",
        sc.enabled,
        if sc.has_location() { format!("{} ({:.2},{:.2})", sc.city, sc.lat, sc.lon) } else { "none (fixed times)".into() },
        model::format_hm(ev.day_start.round() as u32),
        model::format_hm(ev.sunset.round() as u32),
        model::format_hm(ev.night_start.round() as u32),
        model::format_hm(t as u32),
        tg.kelvin,
        tg.phase,
        off / 60.0
    );
    // Pure-logic sanity: neutral white and ramp construction.
    let ok = color::white_point(6500).iter().all(|c| (c - 1.0).abs() < 1e-3)
        && color::build_ramp([1.0; 3], 1.0, 1.0) == color::identity_ramp();
    println!("  color math: {}", if ok { "ok" } else { "FAILED" });
    if ok {
        0
    } else {
        1
    }
}
