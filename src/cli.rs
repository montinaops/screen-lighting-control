//! Command-line parsing (no dependencies).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Start the tray app (or focus the running instance).
    Run,
    /// Restore neutral gamma / full brightness and exit.
    Reset,
    Install,
    Uninstall,
    /// Elevated helper: set `GdiICMGammaRange=256`.
    ExpandRange,
    /// Non-interactive checks for CI.
    SelfTest,
    Help,
    Version,
    /// Forward a request to the running instance (`--set`, `--scene`, `--pause`).
    Forward(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub command: Command,
    /// `--log <file>` mirrors the log to a file.
    pub log_file: Option<String>,
    /// `--minimized`: started by autostart; don't show anything but the tray icon.
    pub minimized: bool,
    /// `--quiet`: no dialogs (uninstall keeps settings).
    pub quiet: bool,
    /// `--yes`: the user already confirmed (uninstall started from the settings window).
    pub yes: bool,
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut cli = Cli { command: Command::Run, log_file: None, minimized: false, quiet: false, yes: false };
    let mut it = args.into_iter();
    let mut forward: Vec<String> = Vec::new();
    while let Some(a) = it.next() {
        let a_l = a.to_ascii_lowercase();
        let set = |cli: &mut Cli, c: Command| -> Result<(), String> {
            if cli.command != Command::Run {
                return Err("only one command may be given".into());
            }
            cli.command = c;
            Ok(())
        };
        match a_l.as_str() {
            "--reset" => set(&mut cli, Command::Reset)?,
            "--install" => set(&mut cli, Command::Install)?,
            "--uninstall" => set(&mut cli, Command::Uninstall)?,
            "--expand-range" => set(&mut cli, Command::ExpandRange)?,
            "--self-test" => set(&mut cli, Command::SelfTest)?,
            "-h" | "--help" | "/?" => set(&mut cli, Command::Help)?,
            "-v" | "--version" => set(&mut cli, Command::Version)?,
            "--minimized" => cli.minimized = true,
            "--quiet" => cli.quiet = true,
            "--yes" => cli.yes = true,
            "--log" => {
                cli.log_file = Some(it.next().ok_or("--log needs a file path")?);
            }
            "--settings" => {
                forward.push(a_l.clone());
                if let Some(page) = it.next() {
                    forward.push(page);
                }
            }
            "--set" | "--scene" | "--pause" | "--resume" | "--exit" => {
                forward.push(a_l.clone());
                if a_l != "--resume" && a_l != "--exit" {
                    let v = it.next().ok_or_else(|| format!("{a} needs a value"))?;
                    forward.push(v);
                }
            }
            _ if !forward.is_empty() && a.contains('=') => forward.push(a),
            _ => return Err(format!("unknown argument: {a}")),
        }
    }
    if !forward.is_empty() {
        if cli.command != Command::Run {
            return Err("cannot combine a command with --set/--scene/--pause".into());
        }
        cli.command = Command::Forward(forward.join(" "));
    }
    Ok(cli)
}

/// A request forwarded to the running instance.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Brightness {
        value: f32,
        monitor: Option<usize>,
    },
    Kelvin(u32),
    Scene(String),
    /// Minutes; 0 = until resumed.
    Pause(u32),
    Resume,
    /// Close the running instance (restoring the screens).
    Exit,
    /// Open the settings window (optionally on a page).
    Settings(Option<String>),
}

/// Parses the forwarded command line (e.g. `--set brightness=40 monitor=2 kelvin=3400`).
pub fn parse_forward(s: &str) -> Result<Vec<Request>, String> {
    let mut out = Vec::new();
    let mut words = s.split_whitespace().peekable();
    while let Some(w) = words.next() {
        match w {
            "--set" => {
                let mut monitor = None;
                let mut brightness = None;
                while let Some(kv) = words.next_if(|n| !n.starts_with("--")) {
                    let (k, v) = kv.split_once('=').ok_or_else(|| format!("expected key=value, got {kv}"))?;
                    match k.to_ascii_lowercase().as_str() {
                        "brightness" | "b" => {
                            brightness = Some(
                                v.trim_end_matches('%')
                                    .parse::<f32>()
                                    .map_err(|_| format!("bad brightness: {v}"))?,
                            )
                        }
                        "kelvin" | "k" => out.push(Request::Kelvin(
                            v.trim_end_matches(['k', 'K']).parse().map_err(|_| format!("bad kelvin: {v}"))?,
                        )),
                        "monitor" | "m" => {
                            let n: usize = v.parse().map_err(|_| format!("bad monitor: {v}"))?;
                            monitor = Some(n.checked_sub(1).ok_or("monitors are numbered from 1")?);
                        }
                        _ => return Err(format!("unknown setting: {k}")),
                    }
                }
                if let Some(value) = brightness {
                    out.push(Request::Brightness { value, monitor });
                }
            }
            "--scene" => out.push(Request::Scene(words.next().ok_or("--scene needs a name")?.to_string())),
            "--pause" => out.push(Request::Pause(
                words.next().ok_or("--pause needs minutes")?.parse().map_err(|_| "bad minutes")?,
            )),
            "--resume" => out.push(Request::Resume),
            "--exit" => out.push(Request::Exit),
            "--settings" => {
                out.push(Request::Settings(words.next_if(|n| !n.starts_with("--")).map(str::to_string)))
            }
            other => return Err(format!("unexpected: {other}")),
        }
    }
    Ok(out)
}

pub const HELP: &str = "\
Screen Lighting Control (SLC)

Usage:
  slc.exe                        start in the tray (or show the running instance)
  slc.exe --reset                restore neutral colors and full brightness, then exit
  slc.exe --install              install for the current user (Start menu, autostart)
  slc.exe --uninstall            remove the installed copy and restore the screens
  slc.exe --expand-range         (admin) allow warmer colors / deeper gamma dimming
  slc.exe --set brightness=40 [monitor=2] [kelvin=3400]
  slc.exe --scene <name>         apply a scene in the running instance
  slc.exe --pause <minutes>      pause the running instance (0 = until resumed)
  slc.exe --resume
  slc.exe --settings [page]      open settings (general, displays, schedule, scenes, hotkeys, about)
  slc.exe --exit                 close the running instance (restores the screens)
  slc.exe --log <file>           write a diagnostic log
  slc.exe --version
";

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Cli, String> {
        parse(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_to_run() {
        assert_eq!(p(&[]).unwrap().command, Command::Run);
    }

    #[test]
    fn parses_commands_case_insensitively() {
        assert_eq!(p(&["--RESET"]).unwrap().command, Command::Reset);
        assert_eq!(p(&["/?"]).unwrap().command, Command::Help);
    }

    #[test]
    fn rejects_two_commands_and_unknown() {
        assert!(p(&["--reset", "--install"]).is_err());
        assert!(p(&["--bogus"]).is_err());
    }

    #[test]
    fn log_and_minimized_flags() {
        let c = p(&["--minimized", "--log", "x.txt"]).unwrap();
        assert!(c.minimized);
        assert_eq!(c.log_file.as_deref(), Some("x.txt"));
    }

    #[test]
    fn parses_forwarded_requests() {
        let r = parse_forward("--set brightness=40% monitor=2 kelvin=3400K").unwrap();
        assert_eq!(r, vec![Request::Kelvin(3400), Request::Brightness { value: 40.0, monitor: Some(1) }]);
        assert_eq!(parse_forward("--pause 60").unwrap(), vec![Request::Pause(60)]);
        assert_eq!(parse_forward("--exit").unwrap(), vec![Request::Exit]);
        assert_eq!(p(&["--exit"]).unwrap().command, Command::Forward("--exit".into()));
        assert_eq!(parse_forward("--scene Night").unwrap(), vec![Request::Scene("Night".into())]);
        assert!(parse_forward("--set monitor=0 brightness=1").is_err());
        assert!(parse_forward("--set foo=1").is_err());
    }

    #[test]
    fn forwards_set_with_extra_pairs() {
        let c = p(&["--set", "brightness=40", "monitor=2"]).unwrap();
        assert_eq!(c.command, Command::Forward("--set brightness=40 monitor=2".into()));
        let c = p(&["--resume"]).unwrap();
        assert_eq!(c.command, Command::Forward("--resume".into()));
    }
}
