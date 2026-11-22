//! Cover art for the library grid: the user's own image, Steam's cached capsule for Steam
//! games, or one downloaded (from a URL or SteamGridDB) into `~/.local/share/arkor/covers`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::library::{GameKind, LibraryGame};
use serde::Deserialize;

const SGDB: &str = "https://www.steamgriddb.com/api/v2";

pub fn covers_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/covers"))
}

/// Steam's portrait capsule for an app, if Steam has cached it.
pub fn steam_cover(appid: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let roots = [
        home.join(".local/share/Steam/appcache/librarycache"),
        home.join(".steam/steam/appcache/librarycache"),
    ];
    for root in roots {
        let dir = root.join(appid);
        for name in ["library_600x900.jpg", "library_capsule.jpg"] {
            if dir.join(name).is_file() {
                return Some(dir.join(name));
            }
        }
        // Newer Steam keeps each image in a hash-named subfolder.
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            for name in ["library_600x900.jpg", "library_capsule.jpg"] {
                if e.path().join(name).is_file() {
                    return Some(e.path().join(name));
                }
            }
        }
    }
    None
}

fn own_cover(id: &str) -> Option<PathBuf> {
    let dir = covers_dir().ok()?;
    ["png", "jpg", "jpeg", "webp"]
        .iter()
        .map(|ext| dir.join(format!("{id}.{ext}")))
        .find(|p| p.is_file())
}

/// The image to show for a game, if any.
pub fn cover_for(game: &LibraryGame) -> Option<PathBuf> {
    if let Some(p) = game.cover.as_ref().filter(|p| p.is_file()) {
        return Some(p.clone());
    }
    if let Some(own) = own_cover(&game.id) {
        return Some(own);
    }
    match &game.kind {
        GameKind::Steam { appid } => steam_cover(appid),
        _ => None,
    }
}

fn remove_own(id: &str) {
    if let Ok(dir) = covers_dir() {
        for ext in ["png", "jpg", "jpeg", "webp"] {
            let _ = std::fs::remove_file(dir.join(format!("{id}.{ext}")));
        }
    }
}

/// Copies an image file in as the game's cover.
pub fn set_from_file(id: &str, src: &Path) -> anyhow::Result<PathBuf> {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    anyhow::ensure!(
        ["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()),
        "not a png/jpg/webp image"
    );
    let dir = covers_dir()?;
    std::fs::create_dir_all(&dir)?;
    remove_own(id);
    let dest = dir.join(format!("{id}.{ext}"));
    std::fs::copy(src, &dest)?;
    Ok(dest)
}

/// Downloads an image as the game's cover. Blocking.
pub fn set_from_url(id: &str, url: &str) -> anyhow::Result<PathBuf> {
    let url = url.trim();
    anyhow::ensure!(
        url.starts_with("https://") || url.starts_with("http://"),
        "not a web address"
    );
    let ext = url
        .rsplit('.')
        .next()
        .map(|e| e.split(['?', '#']).next().unwrap_or("").to_lowercase())
        .filter(|e| ["png", "jpg", "jpeg", "webp"].contains(&e.as_str()))
        .unwrap_or_else(|| "png".into());
    let dir = covers_dir()?;
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".{id}.download.{ext}"));
    crate::tools::download(url, &tmp)?;
    let dest = set_from_file(id, &tmp);
    let _ = std::fs::remove_file(&tmp);
    dest
}

/// Forgets the game's own cover (Steam games fall back to Steam's).
pub fn reset(id: &str) {
    remove_own(id);
}

#[derive(Deserialize)]
struct SgdbResponse<T> {
    success: bool,
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Deserialize)]
struct SgdbGame {
    id: u64,
}

#[derive(Deserialize)]
struct SgdbGrid {
    url: String,
}

/// Finds the game on SteamGridDB and downloads its top-rated 600×900 grid. Blocking.
pub fn set_from_steamgriddb(id: &str, name: &str, api_key: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !api_key.trim().is_empty(),
        "add a SteamGridDB API key in Global settings first"
    );
    let term: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("%20");
    let found: SgdbResponse<SgdbGame> = serde_json::from_str(&crate::tools::http_get_bearer(
        &format!("{SGDB}/search/autocomplete/{term}"),
        api_key,
    )?)
    .context("unexpected SteamGridDB response")?;
    anyhow::ensure!(
        found.success,
        "SteamGridDB refused the request (check the API key)"
    );
    let game = found
        .data
        .first()
        .with_context(|| format!("“{name}” not found on SteamGridDB"))?;
    let grids: SgdbResponse<SgdbGrid> = serde_json::from_str(&crate::tools::http_get_bearer(
        &format!(
            "{SGDB}/grids/game/{}?dimensions=600x900&types=static",
            game.id
        ),
        api_key,
    )?)
    .context("unexpected SteamGridDB response")?;
    let grid = grids
        .data
        .first()
        .context("SteamGridDB has no 600×900 cover for this game")?;
    set_from_url(id, &grid.url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_cover_replaces_previous() {
        let _lock = crate::CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests that change XDG dirs hold CONFIG_ENV_LOCK.
        unsafe { std::env::set_var("XDG_DATA_HOME", tmp.path()) };
        let a = tmp.path().join("a.jpg");
        let b = tmp.path().join("b.png");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        set_from_file("game", &a).unwrap();
        set_from_file("game", &b).unwrap();
        assert_eq!(
            own_cover("game"),
            Some(covers_dir().unwrap().join("game.png"))
        );
        assert!(!covers_dir().unwrap().join("game.jpg").exists());
        reset("game");
        assert_eq!(own_cover("game"), None);
        assert!(set_from_file("game", Path::new("/x/readme.txt")).is_err());
    }
}
