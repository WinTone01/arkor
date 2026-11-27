//! Which of Amethyst's games an Arkor game is, and where it is: its folder (the one the game's
//! own files start from) and its Wine prefix. The choice is kept in the game's config folder
//! (`mods.toml`), not in its profile, so older Arkor versions still read the profile.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::GameId;
use arkor_core::library::{GameKind, LibraryGame};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::rpc::Client;

/// An Arkor game's place in Amethyst.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// Amethyst's game id (`Valheim`, `skyrim_se`…).
    pub game: String,
    /// Its mod profile.
    pub profile: String,
    /// The DLL overrides Amethyst set for the game's mod loaders, kept so a launch gives
    /// them to Wine itself: Proton may rewrite the prefix's registry (its first start does).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dll_overrides: BTreeMap<String, String>,
}

fn file(id: &GameId) -> anyhow::Result<PathBuf> {
    Ok(arkor_core::paths::game_config_dir(id)?.join("mods.toml"))
}

pub fn load(id: &GameId) -> Option<Link> {
    toml::from_str(&std::fs::read_to_string(file(id).ok()?).ok()?).ok()
}

pub fn save(id: &GameId, link: &Link) -> anyhow::Result<()> {
    let path = file(id)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, toml::to_string(link)?)?;
    Ok(())
}

/// Forgets the link (Amethyst keeps the game and its mods).
pub fn forget(id: &GameId) -> anyhow::Result<()> {
    match std::fs::remove_file(file(id)?) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Where a game is, for Amethyst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub game_dir: PathBuf,
    /// The prefix (`drive_c` is in it); `None` for a game that runs natively.
    pub prefix: Option<PathBuf>,
    pub steam_appid: Option<String>,
}

/// Folders a game's program often sits in below the game's own folder.
const BELOW_ROOT: [&str; 12] = [
    "win64", "win32", "x64", "x86", "bin", "bin64", "bin_x64", "bin32", "binaries", "_retail_",
    "shipping", "game",
];

/// The game's own folder for its program `exe`: the program's folder, climbing out of
/// folders like `Binaries/Win64` (Unreal games keep the program two folders down in their
/// project folder, so its folder is climbed out of too).
pub fn game_root(exe: &Path) -> PathBuf {
    let mut dir = exe.parent().unwrap_or(exe).to_path_buf();
    let lower = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let mut unreal = false;
    while BELOW_ROOT.contains(&lower(&dir).as_str()) {
        unreal |= lower(&dir) == "binaries";
        match dir.parent() {
            Some(up) if up.parent().is_some() => dir = up.to_path_buf(),
            _ => break,
        }
    }
    // `<Game>/<Project>/Binaries/Win64`: the project folder is not the game's folder.
    if unreal
        && let Some(up) = dir.parent()
        && up.parent().is_some()
    {
        dir = up.to_path_buf();
    }
    dir
}

/// Where `game` is, or `None` when it cannot be told (not installed, nothing to mod).
pub fn place(game: &LibraryGame, prefixes_dir: &Path) -> Option<Place> {
    match &game.kind {
        GameKind::Steam { appid } => {
            let game_dir = arkor_stores::steam::app_dir(appid)?;
            // `<library>/steamapps/common/<dir>` → `<library>/steamapps/compatdata/<id>/pfx`
            let prefix = game_dir
                .parent()
                .and_then(Path::parent)
                .map(|steamapps| steamapps.join("compatdata").join(appid).join("pfx"))
                .filter(|p| p.join("drive_c").is_dir());
            Some(Place {
                game_dir,
                prefix,
                steam_appid: Some(appid.clone()),
            })
        }
        GameKind::Epic { .. } | GameKind::Amazon { .. } => {
            (!game.exe.as_os_str().is_empty() && game.exe.is_dir()).then(|| Place {
                game_dir: game.exe.clone(),
                prefix: Some(game.effective_prefix(prefixes_dir)),
                steam_appid: None,
            })
        }
        GameKind::Windows => (!game.exe.as_os_str().is_empty()).then(|| Place {
            game_dir: game_root(&game.exe),
            prefix: Some(game.effective_prefix(prefixes_dir)),
            steam_appid: None,
        }),
        GameKind::Linux => (!game.exe.as_os_str().is_empty()).then(|| Place {
            game_dir: game_root(&game.exe),
            prefix: None,
            steam_appid: None,
        }),
    }
}

/// One of Amethyst's games.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub configured: bool,
    pub game_path: Option<String>,
    pub steam_app_ids: Vec<String>,
    /// Mods for it can come from Nexus Mods (the site's names for it, first the main one)
    /// or Thunderstore.
    pub nexus: Vec<String>,
    pub thunderstore: bool,
    /// Its community on Thunderstore (`repo`, `valheim`…).
    pub thunderstore_community: Option<String>,
}

/// Amethyst's games (`games.list`).
pub fn games(c: &mut Client) -> anyhow::Result<Vec<Candidate>> {
    let list = c.call("games.list", json!({}))?;
    Ok(list["games"]
        .as_array()
        .map(|a| a.iter().filter_map(candidate).collect())
        .unwrap_or_default())
}

fn candidate(v: &Value) -> Option<Candidate> {
    if v["unavailable_reason"].is_string() {
        return None;
    }
    Some(Candidate {
        id: v["id"].as_str()?.to_string(),
        name: v["name"].as_str()?.to_string(),
        configured: v["configured"].as_bool().unwrap_or(false),
        game_path: v["game_path"].as_str().map(str::to_string),
        steam_app_ids: v["steam_app_ids"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.as_str()
                            .map(str::to_string)
                            .or_else(|| x.as_u64().map(|n| n.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        nexus: v["nexus_domains"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        thunderstore: v["features"]["thunderstore"].as_bool().unwrap_or(false),
        thunderstore_community: v["thunderstore_community"].as_str().map(str::to_string),
    })
}

/// A name with only its letters and digits, lower case (`The Witcher® 3: Wild Hunt` →
/// `thewitcher3wildhunt`).
fn plain(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Amethyst's game for `name`: by Steam app id first, then by name.
pub fn best_match<'a>(
    candidates: &'a [Candidate],
    name: &str,
    steam_appid: Option<&str>,
) -> Option<&'a Candidate> {
    if let Some(id) = steam_appid
        && let Some(c) = candidates
            .iter()
            .find(|c| c.steam_app_ids.iter().any(|s| s == id))
    {
        return Some(c);
    }
    let wanted = plain(name);
    candidates.iter().find(|c| plain(&c.name) == wanted)
}

/// Sets the game up in Amethyst at `place` (when it is not yet) and remembers the link.
/// A game Amethyst already has somewhere else is not moved: that is an error saying where.
pub fn connect(
    c: &mut Client,
    id: &GameId,
    candidate: &Candidate,
    place: &Place,
) -> anyhow::Result<Link> {
    let wanted = place.game_dir.to_string_lossy().to_string();
    if candidate.configured {
        let at = candidate.game_path.clone().unwrap_or_default();
        anyhow::ensure!(
            same_dir(Path::new(&at), &place.game_dir),
            "Amethyst already has {} at {at}",
            candidate.name
        );
    } else {
        let mut params = json!({
            "game": candidate.id,
            "game_path": wanted,
            "deploy_mode": "symlink",
        });
        if let Some(prefix) = &place.prefix {
            params["prefix_path"] = json!(prefix.to_string_lossy());
        }
        c.job("games.configure.start", params, |_| {})
            .with_context(|| format!("setting {} up in Amethyst", candidate.name))?;
    }
    let mut link = Link {
        game: candidate.id.clone(),
        profile: "default".into(),
        ..Link::default()
    };
    save(id, &link)?;
    refresh(c, id, &mut link)?;
    Ok(link)
}

/// Reads the DLL overrides Amethyst set for the game again and keeps them with the link.
pub fn refresh(c: &mut Client, id: &GameId, link: &mut Link) -> anyhow::Result<()> {
    let now: BTreeMap<String, String> = super::dll_overrides(c, link)?.into_iter().collect();
    if now != link.dll_overrides {
        link.dll_overrides = now;
        save(id, link)?;
    }
    Ok(())
}

/// `native,builtin` → `n,b`: how `WINEDLLOVERRIDES` writes a load order (empty: disabled).
pub fn short_order(order: &str) -> String {
    order
        .split(',')
        .map(str::trim)
        .filter_map(|w| match w.to_ascii_lowercase().as_str() {
            "native" | "n" => Some("n"),
            "builtin" | "b" => Some("b"),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The overrides of the game's link, for a launch: `(dll, n,b)`.
pub fn launch_overrides(id: &GameId) -> Vec<(String, String)> {
    load(id)
        .map(|l| {
            l.dll_overrides
                .iter()
                .map(|(dll, order)| (dll.clone(), short_order(order)))
                .collect()
        })
        .unwrap_or_default()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_load_orders() {
        assert_eq!(short_order("native,builtin"), "n,b");
        assert_eq!(short_order("builtin"), "b");
        assert_eq!(short_order(""), "", "disabled");
        assert_eq!(short_order("n, b"), "n,b");
    }

    #[test]
    fn an_older_link_file_still_reads() {
        let l: Link = toml::from_str("game = \"repo\"\nprofile = \"default\"\n").unwrap();
        assert!(l.dll_overrides.is_empty());
        let mut l = l;
        l.dll_overrides
            .insert("winhttp".into(), "native,builtin".into());
        let back: Link = toml::from_str(&toml::to_string(&l).unwrap()).unwrap();
        assert_eq!(back, l);
    }

    #[test]
    fn finds_the_games_own_folder() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(game_root(&p("/g/Valheim/valheim.exe")), p("/g/Valheim"));
        assert_eq!(
            game_root(&p("/g/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe")),
            p("/g/Cyberpunk 2077")
        );
        assert_eq!(
            game_root(&p(
                "/g/Stray/Hk_project/Binaries/Win64/Stray-Win64-Shipping.exe"
            )),
            p("/g/Stray"),
            "an Unreal project folder is climbed out of"
        );
        assert_eq!(game_root(&p("/x.exe")), p("/"), "never above the root");
    }

    fn cand(id: &str, name: &str, appids: &[&str]) -> Candidate {
        Candidate {
            id: id.into(),
            name: name.into(),
            configured: false,
            game_path: None,
            steam_app_ids: appids.iter().map(|s| s.to_string()).collect(),
            nexus: vec!["x".into()],
            thunderstore: false,
            thunderstore_community: None,
        }
    }

    #[test]
    fn matches_by_steam_id_then_by_name() {
        let list = [
            cand("skyrim_se", "Skyrim Special Edition", &["489830"]),
            cand("witcher3", "The Witcher 3", &["292030"]),
            cand("Valheim", "Valheim", &["892970"]),
        ];
        assert_eq!(
            best_match(&list, "Whatever", Some("292030")).unwrap().id,
            "witcher3"
        );
        assert_eq!(best_match(&list, "VALHEIM", None).unwrap().id, "Valheim");
        assert_eq!(
            best_match(&list, "Skyrim® Special Edition", Some("1"))
                .unwrap()
                .id,
            "skyrim_se",
            "an unknown app id falls back to the name"
        );
        assert!(best_match(&list, "Hades", None).is_none());
    }

    #[test]
    fn reads_amethysts_games() {
        let v: Value = serde_json::from_str(
            r#"{"games":[
              {"id":"Valheim","name":"Valheim","configured":true,"game_path":"/g/V","steam_app_ids":["892970"],"nexus_domains":["valheim"],"features":{"nexus":true,"thunderstore":true}},
              {"id":"gone","name":"Gone","unavailable_reason":"handler failed"},
              {"id":"bg3","name":"Baldur's Gate 3","configured":false,"game_path":null,"steam_app_ids":[1086940],"features":{"nexus":true}}
            ]}"#,
        )
        .unwrap();
        let list: Vec<Candidate> = v["games"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(candidate)
            .collect();
        assert_eq!(list.len(), 2, "an unavailable handler is left out");
        assert_eq!(list[0].game_path.as_deref(), Some("/g/V"));
        assert!(list[0].thunderstore);
        assert_eq!(list[0].nexus, ["valheim"]);
        assert_eq!(list[1].steam_app_ids, ["1086940"]);
        assert!(!list[1].configured);
    }
}
