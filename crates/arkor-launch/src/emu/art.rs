//! Box art for emulator games: libretro's thumbnails (`thumbnails.libretro.com`), named by
//! the No-Intro and Redump names ROM files usually keep, or LaunchBox's box of the game's
//! region when that is switched on (see [`super::launchbox`]). A cover the user chose wins;
//! a game nothing was found for is not asked about again.

use std::path::{Path, PathBuf};

use arkor_core::library::{GameKind, LibraryGame};

use super::{names, systems};

/// The file name a game's thumbnail goes by: its ROM's name without the disc marker (a
/// playlist of discs names its first disc).
pub fn stem_of(game: &Path) -> Option<String> {
    let mut path = game.to_path_buf();
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u"))
        && let Some(first) = std::fs::read_to_string(&path).ok().and_then(|t| {
            t.lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .map(PathBuf::from)
        })
    {
        path = first;
    }
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    if names::parse(&stem).disc.is_none() {
        return Some(stem);
    }
    // "Final Fantasy VII (USA) (Disc 1)" → "Final Fantasy VII (USA)".
    let disc = regex::Regex::new(r"(?i)\s*[\(\[](disc|disk|cd)\s*\d+[^\)\]]*[\)\]]").ok()?;
    Some(disc.replace_all(&stem, "").trim().to_string())
}

fn encode(segment: &str) -> String {
    let mut out = String::new();
    for b in segment.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// libretro's box art of a game of `system` named `stem`: from its `libretro-thumbnails`
/// repository on GitHub (`thumbnails.libretro.com` serves nothing at times), then from
/// libretro's own server.
pub fn thumbnail_urls(system: &str, stem: &str) -> Vec<String> {
    let Some(s) = systems::get(system).filter(|s| !s.libretro.is_empty()) else {
        return Vec::new();
    };
    let file = encode(&names::thumbnail_name(stem));
    vec![
        format!(
            "https://raw.githubusercontent.com/libretro-thumbnails/{}/master/Named_Boxarts/{file}.png",
            encode(&s.libretro.replace(' ', "_"))
        ),
        format!(
            "https://thumbnails.libretro.com/{}/Named_Boxarts/{file}.png",
            encode(&s.libretro)
        ),
    ]
}

fn tried_file() -> PathBuf {
    super::dirs::cache().join("art-tried.json")
}

/// Games looked up already (by their file), found or not.
fn tried() -> Vec<PathBuf> {
    super::read_json(&tried_file())
}

/// Downloads `url` to `dest` quietly (no progress entry: covers are many and small).
fn fetch(url: &str, dest: &Path) -> anyhow::Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "30", "-A", "arkor", "-o"])
        .arg(dest)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .output()?;
    anyhow::ensure!(out.status.success(), "no picture at {url}");
    Ok(())
}

/// A cover for an emulator game without one: LaunchBox's (when on and it has the game),
/// else libretro's. `Ok(None)`: none found (or it has one, or it was looked up before).
/// Blocking (a request or two).
pub fn cover(game: &LibraryGame, launchbox: bool) -> anyhow::Result<Option<PathBuf>> {
    let GameKind::Emulated { system, .. } = &game.kind else {
        return Ok(None);
    };
    if crate::covers::cover_for(game).is_some() {
        return Ok(None);
    }
    let mut seen = tried();
    if seen.contains(&game.exe) {
        return Ok(None);
    }
    let stem = stem_of(&game.exe).unwrap_or_else(|| game.name.clone());
    let mut urls = Vec::new();
    if launchbox && let Some(url) = super::launchbox::cover_url(system, &stem) {
        urls.push(url);
    }
    urls.extend(thumbnail_urls(system, &stem));
    let mut got = None;
    for url in urls {
        let ext = if url.ends_with(".jpg") || url.ends_with(".jpeg") {
            "jpg"
        } else {
            "png"
        };
        let tmp = super::dirs::cache()
            .join("art")
            .join(format!("{}.{ext}", game.id));
        if fetch(&url, &tmp).is_ok() {
            got = Some(crate::covers::set_from_file(&game.id, &tmp)?);
            let _ = std::fs::remove_file(&tmp);
            break;
        }
    }
    seen.push(game.exe.clone());
    let _ = super::write_json(&tried_file(), &seen);
    Ok(got)
}

/// Forgets which games were looked up (LaunchBox was switched on: they are asked again).
pub fn forget_tried() {
    let _ = std::fs::remove_file(tried_file());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_libretro_thumbnails() {
        assert_eq!(
            thumbnail_urls("snes", "Chrono Trigger (USA)"),
            [
                "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Super_Nintendo_Entertainment_System/master/Named_Boxarts/Chrono%20Trigger%20%28USA%29.png",
                "https://thumbnails.libretro.com/Nintendo%20-%20Super%20Nintendo%20Entertainment%20System/Named_Boxarts/Chrono%20Trigger%20%28USA%29.png"
            ]
        );
        // Characters file systems allow but thumbnails replace.
        assert!(thumbnail_urls("gba", "Mario & Luigi (USA)")[0].contains("Mario%20_%20Luigi"));
    }

    #[test]
    fn discs_go_by_their_game() {
        let dir = super::super::test_root("art-stem");
        let disc1 = dir.join("Final Fantasy VII (USA) (Disc 1).cue");
        let m3u = dir.join("ff7.m3u");
        std::fs::write(&m3u, format!("{}\n", disc1.display())).unwrap();
        assert_eq!(stem_of(&m3u).as_deref(), Some("Final Fantasy VII (USA)"));
        assert_eq!(
            stem_of(Path::new("/r/Metroid Fusion (Europe).gba")).as_deref(),
            Some("Metroid Fusion (Europe)")
        );
    }
}
