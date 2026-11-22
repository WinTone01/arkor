//! Epic Games Store through legendary (https://github.com/derrod/legendary).
//!
//! Arkor does not talk to Epic itself: it reads legendary's own files and launches games
//! through `legendary launch`, which handles login and ownership tokens. It works with a
//! standalone legendary and with the copy Heroic bundles, sharing Heroic's login.

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
const HEROIC_LEGENDARY_BIN: &str = "app.asar.unpacked/build/bin/x64/linux/legendary";

impl Legendary {
    /// Finds legendary, preferring a config dir that has a logged-in user.
    /// Order: `$LEGENDARY_CONFIG_PATH`, standalone `~/.config/legendary`, then Heroic's.
    pub fn locate() -> Option<Self> {
        let home = dirs::home_dir()?;
        let path_bin = find_in_path("legendary");
        let mut candidates: Vec<Self> = Vec::new();

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
            let bundled = Path::new(resources).join(HEROIC_LEGENDARY_BIN);
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
        let logged_in = candidates
            .iter()
            .position(|c| c.config_dir.join("user.json").is_file());
        match logged_in {
            Some(ix) => Some(candidates.swap_remove(ix)),
            None => candidates.into_iter().next(),
        }
    }

    pub fn is_logged_in(&self) -> bool {
        self.config_dir.join("user.json").is_file()
    }

    /// Owned base games (DLC and non-game assets skipped), installed ones with their path.
    pub fn games(&self) -> anyhow::Result<Vec<Game>> {
        let installed = self.installed()?;
        let mut games: BTreeMap<String, Game> = BTreeMap::new();

        let metadata_dir = self.config_dir.join("metadata");
        if let Ok(entries) = std::fs::read_dir(&metadata_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "json") {
                    continue;
                }
                // One unreadable entry should not hide the rest of the library.
                let Ok(meta) = read_json::<Metadata>(&path) else {
                    continue;
                };
                if !meta.is_base_game() {
                    continue;
                }
                games.insert(
                    meta.app_name.clone(),
                    Game {
                        id: GameId::new(Store::Epic, &meta.app_name),
                        name: meta.app_title,
                        install_dir: None,
                        size_on_disk: None,
                    },
                );
            }
        }

        // Installed entries win: they exist even if metadata is missing (e.g. offline install).
        for (app_name, inst) in installed {
            if inst.is_dlc {
                continue;
            }
            games.insert(
                app_name.clone(),
                Game {
                    id: GameId::new(Store::Epic, &app_name),
                    name: inst.title,
                    install_dir: Some(inst.install_path),
                    size_on_disk: inst.install_size,
                },
            );
        }
        Ok(games.into_values().collect())
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

#[derive(Deserialize)]
struct Metadata {
    app_name: String,
    app_title: String,
    #[serde(default)]
    metadata: MetadataInner,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct MetadataInner {
    #[serde(default)]
    main_game_item: Option<serde_json::Value>,
    #[serde(default)]
    categories: Vec<Category>,
}

#[derive(Deserialize)]
struct Category {
    path: String,
}

impl Metadata {
    fn is_base_game(&self) -> bool {
        self.metadata.main_game_item.is_none()
            && self.metadata.categories.iter().any(|c| c.path == "games")
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

pub(crate) fn find_in_path(name: &str) -> Option<PathBuf> {
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
    fn reads_library_and_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path();
        write(
            cfg,
            "metadata/a.json",
            r#"{"app_name":"Fish","app_title":"Shell Game","metadata":{"categories":[{"path":"games"},{"path":"applications"}]}}"#,
        );
        write(
            cfg,
            "metadata/b.json",
            r#"{"app_name":"FishDLC","app_title":"DLC","metadata":{"mainGameItem":{"id":"x"},"categories":[{"path":"games"}]}}"#,
        );
        write(
            cfg,
            "metadata/c.json",
            r#"{"app_name":"Asset","app_title":"UE asset","metadata":{"categories":[{"path":"assets"}]}}"#,
        );
        write(cfg, "metadata/broken.json", "{");
        write(
            cfg,
            "installed.json",
            r#"{"Owl":{"app_name":"Owl","title":"Owl Game","install_path":"/games/Owl","install_size":1024,"is_dlc":false,"executable":"Owl.exe"}}"#,
        );

        let leg = Legendary {
            bin: "/bin/legendary".into(),
            config_dir: cfg.into(),
        };
        let games = leg.games().unwrap();
        let names: Vec<_> = games
            .iter()
            .map(|g| (g.id.to_string(), g.name.as_str(), g.is_installed()))
            .collect();
        assert_eq!(
            names,
            [
                ("epic:Fish".to_string(), "Shell Game", false),
                ("epic:Owl".to_string(), "Owl Game", true)
            ]
        );
        assert_eq!(games[1].size_on_disk, Some(1024));
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
