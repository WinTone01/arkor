//! Game sources. The library is the user's own list (`arkor_core::library`); the other
//! launchers here only *suggest* games to add: Steam, Epic/GOG (via legendary/Heroic),
//! games added to Heroic, Faugus or Lutris. Store games Arkor installs itself
//! (`arkor_launch::shops`) are found the same way.

pub mod amazon;
pub mod bottles;
pub mod carry;
pub mod epic;
pub mod faugus;
pub mod gog;
pub mod heroic;
pub mod lutris;
pub mod prefix;
pub mod steam;
pub mod steam_library;
pub mod target;
pub mod vdf;
pub mod vdf_edit;

use anyhow::Context;
use arkor_core::library::{GameKind, Library, NewGame};
use arkor_core::{Game, GameId, Store};

use target::ExeTarget;

/// The helper program `name` (`legendary`, `gogdl`, `nile`) Arkor downloaded, if it did.
pub fn helper_bin(name: &str) -> Option<std::path::PathBuf> {
    let dir = arkor_core::paths::stores_dir().ok()?.join("bin");
    Some(dir.join(name)).filter(|p| p.is_file())
}

/// The user's library as games, sorted by name.
pub fn library_games() -> anyhow::Result<Vec<Game>> {
    let mut games = Library::load()?.games();
    games.sort_by_key(|g| g.name.to_lowercase());
    Ok(games)
}

/// A game found in another launcher, ready to add to the library.
#[derive(Debug, Clone)]
pub struct Detected {
    pub game: Game,
    /// `Err` when Arkor found the game but cannot tell how to start it.
    pub new: Result<NewGame, String>,
}

/// Installed games in other launchers, sorted by name. A failing source is reported in the
/// second value and does not hide the others.
pub fn detect() -> (Vec<Detected>, Vec<anyhow::Error>) {
    let (mut games, mut errors) = steam::installed_games();
    for legendary in epic::Legendary::all() {
        match legendary.games() {
            Ok(epic) => games.extend(epic),
            Err(e) => errors.push(e.context("Epic (legendary)")),
        }
    }
    for (found, errs) in [
        gog::installed_games(),
        amazon::installed_games(),
        heroic::sideload_games(),
        lutris::games(),
        bottles::games(),
    ] {
        games.extend(found);
        errors.extend(errs);
    }
    match faugus::games() {
        Ok(found) => games.extend(found),
        Err(e) => errors.push(e.context("Faugus Launcher")),
    }
    // The same game through two launchers (or two logins) is listed once.
    let mut seen = std::collections::HashSet::new();
    games.retain(|g| seen.insert(g.id.clone()));
    games.sort_by_key(|g| g.name.to_lowercase());
    let detected = games
        .into_iter()
        .map(|game| {
            let new = to_new_game(&game).map_err(|e| format!("{e:#}"));
            Detected { game, new }
        })
        .collect();
    (detected, errors)
}

/// How to add a detected game: Steam/Epic stay with their clients, everything else becomes
/// a Windows/Linux entry pointing at the same files (and prefix/Proton) the launcher uses.
pub fn to_new_game(game: &Game) -> anyhow::Result<NewGame> {
    let origin = Some(game.id.to_string());
    let base = |kind: GameKind| NewGame {
        name: game.name.clone(),
        kind,
        exe: game.install_dir.clone().unwrap_or_default(),
        prefix: None,
        args: Vec::new(),
        working_dir: None,
        source_proton: None,
        origin: origin.clone(),
    };
    match game.id.store {
        Store::Steam => Ok(base(GameKind::Steam {
            appid: game.id.id.clone(),
        })),
        Store::Epic => {
            let mut new = base(GameKind::Epic {
                app_name: game.id.id.clone(),
            });
            new.prefix = heroic::wine_prefix(&game.id.id);
            new.source_proton = heroic::proton(&game.id.id);
            Ok(new)
        }
        Store::Amazon => Ok(base(GameKind::Amazon {
            id: game.id.id.clone(),
        })),
        Store::Gog | Store::Heroic | Store::Faugus | Store::Lutris | Store::Bottles => {
            let target = source_target(&game.id)?;
            Ok(NewGame {
                name: game.name.clone(),
                kind: if target.native {
                    GameKind::Linux
                } else {
                    GameKind::Windows
                },
                working_dir: (target.exe.parent() != Some(target.working_dir.as_path()))
                    .then_some(target.working_dir),
                exe: target.exe,
                prefix: target.prefix,
                args: target.args,
                source_proton: target.proton,
                origin,
            })
        }
        Store::Local => anyhow::bail!("already a library game"),
    }
}

fn source_target(id: &GameId) -> anyhow::Result<ExeTarget> {
    match id.store {
        Store::Gog => gog::launch_target(&id.id),
        Store::Heroic => heroic::sideload_target(&id.id),
        Store::Faugus => faugus::launch_target(&id.id),
        Store::Lutris => lutris::launch_target(&id.id),
        Store::Bottles => bottles::launch_target(&id.id),
        _ => anyhow::bail!("{} games have no executable target", id.store.as_str()),
    }
}

/// What to execute for a Windows/Linux library game.
pub fn launch_target(id: &GameId, prefixes_dir: &std::path::Path) -> anyhow::Result<ExeTarget> {
    anyhow::ensure!(id.store == Store::Local, "`{id}` is not a library game");
    let library = Library::load()?;
    let game = library
        .get(&id.id)
        .with_context(|| format!("game `{id}` is not in the library"))?;
    let mut target = ExeTarget::windows(game.exe.clone());
    target.native = game.kind == GameKind::Linux;
    target.args = game.args.clone();
    if let Some(dir) = &game.working_dir {
        target.working_dir = dir.clone();
    }
    if game.needs_prefix() {
        target.prefix = Some(game.effective_prefix(prefixes_dir));
    }
    target.proton = game.source_proton.clone();
    Ok(target)
}
