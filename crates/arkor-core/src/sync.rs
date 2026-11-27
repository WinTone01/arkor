//! Keeping what the user said about their games the same on several computers (or both
//! sides of a dual boot): status, rating, favourite, category and hidden, the sessions
//! played (so play time and the journal add up), journal notes and status changes.
//!
//! Each computer writes one file of its own, `<machine>.json`, into a shared folder, and
//! reads the others'. Games are matched between computers by where they came from
//! (`origin`, e.g. `gog:1207658924`) or else by title, never by their library id. For the
//! user's fields of a game the newest change wins; sessions, notes and status changes are
//! put together (each once).
//!
//! When a field changed is noticed at sync time: the last synced values are kept in
//! `sync-state.json`, and a game whose fields differ from them changed "now".

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::library::{Library, LibraryGame, PlayStatus, title_key};
use crate::stats::{self, Note, Session, StatusChange};

/// The user's own fields of a game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fields {
    pub status: Option<PlayStatus>,
    pub rating: Option<u8>,
    pub favorite: bool,
    pub category: Option<String>,
    pub hidden: bool,
}

impl Fields {
    fn of(g: &LibraryGame) -> Self {
        Self {
            status: g.status,
            rating: g.rating,
            favorite: g.favorite,
            category: g.category.clone(),
            hidden: g.hidden,
        }
    }

    fn apply(&self, g: &mut LibraryGame) {
        g.status = self.status;
        g.rating = self.rating;
        g.favorite = self.favorite;
        g.category.clone_from(&self.category);
        // A copy stays hidden (it is shown through its main entry).
        if g.copy_of.is_none() {
            g.hidden = self.hidden;
        }
    }
}

/// What one computer knows, as written into the shared folder.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Export {
    pub machine: String,
    pub written: u64,
    /// By game key: the fields and when they last changed.
    pub games: BTreeMap<String, (Fields, u64)>,
    /// Sessions, with the game key instead of the library id.
    pub sessions: Vec<Session>,
    pub notes: Vec<Note>,
    pub statuses: Vec<StatusChange>,
}

/// The key a game is known by on every computer: its origin, or its title.
pub fn game_key(g: &LibraryGame) -> String {
    match &g.origin {
        Some(o) => format!("o:{o}"),
        None => format!("t:{}", title_key(&g.name)),
    }
}

/// This computer's name in the shared folder.
pub fn machine_name() -> String {
    if let Ok(name) = std::env::var("ARKOR_SYNC_NAME")
        && !name.trim().is_empty()
    {
        return name.trim().to_string();
    }
    let host = std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "pc".into());
    let id = std::fs::read_to_string("/etc/machine-id")
        .ok()
        .map(|m| m.trim().chars().take(8).collect::<String>())
        .unwrap_or_default();
    let safe: String = format!("{host}-{id}")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    safe
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    /// The fields of each game at the last sync, and when they changed.
    games: BTreeMap<String, (Fields, u64)>,
}

fn state_path() -> anyhow::Result<PathBuf> {
    Ok(dirs::state_dir()
        .context("no XDG state dir")?
        .join("arkor/sync-state.json"))
}

/// The local library's sessions with game keys (`local:<id>` → key).
fn keyed_sessions(lib: &Library, sessions: &[Session]) -> Vec<Session> {
    let key_of = |game: &str| {
        let id = game.split_once(':').map_or(game, |(_, id)| id);
        lib.get(id).map(game_key)
    };
    sessions
        .iter()
        .filter_map(|s| {
            Some(Session {
                game: key_of(&s.game)?,
                start: s.start,
                end: s.end,
            })
        })
        .collect()
}

/// This computer's export; `state` gives when each game's fields changed.
fn export(lib: &Library, state: &mut State, now: u64) -> Export {
    let mut games: BTreeMap<String, (Fields, u64)> = BTreeMap::new();
    // Two entries with one key (the same title twice): the shown one with something set.
    let mut entries: Vec<&LibraryGame> = lib.games.iter().collect();
    entries.sort_by_key(|g| (g.copy_of.is_none(), Fields::of(g) != Fields::default()));
    for g in entries {
        let key = game_key(g);
        let fields = Fields::of(g);
        let at = match state.games.get(&key) {
            Some((old, at)) if *old == fields => *at,
            // Never synced and nothing set: older than anything another computer says.
            None if fields == Fields::default() => 1,
            _ => now,
        };
        state.games.insert(key.clone(), (fields.clone(), at));
        games.insert(key, (fields, at));
    }
    let key_of = |game: &str| {
        let id = game.split_once(':').map_or(game, |(_, id)| id);
        lib.get(id).map(game_key)
    };
    Export {
        machine: machine_name(),
        written: now,
        games,
        sessions: keyed_sessions(lib, &stats::sessions()),
        notes: stats::notes()
            .into_iter()
            .filter_map(|mut n| {
                if !n.game.is_empty() {
                    n.game = key_of(&n.game)?;
                }
                Some(n)
            })
            .collect(),
        statuses: stats::status_history()
            .into_iter()
            .filter_map(|mut c| {
                c.game = key_of(&c.game)?;
                Some(c)
            })
            .collect(),
    }
}

/// What a sync changed here.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub machines: usize,
    pub games: usize,
    pub sessions: usize,
    pub notes: usize,
}

/// Puts the others' exports into this computer's library and history. Pure on `lib`; the
/// sessions, notes and status changes to add are returned with local game ids.
fn merge(
    lib: &mut Library,
    state: &mut State,
    others: &[Export],
    local: &Export,
) -> (Report, Vec<Session>, Vec<Note>, Vec<StatusChange>) {
    let mut report = Report {
        machines: others.len(),
        ..Default::default()
    };
    // Fields: the newest change wins.
    for g in lib.games.iter_mut() {
        let key = game_key(g);
        let mine = local.games.get(&key).map_or(0, |(_, at)| *at);
        let newest = others
            .iter()
            .filter_map(|e| e.games.get(&key))
            .filter(|(_, at)| *at > mine)
            .max_by_key(|(_, at)| *at);
        if let Some((fields, at)) = newest
            && Fields::of(g) != *fields
        {
            fields.apply(g);
            state.games.insert(key, (fields.clone(), *at));
            report.games += 1;
        }
    }
    let local_id = |key: &str| -> Option<String> {
        lib.games
            .iter()
            .find(|g| game_key(g) == key)
            .map(|g| format!("local:{}", g.id))
    };
    // Sessions, notes and status changes this computer does not have.
    let have: BTreeSet<(String, u64)> = local
        .sessions
        .iter()
        .map(|s| (s.game.clone(), s.start))
        .collect();
    let mut seen = have.clone();
    let mut sessions = Vec::new();
    for s in others.iter().flat_map(|e| &e.sessions) {
        if !seen.insert((s.game.clone(), s.start)) {
            continue;
        }
        if let Some(id) = local_id(&s.game) {
            sessions.push(Session {
                game: id,
                start: s.start,
                end: s.end,
            });
        }
    }
    report.sessions = sessions.len();
    let mut seen_notes: BTreeSet<(u64, String)> =
        local.notes.iter().map(|n| (n.at, n.text.clone())).collect();
    let mut notes = Vec::new();
    for n in others.iter().flat_map(|e| &e.notes) {
        if !seen_notes.insert((n.at, n.text.clone())) {
            continue;
        }
        let game = if n.game.is_empty() {
            String::new()
        } else {
            match local_id(&n.game) {
                Some(id) => id,
                None => continue,
            }
        };
        notes.push(Note {
            at: n.at,
            game,
            text: n.text.clone(),
        });
    }
    report.notes = notes.len();
    let mut seen_st: BTreeSet<(u64, String)> = local
        .statuses
        .iter()
        .map(|c| (c.at, c.game.clone()))
        .collect();
    let mut statuses = Vec::new();
    for c in others.iter().flat_map(|e| &e.statuses) {
        if !seen_st.insert((c.at, c.game.clone())) {
            continue;
        }
        if let Some(id) = local_id(&c.game) {
            statuses.push(StatusChange {
                at: c.at,
                game: id,
                status: c.status,
            });
        }
    }
    (report, sessions, notes, statuses)
}

/// Syncs through `folder` (a folder other computers also write to): writes this computer's
/// file, takes in the others'. Blocking.
pub fn sync_folder(folder: &Path) -> anyhow::Result<Report> {
    std::fs::create_dir_all(folder)
        .with_context(|| format!("the sync folder {} cannot be made", folder.display()))?;
    let mut lib = Library::load()?;
    let state_file = state_path()?;
    let mut state: State = std::fs::read_to_string(&state_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let now = stats::now();
    let local = export(&lib, &mut state, now);
    let me = format!("{}.json", local.machine);
    let others: Vec<Export> = std::fs::read_dir(folder)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "json")
                && p.file_name().is_some_and(|n| n != me.as_str())
        })
        .filter_map(|p| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok())
        .collect();
    let (report, sessions, notes, statuses) = merge(&mut lib, &mut state, &others, &local);
    if report.games > 0 {
        lib.save()?;
    }
    for s in &sessions {
        stats::Stats::record(&s.game.parse().context("bad game id")?, s.start, s.end)?;
    }
    for n in &notes {
        stats::add_note_at(n)?;
    }
    for c in &statuses {
        stats::add_status_change(c)?;
    }
    // Written after taking the others in, so this file has everything.
    let local = export(&lib, &mut state, now);
    let tmp = folder.join(format!(".{me}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec(&local)?)?;
    std::fs::rename(&tmp, folder.join(&me))?;
    if let Some(dir) = state_file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&state_file, serde_json::to_vec(&state)?)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::NewGame;

    fn lib_with(names: &[&str]) -> Library {
        let mut lib = Library::default();
        for n in names {
            let mut g = NewGame::windows(*n, format!("/g/{n}.exe").into());
            g.origin = Some(format!("gog:{}", n.len()));
            lib.add(g);
        }
        lib
    }

    #[test]
    fn the_newest_change_wins_and_histories_join() {
        let mut here = lib_with(&["Hades", "Celeste!"]);
        let mut state = State::default();
        let local = export(&here, &mut state, 100);
        let hades = game_key(here.get("hades").unwrap());
        let mut other = Export {
            machine: "laptop".into(),
            written: 200,
            ..Default::default()
        };
        other.games.insert(
            hades.clone(),
            (
                Fields {
                    status: Some(PlayStatus::Beaten),
                    favorite: true,
                    ..Default::default()
                },
                150,
            ),
        );
        other.sessions = vec![
            Session {
                game: hades.clone(),
                start: 10,
                end: 70,
            },
            Session {
                game: "o:steam:1".into(),
                start: 20,
                end: 30,
            },
        ];
        other.notes = vec![Note {
            at: 5,
            game: hades.clone(),
            text: "boss".into(),
        }];
        let (r, sessions, notes, _) = merge(&mut here, &mut state, &[other.clone()], &local);
        let g = here.get("hades").unwrap();
        assert!(g.favorite && g.status == Some(PlayStatus::Beaten));
        assert_eq!(r.games, 1);
        assert_eq!(sessions.len(), 1, "a game this computer lacks is left out");
        assert_eq!(sessions[0].game, "local:hades");
        assert_eq!(notes[0].game, "local:hades");
        // A computer that never synced takes the other's values over its empty ones.
        let mut fresh = lib_with(&["Hades"]);
        let fresh_local = export(&fresh, &mut State::default(), 999);
        merge(
            &mut fresh,
            &mut State::default(),
            &[other.clone()],
            &fresh_local,
        );
        assert!(fresh.get("hades").unwrap().favorite);
        // A change made here later than the other's stays.
        here.get_mut("hades").unwrap().favorite = false;
        let local = export(&here, &mut state, 300);
        let (r, ..) = merge(&mut here, &mut state, &[other], &local);
        assert_eq!(r.games, 0);
        assert!(!here.get("hades").unwrap().favorite);
    }
}
