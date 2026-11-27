//! Saves of store games: kept in the store's cloud, and copies of them kept here.
//!
//! The cloud's saves are always fetched into a folder of Arkor's first. They are compared by
//! contents with the game's own and with what both had when they were last the same, so it is
//! known which side changed. When both did, nothing is touched until the user chose a side.
//! Before anything is put over saves a copy is kept ([`snapshots`]).

pub mod ludusavi;
mod places;
mod remote;
pub mod snapshots;
#[cfg(test)]
mod tests;
mod tree;

use std::collections::BTreeMap;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::Context;
use arkor_core::library::{GameKind, Library, LibraryGame};
use arkor_core::{GameId, Global, Profile, paths};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shops::{self, Shop};
use places::{Folders, Place};
use remote::Remote;
pub use snapshots::{Snapshot, Why};
use tree::{Filter, Tree};

/// A library game whose saves may be in a store's cloud.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudGame {
    pub id: GameId,
    pub name: String,
    pub shop: Shop,
    /// The store's id of the game.
    pub app: String,
    install: PathBuf,
    prefix: PathBuf,
}

/// The game as its store knows it, when Arkor installed it from Epic or GOG. Reads files only.
pub fn cloud_game(game: &LibraryGame, global: &Global) -> Option<CloudGame> {
    let (shop, app) = match &game.kind {
        GameKind::Epic { app_name } => (Shop::Epic, app_name.as_str()),
        GameKind::Windows => {
            shops::from_origin(game.origin.as_deref()?).filter(|(shop, _)| *shop == Shop::Gog)?
        }
        _ => return None,
    };
    let installed = shops::installed(shop).into_iter().find(|g| g.id == app)?;
    if installed.linux || shop == Shop::Epic && epic_metadata(app)?.1.is_empty() {
        return None;
    }
    Some(CloudGame {
        id: game.game_id(),
        name: game.name.clone(),
        shop,
        app: app.to_string(),
        install: installed.path,
        prefix: game.effective_prefix(&global.prefixes_dir()),
    })
}

/// The library game with this id, as [`cloud_game`].
pub fn find(id: &GameId) -> anyhow::Result<CloudGame> {
    let library = Library::load()?;
    let game = library
        .get(&id.id)
        .with_context(|| format!("`{id}` is not in the library"))?;
    cloud_game(game, &Global::load_or_import()?)
        .with_context(|| format!("{} is no Epic or GOG game with cloud saves", game.name))
}

/// Epic's description of a game, and its save folder.
fn epic_metadata(app: &str) -> Option<(Value, String)> {
    let path = paths::stores_dir().ok()?.join("epic/metadata");
    // The name comes from Epic: one plain name.
    if app.contains(['/', '\0']) {
        return None;
    }
    let text = std::fs::read_to_string(path.join(format!("{app}.json"))).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let folder = v["metadata"]["customAttributes"]["CloudSaveFolder"]["value"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Some((v, folder))
}

/// Whether the game's saves are to follow the cloud: wanted, and its store is logged in.
pub fn wanted(game: &CloudGame, global: &Global) -> bool {
    Profile::load(&game.id)
        .ok()
        .and_then(|p| p.cloud_saves)
        .unwrap_or(!global.no_cloud_saves)
        && shops::logged_in(game.shop)
        && state(&game.id).cloud != Some(false)
}

// ---- How things stand ----

/// What is known of one save folder.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct PlaceState {
    /// What both sides had when they were last the same.
    base: Option<String>,
    /// The cloud's mark then.
    mark: Option<String>,
}

/// How a game's saves stand.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    account: String,
    places: BTreeMap<String, PlaceState>,
    /// Whether the store keeps saves of this game at all, when known.
    pub cloud: Option<bool>,
    /// When both sides were last the same (unix time).
    pub synced: Option<u64>,
    /// Both sides changed: the user has to choose.
    pub conflict: Option<Conflict>,
    /// Why the last attempt failed.
    pub error: Option<String>,
}

/// Saves that changed on both sides.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Conflict {
    /// When the saves here and those in the cloud were last changed (unix time).
    pub here: Option<u64>,
    pub cloud: Option<u64>,
    pub here_bytes: u64,
    pub cloud_bytes: u64,
}

/// How a game's saves stood after the last attempt. Reads a file only.
pub fn state(id: &GameId) -> State {
    paths::saves_dir(id)
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join("state.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_state(dir: &Path, state: &State) -> anyhow::Result<()> {
    let fresh = dir.join("state.json.new");
    std::fs::write(&fresh, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(&fresh, dir.join("state.json")).context("noting how the saves stand")
}

/// The copies kept of a game's saves, newest first.
pub fn copies(id: &GameId) -> Vec<Snapshot> {
    paths::saves_dir(id)
        .map(|d| snapshots::list(&d))
        .unwrap_or_default()
}

// ---- Syncing ----

/// Which side counts when both changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prefer {
    /// Neither: the user is asked.
    Ask,
    Here,
    Cloud,
}

/// What a sync did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub downloaded: bool,
    pub uploaded: bool,
    pub conflict: Option<Conflict>,
}

/// Brings the game's saves here and in the cloud together. Must not run while the game
/// does. Blocking.
pub fn sync(game: &CloudGame, prefer: Prefer, stop: &AtomicBool) -> anyhow::Result<Report> {
    with_dir(&game.id, |dir| {
        let mut state = state(&game.id);
        let result = sync_places(game, dir, &mut state, prefer, stop);
        match &result {
            Ok(r) => {
                state.error = None;
                state.conflict = r.conflict.clone();
                if r.conflict.is_none() {
                    state.synced = Some(arkor_core::stats::now());
                }
            }
            Err(e) => state.error = Some(format!("{e:#}")),
        }
        write_state(dir, &state)?;
        result
    })
}

/// Before the game starts, where nobody can be asked: when both sides changed, the saves
/// here stay and the game starts with them. Blocking.
pub fn before_launch(id: &GameId) {
    let Some((game, _)) = wanted_game(id) else {
        return;
    };
    eprintln!("arkor: looking for newer saves in the cloud");
    match sync(&game, Prefer::Ask, &AtomicBool::new(false)) {
        Ok(Report {
            conflict: Some(_), ..
        }) => eprintln!("arkor: the saves changed here and in the cloud; both are kept"),
        Ok(r) if r.downloaded => eprintln!("arkor: saves downloaded from the cloud"),
        Ok(_) => {}
        Err(e) => eprintln!("arkor: the saves were not synced: {e:#}"),
    }
}

/// After the game ended: uploads what it saved. Asks the store only when something changed.
/// Blocking.
pub fn after_exit(id: &GameId) -> Option<anyhow::Result<Report>> {
    let (game, _) = wanted_game(id)?;
    // No one else's sync is to be in the middle of it.
    let changed = with_dir(id, |dir| Ok(changed_here(&game, dir))).unwrap_or(true);
    changed.then(|| sync(&game, Prefer::Ask, &AtomicBool::new(false)))
}

/// Whether the game's saves follow the cloud. Reads files only.
pub fn follows(id: &GameId) -> bool {
    wanted_game(id).is_some()
}

fn wanted_game(id: &GameId) -> Option<(CloudGame, Global)> {
    let global = Global::load_or_import().ok()?;
    let library = Library::load().ok()?;
    let game = cloud_game(library.get(&id.id)?, &global)?;
    wanted(&game, &global).then_some((game, global))
}

/// Whether the saves here are others than when both sides were last the same.
fn changed_here(game: &CloudGame, dir: &Path) -> bool {
    let state = state(&game.id);
    if state.conflict.is_some() || state.error.is_some() {
        return true;
    }
    let Ok(found) = find_places(game, dir, true) else {
        return true;
    };
    found.places.iter().any(|p| {
        let base = state.places.get(&p.name).and_then(|s| s.base.clone());
        Tree::read(&p.path, &p.filter).map(|t| t.print()).ok() != base
    })
}

/// Keeps a copy of the game's saves as they are now. Blocking.
pub fn copy_now(game: &CloudGame) -> anyhow::Result<usize> {
    with_dir(&game.id, |dir| {
        let mut made = 0;
        for place in find_places(game, dir, true)?.places {
            made += snapshots::take(dir, &place, &place.path, Why::Manual)?.is_some() as usize;
        }
        Ok(made)
    })
}

/// Puts a copy back; what is there now is kept as a copy, too. Blocking.
pub fn restore(id: &GameId, copy: &str) -> anyhow::Result<()> {
    with_dir(id, |dir| snapshots::restore(dir, copy))
}

/// One process at a time works in a game's folder.
fn with_dir<T>(id: &GameId, f: impl FnOnce(&Path) -> anyhow::Result<T>) -> anyhow::Result<T> {
    let dir = paths::saves_dir(id)?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("lock"))?;
    // SAFETY: a valid descriptor; released when the file is closed.
    let ok = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) };
    anyhow::ensure!(ok == 0, "locking the saves");
    let result = f(&dir);
    // What a stopped run left, too: nobody else works here now.
    let work = dir.join("work");
    if work.exists() {
        let _ = arkor_core::fs_guard::remove_dir_in(&work, &dir);
    }
    result
}

/// A game's save folders, and what asking its cloud needs.
struct Found {
    places: Vec<Place>,
    account: String,
    /// GOG's name and key of the game.
    client: Option<(String, Option<String>)>,
}

/// `kept`: what GOG said last time is good enough.
fn find_places(game: &CloudGame, dir: &Path, kept: bool) -> anyhow::Result<Found> {
    let stores = paths::stores_dir()?;
    match game.shop {
        Shop::Epic => {
            shops::vault::settle(Shop::Epic);
            #[derive(Deserialize)]
            struct User {
                account_id: String,
            }
            let text = std::fs::read_to_string(stores.join("epic/user.json"))
                .context("not logged in to Epic")?;
            let account = serde_json::from_str::<User>(&text)
                .context("the Epic login is not readable")?
                .account_id;
            let (metadata, _) = epic_metadata(&game.app).context("the game is not known")?;
            let place = places::epic(&metadata, &game.folders(&account))?;
            Ok(Found {
                places: place.into_iter().collect(),
                account,
                client: None,
            })
        }
        Shop::Gog => {
            let (client, secret) = places::gog_client(&stores, &game.install, &game.app)?;
            let file = dir.join("gog.json");
            let old = || -> Option<Value> {
                serde_json::from_str(&std::fs::read_to_string(&file).ok()?).ok()
            };
            let config = match old().filter(|_| kept) {
                Some(v) => v,
                None => match remote::gog_config(&client) {
                    Ok(v) => {
                        let _ = std::fs::write(&file, v.to_string());
                        v
                    }
                    Err(e) => old().ok_or(e)?,
                },
            };
            Ok(Found {
                places: places::gog(&config, &client, &game.folders(""))?,
                account: shops::account(Shop::Gog).unwrap_or_default(),
                client: Some((client, secret)),
            })
        }
        _ => anyhow::bail!("{} keeps no saves Arkor can sync", game.shop.name()),
    }
}

impl CloudGame {
    fn folders(&self, account: &str) -> Folders {
        Folders {
            prefix: self.prefix.clone(),
            install: self.install.clone(),
            account: account.to_string(),
        }
    }
}

fn sync_places(
    game: &CloudGame,
    dir: &Path,
    state: &mut State,
    prefer: Prefer,
    stop: &AtomicBool,
) -> anyhow::Result<Report> {
    let found = find_places(game, dir, false)?;
    state.cloud = Some(!found.places.is_empty());
    if state.account != found.account {
        // Another account's cloud has nothing to do with what was the same before.
        state.account = found.account.clone();
        state.places.clear();
    }
    let empty = snapshots::tempdir(dir)?;
    let remote: Box<dyn Remote> = match &found.client {
        Some((client, secret)) => Box::new(remote::Gog {
            id: &game.app,
            client,
            secret: secret.as_deref(),
            stop,
        }),
        None => Box::new(remote::Epic {
            app: &game.app,
            empty: &empty,
            stop,
        }),
    };
    let mut report = Report::default();
    for place in &found.places {
        let known = state.places.entry(place.name.clone()).or_default();
        match sync_place(&*remote, dir, place, known, prefer)
            .with_context(|| format!("saves of {}", game.name))?
        {
            Step::Same => {}
            Step::Downloaded => report.downloaded = true,
            Step::Uploaded => report.uploaded = true,
            Step::Conflict(c) => {
                let old = report.conflict.get_or_insert_with(Conflict::default);
                old.here = old.here.max(c.here);
                old.cloud = old.cloud.max(c.cloud);
                old.here_bytes += c.here_bytes;
                old.cloud_bytes += c.cloud_bytes;
            }
        }
    }
    Ok(report)
}

#[derive(Debug, PartialEq)]
enum Step {
    Same,
    Downloaded,
    Uploaded,
    Conflict(Conflict),
}

/// Brings one save folder and its cloud together.
fn sync_place(
    remote: &dyn Remote,
    dir: &Path,
    place: &Place,
    known: &mut PlaceState,
    prefer: Prefer,
) -> anyhow::Result<Step> {
    let here = Tree::read(&place.path, &place.filter)?;
    let here_print = here.print();
    let mark = remote.mark(place)?;
    let unchanged = mark.is_some() && mark == known.mark && known.base.is_some();
    if unchanged && known.base.as_ref() == Some(&here_print) {
        return Ok(Step::Same);
    }
    // The cloud is as it was and saves were made here: nothing to fetch.
    if unchanged && !here.is_empty() {
        push(remote, dir, place, &here, known, mark)?;
        return Ok(Step::Uploaded);
    }
    let work = snapshots::tempdir(dir)?;
    if mark.as_deref() != Some("") {
        remote.fetch(place, &work)?;
    }
    let cloud = Tree::read(&work, &place.filter)?;
    let cloud_print = cloud.print();
    if cloud_print == here_print {
        *known = PlaceState {
            base: Some(here_print),
            mark,
        };
        return Ok(Step::Same);
    }
    // The cloud said there is something: an empty folder is then no answer.
    anyhow::ensure!(
        !cloud.is_empty() || mark.is_none() || mark.as_deref() == Some(""),
        "the saves in the cloud could not be fetched"
    );
    let base = known.base.as_ref();
    let up = cloud.is_empty() || base == Some(&cloud_print);
    let down = here.is_empty() || base == Some(&here_print);
    // An empty side never counts as a change: it must not erase the other one.
    let to_cloud = !here.is_empty()
        && match prefer {
            _ if up => true,
            _ if down => false,
            Prefer::Ask => {
                return Ok(Step::Conflict(Conflict {
                    here: here.newest(),
                    cloud: cloud.newest(),
                    here_bytes: here.bytes(),
                    cloud_bytes: cloud.bytes(),
                }));
            }
            Prefer::Here => true,
            Prefer::Cloud => false,
        };
    if to_cloud {
        if base != Some(&cloud_print) {
            snapshots::take(dir, place, &work, Why::Cloud)?;
        }
        push(remote, dir, place, &here, known, mark)?;
        return Ok(Step::Uploaded);
    }
    snapshots::take(dir, place, &place.path, Why::BeforeDownload)?;
    let keep = snapshots::tempdir(dir)?;
    here.apply(&place.path, &keep, &Filter::default())?;
    if let Err(e) = cloud.apply(&work, &place.path, &place.filter) {
        // Half of each would be worse than either.
        here.apply(&keep, &place.path, &place.filter)
            .context("the saves are damaged; a copy of them is kept")?;
        return Err(e);
    }
    *known = PlaceState {
        base: Some(cloud_print),
        mark,
    };
    Ok(Step::Downloaded)
}

/// Sends the saves here to the cloud, from a copy.
fn push(
    remote: &dyn Remote,
    dir: &Path,
    place: &Place,
    here: &Tree,
    known: &mut PlaceState,
    before: Option<String>,
) -> anyhow::Result<()> {
    let work = snapshots::tempdir(dir)?;
    here.apply(&place.path, &work, &Filter::default())?;
    // Stores name saves by the date of the newest file, and take the latest for the one that
    // counts: even when the cloud's are from a clock that is ahead, or were newer.
    let cloud = before.as_deref().and_then(|m| remote.date(m));
    let date = arkor_core::stats::now().max(cloud.map_or(0, |d| d + 2));
    let newest = here.0.values().max_by_key(|e| e.modified);
    if let Some(e) = newest {
        std::fs::File::options()
            .write(true)
            .open(work.join(&e.rel))?
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(date))?;
    }
    remote.push(place, &work)?;
    // From here on the cloud is another one than before, whatever comes next.
    known.mark = None;
    let after = remote.mark(place)?;
    match &after {
        Some(m) => anyhow::ensure!(
            before.as_ref() != Some(m) && !m.is_empty(),
            "the store did not take the saves"
        ),
        // It cannot be told from outside: look at what it has.
        None => {
            let back = snapshots::tempdir(dir)?;
            remote.fetch(place, &back)?;
            anyhow::ensure!(
                Tree::read(&back, &place.filter)?.print() == here.print(),
                "the store did not take the saves"
            );
        }
    }
    *known = PlaceState {
        base: Some(here.print()),
        mark: after,
    };
    Ok(())
}
