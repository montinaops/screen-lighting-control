//! Minimal INI reader/writer (ordered, comment-tolerant) and settings-file locations.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ini {
    sections: Vec<(String, Vec<(String, String)>)>,
}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        let mut ini = Ini::default();
        let mut current = String::new();
        for raw in text.lines() {
            let line = raw.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                current = name.trim().to_string();
                ini.section_mut(&current);
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                // Strip trailing " ; comment" (values never contain " ;").
                let v = v.split(" ;").next().unwrap_or("").trim();
                ini.set(&current, k.trim(), v);
            }
        }
        ini
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (name, pairs) in &self.sections {
            if !name.is_empty() {
                if !out.is_empty() {
                    out.push_str("\r\n");
                }
                out.push_str(&format!("[{name}]\r\n"));
            }
            for (k, v) in pairs {
                out.push_str(&format!("{k}={v}\r\n"));
            }
        }
        out
    }

    fn section_mut(&mut self, name: &str) -> &mut Vec<(String, String)> {
        let idx = match self.sections.iter().position(|(n, _)| n.eq_ignore_ascii_case(name)) {
            Some(i) => i,
            None => {
                self.sections.push((name.to_string(), Vec::new()));
                self.sections.len() - 1
            }
        };
        &mut self.sections[idx].1
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(section))?
            .1
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, section: &str, key: &str, value: impl ToString) {
        let value = value.to_string();
        let s = self.section_mut(section);
        match s.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some(pair) => pair.1 = value,
            None => s.push((key.to_string(), value)),
        }
    }

    pub fn get_parse<T: std::str::FromStr>(&self, section: &str, key: &str) -> Option<T> {
        self.get(section, key)?.parse().ok()
    }

    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        match self.get(section, key)?.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    }

    /// Names of sections starting with `prefix` (e.g. "scene."), prefix removed, in file order.
    pub fn sections_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.sections
            .iter()
            .filter(|(n, _)| n.len() > prefix.len() && n[..prefix.len()].eq_ignore_ascii_case(prefix))
            .map(|(n, _)| n[prefix.len()..].to_string())
            .collect()
    }

    pub fn load(path: &Path) -> Option<Ini> {
        std::fs::read_to_string(path).ok().map(|t| Ini::parse(&t))
    }

    /// Atomic write: temp file + rename (replaces the old file in one step).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("ini.tmp");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, path)
    }
}

/// Where settings live.
#[derive(Clone, Debug, PartialEq)]
pub struct Paths {
    pub settings: PathBuf,
    pub state: PathBuf,
    pub portable: bool,
}

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("slc.exe"))
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// `%LOCALAPPDATA%\Programs\SLC` — where `--install` puts the exe.
pub fn install_dir() -> Option<PathBuf> {
    env_dir("LOCALAPPDATA").map(|d| d.join("Programs").join("SLC"))
}

/// `%APPDATA%\SLC` — settings of the installed copy.
pub fn roaming_dir() -> Option<PathBuf> {
    env_dir("APPDATA").map(|d| d.join("SLC"))
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".slc-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Decides portable vs installed (PRODUCT §10).
pub fn resolve() -> Paths {
    let exe = exe_path();
    let exe_dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let roaming = roaming_dir();
    let installed = install_dir().is_some_and(|d| exe_dir.eq(&d));
    let portable = !installed
        && (exe_dir.join("slc.ini").exists()
            || (!roaming.as_ref().is_some_and(|r| r.join("slc.ini").exists()) && is_writable(&exe_dir)));
    let dir = if portable || roaming.is_none() { exe_dir } else { roaming.unwrap_or_default() };
    Paths { settings: dir.join("slc.ini"), state: dir.join("state.ini"), portable }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}; comment\r\n[General]\r\nautostart = 1 ; trailing\r\n\r\n[scene.Night]\r\nbrightness=35\r\nkelvin=2700\r\n[scene.Reading]\r\nkelvin=4200\r\n";

    #[test]
    fn parse_get_set() {
        let mut ini = Ini::parse(SAMPLE);
        assert_eq!(ini.get("general", "AUTOSTART"), Some("1"));
        assert_eq!(ini.get_bool("general", "autostart"), Some(true));
        assert_eq!(ini.get_parse::<u32>("scene.Night", "kelvin"), Some(2700));
        assert_eq!(ini.get("missing", "x"), None);
        ini.set("general", "autostart", 0);
        ini.set("new", "k", "v");
        assert_eq!(ini.get_bool("general", "autostart"), Some(false));
        assert_eq!(ini.get("new", "k"), Some("v"));
    }

    #[test]
    fn roundtrip_and_prefix() {
        let ini = Ini::parse(SAMPLE);
        let again = Ini::parse(&ini.to_text());
        assert_eq!(ini, again);
        assert_eq!(ini.sections_with_prefix("scene."), vec!["Night".to_string(), "Reading".to_string()]);
    }

    #[test]
    fn atomic_save() {
        let dir = std::env::temp_dir().join(format!("slc-test-{}", std::process::id()));
        let p = dir.join("t.ini");
        let mut ini = Ini::default();
        ini.set("a", "b", "c");
        ini.save(&p).unwrap();
        assert_eq!(Ini::load(&p).unwrap().get("a", "b"), Some("c"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
