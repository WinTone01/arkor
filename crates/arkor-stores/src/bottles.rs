//! Bottles (https://usebottles.com): the programs added to its bottles ("External programs"
//! in each `bottle.yml`), as Windows games in the bottle's own prefix. Bottles runs them with
//! its Wine runners; Arkor runs the same prefix through Proton.
//!
//! Ids are `<bottle>/<program uuid>`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{Game, GameId, Store};
use serde::Deserialize;

use crate::target::ExeTarget;

/// The folders holding the bottles (the package's and the Flatpak's).
fn roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    [
        home.join(".local/share/bottles/bottles"),
        home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
    ]
    .into_iter()
    .filter(|d| d.is_dir())
    .collect()
}

#[derive(Debug, Deserialize)]
struct Bottle {
    #[serde(rename = "External_Programs", default)]
    programs: std::collections::BTreeMap<String, Program>,
}

#[derive(Debug, Clone, Deserialize)]
struct Program {
    #[serde(default)]
    name: String,
    #[serde(default)]
    path: PathBuf,
    #[serde(default)]
    arguments: String,
    #[serde(default)]
    folder: Option<PathBuf>,
}

fn read(dir: &Path) -> Option<Bottle> {
    let text = std::fs::read_to_string(dir.join("bottle.yml")).ok()?;
    serde_yaml::from_str(&text).ok()
}

/// Every program of every bottle: (id, bottle folder, program).
fn programs() -> Vec<(String, PathBuf, Program)> {
    let mut out = Vec::new();
    for root in roots() {
        for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let dir = entry.path();
            let Some(bottle) = read(&dir) else { continue };
            let folder = entry.file_name().to_string_lossy().into_owned();
            for (uuid, p) in bottle.programs {
                if p.path.as_os_str().is_empty() {
                    continue;
                }
                out.push((format!("{folder}/{uuid}"), dir.clone(), p));
            }
        }
    }
    out
}

pub fn games() -> (Vec<Game>, Vec<anyhow::Error>) {
    let games = programs()
        .into_iter()
        .map(|(id, _, p)| Game {
            id: GameId::new(Store::Bottles, id),
            name: if p.name.is_empty() {
                p.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                p.name.clone()
            },
            install_dir: p.path.parent().map(Path::to_path_buf),
            size_on_disk: None,
        })
        .collect();
    (games, Vec::new())
}

pub fn launch_target(id: &str) -> anyhow::Result<ExeTarget> {
    let (dir, p) = programs()
        .into_iter()
        .find(|(i, _, _)| i == id)
        .map(|(_, d, p)| (d, p))
        .with_context(|| format!("Bottles has no program {id}"))?;
    let mut t = ExeTarget::windows(p.path.clone());
    t.args = arkor_core::profile::shell_words(&p.arguments);
    if let Some(folder) = p.folder.filter(|f| f.is_dir()) {
        t.working_dir = folder;
    }
    t.prefix = Some(dir);
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_bottle() {
        let yml = "Name: Games\nExternal_Programs:\n  1a2b:\n    executable: game.exe\n    name: Hollow Game\n    path: /b/drive_c/Games/game.exe\n    arguments: -dx11 -windowed\n";
        let b: Bottle = serde_yaml::from_str(yml).unwrap();
        let p = &b.programs["1a2b"];
        assert_eq!(p.name, "Hollow Game");
        assert_eq!(
            arkor_core::profile::shell_words(&p.arguments),
            ["-dx11", "-windowed"]
        );
    }
}
