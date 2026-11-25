use std::cell::RefCell;
use std::collections::BTreeMap;

use super::*;

/// A cloud in memory.
#[derive(Default)]
struct Cloud {
    files: RefCell<BTreeMap<String, String>>,
    version: RefCell<u32>,
    /// Cannot tell whether it changed.
    blind: bool,
    /// Takes nothing, without saying so.
    deaf: bool,
    fetched: RefCell<u32>,
}

impl Cloud {
    fn put(&self, rel: &str, text: &str) {
        self.files.borrow_mut().insert(rel.into(), text.into());
        *self.version.borrow_mut() += 1;
    }
}

impl Remote for Cloud {
    fn mark(&self, _: &Place) -> anyhow::Result<Option<String>> {
        if self.blind {
            return Ok(None);
        }
        Ok(Some(if self.files.borrow().is_empty() {
            String::new()
        } else {
            self.version.borrow().to_string()
        }))
    }

    fn fetch(&self, _: &Place, into: &Path) -> anyhow::Result<()> {
        *self.fetched.borrow_mut() += 1;
        for (rel, text) in self.files.borrow().iter() {
            let p = into.join(rel);
            std::fs::create_dir_all(p.parent().unwrap())?;
            std::fs::write(p, text)?;
        }
        Ok(())
    }

    fn push(&self, _: &Place, from: &Path) -> anyhow::Result<()> {
        if self.deaf {
            return Ok(());
        }
        let tree = Tree::read(from, &Filter::default())?;
        *self.files.borrow_mut() = tree
            .0
            .values()
            .map(|e| {
                let text = std::fs::read_to_string(from.join(&e.rel)).unwrap();
                (e.rel.to_str().unwrap().to_string(), text)
            })
            .collect();
        *self.version.borrow_mut() += 1;
        Ok(())
    }
}

struct Game {
    tmp: tempfile::TempDir,
    place: Place,
    known: PlaceState,
    cloud: Cloud,
}

impl Game {
    fn new() -> Game {
        let tmp = tempfile::tempdir().unwrap();
        Game {
            place: Place {
                name: "saves".into(),
                path: tmp.path().join("game/saves"),
                filter: Filter::new("*.sav", ""),
            },
            known: PlaceState::default(),
            cloud: Cloud::default(),
            tmp,
        }
    }

    fn dir(&self) -> PathBuf {
        self.tmp.path().join("arkor/saves/game")
    }

    fn save(&self, text: &str) {
        std::fs::create_dir_all(&self.place.path).unwrap();
        std::fs::write(self.place.path.join("one.sav"), text).unwrap();
    }

    fn here(&self) -> String {
        std::fs::read_to_string(self.place.path.join("one.sav")).unwrap_or_default()
    }

    fn there(&self) -> String {
        self.cloud
            .files
            .borrow()
            .get("one.sav")
            .cloned()
            .unwrap_or_default()
    }

    fn sync(&mut self, prefer: Prefer) -> anyhow::Result<Step> {
        std::fs::create_dir_all(self.dir()).unwrap();
        sync_place(
            &self.cloud,
            &self.dir(),
            &self.place,
            &mut self.known,
            prefer,
        )
    }

    fn ask(&mut self) -> Step {
        self.sync(Prefer::Ask).unwrap()
    }

    fn copies(&self) -> Vec<(Why, String)> {
        snapshots::list(&self.dir())
            .iter()
            .map(|s| {
                let f = self
                    .dir()
                    .join("snapshots")
                    .join(&s.id)
                    .join("files/one.sav");
                (s.why, std::fs::read_to_string(f).unwrap())
            })
            .collect()
    }
}

#[test]
fn nothing_anywhere() {
    let mut g = Game::new();
    assert_eq!(g.ask(), Step::Same);
    assert!(!g.place.path.exists());
}

#[test]
fn first_time_on_this_computer() {
    let mut g = Game::new();
    g.cloud.put("one.sav", "cloud");
    assert_eq!(g.ask(), Step::Downloaded);
    assert_eq!(g.here(), "cloud");
    assert_eq!(g.ask(), Step::Same);
    assert_eq!(*g.cloud.fetched.borrow(), 1, "unchanged: not fetched again");
}

#[test]
fn back_and_forth() {
    let mut g = Game::new();
    g.save("1");
    std::fs::write(g.place.path.join("settings.ini"), "mine").unwrap();
    assert_eq!(g.ask(), Step::Uploaded);
    assert_eq!(g.there(), "1");
    assert_eq!(g.cloud.files.borrow().len(), 1, "only saves go up");
    g.save("2");
    assert_eq!(g.ask(), Step::Uploaded);
    assert_eq!(g.there(), "2");
    // Played elsewhere.
    g.cloud.put("one.sav", "3");
    assert_eq!(g.ask(), Step::Downloaded);
    assert_eq!(g.here(), "3");
    assert!(g.place.path.join("settings.ini").exists());
    assert_eq!(g.copies(), [(Why::BeforeDownload, "2".into())]);
}

#[test]
fn both_changed_is_asked() {
    let mut g = Game::new();
    g.save("1");
    g.ask();
    g.save("here");
    g.cloud.put("one.sav", "there");
    let Step::Conflict(c) = g.ask() else {
        panic!("not asked")
    };
    assert_eq!((c.here_bytes, c.cloud_bytes), (4, 5));
    // Nothing was touched.
    assert_eq!((g.here(), g.there()), ("here".into(), "there".into()));
    assert!(matches!(g.ask(), Step::Conflict(_)), "asked again");
    assert_eq!(g.sync(Prefer::Here).unwrap(), Step::Uploaded);
    assert_eq!(g.there(), "here");
    // The other side is not lost.
    assert_eq!(g.copies(), [(Why::Cloud, "there".into())]);
    assert_eq!(g.ask(), Step::Same);

    g.save("again");
    g.cloud.put("one.sav", "there 2");
    assert_eq!(g.sync(Prefer::Cloud).unwrap(), Step::Downloaded);
    assert_eq!(g.here(), "there 2");
    assert_eq!(g.copies()[0], (Why::BeforeDownload, "again".into()));
}

#[test]
fn unknown_on_both_sides_is_asked() {
    let mut g = Game::new();
    g.save("here");
    g.cloud.put("one.sav", "there");
    assert!(matches!(g.ask(), Step::Conflict(_)));
    // The same on both sides is no question.
    let mut g = Game::new();
    g.save("same");
    g.cloud.put("one.sav", "same");
    assert_eq!(g.ask(), Step::Same);
    assert!(g.known.base.is_some());
}

#[test]
fn an_empty_side_erases_nothing() {
    let mut g = Game::new();
    g.save("1");
    g.ask();
    // The prefix was made anew.
    std::fs::remove_dir_all(&g.place.path).unwrap();
    assert_eq!(g.ask(), Step::Downloaded);
    assert_eq!(g.here(), "1");
    // The cloud was emptied.
    g.cloud.files.borrow_mut().clear();
    assert_eq!(g.ask(), Step::Uploaded);
    assert_eq!(g.there(), "1");
    // Even when the answer was "the cloud's" or "this computer's".
    g.cloud.files.borrow_mut().clear();
    assert_eq!(g.sync(Prefer::Cloud).unwrap(), Step::Uploaded);
    std::fs::remove_dir_all(&g.place.path).unwrap();
    assert_eq!(g.sync(Prefer::Here).unwrap(), Step::Downloaded);
    assert_eq!(g.here(), "1");
}

#[test]
fn cloud_that_cannot_be_compared() {
    let mut g = Game::new();
    g.cloud.blind = true;
    g.save("1");
    assert_eq!(g.ask(), Step::Uploaded);
    assert_eq!(g.ask(), Step::Same);
    g.cloud.put("one.sav", "2");
    assert_eq!(g.ask(), Step::Downloaded);
    assert_eq!(g.here(), "2");
}

#[test]
fn upload_that_went_nowhere_is_said() {
    for blind in [false, true] {
        let mut g = Game::new();
        g.cloud.put("one.sav", "1");
        g.ask();
        (g.cloud.deaf, g.cloud.blind) = (true, blind);
        g.save("2");
        assert!(g.sync(Prefer::Ask).is_err());
        assert_eq!(g.here(), "2");
        g.cloud.deaf = false;
        assert_eq!(g.ask(), Step::Uploaded);
        assert_eq!(g.there(), "2");
    }
}

#[test]
fn unreadable_cloud_changes_nothing() {
    struct Broken;
    impl Remote for Broken {
        fn mark(&self, _: &Place) -> anyhow::Result<Option<String>> {
            Ok(Some("5".into()))
        }
        fn fetch(&self, _: &Place, _: &Path) -> anyhow::Result<()> {
            Ok(())
        }
        fn push(&self, _: &Place, _: &Path) -> anyhow::Result<()> {
            panic!("pushed over a cloud that could not be read")
        }
    }
    let g = Game::new();
    g.save("here");
    std::fs::create_dir_all(g.dir()).unwrap();
    let mut known = PlaceState::default();
    assert!(sync_place(&Broken, &g.dir(), &g.place, &mut known, Prefer::Ask).is_err());
    assert_eq!(g.here(), "here");
}

#[test]
fn failed_download_puts_the_saves_back() {
    let mut g = Game::new();
    g.save("1");
    g.ask();
    g.cloud.put("one.sav", "2");
    // A file of the cloud's cannot be written: a folder has its name.
    g.cloud.put("two.sav", "x");
    std::fs::create_dir_all(g.place.path.join("two.sav")).unwrap();
    assert!(g.sync(Prefer::Ask).is_err());
    assert_eq!(g.here(), "1");
    std::fs::remove_dir(g.place.path.join("two.sav")).unwrap();
    assert_eq!(g.ask(), Step::Downloaded);
    assert_eq!(g.here(), "2");
}

#[test]
fn reads_the_stores_answers() {
    let log = "[cli] INFO: Got 1 remote save game(s)\n[cli] INFO: Cloud save for \"X\" is newer:\n\
               [cli] INFO: - Cloud save date: 2026-11-25 20:01:02\n[cli] INFO: - Local save date: N/A\n";
    assert_eq!(remote::epic_date(log), Some("2026-11-25 20:01:02"));
    assert_eq!(remote::epic_date("nothing"), None);
    assert_eq!(remote::utc("2026-11-25 20:01:02"), Some(1795636862));
    assert_eq!(remote::utc("5"), None);
    let list = r#"[{"name":"saves/a","hash":"1","last_modified":"x"},
                   {"name":"saves2/a","hash":"9","last_modified":"x"},
                   {"name":"saves/gone","hash":"aadd86936a80ee8a369579c3926f1b3c","last_modified":"x"}]"#;
    let m = remote::gog_mark(list, "saves").unwrap();
    assert_eq!(
        m,
        remote::gog_mark(
            r#"[{"name":"saves/a","hash":"1","last_modified":"x"}]"#,
            "saves"
        )
        .unwrap()
    );
    assert_ne!(m, remote::gog_mark(list, "saves2").unwrap());
    assert_eq!(remote::gog_mark(list, "other").unwrap(), "");
}
