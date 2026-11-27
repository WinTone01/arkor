//! Saves of any game, found through Ludusavi's manifest ([`super::ludusavi`]): its rules
//! (`<winAppData>/Game/*.sav`) are turned into folders of this computer, in the game's Wine
//! prefix, its install folder or its store's folder, or the Linux user's own folders for a
//! game that runs natively. Copies of them are kept as for cloud saves ([`super::snapshots`]).

use std::path::{Component, Path, PathBuf};

use arkor_core::Global;
use arkor_core::library::{GameKind, LibraryGame};

use super::ludusavi::{Rule, Saves};
use super::places::Place;
use super::tree::Filter;

/// Where a game is: what its save rules are read against.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Where {
    /// The prefix's root (the folder `drive_c` is in), for a game run through Wine.
    pub prefix: Option<PathBuf>,
    pub install: Option<PathBuf>,
    /// The store Ludusavi names (`steam`, `gog`, `epic`, `uplay`…).
    pub store: Option<&'static str>,
    /// The store's own folder (`<root>`): Steam's, or Ubisoft Connect's in the prefix.
    pub root: Option<PathBuf>,
    /// The Linux user's home, for a game that runs natively.
    pub home: Option<PathBuf>,
}

/// A prefix's root: the folder with `drive_c`, also inside a Steam/umu `compatdata` folder.
fn prefix_root(path: &Path) -> Option<PathBuf> {
    [path.to_path_buf(), path.join("pfx")]
        .into_iter()
        .find(|p| p.join("drive_c").is_dir())
}

/// Where `game` is, as far as its saves go.
pub fn where_of(game: &LibraryGame, global: &Global) -> Option<Where> {
    let home = dirs::home_dir();
    let mut w = Where::default();
    match &game.kind {
        GameKind::Steam { appid } => {
            let install = arkor_stores::steam::app_dir(appid)?;
            w.prefix = install
                .parent()
                .and_then(Path::parent)
                .and_then(|steamapps| prefix_root(&steamapps.join("compatdata").join(appid)));
            w.store = Some("steam");
            w.root = arkor_stores::steam::steam_roots().into_iter().next();
            if w.prefix.is_none() {
                w.home = home;
            }
            w.install = Some(install);
        }
        GameKind::Linux => {
            w.install = game.exe.parent().map(Path::to_path_buf);
            w.home = home;
        }
        // The emulator keeps the saves; the Ludusavi manifest knows no ROMs.
        GameKind::Emulated { .. } => return None,
        GameKind::Windows | GameKind::Epic { .. } | GameKind::Amazon { .. } => {
            w.prefix = prefix_root(&game.effective_prefix(&global.prefixes_dir()));
            w.install = match &game.kind {
                GameKind::Windows => Some(crate::mods::link::game_root(&game.exe)),
                _ => Some(game.exe.clone()).filter(|p| p.is_dir()),
            };
            w.store = match &game.kind {
                GameKind::Epic { .. } => Some("epic"),
                GameKind::Amazon { .. } => Some("prime"),
                _ => game
                    .origin
                    .as_deref()
                    .and_then(|o| o.split_once(':'))
                    .and_then(|(s, _)| match s {
                        "gog" => Some("gog"),
                        "epic" | "legendary" => Some("epic"),
                        _ => None,
                    }),
            };
        }
    }
    // Ubisoft Connect in the prefix keeps the saves of the games it starts (`<root>` of
    // the manifest's `uplay` rules), whichever store sold them.
    if w.root.is_none()
        && let Some(drive) = w.prefix.as_ref().map(|p| p.join("drive_c"))
    {
        w.root = ["Program Files (x86)", "Program Files"]
            .iter()
            .map(|pf| drive.join(pf).join("Ubisoft/Ubisoft Game Launcher"))
            .find(|d| d.is_dir());
    }
    Some(w)
}

impl Where {
    /// The Windows user's folder in the prefix (Proton's `steamuser`, else Wine's own user).
    fn win_user(&self) -> Option<PathBuf> {
        let users = self.prefix.as_ref()?.join("drive_c/users");
        let proton = users.join("steamuser");
        if proton.is_dir() {
            return Some(proton);
        }
        std::fs::read_dir(&users)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.is_dir() && p.file_name().is_some_and(|n| n != "Public"))
            .or(Some(proton))
    }

    /// Whether a rule applies to this game at all.
    fn takes(&self, rule: &Rule) -> bool {
        let os = match rule.os.as_deref() {
            None => true,
            Some("windows") => self.prefix.is_some(),
            Some("linux") => self.home.is_some(),
            Some(_) => false,
        };
        let store = match rule.store.as_deref() {
            None => true,
            // Ubisoft Connect's folder serves its games from any store.
            Some("uplay") => self
                .root
                .as_ref()
                .is_some_and(|r| r.ends_with("Ubisoft Game Launcher")),
            Some(s) => self.store == Some(s),
        };
        os && store
    }

    /// What a placeholder stands for (`None`: unknown here, so the rule is not used).
    fn value(&self, name: &str) -> Option<PathBuf> {
        let win = || self.win_user();
        let xdg = |var: &str, under: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .or_else(|| self.home.as_ref().map(|h| h.join(under)))
        };
        Some(match name.to_ascii_lowercase().as_str() {
            "<base>" => self.install.clone()?,
            "<root>" => self.root.clone()?,
            "<storeuserid>" => PathBuf::from("*"),
            "<osusername>" => match &self.prefix {
                Some(_) => win()?.file_name().map(PathBuf::from)?,
                None => PathBuf::from(std::env::var_os("USER")?),
            },
            "<game>" => self.install.as_ref()?.file_name().map(PathBuf::from)?,
            "<home>" => match &self.prefix {
                Some(_) => win()?,
                None => self.home.clone()?,
            },
            "<winappdata>" => win()?.join("AppData/Roaming"),
            "<winlocalappdata>" => win()?.join("AppData/Local"),
            "<winlocalappdatalow>" => win()?.join("AppData/LocalLow"),
            "<windocuments>" => win()?.join("Documents"),
            "<winpublic>" => self.prefix.as_ref()?.join("drive_c/users/Public"),
            "<winprogramdata>" => self.prefix.as_ref()?.join("drive_c/ProgramData"),
            "<windir>" => self.prefix.as_ref()?.join("drive_c/windows"),
            "<xdgdata>" if self.home.is_some() => xdg("XDG_DATA_HOME", ".local/share")?,
            "<xdgconfig>" if self.home.is_some() => xdg("XDG_CONFIG_HOME", ".config")?,
            _ => return None,
        })
    }

    /// Folders that hold more than one game's files: never a save folder of their own.
    fn shared(&self, path: &Path) -> bool {
        let mut bases: Vec<PathBuf> = Vec::new();
        if let Some(u) = self.win_user() {
            bases.push(u.clone());
            for d in [
                "AppData",
                "AppData/Local",
                "AppData/LocalLow",
                "AppData/Roaming",
                "Documents",
                "Documents/My Games",
                "Saved Games",
                "Desktop",
            ] {
                bases.push(u.join(d));
            }
        }
        if let Some(h) = &self.home {
            bases.push(h.clone());
            for d in [".config", ".local", ".local/share", "Documents", "Desktop"] {
                bases.push(h.join(d));
            }
        }
        bases.iter().any(|b| b == path)
    }

    /// Whether `path` lies inside what this game may read and write.
    fn inside(&self, path: &Path) -> bool {
        let plain = path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
        let bases = [
            self.prefix.as_ref().map(|p| p.join("drive_c")),
            self.install.clone(),
            self.root.clone(),
            self.home.clone(),
        ];
        plain
            && bases
                .iter()
                .flatten()
                .any(|b| path.starts_with(b) && path != b.as_path())
    }

    /// The save folders of a game's rules here.
    pub fn places(&self, saves: &Saves) -> Vec<Place> {
        let mut out: Vec<Place> = Vec::new();
        for rule in saves.files.iter().filter(|r| self.takes(r)) {
            for place in self.resolve(&rule.path) {
                if !out
                    .iter()
                    .any(|p| p.path == place.path && p.filter == place.filter)
                {
                    out.push(place);
                }
            }
        }
        out
    }

    /// One rule's folders (a rule may name several, through `*`).
    fn resolve(&self, text: &str) -> Vec<Place> {
        let text = text.replace('\\', "/");
        let mut parts: Vec<String> = Vec::new();
        for part in text.split('/').filter(|p| !p.is_empty() && *p != ".") {
            if part.starts_with('<') && part.ends_with('>') {
                let Some(value) = self.value(part) else {
                    return Vec::new();
                };
                parts.extend(
                    value
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned()),
                );
            } else if part.contains(['<', '>']) || part == ".." {
                return Vec::new();
            } else {
                parts.push(part.to_string());
            }
        }
        let Some((last, dirs)) = parts.split_last() else {
            return Vec::new();
        };
        let wild = |p: &str| p.contains(['*', '?']);
        let mut out = Vec::new();
        for parent in expand(dirs) {
            // `Saves/*.sav`: the folder, with the pattern as its filter.
            if wild(last) {
                if parent.is_dir() {
                    out.push((parent.clone(), Filter::new(last, "")));
                }
                continue;
            }
            let Some(path) = find(&parent, last) else {
                continue;
            };
            if path.is_dir() {
                out.push((path, Filter::default()));
            } else if path.is_file() {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                out.push((parent.clone(), Filter::new(&name, "")));
            }
        }
        out.into_iter()
            .filter(|(path, filter)| {
                self.inside(path) && (!filter.include.is_empty() || !self.shared(path))
            })
            .map(|(path, filter)| Place {
                name: format!("local:{}", path.display()),
                path,
                filter,
            })
            .collect()
    }
}

/// A child of `dir` named `name`, as Windows finds it (upper and lower case alike).
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        })
}

/// The folders a path with `*` and `?` in it names (case does not matter), at most 64.
fn expand(parts: &[String]) -> Vec<PathBuf> {
    let mut found = vec![PathBuf::from("/")];
    for part in parts {
        let mut next = Vec::new();
        for base in &found {
            if part.contains(['*', '?']) {
                let Ok(read) = std::fs::read_dir(base) else {
                    continue;
                };
                for e in read.flatten() {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if e.path().is_dir()
                        && super::tree::wildcard(name.as_bytes(), part.to_lowercase().as_bytes())
                    {
                        next.push(e.path());
                    }
                }
            } else if let Some(p) = find(base, part) {
                next.push(p);
            }
        }
        next.truncate(64);
        found = next;
        if found.is_empty() {
            break;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saves::ludusavi::Rule;

    fn rule(path: &str, os: Option<&str>, store: Option<&str>) -> Rule {
        Rule {
            path: path.into(),
            os: os.map(Into::into),
            store: store.map(Into::into),
        }
    }

    #[test]
    fn finds_saves_in_a_prefix_and_beside_the_game() {
        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path();
        let user = t.join("pfx/drive_c/users/steamuser");
        std::fs::create_dir_all(user.join("AppData/LocalLow/semiwork/Repo/saves")).unwrap();
        std::fs::create_dir_all(user.join("Documents/My Games/Game")).unwrap();
        std::fs::write(user.join("Documents/My Games/Game/slot1.sav"), "a").unwrap();
        let ubi = t.join("pfx/drive_c/Program Files (x86)/Ubisoft/Ubisoft Game Launcher");
        std::fs::create_dir_all(ubi.join("savegames/abc-123/4311")).unwrap();
        std::fs::create_dir_all(t.join("game/Saves")).unwrap();
        let w = Where {
            prefix: Some(t.join("pfx")),
            install: Some(t.join("game")),
            store: None,
            root: Some(ubi.clone()),
            home: None,
        };
        let saves = Saves {
            files: vec![
                rule(
                    "<home>/AppData/LocalLow/semiwork/Repo/saves",
                    Some("windows"),
                    None,
                ),
                rule("<winDocuments>/My Games/game/*.sav", None, None),
                rule("<root>/savegames/<storeUserId>/4311", None, Some("uplay")),
                rule("<base>/saves", None, None),
                rule("<xdgConfig>/unity3d/x", Some("linux"), None),
                rule("<winDocuments>", None, None),
                rule("<home>/../outside", None, None),
            ],
            steam: vec![],
        };
        let places = w.places(&saves);
        let paths: Vec<_> = places.iter().map(|p| p.path.clone()).collect();
        assert_eq!(
            paths,
            [
                user.join("AppData/LocalLow/semiwork/Repo/saves"),
                user.join("Documents/My Games/Game"),
                ubi.join("savegames/abc-123/4311"),
                t.join("game/Saves"),
            ],
            "the Linux rule, all of Documents and a way out are left out"
        );
        assert_eq!(places[1].filter.include, ["*.sav"]);
        assert!(places[0].filter.include.is_empty());
    }

    #[test]
    fn a_native_game_uses_the_linux_users_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join(".config/unity3d/IronGate/Valheim")).unwrap();
        let w = Where {
            home: Some(home.clone()),
            ..Where::default()
        };
        let saves = Saves {
            files: vec![
                rule(
                    "<home>/AppData/LocalLow/IronGate/Valheim",
                    Some("windows"),
                    None,
                ),
                rule(
                    "<home>/.config/unity3d/IronGate/Valheim",
                    Some("linux"),
                    None,
                ),
            ],
            steam: vec![],
        };
        let places = w.places(&saves);
        assert_eq!(places.len(), 1);
        assert_eq!(
            places[0].path,
            home.join(".config/unity3d/IronGate/Valheim")
        );
    }
}
