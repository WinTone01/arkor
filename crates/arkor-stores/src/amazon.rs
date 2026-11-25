//! Amazon Games through nile (https://github.com/imLinguin/nile).
//!
//! nile keeps what is installed in `installed.json` and what the account owns in
//! `library.json`. Games start through `nile launch`, which reads the game's `fuel.json`
//! and gives it the Amazon SDK environment it expects.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use arkor_core::{Game, GameId, Store};
use serde::Deserialize;

use crate::epic::{find_in_path, heroic_bin, shell_join};

/// A usable nile: the binary plus its config folder (`$NILE_CONFIG_PATH`; nile keeps its
/// files in the `nile` folder in it).
#[derive(Debug, Clone)]
pub struct Nile {
    pub bin: PathBuf,
    pub config_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Installed {
    pub id: String,
    pub path: PathBuf,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Deserialize)]
struct Entry {
    product: Product,
}

#[derive(Deserialize)]
struct Product {
    id: String,
    #[serde(default)]
    title: Option<String>,
}

impl Nile {
    /// Every usable nile: Arkor's own, then Heroic's.
    pub fn all() -> Vec<Self> {
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        let Some(bin) = crate::helper_bin("nile")
            .or_else(|| find_in_path("nile"))
            .or_else(|| heroic_bin("nile"))
        else {
            return Vec::new();
        };
        arkor_core::paths::stores_dir()
            .map(|d| d.join("amazon"))
            .into_iter()
            .chain(
                [
                    ".config/heroic/nile_config",
                    ".var/app/com.heroicgameslauncher.hgl/config/heroic/nile_config",
                ]
                .map(|p| home.join(p)),
            )
            .filter(|d| d.join("nile").is_dir())
            .map(|config_dir| Self {
                bin: bin.clone(),
                config_dir,
            })
            .collect()
    }

    /// The nile that installed `id`.
    pub fn locate_for(id: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|n| n.installed().is_ok_and(|l| l.iter().any(|g| g.id == id)))
    }

    pub fn installed(&self) -> anyhow::Result<Vec<Installed>> {
        self.read("installed.json")
    }

    fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> anyhow::Result<Vec<T>> {
        let path = self.config_dir.join("nile").join(name);
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        // nile writes an empty object when it has nothing yet.
        if text.trim().is_empty() || text.trim() == "{}" || text.trim() == "null" {
            return Ok(Vec::new());
        }
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn games(&self) -> anyhow::Result<Vec<Game>> {
        let titles: BTreeMap<String, String> = self
            .read::<Entry>("library.json")
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| Some((e.product.id, e.product.title?)))
            .collect();
        Ok(self
            .installed()?
            .into_iter()
            .map(|g| Game {
                name: titles.get(&g.id).cloned().unwrap_or_else(|| {
                    g.path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| g.id.clone())
                }),
                id: GameId::new(Store::Amazon, g.id),
                install_dir: Some(g.path),
                size_on_disk: g.size,
            })
            .collect())
    }

    /// argv + env for `nile launch`, with `wrapper` in front of the game's command line.
    pub fn launch_command(
        &self,
        id: &str,
        wrapper: &[String],
    ) -> (Vec<String>, BTreeMap<String, String>) {
        let argv = vec![
            self.bin.display().to_string(),
            "launch".into(),
            "--no-wine".into(),
            "--wrapper".into(),
            shell_join(wrapper),
            id.into(),
        ];
        let env = [(
            "NILE_CONFIG_PATH".to_string(),
            self.config_dir.display().to_string(),
        )]
        .into();
        (argv, env)
    }
}

/// All installed Amazon games. Errors are per nile folder and do not stop the others.
pub fn installed_games() -> (Vec<Game>, Vec<anyhow::Error>) {
    let mut games = Vec::new();
    let mut errors = Vec::new();
    for nile in Nile::all() {
        match nile.games() {
            Ok(list) => games.extend(list),
            Err(e) => errors.push(e.context("Amazon (nile)")),
        }
    }
    (games, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_installed_games_with_their_titles() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("nile")).unwrap();
        let nile = Nile {
            bin: "/bin/nile".into(),
            config_dir: tmp.path().into(),
        };
        assert!(nile.games().unwrap().is_empty());
        std::fs::write(
            tmp.path().join("nile/installed.json"),
            r#"[{"id":"amzn1.a","version":"v1","path":"/g/Fox","size":10},
                {"id":"amzn1.b","version":"v1","path":"/g/Hen","size":20}]"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("nile/library.json"),
            r#"[{"id":"e","product":{"id":"amzn1.a","title":"Fox Tale"}}]"#,
        )
        .unwrap();
        let games = nile.games().unwrap();
        assert_eq!(games[0].name, "Fox Tale");
        assert_eq!(games[0].id.to_string(), "amazon:amzn1.a");
        assert_eq!(games[1].name, "Hen", "the folder's name without a title");
        let (argv, env) = nile.launch_command("amzn1.a", &["/a b".into(), "run".into()]);
        assert_eq!(argv[1..4], ["launch", "--no-wine", "--wrapper"]);
        assert_eq!(argv[4..], ["'/a b' run", "amzn1.a"]);
        assert_eq!(env["NILE_CONFIG_PATH"], tmp.path().display().to_string());
    }
}
