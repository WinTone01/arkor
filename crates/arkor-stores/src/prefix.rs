//! Importing games from an existing Wine prefix: validate the folder and find executables
//! that look like games.

use std::path::{Path, PathBuf};

/// Accepts a prefix root (has `drive_c`), a Steam/umu `compatdata` dir (has `pfx/drive_c`)
/// or a `drive_c` folder, and returns the prefix root.
pub fn resolve(path: &Path) -> Option<PathBuf> {
    if path.join("drive_c").is_dir() {
        return Some(path.to_path_buf());
    }
    if path.join("pfx/drive_c").is_dir() {
        return Some(path.join("pfx"));
    }
    if path.file_name().is_some_and(|n| n == "drive_c") && path.is_dir() {
        return path.parent().map(Path::to_path_buf);
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub path: PathBuf,
    pub size: u64,
    /// The file name matches its game folder (e.g. `FarCry5.exe` in `Far Cry 5/bin`).
    pub name_matches_folder: bool,
}

/// Name fragments of executables that are never the game itself.
const NOT_GAMES: &[&str] = &[
    "unins",
    "setup",
    "install",
    "redist",
    "vcredist",
    "vc_redist",
    "dxsetup",
    "dotnet",
    "crash",
    "report",
    "updater",
    "update",
    "helper",
    "cefprocess",
    "webhelper",
    "touchup",
    "easyanticheat_setup",
    "uninst",
    "service",
    "elevation",
    "webcore",
    "shareplay",
    "bugsplat",
];

/// Folders Wine itself creates under Program Files, holding stub programs (iexplore, wordpad…).
fn is_wine_stub_dir(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("windows ")
        || [
            "internet explorer",
            "common files",
            "microsoft.net",
            "msbuild",
            "reference assemblies",
        ]
        .contains(&n.as_str())
}

/// Folders under `drive_c` that hold Windows itself or per-user data, not installed games.
const SKIP_DIRS: &[&str] = &["windows", "programdata", "users", "temp"];

/// Executables that look like games. Ones named like their game folder come first (some
/// games ship a tiny loader exe), then the biggest (game binaries tend to be the largest).
/// Also includes Desktop shortcuts (`.lnk`/`.url`) that launchers such as Ubisoft Connect
/// create. Scans a bounded depth so huge prefixes stay quick.
pub fn find_executables(prefix: &Path) -> Vec<Candidate> {
    let drive_c = prefix.join("drive_c");
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&drive_c) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if entry.path().is_dir() && !SKIP_DIRS.contains(&name.as_str()) {
                walk(&entry.path(), 5, &mut found);
            }
        }
    }
    if let Ok(users) = std::fs::read_dir(drive_c.join("users")) {
        for user in users.flatten() {
            if let Ok(items) = std::fs::read_dir(user.path().join("Desktop")) {
                for item in items.flatten() {
                    let path = item.path();
                    if has_ext(&path, &["lnk", "url"]) && looks_like_game(&path) {
                        found.push(Candidate {
                            size: 0,
                            path,
                            name_matches_folder: false,
                        });
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| {
        b.name_matches_folder
            .cmp(&a.name_matches_folder)
            .then(b.size.cmp(&a.size))
            .then_with(|| a.path.cmp(&b.path))
    });
    found.truncate(200);
    found
}

/// Executables in a game's own install folder, the likeliest main binary first: Unreal
/// Engine `*-Shipping.exe` (the real game, next to which proxy DLLs must go), then ones
/// named like their folder, then by size.
pub fn game_executables(install_dir: &Path) -> Vec<Candidate> {
    let mut found = Vec::new();
    walk(install_dir, 6, &mut found);
    let shipping = |c: &Candidate| {
        c.path
            .file_stem()
            .is_some_and(|s| s.to_string_lossy().to_lowercase().ends_with("-shipping"))
    };
    found.sort_by(|a, b| {
        shipping(b)
            .cmp(&shipping(a))
            .then(b.name_matches_folder.cmp(&a.name_matches_folder))
            .then(b.size.cmp(&a.size))
            .then_with(|| a.path.cmp(&b.path))
    });
    found.truncate(50);
    found
}

fn walk(dir: &Path, depth: u32, found: &mut Vec<Candidate>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            let stub = entry.file_name().to_str().is_some_and(is_wine_stub_dir);
            if depth > 0 && !stub {
                walk(&path, depth - 1, found);
            }
        } else if has_ext(&path, &["exe"]) && looks_like_game(&path) {
            found.push(Candidate {
                size: meta.len(),
                name_matches_folder: matches_folder(&path),
                path,
            });
        }
    }
}

/// Lowercase letters and digits only, so "Far Cry® 5" and "FarCry5" compare equal.
fn squash(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn matches_folder(exe: &Path) -> bool {
    let stem = squash(
        &exe.file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default(),
    );
    stem.len() >= 3
        && exe
            .ancestors()
            .skip(1)
            .take(3)
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .any(|folder| squash(folder) == stem)
}

/// "215 KB", "1.4 GB" — for listing candidates.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value >= 10.0 || unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

fn looks_like_game(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    !NOT_GAMES.iter().any(|bad| stem.contains(bad))
}

/// A readable default name: the shortcut's or game folder's name rather than e.g. `bin`.
pub fn suggested_name(exe: &Path) -> String {
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if has_ext(exe, &["lnk", "url"]) {
        return stem;
    }
    const GENERIC: &[&str] = &[
        "bin", "binaries", "win64", "win32", "x64", "x86", "game", "retail",
    ];
    exe.ancestors()
        .skip(1)
        .filter_map(|p| p.file_name()?.to_str())
        .find(|n| !GENERIC.contains(&n.to_lowercase().as_str()))
        .map(str::to_owned)
        .unwrap_or(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path, size: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![0u8; size]).unwrap();
    }

    #[test]
    fn resolves_prefix_shapes() {
        let tmp = tempfile::tempdir().unwrap();
        let pfx = tmp.path().join("compat/pfx");
        std::fs::create_dir_all(pfx.join("drive_c")).unwrap();
        assert_eq!(resolve(&tmp.path().join("compat")), Some(pfx.clone()));
        assert_eq!(resolve(&pfx), Some(pfx.clone()));
        assert_eq!(resolve(&pfx.join("drive_c")), Some(pfx.clone()));
        assert_eq!(resolve(tmp.path()), None);
    }

    #[test]
    fn finds_games_not_installers() {
        let tmp = tempfile::tempdir().unwrap();
        let c = tmp.path().join("drive_c");
        touch(
            &c.join("Program Files/Studio/Big Game/bin/BigGame.exe"),
            300,
        );
        touch(&c.join("Program Files/Studio/Big Game/unins000.exe"), 900);
        touch(
            &c.join("Program Files/Studio/Big Game/_redist/vc_redist.x64.exe"),
            900,
        );
        touch(&c.join("Games/Small/Small.exe"), 100);
        // Tiny loader named like its game folder beats a big launcher exe.
        touch(&c.join("Ubi/Launcher/upc.exe"), 800);
        touch(&c.join("Ubi/Launcher/UplayService.exe"), 800);
        touch(&c.join("Ubi/Launcher/games/Far Cry 5/bin/FarCry5.exe"), 50);
        touch(&c.join("Program Files/Internet Explorer/iexplore.exe"), 999);
        touch(&c.join("windows/system32/notepad.exe"), 999);
        touch(&c.join("users/steamuser/Desktop/Far Cry 5.url"), 10);
        touch(&c.join("users/steamuser/Desktop/notes.txt"), 10);

        let found: Vec<String> = find_executables(tmp.path())
            .iter()
            .map(|c| {
                c.path
                    .strip_prefix(tmp.path())
                    .unwrap()
                    .display()
                    .to_string()
            })
            .collect();
        assert_eq!(
            found,
            [
                // Named like their folders, biggest first…
                "drive_c/Program Files/Studio/Big Game/bin/BigGame.exe",
                "drive_c/Games/Small/Small.exe",
                "drive_c/Ubi/Launcher/games/Far Cry 5/bin/FarCry5.exe",
                // …then the rest.
                "drive_c/Ubi/Launcher/upc.exe",
                "drive_c/users/steamuser/Desktop/Far Cry 5.url",
            ]
        );
    }

    #[test]
    fn unreal_shipping_exe_comes_first() {
        let tmp = tempfile::tempdir().unwrap();
        let g = tmp.path();
        touch(&g.join("MyGame.exe"), 500);
        touch(
            &g.join("MyGame/Binaries/Win64/MyGame-Win64-Shipping.exe"),
            100,
        );
        touch(&g.join("Engine/Extras/Redist/UEPrereqSetup_x64.exe"), 900);
        let found: Vec<_> = game_executables(g).into_iter().map(|c| c.path).collect();
        assert_eq!(
            found,
            [
                g.join("MyGame/Binaries/Win64/MyGame-Win64-Shipping.exe"),
                g.join("MyGame.exe")
            ]
        );
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(human_size(215_560), "216 KB");
        assert_eq!(human_size(1_400_000), "1.4 MB");
        assert_eq!(human_size(52_000_000_000), "52 GB");
    }

    #[test]
    fn names_skip_generic_folders() {
        assert_eq!(
            suggested_name(Path::new("/c/Program Files/Big Game/bin/x64/BG.exe")),
            "Big Game"
        );
        assert_eq!(
            suggested_name(Path::new("/c/users/u/Desktop/Far Cry 5.url")),
            "Far Cry 5"
        );
    }
}
