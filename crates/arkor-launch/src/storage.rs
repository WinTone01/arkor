//! What games take on each disk, and the leftovers that can go: prefixes of games no longer
//! in the library, the installers Arkor kept, sandbox folders of removed games. Read only;
//! deleting is asked for one leftover at a time.

use std::path::{Path, PathBuf};

use arkor_core::Global;
use arkor_core::library::Library;

/// A disk: where it is mounted, its size and what is free (bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    pub mount: PathBuf,
    pub size: u64,
    pub free: u64,
}

/// The disks that hold the home folder and the games (from `df`).
pub fn disks(paths: &[PathBuf]) -> Vec<Disk> {
    let mut out: Vec<Disk> = Vec::new();
    for p in paths.iter().filter(|p| p.exists()) {
        let Ok(o) = std::process::Command::new("df")
            .args(["-B1", "--output=target,size,avail"])
            .arg(p)
            .output()
        else {
            continue;
        };
        let text = String::from_utf8_lossy(&o.stdout);
        if let Some(line) = text.lines().nth(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let n = parts.len();
                let disk = Disk {
                    mount: PathBuf::from(parts[..n - 2].join(" ")),
                    size: parts[n - 2].parse().unwrap_or(0),
                    free: parts[n - 1].parse().unwrap_or(0),
                };
                if !out.iter().any(|d| d.mount == disk.mount) {
                    out.push(disk);
                }
            }
        }
    }
    out
}

/// Bytes under `path` (links not followed).
pub fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() && !e.file_type().is_ok_and(|t| t.is_symlink()) {
                stack.push(e.path());
            } else if m.is_file() {
                total += m.len();
            }
        }
    }
    total
}

/// Each library game's folder and size, biggest first.
pub fn game_sizes() -> Vec<(String, PathBuf, u64)> {
    let Ok(lib) = Library::load() else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf, u64)> = lib
        .games
        .iter()
        .filter_map(|g| {
            let dir = g.to_game().install_dir?;
            // A folder of its own, not the home folder or a whole drive.
            (dir.components().count() > 3).then(|| (g.name.clone(), dir))
        })
        .map(|(n, d)| {
            let s = size_of(&d);
            (n, d, s)
        })
        .filter(|(_, _, s)| *s > 0)
        .collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.2));
    out
}

/// Something that can be deleted: what it is, where, how big.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    /// `prefix`, `installers`, `sandbox`.
    pub kind: &'static str,
    pub path: PathBuf,
    pub bytes: u64,
}

/// Leftovers in Arkor's own folders.
pub fn leftovers() -> Vec<Leftover> {
    let (Ok(lib), Ok(global)) = (Library::load(), Global::load_or_import()) else {
        return Vec::new();
    };
    let prefixes_dir = global.prefixes_dir();
    let used: Vec<PathBuf> = lib
        .games
        .iter()
        .map(|g| g.effective_prefix(&prefixes_dir))
        .collect();
    let mut out = Vec::new();
    // Prefixes Arkor made for games that are gone.
    for e in std::fs::read_dir(&prefixes_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let p = e.path();
        if p.join("drive_c").is_dir() && !used.contains(&p) {
            out.push(Leftover {
                kind: "prefix",
                bytes: size_of(&p),
                path: p,
            });
        }
    }
    // Installers of store launchers, kept after they ran.
    if let Some(dir) = dirs::cache_dir().map(|c| c.join("arkor/installers"))
        && dir.is_dir()
    {
        let bytes = size_of(&dir);
        if bytes > 0 {
            out.push(Leftover {
                kind: "installers",
                path: dir,
                bytes,
            });
        }
    }
    // Sandbox folders of games no longer in the library.
    let stems: Vec<String> = lib.games.iter().map(|g| g.game_id().file_stem()).collect();
    if let Some(dir) = dirs::data_dir().map(|d| d.join("arkor/sandbox")) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !stems.contains(&name) {
                out.push(Leftover {
                    kind: "sandbox",
                    bytes: size_of(&e.path()),
                    path: e.path(),
                });
            }
        }
    }
    out.sort_by_key(|l| std::cmp::Reverse(l.bytes));
    out
}

/// Deletes a leftover, only inside the folders `leftovers` looks in.
pub fn remove(l: &Leftover) -> anyhow::Result<()> {
    let global = Global::load_or_import()?;
    let allowed = [
        Some(global.prefixes_dir()),
        dirs::cache_dir().map(|c| c.join("arkor")),
        dirs::data_dir().map(|d| d.join("arkor/sandbox")),
    ];
    anyhow::ensure!(
        allowed
            .iter()
            .flatten()
            .any(|a| l.path.starts_with(a) && l.path != *a),
        "{} is not one of Arkor's folders",
        l.path.display()
    );
    std::fs::remove_dir_all(&l.path)?;
    Ok(())
}

/// "12.4 GB", "830 MB".
pub fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e12 {
        format!("{:.1} TB", b / 1e12)
    } else if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", b / 1e3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_folders_without_following_links() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        std::fs::write(tmp.path().join("a/b/f"), vec![0u8; 1000]).unwrap();
        std::fs::write(tmp.path().join("a/g"), vec![0u8; 24]).unwrap();
        std::os::unix::fs::symlink("/usr", tmp.path().join("a/link")).unwrap();
        assert_eq!(size_of(&tmp.path().join("a")), 1024);
        assert_eq!(human(12_400_000_000), "12.4 GB");
        assert_eq!(human(830_000_000), "830 MB");
    }
}
