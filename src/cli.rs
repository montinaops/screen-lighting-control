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
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut cli = Cli { command: Command::Run, log_file: None, minimized: false };
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
            "--log" => {
                cli.log_file = Some(it.next().ok_or("--log needs a file path")?);
            }
            "--set" | "--scene" | "--pause" | "--resume" => {
                forward.push(a_l.clone());
                if a_l != "--resume" {
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
    fn forwards_set_with_extra_pairs() {
        let c = p(&["--set", "brightness=40", "monitor=2"]).unwrap();
        assert_eq!(c.command, Command::Forward("--set brightness=40 monitor=2".into()));
        let c = p(&["--resume"]).unwrap();
        assert_eq!(c.command, Command::Forward("--resume".into()));
    }
}
