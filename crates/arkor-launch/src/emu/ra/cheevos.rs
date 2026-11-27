//! Signing emulators in to RetroAchievements (ported from Irori's `cheevos`), by writing
//! the settings each one documents for it:
//!
//! - RetroArch: `cheevos_*` in the `retroarch.cfg` Arkor runs it with.
//! - PCSX2: `[Achievements]` in `inis/PCSX2.ini`, the token in `inis/secrets.ini`.
//! - PPSSPP: `[Achievements]` in `PSP/SYSTEM/ppsspp.ini`, the token beside it.
//! - Dolphin: `[Achievements]` in `Config/RetroAchievements.ini`.
//! - DuckStation keeps its token encrypted with a key of this computer, so Arkor only turns
//!   achievements on and the user signs in there.
//!
//! The token is the one RetroAchievements gives emulators at sign-in (not the Web API key).
//! Existing settings are kept; only these keys change.

use std::path::{Path, PathBuf};

use anyhow::Context;

use super::super::discovery::{Installation, Origin};

/// Emulators Arkor can sign in or turn achievements on for.
pub const SUPPORTED: [&str; 5] = ["retroarch", "pcsx2", "ppsspp", "dolphin", "duckstation"];

/// The account an emulator signs in with.
#[derive(Clone, PartialEq, Eq)]
pub struct Login {
    pub username: String,
    pub token: String,
    /// Hardcore mode: no save states, rewinding or cheats.
    pub hardcore: bool,
}

impl std::fmt::Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Login")
            .field("username", &self.username)
            .field("hardcore", &self.hardcore)
            .finish_non_exhaustive()
    }
}

/// What signing an emulator in did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    SignedIn(PathBuf),
    /// Achievements turned on; the user signs in inside the emulator.
    Enabled(PathBuf),
}

/// Folders the emulators keep their settings under.
#[derive(Debug, Clone)]
pub struct Roots {
    pub home: PathBuf,
    pub xdg_config: PathBuf,
    pub retroarch_home: PathBuf,
}

impl Roots {
    pub fn current() -> Self {
        let home = ::dirs::home_dir().unwrap_or_default();
        let xdg_config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        Self {
            xdg_config,
            retroarch_home: super::super::retroarch::home(),
            home,
        }
    }

    fn flatpak_config(&self, app: &str) -> PathBuf {
        self.home.join(".var/app").join(app).join("config")
    }
}

fn flatpak(install: &Installation) -> bool {
    install.origin == Origin::Flatpak
}

/// Set `key = value` in `[section]` of an INI text, keeping everything
/// else; the section is added when missing.
pub fn set_ini(text: &str, section: &str, pairs: &[(&str, String)]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = format!("[{section}]");
    let start = match lines.iter().position(|l| l.trim() == header) {
        Some(i) => i,
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.len() - 1
        }
    };
    for (key, value) in pairs {
        let end = lines[start + 1..]
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .map_or(lines.len(), |p| start + 1 + p);
        let line = format!("{key} = {value}");
        let found = lines[start + 1..end].iter().position(|l| {
            l.split_once('=')
                .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
        });
        match found {
            Some(i) => lines[start + 1 + i] = line,
            None => {
                // After the section's last setting, before blank lines.
                let mut at = end;
                while at > start + 1 && lines[at - 1].trim().is_empty() {
                    at -= 1;
                }
                lines.insert(at, line);
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn update_ini(path: &Path, section: &str, pairs: &[(&str, String)]) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let text = set_ini(&text, section, pairs);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn write_private(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    f.write_all(data)?;
    Ok(())
}

fn flag(on: bool, title: bool) -> String {
    match (title, on) {
        (false, true) => "true".into(),
        (false, false) => "false".into(),
        (true, true) => "True".into(),
        (true, false) => "False".into(),
    }
}

/// Signs `engine` (installed as `install`) in with `login`. Blocking: writes files.
pub fn sign_in(
    engine: &str,
    install: &Installation,
    login: &Login,
    roots: &Roots,
) -> anyhow::Result<Outcome> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string();
    let user = login.username.trim().to_string();
    match engine {
        "retroarch" => {
            let cfg = roots.retroarch_home.join("retroarch.cfg");
            super::super::retroarch::write_config(
                &cfg,
                &[
                    ("cheevos_enable", "true".into()),
                    ("cheevos_username", user),
                    ("cheevos_token", login.token.clone()),
                    // RetroArch signs in with the token; a stale password would replace it.
                    ("cheevos_password", String::new()),
                    ("cheevos_hardcore_mode_enable", flag(login.hardcore, false)),
                ],
            )?;
            Ok(Outcome::SignedIn(cfg))
        }
        "pcsx2" => {
            let root = if flatpak(install) {
                roots.flatpak_config("net.pcsx2.PCSX2").join("PCSX2")
            } else {
                roots.xdg_config.join("PCSX2")
            };
            let inis = root.join("inis");
            let main = inis.join("PCSX2.ini");
            update_ini(
                &main,
                "Achievements",
                &[
                    ("Enabled", flag(true, false)),
                    ("ChallengeMode", flag(login.hardcore, false)),
                    ("Username", user),
                    ("LoginTimestamp", now),
                ],
            )?;
            let secrets = inis.join("secrets.ini");
            let text = set_ini(
                &std::fs::read_to_string(&secrets).unwrap_or_default(),
                "Achievements",
                &[("Token", login.token.clone())],
            );
            write_private(&secrets, text.as_bytes())?;
            Ok(Outcome::SignedIn(main))
        }
        "ppsspp" => {
            let root = if flatpak(install) {
                roots.flatpak_config("org.ppsspp.PPSSPP").join("ppsspp")
            } else {
                roots.xdg_config.join("ppsspp")
            };
            let system = root.join("PSP").join("SYSTEM");
            let ini = system.join("ppsspp.ini");
            update_ini(
                &ini,
                "Achievements",
                &[
                    ("AchievementsEnable", flag(true, true)),
                    ("AchievementsChallengeMode", flag(login.hardcore, true)),
                    ("AchievementsUserName", user),
                ],
            )?;
            write_private(
                &system.join("ppsspp_retroachievements.dat"),
                login.token.as_bytes(),
            )?;
            Ok(Outcome::SignedIn(ini))
        }
        "dolphin" => {
            let dir = if flatpak(install) {
                roots
                    .flatpak_config("org.DolphinEmu.dolphin-emu")
                    .join("dolphin-emu")
            } else {
                roots.xdg_config.join("dolphin-emu")
            };
            let ini = dir.join("RetroAchievements.ini");
            let text = set_ini(
                &std::fs::read_to_string(&ini).unwrap_or_default(),
                "Achievements",
                &[
                    ("Enabled", flag(true, true)),
                    ("HardcoreEnabled", flag(login.hardcore, true)),
                    ("Username", user),
                    ("ApiToken", login.token.clone()),
                ],
            );
            write_private(&ini, text.as_bytes())?;
            Ok(Outcome::SignedIn(ini))
        }
        "duckstation" => {
            let root = if flatpak(install) {
                roots
                    .flatpak_config("org.duckstation.DuckStation")
                    .join("duckstation")
            } else if std::env::var_os("XDG_CONFIG_HOME").is_some() {
                // DuckStation reads XDG_CONFIG_HOME only when it is set.
                roots.xdg_config.join("duckstation")
            } else {
                roots.home.join(".local/share/duckstation")
            };
            let ini = root.join("settings.ini");
            update_ini(
                &ini,
                "Cheevos",
                &[
                    ("Enabled", flag(true, false)),
                    ("ChallengeMode", flag(login.hardcore, false)),
                ],
            )?;
            Ok(Outcome::Enabled(ini))
        }
        other => anyhow::bail!("Arkor can't sign {other} in to RetroAchievements"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_emulators_in_keeping_their_other_settings() {
        let root = super::super::super::test_root("cheevos");
        let roots = Roots {
            home: root.join("home"),
            xdg_config: root.join("config"),
            retroarch_home: root.join("ra"),
        };
        let install = Installation {
            engine: "x".into(),
            origin: Origin::System,
            version: None,
            program: "/usr/bin/x".into(),
            flatpak: None,
            dir: None,
        };
        let login = Login {
            username: "me".into(),
            token: "tok".into(),
            hardcore: true,
        };
        std::fs::create_dir_all(root.join("config/PCSX2/inis")).unwrap();
        std::fs::write(
            root.join("config/PCSX2/inis/PCSX2.ini"),
            "[UI]\nTheme = dark\n\n[Achievements]\nEnabled = false\n",
        )
        .unwrap();
        sign_in("pcsx2", &install, &login, &roots).unwrap();
        let ini = std::fs::read_to_string(root.join("config/PCSX2/inis/PCSX2.ini")).unwrap();
        assert!(ini.contains("Theme = dark") && ini.contains("Enabled = true"));
        assert!(ini.contains("Username = me") && ini.contains("ChallengeMode = true"));
        let secrets = std::fs::read_to_string(root.join("config/PCSX2/inis/secrets.ini")).unwrap();
        assert!(secrets.contains("Token = tok"));
        sign_in("retroarch", &install, &login, &roots).unwrap();
        let cfg = std::fs::read_to_string(root.join("ra/retroarch.cfg")).unwrap();
        assert!(cfg.contains("cheevos_token = \"tok\""));
        assert!(matches!(
            sign_in("duckstation", &install, &login, &roots).unwrap(),
            Outcome::Enabled(_)
        ));
        assert!(sign_in("mgba", &install, &login, &roots).is_err());
    }
}
