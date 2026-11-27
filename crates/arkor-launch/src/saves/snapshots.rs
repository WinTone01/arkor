//! Copies of a game's saves, made before anything is put over them.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::places::Place;
use super::tree::{Filter, Tree};

/// Copies kept per save folder (and as many of those the user made).
const KEEP: usize = 12;

/// Why a copy was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Why {
    /// Before saves from the cloud were put over them.
    BeforeDownload,
    /// What the cloud had, before this computer's saves were put over it.
    Cloud,
    /// Before an older copy was put back.
    BeforeRestore,
    /// Asked for.
    Manual,
    /// After the game was played (saves of any game, found through Ludusavi's manifest).
    AfterPlay,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Its folder's name.
    #[serde(skip)]
    pub id: String,
    /// When it was made (unix time).
    pub time: u64,
    pub why: Why,
    /// The save folder it is of.
    pub place: String,
    pub path: PathBuf,
    /// When the saves in it were last changed (unix time).
    pub saved: Option<u64>,
    pub files: usize,
    pub bytes: u64,
    print: String,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

impl Snapshot {
    fn filter(&self) -> Filter {
        Filter {
            include: self.include.clone(),
            exclude: self.exclude.clone(),
        }
    }
}

/// The second part of "<time>-<number>".
fn number(s: &Snapshot) -> u32 {
    s.id.rsplit('-')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// The copies in a game's folder, newest first.
pub fn list(dir: &Path) -> Vec<Snapshot> {
    let mut out: Vec<Snapshot> = std::fs::read_dir(dir.join("snapshots"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path().join("about.json")).ok()?;
            let mut s: Snapshot = serde_json::from_str(&text).ok()?;
            s.id = e.file_name().to_str()?.to_string();
            // One that is still being made.
            (!s.id.starts_with('.')).then_some(s)
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse((s.time, number(s))));
    out
}

/// Copies what is in `from` (a place's folder, or a copy of one). `None` when there is
/// nothing, or a copy with the same contents is there already.
pub fn take(dir: &Path, place: &Place, from: &Path, why: Why) -> anyhow::Result<Option<Snapshot>> {
    let tree = Tree::read(from, &place.filter)?;
    if tree.is_empty() {
        return Ok(None);
    }
    let print = tree.print();
    let old = list(dir);
    if old
        .iter()
        .any(|s| s.place == place.name && s.print == print)
    {
        return Ok(None);
    }
    let root = dir.join("snapshots");
    std::fs::create_dir_all(&root)?;
    // Never older than the copies there are, whatever the clock says.
    let time = arkor_core::stats::now().max(old.first().map_or(0, |s| s.time));
    let id = (old.first().map_or(0, |s| number(s) + 1)..)
        .map(|n| format!("{time}-{n}"))
        .find(|id| !root.join(id).exists())
        .expect("endless");
    // Built aside, so a copy is there whole or not at all.
    let fresh = root.join(format!(".{id}"));
    let result = (|| -> anyhow::Result<_> {
        let files = fresh.join("files");
        std::fs::create_dir_all(&files)?;
        tree.apply(from, &files, &Filter::default())?;
        let mut s = Snapshot {
            id: id.clone(),
            time,
            why,
            place: place.name.clone(),
            path: place.path.clone(),
            saved: tree.newest(),
            files: tree.0.len(),
            bytes: tree.bytes(),
            print,
            include: place.filter.include.clone(),
            exclude: place.filter.exclude.clone(),
        };
        std::fs::write(fresh.join("about.json"), serde_json::to_vec_pretty(&s)?)?;
        std::fs::rename(&fresh, root.join(&id))?;
        s.id = id;
        Ok(s)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&fresh);
    }
    let s = result.context("keeping a copy of the saves")?;
    for old in list(dir)
        .iter()
        // Copies the user made do not give way to those made on the way.
        .filter(|o| o.place == place.name && (o.why == Why::Manual) == (why == Why::Manual))
        .skip(KEEP)
    {
        let _ = arkor_core::fs_guard::remove_dir_in(&root.join(&old.id), &root);
    }
    Ok(Some(s))
}

/// Puts a copy back, keeping a copy of what is there now.
pub fn restore(dir: &Path, id: &str) -> anyhow::Result<()> {
    let s = list(dir)
        .into_iter()
        .find(|s| s.id == id)
        .context("that copy is gone")?;
    let files = dir.join("snapshots").join(&s.id).join("files");
    let tree = Tree::read(&files, &Filter::default())?;
    anyhow::ensure!(
        !tree.is_empty() && tree.print() == s.print,
        "that copy is damaged"
    );
    // Where it was then. Its disk may not be there now.
    let there = s.path.ancestors().nth(3).is_some_and(Path::is_dir);
    anyhow::ensure!(
        s.path.exists() || there,
        "{} is not there (is its drive mounted?)",
        s.path.display()
    );
    let place = Place {
        name: s.place.clone(),
        path: s.path.clone(),
        filter: s.filter(),
    };
    // Pruning must not take the copy that is being put back.
    let work = tempdir(dir)?;
    let result = (|| -> anyhow::Result<_> {
        tree.apply(&files, &work, &Filter::default())?;
        take(dir, &place, &place.path, Why::BeforeRestore)?;
        tree.apply(&work, &place.path, &place.filter)
    })();
    let _ = arkor_core::fs_guard::remove_dir_in(&work, dir);
    result
}

/// An empty folder for work in a game's folder.
pub fn tempdir(dir: &Path) -> anyhow::Result<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let work = dir.join("work").join(format!("{}-{n}", std::process::id()));
    if work.exists() {
        arkor_core::fs_guard::remove_dir_in(&work, dir)?;
    }
    std::fs::create_dir_all(&work)?;
    Ok(work)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(tmp: &Path) -> Place {
        Place {
            name: "saves".into(),
            path: tmp.join("game/saves"),
            filter: Filter::new("*.sav", ""),
        }
    }

    fn save(p: &Place, text: &str) {
        std::fs::create_dir_all(&p.path).unwrap();
        std::fs::write(p.path.join("one.sav"), text).unwrap();
    }

    fn read(p: &Place) -> String {
        std::fs::read_to_string(p.path.join("one.sav")).unwrap()
    }

    #[test]
    fn restoring_keeps_what_was_there() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("saves/game");
        let p = place(tmp.path());
        assert_eq!(take(&dir, &p, &p.path, Why::Manual).unwrap(), None);
        save(&p, "first");
        std::fs::write(p.path.join("settings.ini"), "mine").unwrap();
        let s = take(&dir, &p, &p.path, Why::Manual).unwrap().unwrap();
        assert_eq!(s.files, 1);
        // The same again is no second copy.
        assert_eq!(take(&dir, &p, &p.path, Why::BeforeDownload).unwrap(), None);
        save(&p, "second");
        restore(&dir, &s.id).unwrap();
        assert_eq!(read(&p), "first");
        assert!(p.path.join("settings.ini").exists());
        let l = list(&dir);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].why, Why::BeforeRestore);
        restore(&dir, &l[0].id).unwrap();
        assert_eq!(read(&p), "second");
        assert!(restore(&dir, "none").is_err());
    }

    #[test]
    fn damaged_copies_are_not_put_back() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("saves/game");
        let p = place(tmp.path());
        save(&p, "first");
        let s = take(&dir, &p, &p.path, Why::Manual).unwrap().unwrap();
        std::fs::write(
            dir.join("snapshots").join(&s.id).join("files/one.sav"),
            "broken",
        )
        .unwrap();
        save(&p, "second");
        assert!(restore(&dir, &s.id).is_err());
        assert_eq!(read(&p), "second");
    }

    #[test]
    fn old_copies_go_the_one_put_back_stays_usable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("saves/game");
        let p = place(tmp.path());
        for n in 0..KEEP + 4 {
            save(&p, &format!("save {n}"));
            take(&dir, &p, &p.path, Why::BeforeDownload)
                .unwrap()
                .unwrap();
        }
        save(&p, "mine");
        let mine = take(&dir, &p, &p.path, Why::Manual).unwrap().unwrap();
        let mut l = list(&dir);
        assert_eq!(l.len(), KEEP + 1);
        l.retain(|s| s.why != Why::Manual);
        save(&p, "now");
        // The oldest one, which making the next copy takes away.
        restore(&dir, &l[KEEP - 1].id).unwrap();
        assert_eq!(read(&p), "save 4");
        assert_eq!(list(&dir).len(), KEEP + 1);
        assert!(list(&dir).contains(&mine));
    }
}
