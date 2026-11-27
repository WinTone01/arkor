//! Where games keep their saves, from Ludusavi's manifest
//! (github.com/mtkennerly/ludusavi-manifest, MIT; its knowledge comes from PCGamingWiki).
//! The manifest (about 18 MB of YAML) is fetched compressed at most once a week and kept as a
//! small index of the files tagged as saves; nothing else of it is kept.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};

const MANIFEST: &str =
    "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml";

/// How old the index may get before it is fetched again.
const FRESH: Duration = Duration::from_secs(7 * 24 * 3600);

/// One place a game's saves may be, as Ludusavi writes it (`<winAppData>/Game/*.sav`), and
/// on which system and from which store it applies (`None`: any).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
}

/// A game's save places and its Steam app ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saves {
    pub files: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steam: Vec<u64>,
}

/// Every game with saves the manifest knows of, by its name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Index {
    pub games: BTreeMap<String, Saves>,
}

// The manifest's own shape, only what is used.
#[derive(Deserialize)]
struct RawGame {
    #[serde(default)]
    files: BTreeMap<String, RawFile>,
    #[serde(default)]
    steam: Option<RawSteam>,
    #[serde(default)]
    id: Option<RawIds>,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    when: Vec<RawWhen>,
}

#[derive(Deserialize)]
struct RawWhen {
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Deserialize)]
struct RawSteam {
    #[serde(default)]
    id: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIds {
    #[serde(default)]
    steam_extra: Vec<u64>,
}

/// The index of a manifest: the files tagged `save`, one rule per condition.
pub fn build(yaml: &str) -> anyhow::Result<Index> {
    let raw: BTreeMap<String, RawGame> =
        serde_yaml::from_str(yaml).context("reading Ludusavi's manifest")?;
    let mut games = BTreeMap::new();
    for (name, game) in raw {
        let mut files = Vec::new();
        for (path, file) in game.files {
            if !file.tags.iter().any(|t| t == "save") {
                continue;
            }
            if file.when.is_empty() {
                files.push(Rule {
                    path: path.clone(),
                    os: None,
                    store: None,
                });
            }
            for w in file.when {
                files.push(Rule {
                    path: path.clone(),
                    os: w.os,
                    store: w.store,
                });
            }
        }
        if files.is_empty() {
            continue;
        }
        let mut steam: Vec<u64> = game.steam.and_then(|s| s.id).into_iter().collect();
        steam.extend(game.id.map(|i| i.steam_extra).unwrap_or_default());
        games.insert(name, Saves { files, steam });
    }
    Ok(Index { games })
}

fn dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::cache_dir()
        .context("no XDG cache dir")?
        .join("arkor/ludusavi"))
}

fn index_path() -> anyhow::Result<PathBuf> {
    Ok(dir()?.join("saves.json"))
}

/// Fetches the manifest again when the index is older than a week (or `force`). Blocking.
pub fn refresh(force: bool) -> anyhow::Result<()> {
    let index = index_path()?;
    let fresh = std::fs::metadata(&index)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < FRESH);
    if fresh && !force {
        return Ok(());
    }
    let dir = dir()?;
    std::fs::create_dir_all(&dir)?;
    let yaml = dir.join("manifest.yaml.part");
    let status = std::process::Command::new("curl")
        .args(["-fsSL", "--compressed", "-A", "arkor", "-o"])
        .arg(&yaml)
        .arg(MANIFEST)
        .stdin(std::process::Stdio::null())
        .status()
        .context("running curl")?;
    let text = std::fs::read_to_string(&yaml);
    let _ = std::fs::remove_file(&yaml);
    anyhow::ensure!(
        status.success(),
        "Ludusavi's manifest could not be downloaded"
    );
    let built = build(&text?)?;
    anyhow::ensure!(
        built.games.len() > 1000,
        "Ludusavi's manifest looks incomplete"
    );
    let tmp = dir.join("saves.json.part");
    std::fs::write(&tmp, serde_json::to_vec(&built)?)?;
    std::fs::rename(&tmp, &index)?;
    Ok(())
}

/// The index, fetched first when there is none yet. Blocking.
pub fn load() -> anyhow::Result<Index> {
    let path = index_path()?;
    if !path.is_file() {
        refresh(true)?;
    }
    serde_json::from_slice(&std::fs::read(&path)?).context("reading the save index")
}

/// A name with only its letters and digits, lower case.
fn plain(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The game's entry: by Steam app id first, then by name.
pub fn lookup<'a>(
    index: &'a Index,
    name: &str,
    steam: Option<u64>,
) -> Option<(&'a str, &'a Saves)> {
    if let Some(id) = steam
        && let Some((n, s)) = index.games.iter().find(|(_, s)| s.steam.contains(&id))
    {
        return Some((n.as_str(), s));
    }
    let wanted = plain(name);
    if wanted.is_empty() {
        return None;
    }
    index
        .games
        .iter()
        .find(|(n, _)| plain(n) == wanted)
        .map(|(n, s)| (n.as_str(), s))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
"!Anyway!":
  cloud:
    steam: true
  files:
    "<winLocalAppData>/anyway":
      tags:
        - save
      when:
        - os: windows
    "<winLocalAppData>/anyway/settings.ini":
      tags:
        - config
  id:
    steamExtra:
      - 867990
  steam:
    id: 867980
"R.E.P.O.":
  files:
    "<home>/AppData/LocalLow/semiwork/Repo/saves":
      tags: [save]
      when:
        - os: windows
        - os: windows
          store: steam
  steam:
    id: 3241660
"No Saves Game":
  steam:
    id: 1
"#;

    /// The real manifest, over the network (run with a scratch `XDG_CACHE_HOME`).
    #[test]
    #[ignore = "downloads Ludusavi's manifest"]
    fn the_real_manifest() {
        let t = std::time::Instant::now();
        refresh(true).unwrap();
        let index = load().unwrap();
        eprintln!(
            "{} games with saves in {:?}",
            index.games.len(),
            t.elapsed()
        );
        for (name, steam) in [
            ("R.E.P.O.", Some(3241660)),
            ("Valheim", Some(892970)),
            ("Far Cry 5", None),
        ] {
            let found = lookup(&index, name, steam);
            eprintln!("{name}: {:?}", found.map(|(n, s)| (n, &s.files)));
            assert!(found.is_some(), "{name}");
        }
    }

    #[test]
    fn keeps_only_saves() {
        let index = build(SAMPLE).unwrap();
        assert_eq!(index.games.len(), 2, "a game without saves is left out");
        let anyway = &index.games["!Anyway!"];
        assert_eq!(anyway.files.len(), 1, "the config file is not a save");
        assert_eq!(anyway.files[0].os.as_deref(), Some("windows"));
        assert_eq!(anyway.steam, [867980, 867990]);
        let repo = &index.games["R.E.P.O."];
        assert_eq!(repo.files.len(), 2, "one rule per condition");
        assert_eq!(repo.files[1].store.as_deref(), Some("steam"));
    }

    #[test]
    fn finds_a_game_by_steam_id_then_name() {
        let index = build(SAMPLE).unwrap();
        assert_eq!(
            lookup(&index, "whatever", Some(867990)).unwrap().0,
            "!Anyway!"
        );
        assert_eq!(lookup(&index, "R.E.P.O", None).unwrap().0, "R.E.P.O.");
        assert!(lookup(&index, "Something Else", Some(5)).is_none());
        assert!(lookup(&index, "", None).is_none());
    }
}
