//! Lutris (https://lutris.net): installed games from its `pga.db`, with what to run read from
//! each game's YAML config. Only `wine` and `linux` runner games are listed: those Arkor can
//! start itself. Games Lutris hands to another client (Steam, …) come from that client.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{Game, GameId, Store};
use serde::Deserialize;

use crate::target::{ExeTarget, proton_by_name};

/// Lutris data dirs (native, then Flatpak); each may hold `pga.db` and `games/`.
pub(crate) fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(data) = dirs::data_dir() {
        dirs.push(data.join("lutris"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".var/app/net.lutris.Lutris/data/lutris"));
    }
    dirs.into_iter()
        .filter(|d| d.join("pga.db").is_file())
        .collect()
}

/// Where a game's `<configpath>.yml` may live: newer Lutris keeps it next to `pga.db`,
/// older versions under the config dir.
fn config_candidates(data_dir: &Path, configpath: &str) -> Vec<PathBuf> {
    let file = format!("{configpath}.yml");
    let mut out = vec![data_dir.join("games").join(&file)];
    if let Some(config) = dirs::config_dir() {
        out.push(config.join("lutris/games").join(&file));
    }
    if let Some(home) = dirs::home_dir() {
        out.push(
            home.join(".var/app/net.lutris.Lutris/config/lutris/games")
                .join(&file),
        );
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
struct Row {
    id: i64,
    name: String,
    runner: String,
    directory: Option<String>,
    configpath: String,
}

fn read_rows(db: &Path) -> anyhow::Result<Vec<Row>> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening {}", db.display()))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, runner, directory, configpath FROM games
         WHERE installed = 1 AND runner IN ('wine', 'linux') AND configpath IS NOT NULL",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                name: r.get(1)?,
                runner: r.get(2)?,
                directory: r.get(3)?,
                configpath: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn games() -> (Vec<Game>, Vec<anyhow::Error>) {
    let mut games = Vec::new();
    let mut errors = Vec::new();
    for dir in data_dirs() {
        match read_rows(&dir.join("pga.db")) {
            Ok(rows) => games.extend(rows.into_iter().map(|r| Game {
                id: GameId::new(Store::Lutris, r.id.to_string()),
                name: r.name,
                install_dir: r.directory.map(PathBuf::from),
                size_on_disk: None,
            })),
            Err(e) => errors.push(e.context("Lutris")),
        }
    }
    (games, errors)
}

#[derive(Debug, Default, Deserialize)]
struct Config {
    #[serde(default)]
    game: GameSection,
    #[serde(default)]
    wine: WineSection,
}

#[derive(Debug, Default, Deserialize)]
struct GameSection {
    exe: Option<PathBuf>,
    prefix: Option<PathBuf>,
    #[serde(default)]
    args: Option<String>,
    working_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
struct WineSection {
    version: Option<String>,
}

pub fn launch_target(id: &str) -> anyhow::Result<ExeTarget> {
    for dir in data_dirs() {
        let rows = read_rows(&dir.join("pga.db"))?;
        let Some(row) = rows.into_iter().find(|r| r.id.to_string() == id) else {
            continue;
        };
        let file = config_candidates(&dir, &row.configpath)
            .into_iter()
            .find(|p| p.is_file())
            .with_context(|| format!("Lutris config `{}.yml` not found", row.configpath))?;
        let text = std::fs::read_to_string(&file)?;
        let config: Config =
            serde_yaml::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;
        return to_target(&row, config);
    }
    anyhow::bail!("Lutris game `{id}` not found")
}

fn to_target(row: &Row, config: Config) -> anyhow::Result<ExeTarget> {
    let exe = config.game.exe.context("Lutris config has no game exe")?;
    // Lutris allows exe paths relative to the game's directory.
    let exe = match (&row.directory, exe.is_relative()) {
        (Some(dir), true) => Path::new(dir).join(exe),
        _ => exe,
    };
    let mut target = ExeTarget::windows(exe);
    target.native = row.runner == "linux";
    target.args = config
        .game
        .args
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if let Some(wd) = config.game.working_dir {
        target.working_dir = wd;
    }
    if !target.native {
        target.prefix = config.game.prefix;
        target.proton = config.wine.version.as_deref().and_then(proton_by_name);
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db(path: &Path) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE games (id INTEGER PRIMARY KEY, name TEXT, slug TEXT, runner TEXT,
                directory TEXT, installed INTEGER, configpath TEXT);
             INSERT INTO games VALUES (1, 'Witcher 3', 'witcher-3', 'wine', '/g/w3', 1, 'witcher-3-1700');
             INSERT INTO games VALUES (2, 'Native', 'native', 'linux', '/g/n', 1, 'native-1700');
             INSERT INTO games VALUES (3, 'Via Steam', 'vs', 'steam', '/g/s', 1, 'vs-1');
             INSERT INTO games VALUES (4, 'Removed', 'rm', 'wine', '/g/r', 0, 'rm-1');",
        )
        .unwrap();
    }

    #[test]
    fn reads_installed_wine_and_linux_games() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("pga.db");
        make_db(&db);
        let rows = read_rows(&db).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Witcher 3", "Native"]
        );
    }

    #[test]
    fn builds_targets_from_yaml() {
        let wine_row = Row {
            id: 1,
            name: "W".into(),
            runner: "wine".into(),
            directory: Some("/g/w3".into()),
            configpath: "w".into(),
        };
        let config: Config = serde_yaml::from_str(
            "game:\n  exe: bin/x64/witcher3.exe\n  prefix: /g/w3/prefix\n  args: -skipintro -dx12\nwine:\n  version: no-such-proton-123\nsystem:\n  env:\n    DXVK_HUD: fps\n",
        )
        .unwrap();
        let t = to_target(&wine_row, config).unwrap();
        assert_eq!(t.exe, PathBuf::from("/g/w3/bin/x64/witcher3.exe"));
        assert_eq!(t.working_dir, PathBuf::from("/g/w3/bin/x64"));
        assert_eq!(t.args, ["-skipintro", "-dx12"]);
        assert_eq!(t.prefix, Some(PathBuf::from("/g/w3/prefix")));
        assert_eq!(
            t.proton, None,
            "unknown wine version falls back to Arkor's default"
        );
        assert!(!t.native);

        let linux_row = Row {
            runner: "linux".into(),
            ..wine_row
        };
        let config: Config =
            serde_yaml::from_str("game:\n  exe: /g/n/run.sh\n  working_dir: /g/n\n").unwrap();
        let t = to_target(&linux_row, config).unwrap();
        assert!(t.native && t.prefix.is_none());
        assert_eq!(t.working_dir, PathBuf::from("/g/n"));

        assert!(to_target(&linux_row, Config::default()).is_err());
    }
}
