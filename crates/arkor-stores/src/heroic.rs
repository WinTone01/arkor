//! Heroic Games Launcher interop: reuse what Heroic already set up, and list the games
//! added to Heroic by hand ("sideloaded" apps, e.g. Ubisoft Connect games).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{Game, GameId, Store};
use serde::Deserialize;

use crate::target::ExeTarget;

const HEROIC_CONFIGS: &[&str] = &[
    ".config/heroic",
    ".var/app/com.heroicgameslauncher.hgl/config/heroic",
];

fn config_dirs() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    HEROIC_CONFIGS
        .iter()
        .map(|c| home.join(c))
        .filter(|p| p.is_dir())
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameConfig {
    #[serde(default)]
    wine_prefix: Option<PathBuf>,
    #[serde(default)]
    wine_version: Option<WineVersion>,
}

#[derive(Deserialize)]
struct WineVersion {
    #[serde(default)]
    bin: PathBuf,
    #[serde(default, rename = "type")]
    kind: String,
}

/// Heroic's per-game settings: `GamesConfig/<app_name>.json` = `{ "<app_name>": {...}, ... }`.
fn game_config(app_name: &str) -> Option<GameConfig> {
    config_dirs().into_iter().find_map(|dir| {
        let text =
            std::fs::read_to_string(dir.join("GamesConfig").join(format!("{app_name}.json")))
                .ok()?;
        let mut root: HashMap<String, serde_json::Value> = serde_json::from_str(&text).ok()?;
        serde_json::from_value(root.remove(app_name)?).ok()
    })
}

/// The Wine prefix Heroic uses for `app_name`. Launching with the same prefix keeps the
/// game's saves and settings. Kept even when its drive is not mounted right now, so the
/// game does not end up in a new, empty prefix.
pub fn wine_prefix(app_name: &str) -> Option<PathBuf> {
    game_config(app_name)?
        .wine_prefix
        .filter(|p| !p.as_os_str().is_empty())
}

/// The Proton build Heroic uses for `app_name`, if it is a Proton (not plain Wine).
pub fn proton(app_name: &str) -> Option<PathBuf> {
    let version = game_config(app_name)?.wine_version?;
    if version.kind != "proton" {
        return None;
    }
    let dir = version.bin.parent()?.to_path_buf();
    dir.join("proton").is_file().then_some(dir)
}

#[derive(Deserialize)]
struct SideloadLibrary {
    #[serde(default)]
    games: Vec<Sideload>,
}

#[derive(Deserialize)]
struct Sideload {
    app_name: String,
    title: String,
    #[serde(default)]
    is_installed: bool,
    install: SideloadInstall,
}

#[derive(Deserialize)]
struct SideloadInstall {
    executable: PathBuf,
    #[serde(default)]
    platform: String,
}

fn read_sideloads(path: &Path) -> anyhow::Result<Vec<Sideload>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let lib: SideloadLibrary =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(lib.games.into_iter().filter(|g| g.is_installed).collect())
}

fn sideload_files() -> Vec<PathBuf> {
    config_dirs()
        .into_iter()
        .map(|d| d.join("sideload_apps/library.json"))
        .filter(|p| p.is_file())
        .collect()
}

/// Games added to Heroic by hand.
pub fn sideload_games() -> (Vec<Game>, Vec<anyhow::Error>) {
    let mut games = Vec::new();
    let mut errors = Vec::new();
    for file in sideload_files() {
        match read_sideloads(&file) {
            Ok(list) => games.extend(list.into_iter().map(|g| Game {
                id: GameId::new(Store::Heroic, &g.app_name),
                // Heroic keeps titles as typed, trailing spaces included.
                name: g.title.trim().to_string(),
                install_dir: g.install.executable.parent().map(Path::to_path_buf),
                size_on_disk: None,
            })),
            Err(e) => errors.push(e),
        }
    }
    (games, errors)
}

pub fn sideload_target(app_name: &str) -> anyhow::Result<ExeTarget> {
    for file in sideload_files() {
        if let Some(game) = read_sideloads(&file)?
            .into_iter()
            .find(|g| g.app_name == app_name)
        {
            let mut target = ExeTarget::windows(game.install.executable);
            target.native = game.install.platform.eq_ignore_ascii_case("linux");
            target.prefix = wine_prefix(app_name);
            target.proton = proton(app_name);
            return Ok(target);
        }
    }
    anyhow::bail!("Heroic game `{app_name}` not found")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sideload_library() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("library.json");
        std::fs::write(
            &p,
            r#"{"games":[
              {"app_name":"a1","title":"Far Cry 6 ","is_installed":true,"runner":"sideload",
               "install":{"executable":"/pfx/drive_c/users/steamuser/Desktop/Far Cry 6.url","platform":"Windows","is_dlc":false},
               "folder_name":"/pfx/drive_c/users/steamuser/Desktop"},
              {"app_name":"a2","title":"Gone","is_installed":false,"install":{"executable":"/x.exe","platform":"Windows"}}]}"#,
        )
        .unwrap();
        let list = read_sideloads(&p).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Far Cry 6 ");
        assert_eq!(
            list[0].install.executable,
            PathBuf::from("/pfx/drive_c/users/steamuser/Desktop/Far Cry 6.url")
        );
    }
}
