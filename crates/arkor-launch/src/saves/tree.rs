//! What lies in a save folder, told apart by its contents (not by dates, which copying,
//! clocks and stores change).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Context;

/// A save folder larger than this is not one (a wrong place, or a game that keeps more there).
pub const MAX_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_FILES: usize = 20_000;

/// Which files of a folder are saves (the store says so for some games).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

impl Filter {
    /// Lists as Epic writes them: `*.sav, settings.ini`.
    pub fn new(include: &str, exclude: &str) -> Filter {
        let list = |s: &str| {
            s.split(',')
                .map(|p| p.trim().replace('\\', "/"))
                .filter(|p| !p.is_empty())
                .collect()
        };
        Filter {
            include: list(include),
            exclude: list(exclude),
        }
    }

    fn takes(&self, rel: &str) -> bool {
        let any = |list: &[String]| list.iter().any(|p| matches(rel, p));
        (self.include.is_empty() || any(&self.include)) && !any(&self.exclude)
    }
}

/// As Epic's launcher matches: a folder's beginning, a name's end, or with `*` and `?`
/// (which go over folders, too). Windows does not tell upper and lower case apart.
fn matches(rel: &str, pattern: &str) -> bool {
    let (rel, pattern) = (rel.to_lowercase(), pattern.to_lowercase());
    if pattern.ends_with('/') && rel.starts_with(&pattern) {
        return true;
    }
    rel.ends_with(&pattern) || wildcard(rel.as_bytes(), pattern.as_bytes())
}

fn wildcard(text: &[u8], pattern: &[u8]) -> bool {
    let (mut t, mut p) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(c) if *c == b'?' || *c == text[t] => {
                t += 1;
                p += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    p = sp + 1;
                    t = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|c| *c == b'*')
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The path below the folder, as it is on the disk.
    pub rel: PathBuf,
    pub size: u64,
    pub hash: u128,
    pub modified: SystemTime,
}

/// The save files in a folder. The key is the path in lower case, as Windows compares them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tree(pub BTreeMap<String, Entry>);

impl Tree {
    /// Reads a folder; one that is not there is empty. Links are left out: they could lead
    /// anywhere.
    pub fn read(dir: &Path, filter: &Filter) -> anyhow::Result<Tree> {
        let mut tree = Tree::default();
        let mut bytes = 0;
        if dir.exists() {
            tree.walk(dir, Path::new(""), filter, &mut bytes)?;
        }
        Ok(tree)
    }

    fn walk(
        &mut self,
        dir: &Path,
        rel: &Path,
        filter: &Filter,
        bytes: &mut u64,
    ) -> anyhow::Result<()> {
        let list = std::fs::read_dir(dir.join(rel))
            .with_context(|| format!("reading {}", dir.join(rel).display()))?;
        for e in list {
            let e = e?;
            let rel = rel.join(e.file_name());
            let kind = e.file_type()?;
            if kind.is_dir() {
                self.walk(dir, &rel, filter, bytes)?;
            } else if kind.is_file() {
                // A name that is no text cannot be a Windows game's.
                let Some(text) = rel.to_str() else { continue };
                if !filter.takes(text) {
                    continue;
                }
                let meta = e.metadata()?;
                *bytes += meta.len();
                anyhow::ensure!(
                    *bytes <= MAX_BYTES && self.0.len() < MAX_FILES,
                    "{} holds too much to be only saves",
                    dir.display()
                );
                self.0.insert(
                    text.to_lowercase(),
                    Entry {
                        size: meta.len(),
                        hash: hash_file(&e.path())?,
                        modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        rel,
                    },
                );
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn bytes(&self) -> u64 {
        self.0.values().map(|e| e.size).sum()
    }

    /// When something in it was last changed (unix time).
    pub fn newest(&self) -> Option<u64> {
        self.0
            .values()
            .filter_map(|e| e.modified.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .max()
    }

    /// The same for the same files with the same contents.
    pub fn print(&self) -> String {
        let mut h = Fnv::default();
        for (key, e) in &self.0 {
            h.add(key.as_bytes());
            h.add(&[0]);
            h.add(&e.size.to_le_bytes());
            h.add(&e.hash.to_le_bytes());
        }
        format!("{}-{:032x}", self.0.len(), h.0)
    }

    /// Makes `to` hold just these files of `from` (files that are not saves stay).
    pub fn apply(&self, from: &Path, to: &Path, filter: &Filter) -> anyhow::Result<()> {
        let old = Tree::read(to, filter)?;
        for (key, e) in &self.0 {
            let same = old.0.get(key);
            if same.is_some_and(|o| o.hash == e.hash && o.size == e.size) {
                continue;
            }
            // Where the game has it, whatever its case.
            let rel = same.map_or_else(|| in_case_of(to, &e.rel), |o| o.rel.clone());
            place(&from.join(&e.rel), to, &rel, e.modified)?;
        }
        for (key, e) in &old.0 {
            if !self.0.contains_key(key) {
                let path = to.join(&e.rel);
                std::fs::remove_file(&path)
                    .with_context(|| format!("removing {}", path.display()))?;
            }
        }
        let now = Tree::read(to, filter)?;
        anyhow::ensure!(
            now.print() == self.print(),
            "{} did not become what it should",
            to.display()
        );
        Ok(())
    }
}

/// Writes one file whole, with its date.
fn place(from: &Path, to: &Path, rel: &Path, modified: SystemTime) -> anyhow::Result<()> {
    let dest = to.join(rel);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut name = std::ffi::OsString::from(".arkor-");
    name.push(dest.file_name().context("no file name")?);
    let fresh = dest.with_file_name(name);
    let result = (|| {
        std::fs::copy(from, &fresh)?;
        let f = std::fs::File::options().write(true).open(&fresh)?;
        f.set_modified(modified)?;
        f.sync_all()?;
        std::fs::rename(&fresh, &dest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&fresh);
    }
    result.with_context(|| format!("writing {}", dest.display()))
}

/// `rel` below `base`, with the folders that are there in their own case.
pub fn in_case_of(base: &Path, rel: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in rel.components() {
        let name = part.as_os_str();
        let here = base.join(&out);
        if !here.join(name).exists()
            && let Some(other) = std::fs::read_dir(&here)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.file_name())
                .find(|n| {
                    n.to_string_lossy().to_lowercase() == name.to_string_lossy().to_lowercase()
                })
        {
            out.push(other);
        } else {
            out.push(name);
        }
    }
    out
}

fn hash_file(path: &Path) -> anyhow::Result<u128> {
    let mut f = std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut h = Fnv::default();
    let mut buf = vec![0; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(h.0);
        }
        h.add(&buf[..n]);
    }
}

/// FNV-1a with 128 bits: the same on every version, enough to tell saves apart.
struct Fnv(u128);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0x6c62272e07bb014262b821756295c58d)
    }
}

impl Fnv {
    fn add(&mut self, data: &[u8]) {
        for b in data {
            self.0 ^= u128::from(*b);
            self.0 = self.0.wrapping_mul(0x0000000001000000000000000000013B);
        }
    }
}

/// A short mark of any text.
pub fn mark(text: &str) -> String {
    let mut h = Fnv::default();
    h.add(text.as_bytes());
    format!("{:032x}", h.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn filters_as_epic_does() {
        let f = Filter::new("falloutprefs.ini, *.fos", "");
        assert!(f.takes("falloutprefs.ini"));
        assert!(f.takes("Save 1.fos"));
        assert!(!f.takes("other.bak"));
        let f = Filter::new("extraContent.xml, inputBinding.xml, savegame*", "");
        assert!(f.takes("savegame1/careerSavegame.xml"));
        assert!(!f.takes("music/one.ogg"));
        let f = Filter::new("", "announcementBanners.bin,cache/");
        assert!(f.takes("save.bin"));
        assert!(!f.takes("announcementbanners.bin"));
        assert!(!f.takes("Cache/x"));
        assert!(wildcard(b"a.bak.1", b"*.bak.*"));
        assert!(!wildcard(b"a.bak", b"*.bak.*"));
        assert!(wildcard(b"abc", b"a?c"));
    }

    #[test]
    fn same_contents_have_the_same_print() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        put(&a, "Slots/One.sav", "1");
        put(&b, "slots/one.sav", "1");
        let read = |d: &Path| Tree::read(d, &Filter::default()).unwrap();
        assert_eq!(read(&a).print(), read(&b).print());
        put(&b, "slots/one.sav", "2");
        assert_ne!(read(&a).print(), read(&b).print());
        assert!(read(&tmp.path().join("none")).is_empty());
        // Written down in state files: must never change.
        assert_eq!(mark("arkor"), mark("arkor"),);
        assert_eq!(hash_file(&a.join("Slots/One.sav")).unwrap(), {
            let mut h = Fnv::default();
            h.add(b"1");
            h.0
        });
    }

    #[test]
    fn applying_keeps_case_dates_and_other_files() {
        let tmp = tempfile::tempdir().unwrap();
        let (from, to) = (tmp.path().join("from"), tmp.path().join("to"));
        put(&from, "slots/one.sav", "new");
        put(&from, "extra/two.sav", "2");
        put(&to, "Slots/One.sav", "old");
        put(&to, "gone.sav", "x");
        put(&to, "settings.ini", "mine");
        let f = Filter::new("*.sav", "");
        let date = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        std::fs::File::options()
            .write(true)
            .open(from.join("slots/one.sav"))
            .unwrap()
            .set_modified(date)
            .unwrap();
        let tree = Tree::read(&from, &f).unwrap();
        tree.apply(&from, &to, &f).unwrap();
        assert_eq!(
            std::fs::read_to_string(to.join("Slots/One.sav")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::metadata(to.join("Slots/One.sav"))
                .unwrap()
                .modified()
                .unwrap(),
            date
        );
        assert!(to.join("extra/two.sav").exists());
        assert!(!to.join("gone.sav").exists());
        assert!(to.join("settings.ini").exists());
        assert!(!to.join("slots").exists());
    }

    #[test]
    fn links_are_left_out() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a");
        put(&dir, "one.sav", "1");
        put(tmp.path(), "secret", "s");
        std::os::unix::fs::symlink(tmp.path().join("secret"), dir.join("link.sav")).unwrap();
        std::os::unix::fs::symlink(tmp.path(), dir.join("up")).unwrap();
        let t = Tree::read(&dir, &Filter::default()).unwrap();
        assert_eq!(t.0.keys().collect::<Vec<_>>(), ["one.sav"]);
    }
}
