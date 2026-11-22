//! Faugus Launcher (https://github.com/Faugus/faugus-launcher): games the user added there,
//! read from `$XDG_DATA_HOME/faugus-launcher/games.json`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{Game, GameId, Store};
use serde::Deserialize;

use crate::target::{ExeTarget, proton_by_name};

#[derive(Debug, Deserialize)]
struct Entry {
    gameid: String,
    title: String,
    path: PathBuf,
    #[serde(default)]
    prefix: PathBuf,
    #[serde(default)]
    game_arguments: String,
    #[serde(default)]
    runner: String,
    #[serde(default)]
    hidden: bool,
}

impl Entry {
    /// Faugus can also hold Steam shortcuts; those are Steam's, not runnable here.
    fn is_listable(&self) -> bool {
        !self.hidden && self.runner != "Steam" && !self.gameid.is_empty()
    }
}

fn games_file() -> Option<PathBuf> {
    let p = dirs::data_dir()?.join("faugus-launcher/games.json");
    p.is_file().then_some(p)
}

fn read_entries(path: &Path) -> anyhow::Result<Vec<Entry>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let entries: Vec<Entry> =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(entries.into_iter().filter(Entry::is_listable).collect())
}

pub fn games() -> anyhow::Result<Vec<Game>> {
    let Some(file) = games_file() else {
        return Ok(Vec::new());
    };
    Ok(read_entries(&file)?
        .into_iter()
        .map(|e| Game {
            id: GameId::new(Store::Faugus, &e.gameid),
            name: e.title,
            install_dir: e.path.parent().map(Path::to_path_buf),
            size_on_disk: None,
        })
        .collect())
}

pub fn launch_target(gameid: &str) -> anyhow::Result<ExeTarget> {
    let file = games_file().context("Faugus Launcher has no games.json")?;
    let entry = read_entries(&file)?
        .into_iter()
        .find(|e| e.gameid == gameid)
        .with_context(|| format!("Faugus game `{gameid}` not found"))?;
    Ok(to_target(entry))
}

fn to_target(e: Entry) -> ExeTarget {
    let native = e.runner == "Linux" || e.path.extension().is_some_and(|x| x == "sh");
    let mut target = ExeTarget::windows(e.path);
    target.native = native;
    target.args = e
        .game_arguments
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    target.prefix = (!e.prefix.as_os_str().is_empty()).then_some(e.prefix);
    // Faugus names runners after their folder (e.g. "GE-Proton10-25"); use it when installed.
    target.proton = proton_by_name(&e.runner);
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_entries_and_targets() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("games.json");
        std::fs::write(
            &p,
            r#"[
              {"gameid":"witcher","title":"The Witcher","path":"/g/w/witcher.exe","prefix":"/pfx/w",
               "launch_arguments":"","game_arguments":"-skipintro -dx12","mangohud":"","runner":"GE-Proton10-25","hidden":false},
              {"gameid":"hid","title":"Hidden","path":"/x.exe","prefix":"/p","hidden":true},
              {"gameid":"cs","title":"Steam one","path":"","prefix":"","runner":"Steam"},
              {"gameid":"nat","title":"Native","path":"/g/n/run.sh","prefix":"","runner":"Linux"}]"#,
        )
        .unwrap();
        let entries = read_entries(&p).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|e| e.gameid.as_str())
                .collect::<Vec<_>>(),
            ["witcher", "nat"]
        );

        let mut it = entries.into_iter();
        let w = to_target(it.next().unwrap());
        assert_eq!(w.args, ["-skipintro", "-dx12"]);
        assert_eq!(w.prefix, Some(PathBuf::from("/pfx/w")));
        assert_eq!(w.working_dir, PathBuf::from("/g/w"));
        assert!(!w.native);
        let n = to_target(it.next().unwrap());
        assert!(n.native && n.prefix.is_none());
    }
}
