//! Cover art for the library grid: the user's own image, Steam's cached capsule for Steam
//! games, or one downloaded (from a URL or SteamGridDB) into `~/.local/share/arkor/covers`.
//! SteamGridDB covers can be picked from its list (still pictures or animated ones), and a
//! game added without a cover gets the top one when SteamGridDB knows the exact name.

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
    ["png", "jpg", "jpeg", "webp", "gif"]
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
        for ext in ["png", "jpg", "jpeg", "webp", "gif"] {
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
        ["png", "jpg", "jpeg", "webp", "gif"].contains(&ext.as_str()),
        "not a png/jpg/webp/gif image"
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
        .filter(|e| ["png", "jpg", "jpeg", "webp", "gif"].contains(&e.as_str()))
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

/// Finds the game on SteamGridDB and uses its top-rated still cover. Blocking.
pub fn set_from_steamgriddb(id: &str, name: &str, api_key: &str) -> anyhow::Result<PathBuf> {
    let found = sgdb_search(name, api_key)?;
    let game = found
        .first()
        .with_context(|| format!("“{name}” not found on SteamGridDB"))?;
    let covers = sgdb_covers(game.id, false, api_key)?;
    let image = covers
        .first()
        .context("SteamGridDB has no portrait cover for this game")?;
    set_from_sgdb(id, image)
}

/// A game on SteamGridDB.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SgdbMatch {
    pub id: u64,
    pub name: String,
    /// Unix seconds.
    #[serde(default)]
    pub release_date: Option<i64>,
}

impl SgdbMatch {
    pub fn year(&self) -> Option<i32> {
        // 365.2425 days a year from 1970.
        self.release_date
            .filter(|t| *t > 0)
            .map(|t| 1970 + (t as f64 / 31_556_952.0) as i32)
    }
}

/// One cover on SteamGridDB.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SgdbImage {
    pub id: u64,
    pub url: String,
    pub thumb: String,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// From the animated list (an animated WebP).
    #[serde(skip)]
    pub animated: bool,
}

fn sgdb_get<T: serde::de::DeserializeOwned>(path: &str, api_key: &str) -> anyhow::Result<Vec<T>> {
    anyhow::ensure!(
        !api_key.trim().is_empty(),
        "add a SteamGridDB API key in Settings → Integrations first"
    );
    let r: SgdbResponse<T> = serde_json::from_str(&crate::tools::http_get_bearer(
        &format!("{SGDB}{path}"),
        api_key,
    )?)
    .context("unexpected SteamGridDB response")?;
    anyhow::ensure!(
        r.success,
        "SteamGridDB refused the request (check the API key)"
    );
    Ok(r.data)
}

fn search_term(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("%20")
}

/// Games on SteamGridDB matching `name`, best first. Blocking.
pub fn sgdb_search(name: &str, api_key: &str) -> anyhow::Result<Vec<SgdbMatch>> {
    let term = search_term(name);
    anyhow::ensure!(!term.is_empty(), "type a game name");
    sgdb_get(&format!("/search/autocomplete/{term}"), api_key)
}

/// Portrait covers of a SteamGridDB game: still pictures, or animated ones. Blocking.
pub fn sgdb_covers(game: u64, animated: bool, api_key: &str) -> anyhow::Result<Vec<SgdbImage>> {
    let query = if animated {
        // SteamGridDB serves animated covers as WebP (and APNG, which is not played here).
        "types=animated&mimes=image/webp"
    } else {
        "types=static&mimes=image/png,image/jpeg,image/webp"
    };
    let mut list: Vec<SgdbImage> = sgdb_get(
        &format!("/grids/game/{game}?dimensions=600x900,342x482,660x930&{query}"),
        api_key,
    )?;
    for image in &mut list {
        image.animated = animated;
    }
    Ok(list)
}

fn has_ffmpeg() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Turns an animation (SteamGridDB's preview video, or the animated cover itself) into an
/// animated WebP `width` pixels wide at most, at the source's own frame rate. Blocking.
fn video_to_webp(src: &Path, dest: &Path, width: u32) -> anyhow::Result<()> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(src)
        .args([
            "-vf",
            &format!("scale='min({width},iw)':-2:flags=lanczos"),
            "-c:v",
            "libwebp_anim",
            "-lossless",
            "0",
            "-q:v",
            "75",
            "-loop",
            "0",
            "-an",
        ])
        .arg(dest)
        .output()
        .context("running ffmpeg")?;
    anyhow::ensure!(
        out.status.success(),
        "ffmpeg failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(())
}

fn sgdb_cache() -> anyhow::Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("no cache dir")?
        .join("arkor/steamgriddb");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn is_video(url: &str) -> bool {
    [".webm", ".mp4"].iter().any(|e| url.ends_with(e))
}

/// An animated cover as a small animated WebP (the originals can be tens of MB, all of it
/// decoded into memory to play). Previews come from SteamGridDB's small preview video;
/// `full` covers (the one put in the library) from the full-size animation, so they stay
/// sharp at cover size.
fn animated_webp(image: &SgdbImage, width: u32, full: bool) -> anyhow::Result<PathBuf> {
    let dir = sgdb_cache()?;
    let path = dir.join(format!("{}-{width}-v2.webp", image.id));
    if !path.is_file() {
        let (source, name) = if full {
            (&image.url, format!("{}.full", image.id))
        } else {
            (&image.thumb, format!("{}.video", image.id))
        };
        let src = dir.join(&name);
        if !src.is_file() {
            let tmp = dir.join(format!(".{name}.part"));
            crate::tools::download(source, &tmp)?;
            std::fs::rename(&tmp, &src)?;
        }
        let tmp = dir.join(format!(".{}-{width}.webp", image.id));
        let made = video_to_webp(&src, &tmp, width);
        if full {
            // The original is only a source; it can be very large.
            let _ = std::fs::remove_file(&src);
        }
        made?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(path)
}

/// The small preview of a cover, downloaded once into Arkor's cache. Blocking.
pub fn sgdb_thumb(image: &SgdbImage) -> anyhow::Result<PathBuf> {
    crate::progress::quietly(|| thumb(image))
}

fn thumb(image: &SgdbImage) -> anyhow::Result<PathBuf> {
    if is_video(&image.thumb) && has_ffmpeg() {
        return animated_webp(image, 208, false);
    }
    // Without ffmpeg an animated cover's preview is the cover itself.
    let source = if is_video(&image.thumb) {
        &image.url
    } else {
        &image.thumb
    };
    let ext = source
        .rsplit('.')
        .next()
        .map(|e| e.split(['?', '#']).next().unwrap_or("").to_lowercase())
        .filter(|e| ["png", "jpg", "jpeg", "webp", "gif"].contains(&e.as_str()))
        .unwrap_or_else(|| "png".into());
    let dir = sgdb_cache()?;
    let path = dir.join(format!("{}.{ext}", image.id));
    if !path.is_file() {
        let tmp = dir.join(format!(".{}.part", image.id));
        crate::tools::download(source, &tmp)?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(path)
}

/// Uses a SteamGridDB cover for the game (an animated one as a small animated WebP when
/// ffmpeg is there). Blocking.
pub fn set_from_sgdb(id: &str, image: &SgdbImage) -> anyhow::Result<PathBuf> {
    if image.animated && is_video(&image.thumb) && has_ffmpeg() {
        return set_from_file(id, &animated_webp(image, 480, true)?);
    }
    set_from_url(id, &image.url)
}

fn same_name(a: &str, b: &str) -> bool {
    let squash = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    squash(a) == squash(b)
}

/// For a game added without a cover: SteamGridDB's top still cover, but only when a game
/// there has exactly this name (so a guess never puts a wrong picture in). Blocking.
pub fn auto_cover(game: &LibraryGame, api_key: &str) -> anyhow::Result<Option<PathBuf>> {
    if api_key.trim().is_empty() || cover_for(game).is_some() {
        return Ok(None);
    }
    let Some(found) = sgdb_search(&game.name, api_key)?
        .into_iter()
        .find(|m| same_name(&m.name, &game.name))
    else {
        return Ok(None);
    };
    match sgdb_covers(found.id, false, api_key)?.first() {
        Some(image) => set_from_sgdb(&game.id, image).map(Some),
        None => Ok(None),
    }
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

#[cfg(test)]
mod sgdb_network {
    /// Network: needs ARKOR_SGDB_KEY.
    #[test]
    #[ignore]
    fn searches_and_lists_covers() {
        let key = std::env::var("ARKOR_SGDB_KEY").unwrap();
        let found = super::sgdb_search("Warframe", &key).unwrap();
        println!(
            "matches: {:?}",
            found
                .iter()
                .take(3)
                .map(|m| (&m.name, m.year()))
                .collect::<Vec<_>>()
        );
        let game = found[0].id;
        let still = super::sgdb_covers(game, false, &key).unwrap();
        let moving = super::sgdb_covers(game, true, &key).unwrap();
        println!("still {} animated {}", still.len(), moving.len());
        println!(
            "first still {} {}x{} {}",
            still[0].mime, still[0].width, still[0].height, still[0].thumb
        );
        if let Some(m) = moving.first() {
            println!("first animated {} {}", m.mime, m.thumb);
        }
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: single test, ignored by default.
        unsafe { std::env::set_var("XDG_CACHE_HOME", tmp.path()) };
        let thumb = super::sgdb_thumb(&still[0]).unwrap();
        println!("thumb {} bytes", std::fs::metadata(&thumb).unwrap().len());
        if let Some(m) = moving.first() {
            let t = super::animated_webp(m, 480, true).unwrap();
            println!(
                "animated cover {} ({} bytes)",
                t.display(),
                std::fs::metadata(&t).unwrap().len()
            );
            let t = super::sgdb_thumb(m).unwrap();
            println!(
                "animated preview {} ({} bytes)",
                t.display(),
                std::fs::metadata(&t).unwrap().len()
            );
        }
    }
}
