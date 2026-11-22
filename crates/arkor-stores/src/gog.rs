//! GOG. Games are DRM-free, so once installed Arkor launches them directly (no client).
//!
//! Installed games are read from Heroic's `gog_store/installed.json`. What to run comes
//! from GOG's own `goggame-<id>.info` in the install dir, so it does not depend on Heroic.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{Game, GameId, Store};
use serde::Deserialize;

const HEROIC_CONFIGS: &[&str] = &[
    ".config/heroic",
    ".var/app/com.heroicgameslauncher.hgl/config/heroic",
];

#[derive(Deserialize)]
struct InstalledFile {
    #[serde(default)]
    installed: Vec<Installed>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Installed {
    app_name: String,
    #[serde(rename = "install_path")]
    install_path: PathBuf,
    #[serde(default)]
    platform: String,
    #[serde(default, rename = "is_dlc")]
    is_dlc: bool,
}

/// What to execute for an installed GOG game.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchTarget {
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    /// Linux build: run directly instead of through umu/Proton.
    pub native: bool,
}

fn installed_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    HEROIC_CONFIGS
        .iter()
        .map(|c| home.join(c).join("gog_store/installed.json"))
        .filter(|p| p.is_file())
        .collect()
}

fn read_installed(path: &Path) -> anyhow::Result<Vec<Installed>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file: InstalledFile =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(file.installed.into_iter().filter(|g| !g.is_dlc).collect())
}

/// All installed GOG games. Errors are per source file and do not stop the others.
pub fn installed_games() -> (Vec<Game>, Vec<anyhow::Error>) {
    let mut games = Vec::new();
    let mut errors = Vec::new();
    for file in installed_files() {
        match read_installed(&file) {
            Ok(list) => games.extend(list.iter().map(to_game)),
            Err(e) => errors.push(e),
        }
    }
    (games, errors)
}

fn to_game(inst: &Installed) -> Game {
    // Windows builds carry goggame-<id>.info; Linux builds a `gameinfo` file whose first
    // line is the title.
    let name = read_info(&inst.install_path, &inst.app_name)
        .map(|i| i.name)
        .ok()
        .or_else(|| {
            let text = std::fs::read_to_string(inst.install_path.join("gameinfo")).ok()?;
            text.lines()
                .next()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| inst.app_name.clone());
    Game {
        id: GameId::new(Store::Gog, &inst.app_name),
        name,
        install_dir: Some(inst.install_path.clone()),
        size_on_disk: None,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    name: String,
    #[serde(default)]
    play_tasks: Vec<PlayTask>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayTask {
    #[serde(default)]
    is_primary: bool,
    #[serde(default)]
    category: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    arguments: String,
    #[serde(default)]
    working_dir: String,
}

fn read_info(install_dir: &Path, app_name: &str) -> anyhow::Result<Info> {
    let path = install_dir.join(format!("goggame-{app_name}.info"));
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Resolves what to run for the installed game `app_name`.
pub fn launch_target(app_name: &str) -> anyhow::Result<LaunchTarget> {
    for file in installed_files() {
        if let Some(inst) = read_installed(&file)?
            .into_iter()
            .find(|g| g.app_name == app_name)
        {
            return resolve(&inst);
        }
    }
    anyhow::bail!("GOG game `{app_name}` is not installed")
}

fn resolve(inst: &Installed) -> anyhow::Result<LaunchTarget> {
    let dir = &inst.install_path;
    if inst.platform == "linux" {
        // GOG Linux installers ship a start.sh that sets up the game's environment.
        let start = dir.join("start.sh");
        anyhow::ensure!(start.is_file(), "{} not found", start.display());
        return Ok(LaunchTarget {
            exe: start,
            args: Vec::new(),
            working_dir: dir.clone(),
            native: true,
        });
    }
    let info = read_info(dir, &inst.app_name)?;
    let task = info
        .play_tasks
        .iter()
        .filter(|t| t.kind == "FileTask" && !t.path.is_empty())
        .find(|t| t.is_primary)
        .or_else(|| {
            info.play_tasks
                .iter()
                .find(|t| t.kind == "FileTask" && t.category == "game")
        })
        .context("no playable task in goggame .info")?;
    // GOG paths are Windows-style and relative to the install dir.
    let rel = |p: &str| dir.join(p.replace('\\', "/"));
    Ok(LaunchTarget {
        exe: rel(&task.path),
        args: split_args(&task.arguments),
        working_dir: if task.working_dir.is_empty() {
            dir.clone()
        } else {
            rel(&task.working_dir)
        },
        native: false,
    })
}

/// GOG task arguments are a single Windows-style string; split on spaces outside quotes.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_primary_windows_task() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::write(
            dir.join("goggame-1207658924.info"),
            r#"{"gameId":"1207658924","name":"Some Game","playTasks":[
                {"category":"document","type":"FileTask","path":"manual.pdf"},
                {"isPrimary":true,"category":"game","type":"FileTask","path":"bin\\Game.exe",
                 "arguments":"-windowed \"-profile=a b\"","workingDir":"bin"}]}"#,
        )
        .unwrap();
        let inst = Installed {
            app_name: "1207658924".into(),
            install_path: dir.clone(),
            platform: "windows".into(),
            is_dlc: false,
        };

        assert_eq!(to_game(&inst).name, "Some Game");
        let t = resolve(&inst).unwrap();
        assert_eq!(t.exe, dir.join("bin/Game.exe"));
        assert_eq!(t.args, ["-windowed", "-profile=a b"]);
        assert_eq!(t.working_dir, dir.join("bin"));
        assert!(!t.native);
    }

    #[test]
    fn linux_build_uses_start_sh() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("start.sh"), "#!/bin/sh\n").unwrap();
        let inst = Installed {
            app_name: "1".into(),
            install_path: tmp.path().into(),
            platform: "linux".into(),
            is_dlc: false,
        };
        std::fs::write(tmp.path().join("gameinfo"), "Native Game\n1.0\n").unwrap();
        assert_eq!(to_game(&inst).name, "Native Game");
        let t = resolve(&inst).unwrap();
        assert!(t.native);
        assert_eq!(t.exe, tmp.path().join("start.sh"));
    }

    #[test]
    fn parses_heroic_installed_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("installed.json");
        std::fs::write(
            &p,
            r#"{"installed":[
              {"platform":"windows","executable":"","install_path":"/g/A","install_size":"1 GiB","is_dlc":false,"version":"1","appName":"111","installedWithDLCs":false,"language":"en-US"},
              {"platform":"windows","install_path":"/g/A","is_dlc":true,"appName":"222"}]}"#,
        )
        .unwrap();
        let list = read_installed(&p).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].app_name, "111");
        assert_eq!(list[0].install_path, PathBuf::from("/g/A"));
    }
}
