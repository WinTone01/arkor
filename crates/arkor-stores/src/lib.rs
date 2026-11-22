//! Store backends. Each one turns a store's local state into `arkor_core::Game`s.

pub mod epic;
pub mod gog;
pub mod heroic;
pub mod steam;
pub mod vdf;

use arkor_core::Game;

/// Every game Arkor knows about, from all stores, sorted by name.
/// A failing store is reported in the second value and does not hide the others.
pub fn all_games() -> (Vec<Game>, Vec<anyhow::Error>) {
    let (mut games, mut errors) = steam::installed_games();

    if let Some(legendary) = epic::Legendary::locate() {
        match legendary.games() {
            Ok(epic) => games.extend(epic),
            Err(e) => errors.push(e.context("Epic (legendary)")),
        }
    }

    let (gog, gog_errors) = gog::installed_games();
    games.extend(gog);
    errors.extend(gog_errors);

    games.sort_by_key(|g| g.name.to_lowercase());
    (games, errors)
}
