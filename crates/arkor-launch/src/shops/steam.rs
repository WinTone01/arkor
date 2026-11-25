//! Steam through its own client: the library is read from Steam's files, installing and
//! removing is left to Steam, and its progress is read from the game's manifest.
//!
//! To install without Steam's confirmation window, Arkor leaves a manifest that marks the
//! game as wanted and starts Steam without its window: Steam then downloads it by itself.
//! A Steam that runs a game is not closed for this; its own window is used then.

use std::time::{Duration, Instant};

use anyhow::Context;
use arkor_stores::steam_library as lib;

use super::{Installed, Order, Owned, Shop};
use crate::progress::{self, Transfer};
use crate::{steam_link, tools};

const CDN: &str = "https://shared.steamstatic.com/store_item_assets/steam/apps";

pub(super) fn logged_in() -> bool {
    lib::account().is_some()
}

pub(super) fn account() -> Option<String> {
    lib::account()
        .map(|(_, name)| name)
        .filter(|n| !n.is_empty())
}

pub(super) fn owned() -> anyhow::Result<Vec<Owned>> {
    Ok(lib::games()?
        .into_iter()
        .map(|(id, info)| Owned {
            id: id.to_string(),
            title: info.name,
            cover: Some(format!("{CDN}/{id}/library_600x900.jpg")),
            wide: Some(format!("{CDN}/{id}/library_hero.jpg")),
            linux: false,
            windows: true,
            namespace: None,
        })
        .collect())
}

pub(super) fn installed() -> anyhow::Result<Vec<Installed>> {
    Ok(arkor_stores::steam::installed_games()
        .0
        .into_iter()
        .map(|g| Installed {
            shop: Shop::Steam,
            id: g.id.id,
            title: g.name,
            path: g.install_dir.unwrap_or_default(),
            version: String::new(),
            linux: false,
            size: g.size_on_disk,
        })
        .collect())
}

/// Starts Steam without its window.
fn start_silent() -> anyhow::Result<()> {
    steam_link::start_silent()
}

pub(super) fn install(id: &str, order: &Order) -> anyhow::Result<()> {
    let appid: u32 = id.parse().context("not a Steam app")?;
    if lib::progress(appid).is_none() {
        // Steam is not closed under a running game: the download waits for it to end
        // (Steam's own install window would come up over the game otherwise).
        while steam_link::steam_running() && steam_link::any_game_running() {
            if progress::stop_requested() {
                anyhow::bail!(tools::STOPPED);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        {
            // Steam reads manifests when it starts: closed, given the manifest, started
            // without its window, it downloads the game without asking anything.
            steam_link::shutdown()?;
            let root = lib::root().context("Steam is not installed")?;
            let infos = arkor_stores::steam_library::parse_appinfo(
                &std::fs::read(root.join("appcache/appinfo.vdf"))?,
                &[appid].into(),
            )?;
            let info = infos.get(&appid).cloned().unwrap_or_default();
            let owner = lib::account().map(|(id, _)| id).unwrap_or_default();
            lib::write_install_manifest(&root, appid, &info, &owner)?;
            start_silent()?;
        }
    }
    watch(appid, &order.title)
}

/// Follows Steam's download until the game is installed. Stopping only stops following
/// (Steam goes on with it).
fn watch(appid: u32, label: &str) -> anyhow::Result<()> {
    let transfer = Transfer::begin(label, None);
    let began = Instant::now();
    let mut last = (Instant::now(), 0u64);
    let mut speed = 0.;
    let mut seen_work = false;
    loop {
        if progress::stop_requested() {
            anyhow::bail!(tools::STOPPED);
        }
        match lib::progress(appid) {
            Some(p) => {
                seen_work |= p.total > 0 || p.state & 1024 != 0;
                let secs = last.0.elapsed().as_secs_f64();
                if secs >= 1. {
                    let now = p.done as f64;
                    speed = ((now - last.1 as f64) / secs).max(0.);
                    last = (Instant::now(), p.done);
                }
                let total = (p.total > 0).then_some(p.total);
                if !transfer.report(p.done, total, speed) {
                    anyhow::bail!(tools::STOPPED);
                }
                if p.installed() && (seen_work || began.elapsed() > Duration::from_secs(20)) {
                    return Ok(());
                }
            }
            None if began.elapsed() > Duration::from_secs(10 * 60) => {
                anyhow::bail!("Steam did not start the download (log in to Steam and try again)")
            }
            None => {}
        }
        std::thread::sleep(Duration::from_millis(700));
    }
}

pub(super) fn repair(game: &Installed) -> anyhow::Result<()> {
    steam_link::open_url(&format!("steam://validate/{}", game.id))
}

pub(super) fn uninstall(game: &Installed) -> anyhow::Result<()> {
    let appid: u32 = game.id.parse().context("not a Steam app")?;
    // Without Steam's window: closed, the game's files and manifest removed, started again
    // without its window. With a game running, Steam's own way is used.
    if !steam_link::any_game_running()
        && let Some(manifest) = lib::manifest(appid)
        && game.path.join("..").exists()
    {
        let common = manifest
            .parent()
            .map(|s| s.join("common"))
            .context("no library folder")?;
        // Only a folder in the library's `common` folder is removed.
        let real = std::fs::canonicalize(&game.path)?;
        anyhow::ensure!(
            real.parent() == std::fs::canonicalize(&common).ok().as_deref(),
            "{} is not in Steam's library",
            game.path.display()
        );
        // Some games share their folder with another one (mods, sequels in one install).
        let dir = real.file_name().unwrap_or_default().to_string_lossy();
        anyhow::ensure!(
            !lib::folder_shared(&manifest, &dir),
            "another Steam game uses the same folder: remove it in Steam"
        );
        steam_link::shutdown()?;
        std::fs::remove_dir_all(&real)?;
        std::fs::remove_file(&manifest)?;
        return start_silent();
    }
    steam_link::open_url(&format!("steam://uninstall/{appid}"))?;
    let began = Instant::now();
    while lib::manifest(appid).is_some() {
        anyhow::ensure!(
            began.elapsed() < Duration::from_secs(5 * 60),
            "confirm it in Steam's window"
        );
        std::thread::sleep(Duration::from_millis(700));
    }
    Ok(())
}
