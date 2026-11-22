//! Game sources. Arkor only lists games that are installed, wherever they came from:
//! Steam, Epic/GOG (via legendary/Heroic), games added to Heroic or Faugus by hand, and
//! games imported into Arkor from a Wine prefix. It never downloads games.

pub mod epic;
pub mod faugus;
pub mod gog;
pub mod heroic;
pub mod prefix;
pub mod steam;
pub mod target;
pub mod vdf;

use anyhow::Context;
use arkor_core::imported::Imported;
use arkor_core::{Game, GameId, Store};

use target::ExeTarget;

/// Every installed game Arkor can find, sorted by name.
/// A failing source is reported in the second value and does not hide the others.
pub fn all_games() -> (Vec<Game>, Vec<anyhow::Error>) {
    let (mut games, mut errors) = steam::installed_games();

    if let Some(legendary) = epic::Legendary::locate() {
        match legendary.games() {
            Ok(epic) => games.extend(epic),
            Err(e) => errors.push(e.context("Epic (legendary)")),
        }
    }

    for (found, errs) in [gog::installed_games(), heroic::sideload_games()] {
        games.extend(found);
        errors.extend(errs);
    }

    match faugus::games() {
        Ok(found) => games.extend(found),
        Err(e) => errors.push(e.context("Faugus Launcher")),
    }

    match Imported::load() {
        Ok(imported) => games.extend(imported.games.iter().map(|g| g.to_game())),
        Err(e) => errors.push(e.context("imported games")),
    }

    games.sort_by_key(|g| g.name.to_lowercase());
    (games, errors)
}

/// What to execute for games Arkor starts directly. Steam and Epic are started by their
/// clients instead (see `arkor_launch::start`), so they have no target here.
pub fn launch_target(id: &GameId) -> anyhow::Result<ExeTarget> {
    match id.store {
        Store::Gog => gog::launch_target(&id.id),
        Store::Heroic => heroic::sideload_target(&id.id),
        Store::Faugus => faugus::launch_target(&id.id),
        Store::Local => {
            let imported = Imported::load()?;
            let game = imported
                .get(&id.id)
                .with_context(|| format!("imported game `{}` not found", id.id))?;
            let mut target = ExeTarget::windows(game.exe.clone());
            target.args = game.args.clone();
            target.prefix = Some(game.prefix.clone());
            Ok(target)
        }
        Store::Steam | Store::Epic => anyhow::bail!(
            "{} games are started by their own client",
            id.store.as_str()
        ),
    }
}
