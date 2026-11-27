//! Mods only while the game runs: `arkor run` places them in the game folder just before the
//! game starts and takes them out when it ends, so the folder is otherwise the store's own
//! (its updates and file checks see no mods, and nothing is left behind).

use std::path::Path;

use arkor_core::GameId;
use arkor_core::library::{GameKind, LibraryGame};

use super::{Link, amethyst, link};

/// Whether `game` starts through `arkor run`: every game Arkor starts does, and a Steam game
/// does when Steam's launch options for it are Arkor's (Steam starts it, from Arkor or not).
pub fn through_arkor(id: &GameId, game: &LibraryGame) -> bool {
    match &game.kind {
        GameKind::Steam { appid } => crate::steam_link::launch_options(appid)
            .is_some_and(|o| crate::steam_link::is_wrapper(&o, id)),
        _ => true,
    }
}

/// Before the game starts: its mods are placed in `game_dir` (when they are kept out of it
/// otherwise) and what their loaders need from Wine is read again, for this launch.
/// Returns what takes them out again when it is dropped (also when the launch fails before
/// the game starts).
pub fn before(id: &GameId, game_dir: Option<&Path>) -> Option<Placed> {
    let mut link = link::load(id).filter(|l| l.while_playing)?;
    let placed = amethyst::with(|c| {
        super::deploy(c, &link)?;
        link::refresh(c, id, &mut link)
    });
    // Not needed while the game runs.
    amethyst::release_now();
    if let Err(e) = placed {
        eprintln!("arkor: the mods were not placed: {e:#}");
        return None;
    }
    let missing = game_dir.and_then(super::missing_loader).map(str::to_string);
    if missing != link.loader_missing {
        link.loader_missing = missing;
        let _ = link::save(id, &link);
    }
    Some(Placed(link))
}

/// A game's mods placed for one launch; dropping it takes them out of the game folder.
#[derive(Debug)]
pub struct Placed(Link);

impl Drop for Placed {
    fn drop(&mut self) {
        if let Err(e) = amethyst::with(|c| super::restore(c, &self.0)) {
            eprintln!("arkor: the mods were not taken out of the game folder: {e:#}");
        }
        amethyst::release_now();
    }
}

/// Turns "only while playing" on or off for a game, and puts the folder in the state that
/// goes with it: mods out (on, and the game starts through Arkor) or in.
pub fn set(id: &GameId, on: bool, through: bool) -> anyhow::Result<()> {
    let mut link = link::load(id).ok_or_else(|| anyhow::anyhow!("no mods for {id}"))?;
    link.while_playing = on;
    link::save(id, &link)?;
    link.while_playing = on && through;
    amethyst::with(|c| super::settle(c, &link))
}
