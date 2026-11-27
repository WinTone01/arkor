//! Emulator games in the library: finding them in ROM folders and adding them.
//!
//! A game becomes an ordinary library entry (`GameKind::Emulated`): its file (or folder),
//! its system and, while the user has not picked one, no emulator of its own (the system's
//! default plays it). Its category is the system's name, so the library groups them.

use std::path::{Path, PathBuf};

use arkor_core::library::{GameKind, Library, NewGame};

use super::{rom_sources, scan, systems};

/// A game found in a ROM folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub system: String,
    pub title: String,
    /// What the emulator opens.
    pub path: PathBuf,
    /// Discs of a game on several.
    pub discs: usize,
    /// Already in the library (by its file).
    pub in_library: bool,
}

/// The games in `roots` (ROM folders, or one system's folder), by title. Only systems Arkor
/// has an emulator for. Blocking: reads every folder, and the headers of files several
/// systems share.
pub fn find_in(roots: &[PathBuf]) -> Vec<Found> {
    // What files were detected as last time: unchanged ones are not opened again.
    let cache_file = super::dirs::cache().join("scan.json");
    let previous: scan::ScanCache = super::read_json(&cache_file);
    let scanned = scan::scan(roots, &previous, &super::dirs::playlists(), &|_| {});
    let mut cache = previous;
    cache.retain(|p, _| !roots.iter().any(|r| p.starts_with(r)));
    cache.extend(scanned.cache);
    let _ = super::write_json(&cache_file, &cache);
    let have: Vec<PathBuf> = Library::load()
        .map(|l| l.games.iter().map(|g| g.exe.clone()).collect())
        .unwrap_or_default();
    scanned
        .entries
        .into_iter()
        .filter(|e| super::playable(&e.system))
        .map(|e| Found {
            in_library: have.contains(&e.path),
            system: e.system,
            title: e.title,
            discs: e.discs.len(),
            path: e.path,
        })
        .collect()
}

/// ROM folders that are already there (EmuDeck's, RetroDECK's, a `ROMs` folder in the home
/// folder or at the top of a disk). Blocking (quick: a few folders).
pub fn suggested_folders() -> Vec<rom_sources::RomSource> {
    let home = ::dirs::home_dir().unwrap_or_default();
    rom_sources::find(&home, &rom_sources::mount_roots())
}

/// The system's name, as shown (`PlayStation`).
pub fn system_name(id: &str) -> String {
    systems::get(id).map_or(id.to_string(), |s| s.name.clone())
}

/// Adds the `found` games not yet in `library` (saving is the caller's); how many.
pub fn add(library: &mut Library, found: &[Found]) -> usize {
    add_except(library, found, &[])
}

/// Like [`add`], leaving out the games at `removed` (taken out of the library by the user).
pub fn add_except(library: &mut Library, found: &[Found], removed: &[PathBuf]) -> usize {
    let mut added = 0;
    for f in found {
        if library.games.iter().any(|g| g.exe == f.path) || removed.contains(&f.path) {
            continue;
        }
        let id = library.add(new_game(&f.system, &f.title, &f.path));
        if let Some(g) = library.get_mut(&id) {
            g.category = Some(system_name(&f.system));
        }
        added += 1;
    }
    added
}

/// A library entry for a game of `system` at `path`.
pub fn new_game(system: &str, title: &str, path: &Path) -> NewGame {
    NewGame {
        name: title.to_string(),
        kind: GameKind::Emulated {
            system: system.to_string(),
            emulator: None,
        },
        exe: path.to_path_buf(),
        prefix: None,
        args: Vec::new(),
        working_dir: None,
        source_proton: None,
        origin: Some(format!("rom:{system}")),
    }
}

/// A game file the user picked, as a library entry, when it is one an emulator plays.
pub fn new_game_for(path: &Path) -> Option<NewGame> {
    let system = system_of(path)?;
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    Some(new_game(&system, &super::names::parse(&stem).title, path))
}

/// Whether a file name looks like a game an emulator plays (by its extension; `.zip` and
/// other shared ones only count inside a system's folder).
pub fn looks_like_rom(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| !systems::by_extension(&e.to_ascii_lowercase()).is_empty())
}

/// A single game file the user picked: its system, if Arkor can tell (by the folder it is
/// in, its extension or its header).
pub fn system_of(path: &Path) -> Option<String> {
    let hint = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| systems::by_folder(&n.to_string_lossy()));
    systems::detect(path, hint)
        .map(|s| s.id.clone())
        .filter(|s| super::playable(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_games_and_skips_systems_without_an_emulator() {
        let root = super::super::test_root("games-find");
        let roms = root.join("roms");
        for (dir, file) in [
            ("snes", "Chrono Trigger (USA).sfc"),
            ("gba", "Metroid Fusion (Europe).gba"),
            ("ps5", "PPSA00001/eboot.bin"),
        ] {
            let p = roms.join(dir).join(file);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        let found = find_in(&[roms]);
        let titles: Vec<(&str, &str)> = found
            .iter()
            .map(|f| (f.system.as_str(), f.title.as_str()))
            .collect();
        assert_eq!(
            titles,
            [("snes", "Chrono Trigger"), ("gba", "Metroid Fusion")]
        );
        assert!(found.iter().all(|f| !f.in_library));
        assert_eq!(
            system_of(&root.join("roms/snes/Chrono Trigger (USA).sfc")).as_deref(),
            Some("snes")
        );
        assert_eq!(system_name("psx"), "PlayStation");
    }
}
