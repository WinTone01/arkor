//! Pictures of store games, cached in `~/.cache/arkor/stores`.

use std::path::PathBuf;

use anyhow::Context;

use super::{Owned, Shop, gog};
use crate::{progress, tools};

const EXTENSIONS: [&str; 4] = ["jpg", "png", "webp", "gif"];

fn dir(shop: Shop) -> anyhow::Result<PathBuf> {
    Ok(dirs::cache_dir()
        .context("no cache dir")?
        .join("arkor/stores")
        .join(shop.key()))
}

/// A file name for an id (ids are the store's; only plain characters are kept).
fn stem(id: &str, kind: &str) -> String {
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe}{kind}")
}

fn cached(shop: Shop, id: &str, kind: &str) -> Option<PathBuf> {
    let dir = dir(shop).ok()?;
    let stem = stem(id, kind);
    EXTENSIONS
        .iter()
        .map(|e| dir.join(format!("{stem}.{e}")))
        .find(|p| p.is_file())
}

/// The portrait picture, if it was fetched before.
pub fn cover(shop: Shop, id: &str) -> Option<PathBuf> {
    // Steam keeps its own copy of the pictures of the library.
    if shop == Shop::Steam
        && let Some(p) = crate::covers::steam_cover(id)
    {
        return Some(p);
    }
    cached(shop, id, "")
}

/// The wide picture, if it was fetched before.
pub fn wide(shop: Shop, id: &str) -> Option<PathBuf> {
    if shop == Shop::Steam
        && let Some(p) = crate::covers::steam_hero(id)
    {
        return Some(p);
    }
    cached(shop, id, "-wide")
}

/// What the file is, by its first bytes (the stores' addresses rarely say).
fn kind_of(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xff, 0xd8, 0xff, ..] => Some("jpg"),
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some("webp"),
        _ => None,
    }
}

fn fetch(shop: Shop, id: &str, kind: &str, url: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(url.starts_with("https://"), "not a web address");
    let dir = dir(shop)?;
    let stem = stem(id, kind);
    let tmp = dir.join(format!(".{stem}.new"));
    progress::quietly(|| tools::download(url, &tmp))?;
    let bytes = std::fs::read(&tmp)?;
    let Some(ext) = kind_of(&bytes) else {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("not a picture");
    };
    let path = dir.join(format!("{stem}.{ext}"));
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// Epic resizes pictures on request.
fn sized(shop: Shop, url: &str, width: u32, height: u32) -> String {
    if shop == Shop::Epic && url.contains("epicgames.com") && !url.contains('?') {
        format!("{url}?h={height}&w={width}&resize=1")
    } else {
        url.to_string()
    }
}

/// Fetches the portrait picture. Blocking.
pub fn fetch_cover(shop: Shop, game: &Owned) -> anyhow::Result<PathBuf> {
    if let Some(p) = cover(shop, &game.id) {
        return Ok(p);
    }
    let url = match &game.cover {
        Some(u) => sized(shop, u, 360, 480),
        None if shop == Shop::Gog => gog::cover_url(&game.id)
            .ok()
            .or_else(|| game.wide.clone())
            .context("no cover")?,
        None => game.wide.clone().context("no cover")?,
    };
    fetch(shop, &game.id, "", &url)
}

/// Fetches the wide picture. Blocking.
pub fn fetch_wide(shop: Shop, game: &Owned) -> anyhow::Result<PathBuf> {
    if let Some(p) = wide(shop, &game.id) {
        return Ok(p);
    }
    let url = match shop {
        Shop::Gog => gog::wide_url(&game.id).ok(),
        _ => None,
    };
    let url = url
        .or_else(|| game.wide.clone())
        .context("no wide picture")?;
    fetch(shop, &game.id, "-wide", &sized(shop, &url, 1920, 1080))
}

/// Gives a library game its store's cover. Blocking.
pub fn use_as_cover(shop: Shop, id: &str, library_id: &str) -> anyhow::Result<Option<PathBuf>> {
    let game = super::owned(shop)
        .into_iter()
        .find(|g| g.id == id)
        .context("unknown game")?;
    // Epic's, larger than the store page shows it.
    let file = match &game.cover {
        Some(u) if shop == Shop::Epic => fetch(shop, id, "-large", &sized(shop, u, 600, 800))?,
        _ => fetch_cover(shop, &game)?,
    };
    crate::covers::set_from_file(library_id, &file).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_pictures_by_their_bytes() {
        assert_eq!(kind_of(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
        assert_eq!(kind_of(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(kind_of(b"<html>"), None);
        assert_eq!(stem("../a b", ""), "___a_b");
        assert!(
            sized(Shop::Epic, "https://cdn1.epicgames.com/x.jpg", 3, 4)
                .ends_with("?h=4&w=3&resize=1")
        );
        assert_eq!(sized(Shop::Gog, "https://x/y.jpg", 3, 4), "https://x/y.jpg");
    }
}
