//! Epic Games Store through legendary (https://github.com/derrod/legendary).
//!
//! Arkor does not talk to Epic itself: it reads legendary's own files and launches games
//! through `legendary launch`, which handles login and ownership tokens. It works with
//! Arkor's own legendary (see `arkor_launch::shops`), with a standalone one and with the
//! copy Heroic bundles.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{Game, GameId, Store};
use serde::Deserialize;

/// A usable legendary: the binary plus the config dir holding login and library data.
#[derive(Debug, Clone)]
pub struct Legendary {
    pub bin: PathBuf,
    pub config_dir: PathBuf,
}

/// Where Heroic installs and keeps its bundled legendary (native package, then Flatpak).
const HEROIC_ROOTS: &[(&str, &str)] = &[
    ("/usr/lib/heroic-games-launcher", ".config/heroic"),
    ("/opt/Heroic/resources", ".config/heroic"),
    (
        "/var/lib/flatpak/app/com.heroicgameslauncher.hgl/current/active/files/bin/heroic/resources",
        ".var/app/com.heroicgameslauncher.hgl/config/heroic",
    ),
];
const HEROIC_BIN_DIR: &str = "app.asar.unpacked/build/bin/x64/linux";

/// A helper Heroic ships (`legendary`, `gogdl`, `nile`), if Heroic is installed.
pub fn heroic_bin(name: &str) -> Option<PathBuf> {
    HEROIC_ROOTS
        .iter()
        .map(|(root, _)| Path::new(root).join(HEROIC_BIN_DIR).join(name))
        .find(|p| p.is_file())
}

impl Legendary {
    /// Finds legendary, preferring a config dir that has a logged-in user.
    pub fn locate() -> Option<Self> {
        let mut candidates = Self::all();
        let logged_in = candidates.iter().position(Self::is_logged_in);
        match logged_in {
            Some(ix) => Some(candidates.swap_remove(ix)),
            None => candidates.into_iter().next(),
        }
    }

    /// The legendary that installed `app_name` (each config dir has its own games), else
    /// the one [`Legendary::locate`] finds.
    pub fn locate_for(app_name: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|l| l.installed().is_ok_and(|g| g.contains_key(app_name)))
            .or_else(Self::locate)
    }

    /// Every usable legendary, in order: Arkor's own, `$LEGENDARY_CONFIG_PATH`, standalone
    /// `~/.config/legendary`, then Heroic's.
    pub fn all() -> Vec<Self> {
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        let path_bin = find_in_path("legendary");
        let mut candidates: Vec<Self> = Vec::new();

        if let Ok(dir) = arkor_core::paths::stores_dir()
            && let Some(bin) = crate::helper_bin("legendary")
                .or_else(|| path_bin.clone())
                .or_else(|| heroic_bin("legendary"))
        {
            candidates.push(Self {
                bin,
                config_dir: dir.join("epic"),
            });
        }

        if let (Some(dir), Some(bin)) = (std::env::var_os("LEGENDARY_CONFIG_PATH"), &path_bin) {
            candidates.push(Self {
                bin: bin.clone(),
                config_dir: dir.into(),
            });
        }
        if let (Some(cfg), Some(bin)) = (dirs::config_dir(), &path_bin) {
            candidates.push(Self {
                bin: bin.clone(),
                config_dir: cfg.join("legendary"),
            });
        }
        for (resources, config) in HEROIC_ROOTS {
            let bundled = Path::new(resources).join(HEROIC_BIN_DIR).join("legendary");
            let bin = if bundled.is_file() {
                Some(bundled)
            } else {
                path_bin.clone()
            };
            if let Some(bin) = bin {
                candidates.push(Self {
                    bin,
                    config_dir: home.join(config).join("legendaryConfig/legendary"),
                });
            }
        }

        candidates.retain(|c| c.config_dir.is_dir());
        let mut seen = std::collections::BTreeSet::new();
        candidates.retain(|c| seen.insert(c.config_dir.clone()));
        candidates
    }

    pub fn is_logged_in(&self) -> bool {
        self.config_dir.join("user.json").is_file()
    }

    /// Installed games (DLC skipped). Owned-but-not-installed games are not listed:
    /// Arkor only shows what can be played right now.
    pub fn games(&self) -> anyhow::Result<Vec<Game>> {
        Ok(self
            .installed()?
            .into_iter()
            .filter(|(_, inst)| !inst.is_dlc)
            .map(|(app_name, inst)| Game {
                id: GameId::new(Store::Epic, &app_name),
                name: inst.title,
                install_dir: Some(inst.install_path),
                size_on_disk: inst.install_size,
            })
            .collect())
    }

    fn installed(&self) -> anyhow::Result<BTreeMap<String, Installed>> {
        let path = self.config_dir.join("installed.json");
        if !path.is_file() {
            return Ok(BTreeMap::new());
        }
        read_json(&path)
    }

    /// argv + env for `legendary launch`. `wrapper` is prepended by legendary to the game's
    /// command line, so Arkor can sit between legendary (auth) and the game (umu/Proton).
    pub fn launch_command(
        &self,
        app_name: &str,
        wrapper: &[String],
    ) -> (Vec<String>, BTreeMap<String, String>) {
        let argv = vec![
            self.bin.display().to_string(),
            "launch".into(),
            app_name.into(),
            // Arkor's wrapper runs umu-run, which brings its own Proton.
            "--no-wine".into(),
            "--wrapper".into(),
            shell_join(wrapper),
        ];
        let env = [(
            "LEGENDARY_CONFIG_PATH".to_string(),
            self.config_dir.display().to_string(),
        )]
        .into();
        (argv, env)
    }
}

#[derive(Deserialize)]
struct Installed {
    title: String,
    install_path: PathBuf,
    #[serde(default)]
    install_size: Option<u64>,
    #[serde(default)]
    is_dlc: bool,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Quotes arguments for a POSIX-shell-like splitter (legendary uses Python's `shlex.split`).
pub fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            let safe = !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c));
            if safe {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn lists_installed_games_only() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path();
        // Owned-but-not-installed games live in metadata/ and must not be listed.
        write(
            cfg,
            "metadata/a.json",
            r#"{"app_name":"Fish","app_title":"Shell Game"}"#,
        );
        write(
            cfg,
            "installed.json",
            r#"{"Owl":{"app_name":"Owl","title":"Owl Game","install_path":"/games/Owl","install_size":1024,"is_dlc":false,"executable":"Owl.exe"},
                "OwlDLC":{"app_name":"OwlDLC","title":"Owl DLC","install_path":"/games/Owl","is_dlc":true}}"#,
        );

        let leg = Legendary {
            bin: "/bin/legendary".into(),
            config_dir: cfg.into(),
        };
        let games = leg.games().unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(
            (games[0].id.to_string(), games[0].name.as_str()),
            ("epic:Owl".to_string(), "Owl Game")
        );
        assert_eq!(games[0].size_on_disk, Some(1024));
    }

    #[test]
    fn launch_command_quotes_wrapper() {
        let leg = Legendary {
            bin: "/bin/legendary".into(),
            config_dir: "/cfg".into(),
        };
        let wrapper =
            ["/opt/my arkor/arkor", "run", "epic:Fish", "--", "umu-run"].map(String::from);
        let (argv, env) = leg.launch_command("Fish", &wrapper);
        assert_eq!(
            argv[..5],
            ["/bin/legendary", "launch", "Fish", "--no-wine", "--wrapper"]
        );
        assert_eq!(argv[5], "'/opt/my arkor/arkor' run epic:Fish -- umu-run");
        assert_eq!(env["LEGENDARY_CONFIG_PATH"], "/cfg");
    }

    #[test]
    fn shell_join_escapes_quotes() {
        assert_eq!(
            shell_join(&["it's".into(), "".into(), "ok".into()]),
            r#"'it'\''s' '' ok"#
        );
    }
}
