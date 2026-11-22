//! Games imported into Arkor by hand from a Wine prefix, for anything no launcher knows
//! about. Stored as `~/.config/arkor/imported.toml`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{Game, GameId, Store, paths};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedGame {
    /// Stable id (the part after `local:`), unique within the file.
    pub id: String,
    pub name: String,
    pub prefix: PathBuf,
    pub exe: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Imported {
    #[serde(rename = "game")]
    pub games: Vec<ImportedGame>,
}

impl ImportedGame {
    pub fn to_game(&self) -> Game {
        Game {
            id: GameId::new(Store::Local, &self.id),
            name: self.name.clone(),
            install_dir: self.exe.parent().map(Path::to_path_buf),
            size_on_disk: None,
        }
    }
}

impl Imported {
    pub fn path() -> anyhow::Result<PathBuf> {
        Ok(paths::config_dir()?.join("imported.toml"))
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::profile::write_toml(&Self::path()?, self)
    }

    pub fn get(&self, id: &str) -> Option<&ImportedGame> {
        self.games.iter().find(|g| g.id == id)
    }

    /// Adds a game and returns its new id, derived from the name and made unique.
    pub fn add(&mut self, name: String, prefix: PathBuf, exe: PathBuf) -> String {
        let base: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base = if base.is_empty() {
            "game".to_string()
        } else {
            base
        };
        let mut id = base.clone();
        let mut n = 2;
        while self.get(&id).is_some() {
            id = format!("{base}-{n}");
            n += 1;
        }
        self.games.push(ImportedGame {
            id: id.clone(),
            name,
            prefix,
            exe,
            args: Vec::new(),
        });
        id
    }

    pub fn remove(&mut self, id: &str) {
        self.games.retain(|g| g.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_slugs() {
        let mut i = Imported::default();
        let a = i.add(
            "Far Cry® 5".into(),
            "/p".into(),
            "/p/drive_c/fc5.exe".into(),
        );
        let b = i.add(
            "Far Cry® 5".into(),
            "/p".into(),
            "/p/drive_c/fc5.exe".into(),
        );
        let c = i.add("™".into(), "/p".into(), "/p/x.exe".into());
        assert_eq!(
            (a.as_str(), b.as_str(), c.as_str()),
            ("far-cry-5", "far-cry-5-2", "game")
        );
        i.remove("far-cry-5");
        assert!(i.get("far-cry-5").is_none() && i.get("far-cry-5-2").is_some());
        let game = i.get("game").unwrap().to_game();
        assert_eq!(game.id.to_string(), "local:game");
        assert_eq!(game.install_dir, Some(PathBuf::from("/p")));
    }

    #[test]
    fn toml_roundtrip() {
        let mut i = Imported::default();
        i.add("A".into(), "/p".into(), "/p/a.exe".into());
        let text = toml::to_string(&i).unwrap();
        assert!(text.contains("[[game]]"), "{text}");
        assert_eq!(toml::from_str::<Imported>(&text).unwrap(), i);
    }
}
