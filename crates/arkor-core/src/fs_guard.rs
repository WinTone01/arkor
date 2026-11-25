//! The one place whole folders of the user's are deleted from.
//!
//! [`remove_dir`] refuses anything that could hold more than what is meant (a home folder, a
//! drive, a library, another launcher's folder). [`trash`] moves to the desktop's trash what
//! may hold saves.

use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Component, Path, PathBuf};

use anyhow::Context;

/// Folders whose *contents* may be deleted, but never they themselves, below the home folder.
const KEPT_IN_HOME: &[&str] = &[
    ".cache",
    ".config",
    ".local",
    ".local/bin",
    ".local/share",
    ".local/state",
    ".steam",
    ".steam/root",
    ".steam/steam",
    ".wine",
    ".local/share/Steam",
    ".local/share/Steam/steamapps",
    ".local/share/Steam/steamapps/common",
    ".local/share/Steam/steamapps/compatdata",
    ".local/share/Steam/compatibilitytools.d",
    ".local/share/Steam/userdata",
    ".local/share/lutris",
    ".local/share/bottles",
    ".local/share/umu",
    ".local/share/arkor",
    ".local/share/arkor/stores",
    ".local/share/arkor/saves",
    ".config/arkor",
    ".config/heroic",
    ".config/legendary",
    ".config/faugus-launcher",
    "Games",
    "Games/Arkor",
    "Games/Heroic",
    "Faugus",
];

/// Nothing at or right below these (`/mnt/disk`, `/run/media/user`) is ever deleted whole.
const SYSTEM: &[&str] = &[
    "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/opt", "/proc", "/root", "/sbin", "/sys",
    "/usr", "/var",
];

/// What a check knows about this computer (tests make their own).
#[derive(Debug, Clone, Default)]
pub struct Guard {
    home: Option<PathBuf>,
    /// Folders that hold many games: never deleted, nor anything above them.
    roots: Vec<PathBuf>,
}

impl Guard {
    /// The guard of this user: their home, Arkor's folders and other launchers' libraries.
    pub fn new() -> Guard {
        let mut roots: Vec<PathBuf> = [
            dirs::config_dir(),
            dirs::data_dir(),
            dirs::cache_dir(),
            dirs::state_dir(),
            dirs::document_dir(),
            dirs::download_dir(),
            dirs::desktop_dir(),
            dirs::picture_dir(),
            dirs::video_dir(),
            dirs::audio_dir(),
        ]
        .into_iter()
        .flatten()
        .collect();
        // The settings as they are: this must not create them.
        let global = crate::Global::path()
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| toml::from_str::<crate::Global>(&t).ok())
            .unwrap_or_default();
        roots.extend([global.games_dir(), global.prefixes_dir()]);
        if let Some(data) = dirs::data_dir() {
            roots.extend(["arkor", "arkor/stores", "arkor/saves"].map(|d| data.join(d)));
        }
        Guard {
            home: dirs::home_dir(),
            roots,
        }
    }

    #[cfg(test)]
    fn at(home: &Path, roots: &[&Path]) -> Guard {
        Guard {
            home: Some(home.to_path_buf()),
            roots: roots.iter().map(|r| r.to_path_buf()).collect(),
        }
    }

    /// More folders that must stay (e.g. the library folders of Steam).
    pub fn keeping(mut self, roots: impl IntoIterator<Item = PathBuf>) -> Guard {
        self.roots.extend(roots);
        self
    }

    /// Whether the folder `dir` may be deleted as a whole. Says why not.
    /// Returns its real place (links on the way resolved).
    pub fn check(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        let refuse = |why: &str| anyhow::anyhow!("{} is not deleted: {why}", dir.display());
        if !dir.is_absolute() {
            return Err(refuse("its path is not complete"));
        }
        if dir
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(refuse("its path has `..` in it"));
        }
        let meta = std::fs::symlink_metadata(dir)
            .with_context(|| format!("{} cannot be deleted", dir.display()))?;
        if meta.is_symlink() {
            return Err(refuse("it is a link to another place"));
        }
        if !meta.is_dir() {
            return Err(refuse("it is not a folder"));
        }
        let real = std::fs::canonicalize(dir)
            .with_context(|| format!("{} cannot be deleted", dir.display()))?;
        // Both what was asked for and where it really is.
        for path in [dir, &real] {
            // "/", "/home", "/mnt/disk".
            if path.components().count() < 4 {
                return Err(refuse("it is too near the top of the disk"));
            }
            if SYSTEM.iter().any(|s| path.starts_with(s)) {
                return Err(refuse("it belongs to the system"));
            }
            // "/run/media/user/disk" is a drive.
            if path.starts_with("/run/media") && path.components().count() < 6 {
                return Err(refuse("it is a drive"));
            }
        }
        // Another file system begins here: a drive, not a folder.
        if let Some(parent) = real.parent()
            && std::fs::metadata(parent).is_ok_and(|p| p.dev() != meta.dev())
        {
            return Err(refuse("it is a drive"));
        }
        let home = self.home.iter().flat_map(|h| with_real(h));
        for home in home {
            if home.starts_with(&real) {
                return Err(refuse("your own folder is in it"));
            }
            if real.parent() == Some(&home) {
                return Err(refuse("it lies right in your own folder"));
            }
            for kept in KEPT_IN_HOME {
                if with_real(&home.join(kept))
                    .iter()
                    .any(|k| k.starts_with(&real))
                {
                    return Err(refuse("it holds more than this"));
                }
            }
        }
        if self
            .roots
            .iter()
            .flat_map(|r| with_real(r))
            .any(|r| r.starts_with(&real))
        {
            return Err(refuse("it holds more than this"));
        }
        Ok(real)
    }
}

/// A path, and where it really is when that is another place.
fn with_real(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    if let Ok(real) = std::fs::canonicalize(path)
        && real != path
    {
        out.push(real);
    }
    out
}

/// Whether `dir` may be deleted as a whole (see [`Guard::check`]).
pub fn check(dir: &Path) -> anyhow::Result<PathBuf> {
    Guard::new().check(dir)
}

/// Deletes a folder for good, if it may be. Links in it are removed, not followed.
pub fn remove_dir(dir: &Path) -> anyhow::Result<()> {
    remove_checked(&Guard::new(), dir)
}

/// Deletes a folder for good, if it may be and lies inside `root`.
pub fn remove_dir_in(dir: &Path, root: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        inside(dir, root),
        "{} is not in {}",
        dir.display(),
        root.display()
    );
    remove_dir(dir)
}

/// As [`remove_dir`], with folders that must stay besides the usual ones.
pub fn remove_dir_keeping(
    dir: &Path,
    roots: impl IntoIterator<Item = PathBuf>,
) -> anyhow::Result<()> {
    remove_checked(&Guard::new().keeping(roots), dir)
}

fn remove_checked(guard: &Guard, dir: &Path) -> anyhow::Result<()> {
    let real = guard.check(dir)?;
    std::fs::remove_dir_all(&real).with_context(|| format!("deleting {}", dir.display()))
}

fn inside(dir: &Path, root: &Path) -> bool {
    let real = |p: &Path| std::fs::canonicalize(p).ok();
    match (real(dir), real(root)) {
        (Some(d), Some(r)) => d != r && d.starts_with(r),
        _ => false,
    }
}

// ---- Trash ----

/// Moves a folder to the desktop's trash (it can be put back from there). On another disk
/// that has no trash of its own the folder stays and this says so.
pub fn trash(dir: &Path) -> anyhow::Result<()> {
    let real = Guard::new().check(dir)?;
    let home = dirs::data_dir().context("no XDG data dir")?.join("Trash");
    let dev = std::fs::symlink_metadata(&real)?.dev();
    let home_dev = home
        .ancestors()
        .find_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.dev());
    if home_dev == Some(dev) {
        return trash_into(&real, &home, None);
    }
    // The trash at the top of the folder's own disk, as the desktop's file managers use it.
    let top = real
        .ancestors()
        .take_while(|p| std::fs::metadata(p).is_ok_and(|m| m.dev() == dev))
        .last()
        .context("no disk")?
        .to_path_buf();
    // SAFETY: no arguments, cannot fail.
    let uid = unsafe { libc::getuid() };
    let shared = top.join(".Trash");
    let sticky =
        std::fs::symlink_metadata(&shared).is_ok_and(|m| m.is_dir() && m.mode() & 0o1000 != 0);
    if sticky && trash_into(&real, &shared.join(uid.to_string()), Some(&top)).is_ok() {
        return Ok(());
    }
    trash_into(&real, &top.join(format!(".Trash-{uid}")), Some(&top))
}

fn trash_into(real: &Path, trash: &Path, top: Option<&Path>) -> anyhow::Result<()> {
    let private = |dir: &Path| {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("no trash at {}", dir.display()))
    };
    anyhow::ensure!(!trash.is_symlink(), "{} is a link", trash.display());
    let (files, info) = (trash.join("files"), trash.join("info"));
    private(&files)?;
    private(&info)?;
    let name = real
        .file_name()
        .context("no folder name")?
        .to_string_lossy()
        .into_owned();
    // Paths on another disk are noted from its top.
    let noted = top.and_then(|t| real.strip_prefix(t).ok()).unwrap_or(real);
    let text = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent(noted),
        local_time()
    );
    for n in 1..10_000 {
        let name = if n == 1 {
            name.clone()
        } else {
            format!("{name}.{n}")
        };
        let note = info.join(format!("{name}.trashinfo"));
        use std::io::Write;
        let mut f = match std::fs::File::create_new(&note) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).context("writing to the trash"),
        };
        let dest = files.join(&name);
        let moved = f
            .write_all(text.as_bytes())
            .and_then(|()| f.sync_all())
            .and_then(|()| {
                if dest.symlink_metadata().is_ok() {
                    Err(std::io::ErrorKind::AlreadyExists.into())
                } else {
                    std::fs::rename(real, &dest)
                }
            });
        return match moved {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&note);
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    continue;
                }
                Err(e).with_context(|| format!("moving {} to the trash", real.display()))
            }
        };
    }
    anyhow::bail!("the trash is full of folders named {name}")
}

/// A path as the trash's notes write it.
fn percent(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for b in path.as_os_str().as_bytes() {
        match b {
            b'/' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            b if b.is_ascii_alphanumeric() => out.push(*b as char),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `2026-11-26T18:05:00` in local time.
fn local_time() -> String {
    // SAFETY: plain libc calls into a zeroed struct.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A home with a few folders, deep enough to pass the "too near the top" rule.
    fn home() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home/someone");
        for d in [
            ".config/heroic",
            ".local/share/Steam/steamapps/common/Game",
            "Games/Arkor/Games/epic/Fine Game",
            "Documents",
        ] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        (tmp, home)
    }

    #[test]
    fn refuses_what_holds_more() {
        let (tmp, home) = home();
        let games = home.join("Games/Arkor/Games");
        let g = Guard::at(&home, &[&games]);
        for bad in [
            PathBuf::from("/"),
            PathBuf::from("/home"),
            PathBuf::from("/usr/share/doc"),
            PathBuf::from("relative/path"),
            tmp.path().to_path_buf(),
            home.parent().unwrap().to_path_buf(),
            home.clone(),
            home.join("Documents"),
            home.join(".config"),
            home.join(".config/heroic"),
            home.join(".local/share/Steam"),
            home.join(".local/share/Steam/steamapps/common"),
            home.join("Games"),
            home.join("Games/Arkor"),
            games.clone(),
            games.join("epic/../epic/Fine Game"),
            home.join("not there/at all"),
        ] {
            assert!(g.check(&bad).is_err(), "{} passed", bad.display());
        }
        let ok = [
            games.join("epic/Fine Game"),
            home.join(".local/share/Steam/steamapps/common/Game"),
        ];
        for good in ok {
            g.check(&good).unwrap();
        }
        // Folders the caller adds.
        let g = g.keeping([games.join("epic/Fine Game/library")]);
        assert!(g.check(&games.join("epic/Fine Game")).is_err());
    }

    #[test]
    fn links_are_not_followed() {
        let (_tmp, home) = home();
        let g = Guard::at(&home, &[]);
        let games = home.join("Games/Arkor/Games/epic");
        let link = games.join("linked");
        std::os::unix::fs::symlink(home.join("Documents"), &link).unwrap();
        assert!(g.check(&link).is_err());
        // A link on the way that leads home.
        let way = games.join("way");
        std::os::unix::fs::symlink(&home, &way).unwrap();
        assert!(g.check(&way.join("Documents")).is_err());
        // A link inside a folder: what it points to stays.
        let game = games.join("Fine Game");
        std::fs::write(home.join("Documents/letter"), "keep").unwrap();
        std::os::unix::fs::symlink(home.join("Documents"), game.join("docs")).unwrap();
        remove_checked(&g, &game).unwrap();
        assert!(!game.exists());
        assert!(home.join("Documents/letter").exists());
    }

    #[test]
    fn trash_can_be_put_back() {
        let (tmp, home) = home();
        let trash = tmp.path().join("Trash");
        let games = home.join("Games/Arkor/Games/epic");
        for text in ["one", "two"] {
            let game = games.join("Fine Game");
            std::fs::create_dir_all(&game).unwrap();
            std::fs::write(game.join("save"), text).unwrap();
            trash_into(&game, &trash, None).unwrap();
            assert!(!game.exists());
        }
        let read = |p: &str| std::fs::read_to_string(trash.join(p)).unwrap();
        assert_eq!(read("files/Fine Game/save"), "one");
        assert_eq!(read("files/Fine Game.2/save"), "two");
        let note = read("info/Fine Game.2.trashinfo");
        assert!(note.starts_with("[Trash Info]\nPath=/"), "{note}");
        assert!(note.contains("Fine%20Game\n"), "{note}");
        assert!(note.contains("DeletionDate=2"), "{note}");
    }

    #[test]
    fn inside_means_below() {
        let (_tmp, home) = home();
        let games = home.join("Games/Arkor/Games");
        assert!(inside(&games.join("epic"), &games));
        assert!(!inside(&games, &games));
        assert!(!inside(&home.join("Documents"), &games));
    }
}
