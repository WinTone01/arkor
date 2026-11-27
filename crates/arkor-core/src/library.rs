//! The user's game library: games added by hand (like Faugus), optionally pre-filled from
//! what Arkor detects in other launchers. Stored as `~/.config/arkor/library.toml`.
//!
//! Every game's id is `local:<slug>`; its launch settings live in its profile.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{Game, GameId, Store, paths};

/// How a library game is started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum GameKind {
    /// A Windows executable (or .lnk/.url/.bat shortcut) run through umu/Proton.
    Windows,
    /// A native Linux executable or script.
    Linux,
    /// A Steam game, started through Steam.
    Steam { appid: String },
    /// An Epic game installed with legendary/Heroic, started through legendary.
    Epic { app_name: String },
    /// An Amazon Games game, started through nile.
    Amazon { id: String },
    /// A game an emulator plays: `exe` is its file (a ROM, a disc's `.cue`/`.m3u`) or its
    /// folder. `system` is from Arkor's systems catalog (`psx`, `snes`); `emulator` the
    /// game's own choice (`duckstation`, `retroarch:snes9x`), else the system's default.
    Emulated {
        system: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        emulator: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryGame {
    pub id: String,
    pub name: String,
    pub kind: GameKind,
    /// Executable for Windows/Linux games; install folder for store games (may be empty).
    #[serde(default)]
    pub exe: PathBuf,
    /// Wine prefix (Windows, Epic and Amazon). `None` = a per-game prefix in the default prefixes folder.
    #[serde(default)]
    pub prefix: Option<PathBuf>,
    /// Arguments passed to the game.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory; `None` = the executable's folder.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    /// Proton the launcher the game came from used (kept so its prefix keeps working).
    #[serde(default)]
    pub source_proton: Option<PathBuf>,
    /// Where it was added from, e.g. `heroic:abc` (informational).
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    /// Shown first, and on the home screen.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favorite: bool,
    /// Cover image shown in the library.
    #[serde(default)]
    pub cover: Option<PathBuf>,
    /// The user's rating, 1 to 5 stars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u8>,
    /// Where the user is with the game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<PlayStatus>,
    /// Another copy of the game `copy_of` is (from another store): kept out of the lists
    /// (hidden), played from that game's page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_of: Option<String>,
}

/// A title reduced to letters and digits, for telling that two entries are the same game.
pub fn title_key(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut out: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();
    for tail in [
        "gameoftheyearedition",
        "goty",
        "definitiveedition",
        "enhancededition",
    ] {
        if out.len() > tail.len() + 3 && out.ends_with(tail) {
            out.truncate(out.len() - tail.len());
        }
    }
    out
}

/// Where the user is with a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayStatus {
    PlanToPlay,
    Playing,
    OnHold,
    Beaten,
    Abandoned,
}

impl PlayStatus {
    pub const ALL: [PlayStatus; 5] = [
        PlayStatus::PlanToPlay,
        PlayStatus::Playing,
        PlayStatus::OnHold,
        PlayStatus::Beaten,
        PlayStatus::Abandoned,
    ];
}

impl LibraryGame {
    pub fn game_id(&self) -> GameId {
        GameId::new(Store::Local, &self.id)
    }

    pub fn to_game(&self) -> Game {
        let install_dir = match &self.kind {
            // A launcher's game runs the launcher's program from the game's own folder.
            GameKind::Windows
                if self
                    .origin
                    .as_deref()
                    .is_some_and(|o| o.starts_with("client:")) =>
            {
                self.working_dir
                    .clone()
                    .or_else(|| self.exe.parent().map(Path::to_path_buf))
            }
            GameKind::Windows | GameKind::Linux | GameKind::Emulated { .. } => {
                self.exe.parent().map(Path::to_path_buf)
            }
            GameKind::Steam { .. } | GameKind::Epic { .. } | GameKind::Amazon { .. } => {
                (!self.exe.as_os_str().is_empty()).then(|| self.exe.clone())
            }
        };
        Game {
            id: self.game_id(),
            name: self.name.clone(),
            install_dir,
            size_on_disk: None,
        }
    }

    pub fn needs_prefix(&self) -> bool {
        matches!(
            self.kind,
            GameKind::Windows | GameKind::Epic { .. } | GameKind::Amazon { .. }
        )
    }

    /// The prefix this game runs in: its own setting or the per-game default.
    pub fn effective_prefix(&self, prefixes_dir: &Path) -> PathBuf {
        self.prefix
            .clone()
            // A program inside a prefix (e.g. a launcher's shortcut) runs in that prefix,
            // even when the entry was saved without one (its drive was not mounted then).
            .or_else(|| prefix_containing(&self.exe))
            .unwrap_or_else(|| prefixes_dir.join(&self.id))
    }
}

/// The Wine prefix `path` lies in (`<prefix>/drive_c/...`), if any. Purely by the path.
pub fn prefix_containing(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "drive_c"))
        .and_then(|drive_c| drive_c.parent())
        .map(Path::to_path_buf)
}

/// A game to add: everything except the id, which the library assigns.
#[derive(Debug, Clone, PartialEq)]
pub struct NewGame {
    pub name: String,
    pub kind: GameKind,
    pub exe: PathBuf,
    pub prefix: Option<PathBuf>,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub source_proton: Option<PathBuf>,
    pub origin: Option<String>,
}

impl NewGame {
    pub fn windows(name: impl Into<String>, exe: PathBuf) -> Self {
        Self {
            name: name.into(),
            kind: GameKind::Windows,
            exe,
            prefix: None,
            args: Vec::new(),
            working_dir: None,
            source_proton: None,
            origin: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Library {
    #[serde(rename = "game")]
    pub games: Vec<LibraryGame>,
}

/// The pre-library format (games imported from a prefix), migrated on first load.
#[derive(Deserialize)]
struct LegacyImported {
    #[serde(default, rename = "game")]
    games: Vec<LegacyGame>,
}

#[derive(Deserialize)]
struct LegacyGame {
    id: String,
    name: String,
    prefix: PathBuf,
    exe: PathBuf,
    #[serde(default)]
    args: Vec<String>,
}

impl Library {
    pub fn path() -> anyhow::Result<PathBuf> {
        Ok(paths::config_dir()?.join("library.toml"))
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::migrate_legacy(),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn migrate_legacy() -> anyhow::Result<Self> {
        let legacy_path = paths::config_dir()?.join("imported.toml");
        let Ok(text) = std::fs::read_to_string(&legacy_path) else {
            return Ok(Self::default());
        };
        let legacy: LegacyImported =
            toml::from_str(&text).with_context(|| format!("invalid {}", legacy_path.display()))?;
        let library = Self {
            games: legacy
                .games
                .into_iter()
                .map(|g| LibraryGame {
                    id: g.id,
                    name: g.name,
                    kind: GameKind::Windows,
                    exe: g.exe,
                    prefix: Some(g.prefix),
                    args: g.args,
                    working_dir: None,
                    source_proton: None,
                    origin: None,
                    category: None,
                    hidden: false,
                    favorite: false,
                    cover: None,
                    rating: None,
                    status: None,
                    copy_of: None,
                })
                .collect(),
        };
        library.save()?;
        Ok(library)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::profile::write_toml(&Self::path()?, self)
    }

    pub fn get(&self, id: &str) -> Option<&LibraryGame> {
        self.games.iter().find(|g| g.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut LibraryGame> {
        self.games.iter_mut().find(|g| g.id == id)
    }

    fn unique_id(&self, name: &str) -> String {
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
        id
    }

    /// Adds a game and returns its new id, derived from the name and made unique.
    pub fn add(&mut self, new: NewGame) -> String {
        let id = self.unique_id(&new.name);
        // Whatever the import path: a Windows program inside a prefix keeps that prefix
        // (written down, so the game page shows it and it survives a moved library).
        let prefix = match new.kind {
            GameKind::Windows if new.prefix.is_none() => prefix_containing(&new.exe),
            _ => new.prefix,
        };
        self.games.push(LibraryGame {
            id: id.clone(),
            name: new.name.trim().to_string(),
            kind: new.kind,
            exe: new.exe,
            prefix,
            args: new.args,
            working_dir: new.working_dir,
            source_proton: new.source_proton,
            origin: new.origin,
            category: None,
            hidden: false,
            favorite: false,
            cover: None,
            rating: None,
            status: None,
            copy_of: None,
        });
        id
    }

    /// The other copies of `id` (entries whose `copy_of` is it).
    pub fn copies_of(&self, id: &str) -> Vec<&LibraryGame> {
        self.games
            .iter()
            .filter(|g| g.copy_of.as_deref() == Some(id))
            .collect()
    }

    /// Makes `copy` a copy of `main`: out of the lists, played from `main`'s page. Copies of
    /// `copy` move to `main`.
    pub fn link_copy(&mut self, main: &str, copy: &str) -> anyhow::Result<()> {
        anyhow::ensure!(main != copy, "a game cannot be a copy of itself");
        anyhow::ensure!(self.get(main).is_some(), "`{main}` is not in the library");
        let main = self
            .get(main)
            .and_then(|g| g.copy_of.clone())
            .unwrap_or_else(|| main.to_string());
        for g in &mut self.games {
            if g.copy_of.as_deref() == Some(copy) {
                g.copy_of = Some(main.clone());
            }
        }
        let g = self
            .get_mut(copy)
            .with_context(|| format!("`{copy}` is not in the library"))?;
        g.copy_of = Some(main);
        g.hidden = true;
        Ok(())
    }

    /// Makes a copy a game of its own again.
    pub fn unlink_copy(&mut self, copy: &str) {
        if let Some(g) = self.get_mut(copy)
            && g.copy_of.take().is_some()
        {
            g.hidden = false;
        }
    }

    /// Makes `copy` the one shown and played; the game it was a copy of becomes its copy.
    pub fn make_main(&mut self, copy: &str) -> anyhow::Result<()> {
        let main = self
            .get(copy)
            .and_then(|g| g.copy_of.clone())
            .with_context(|| format!("`{copy}` is not a copy"))?;
        let (hidden, favorite, category, status, rating) = {
            let m = self.get(&main).context("the main copy is gone")?;
            (m.hidden, m.favorite, m.category.clone(), m.status, m.rating)
        };
        if let Some(g) = self.get_mut(copy) {
            g.copy_of = None;
            g.hidden = false;
            // What the user set on the game stays with it.
            g.favorite |= favorite;
            g.category = g.category.take().or(category);
            g.status = g.status.or(status);
            g.rating = g.rating.or(rating);
        }
        let _ = hidden;
        self.link_copy(copy, &main)
    }

    /// Groups of entries that look like the same game (same title, not already linked), the
    /// first of each the one to keep shown.
    pub fn same_games(&self) -> Vec<Vec<String>> {
        let mut by_key: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for g in self.games.iter().filter(|g| g.copy_of.is_none()) {
            let key = title_key(&g.name);
            if key.len() >= 3 {
                by_key.entry(key).or_default().push(g.id.clone());
            }
        }
        by_key.into_values().filter(|ids| ids.len() > 1).collect()
    }

    /// Copies a game under a new id ("<name> (copy)"); its profile is copied by the caller.
    pub fn duplicate(&mut self, id: &str, copy_suffix: &str) -> anyhow::Result<String> {
        let mut copy = self
            .get(id)
            .with_context(|| format!("game `{id}` not found"))?
            .clone();
        copy.name = format!("{} {copy_suffix}", copy.name);
        copy.id = self.unique_id(&copy.name);
        let new_id = copy.id.clone();
        self.games.push(copy);
        Ok(new_id)
    }

    /// Changes the display name. The id stays, so the game's profile stays attached.
    pub fn rename(&mut self, id: &str, name: &str) -> anyhow::Result<()> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "name cannot be empty");
        self.get_mut(id)
            .with_context(|| format!("game `{id}` not found"))?
            .name = name.to_string();
        Ok(())
    }

    pub fn remove(&mut self, id: &str) {
        self.games.retain(|g| g.id != id);
        // Its copies: the first becomes the shown one, the rest its copies.
        let copies: Vec<String> = self.copies_of(id).iter().map(|g| g.id.clone()).collect();
        if let Some((first, rest)) = copies.split_first() {
            self.unlink_copy(first);
            for other in rest {
                let _ = self.link_copy(first, other);
            }
        }
    }

    /// Other games using the same prefix as `id` (to warn before deleting a shared prefix).
    pub fn prefix_shared_with(&self, id: &str, prefixes_dir: &Path) -> Vec<&LibraryGame> {
        let Some(game) = self.get(id).filter(|g| g.needs_prefix()) else {
            return Vec::new();
        };
        let prefix = game.effective_prefix(prefixes_dir);
        self.games
            .iter()
            .filter(|g| {
                g.id != id && g.needs_prefix() && g.effective_prefix(prefixes_dir) == prefix
            })
            .collect()
    }

    /// Whether a game from `origin` (e.g. `heroic:abc`) is already in the library.
    pub fn has_origin(&self, origin: &str) -> bool {
        self.games
            .iter()
            .any(|g| g.origin.as_deref() == Some(origin))
    }

    pub fn games(&self) -> Vec<Game> {
        self.games.iter().map(LibraryGame::to_game).collect()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn exe_inside_a_prefix_runs_in_that_prefix() {
        let exe = Path::new(
            "/mnt/Games/Heroic/Prefixes/Ubisoft Connect/drive_c/users/steamuser/Desktop/Far Cry® 5.url",
        );
        assert_eq!(
            prefix_containing(exe),
            Some(PathBuf::from("/mnt/Games/Heroic/Prefixes/Ubisoft Connect"))
        );
        assert_eq!(prefix_containing(Path::new("/games/fc6/FarCry6.exe")), None);
        let mut lib = Library::default();
        let id = lib.add(NewGame::windows("Far Cry 5", exe.to_path_buf()));
        let game = lib.get(&id).unwrap();
        assert_eq!(
            game.prefix.as_deref(),
            Some(Path::new("/mnt/Games/Heroic/Prefixes/Ubisoft Connect")),
            "written down at import"
        );
        assert_eq!(
            game.effective_prefix(Path::new("/arkor/prefixes")),
            PathBuf::from("/mnt/Games/Heroic/Prefixes/Ubisoft Connect")
        );
        let id = lib.add(NewGame::windows("Other", PathBuf::from("/games/x/x.exe")));
        assert_eq!(
            lib.get(&id)
                .unwrap()
                .effective_prefix(Path::new("/arkor/prefixes")),
            PathBuf::from("/arkor/prefixes/other")
        );
    }

    use super::*;

    fn lib_with(names: &[&str]) -> Library {
        let mut lib = Library::default();
        for n in names {
            lib.add(NewGame::windows(*n, format!("/g/{n}.exe").into()));
        }
        lib
    }

    #[test]
    fn ids_unique_rename_duplicate_remove() {
        let mut lib = lib_with(&["Far Cry® 5", "Far Cry® 5", "™"]);
        let ids: Vec<_> = lib.games.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["far-cry-5", "far-cry-5-2", "game"]);

        lib.rename("far-cry-5-2", "  Far Cry 5 (mods)  ").unwrap();
        assert_eq!(lib.get("far-cry-5-2").unwrap().name, "Far Cry 5 (mods)");
        assert!(lib.rename("far-cry-5-2", "  ").is_err());

        let copy = lib.duplicate("game", "(copy)").unwrap();
        assert_eq!(copy, "copy");
        assert_eq!(lib.get(&copy).unwrap().name, "™ (copy)");

        lib.remove("far-cry-5");
        assert!(lib.get("far-cry-5").is_none());
        assert_eq!(
            lib.get("game").unwrap().to_game().id.to_string(),
            "local:game"
        );
    }

    #[test]
    fn shared_prefix_detection() {
        let mut lib = Library::default();
        let mut a = NewGame::windows("A", "/p/drive_c/a.exe".into());
        a.prefix = Some("/p".into());
        let mut b = NewGame::windows("B", "/p/drive_c/b.exe".into());
        b.prefix = Some("/p".into());
        let c = NewGame::windows("C", "/x/c.exe".into());
        let mut steam = NewGame::windows("S", PathBuf::new());
        steam.kind = GameKind::Steam {
            appid: "730".into(),
        };
        let a_id = lib.add(a);
        lib.add(b);
        let c_id = lib.add(c);
        lib.add(steam);
        let pfx = Path::new("/defaults");
        assert_eq!(
            lib.prefix_shared_with(&a_id, pfx)
                .iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>(),
            ["B"]
        );
        assert!(
            lib.prefix_shared_with(&c_id, pfx).is_empty(),
            "default prefixes are per game"
        );
        assert_eq!(
            lib.get(&c_id).unwrap().effective_prefix(pfx),
            PathBuf::from("/defaults/c")
        );
    }

    #[test]
    fn toml_roundtrip_with_kinds() {
        let mut lib = lib_with(&["W"]);
        let mut s = NewGame::windows("S", "/steam/common/S".into());
        s.kind = GameKind::Steam {
            appid: "730".into(),
        };
        lib.add(s);
        let text = toml::to_string(&lib).unwrap();
        assert!(
            text.contains("[[game]]") && text.contains("appid = \"730\""),
            "{text}"
        );
        assert_eq!(toml::from_str::<Library>(&text).unwrap(), lib);
    }

    #[test]
    fn copies_link_swap_and_survive_removal() {
        let mut lib = Library::default();
        let a = lib.add(NewGame::windows("Cyberpunk 2077", "/gog/c.exe".into()));
        let b = lib.add(NewGame::windows("Cyberpunk 2077®", "/epic/c.exe".into()));
        let c = lib.add(NewGame::windows("Hades", "/h.exe".into()));
        assert_eq!(lib.same_games(), vec![vec![a.clone(), b.clone()]]);
        assert_eq!(
            title_key("The Witcher 3: Wild Hunt - Game of the Year Edition"),
            "thewitcher3wildhunt"
        );
        lib.link_copy(&a, &b).unwrap();
        assert!(lib.get(&b).unwrap().hidden);
        assert_eq!(lib.copies_of(&a).len(), 1);
        assert!(lib.same_games().is_empty());
        lib.get_mut(&a).unwrap().favorite = true;
        lib.make_main(&b).unwrap();
        assert!(!lib.get(&b).unwrap().hidden && lib.get(&b).unwrap().favorite);
        assert_eq!(lib.get(&a).unwrap().copy_of.as_deref(), Some(b.as_str()));
        lib.remove(&b);
        assert!(lib.get(&a).unwrap().copy_of.is_none() && !lib.get(&a).unwrap().hidden);
        assert!(lib.link_copy(&c, &c).is_err());
    }
}
