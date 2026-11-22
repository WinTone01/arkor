//! NVIDIA Streamline SDK updates: replaces the `sl.*.dll` and `nvngx_*.dll` files a game
//! ships with the ones from the latest SDK release (newer DLSS, Frame Generation and Reflex).
//! Only files the game already has are replaced, and each one is kept as a backup.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::GameId;

use crate::optiscaler::{Manifest, Mod};

const API: &str = "https://api.github.com/repos/NVIDIA-RTX/Streamline/releases/latest";

fn root() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/tools/streamline"))
}

/// Latest SDK version, e.g. `v2.14.16`.
pub fn latest_version() -> anyhow::Result<String> {
    let v: serde_json::Value = serde_json::from_str(&crate::tools::http_get(API)?)
        .context("unexpected GitHub response")?;
    Ok(v["tag_name"]
        .as_str()
        .context("no tag in release")?
        .to_string())
}

/// Downloaded SDKs as (version, folder of x64 DLLs), newest first.
pub fn downloaded() -> Vec<(String, PathBuf)> {
    let Ok(entries) = root().and_then(|r| Ok(std::fs::read_dir(r)?)) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| {
            !e.file_name().to_string_lossy().starts_with('.')
                && e.path().join("sl.interposer.dll").is_file()
        })
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    out.sort_by_key(|(v, _)| std::cmp::Reverse(crate::start::natural_key(v)));
    out
}

/// Downloads the SDK (about 280 MB) and keeps only its production x64 DLLs. Blocking.
pub fn download(version: &str) -> anyhow::Result<PathBuf> {
    let dest = root()?.join(version);
    if dest.join("sl.interposer.dll").is_file() {
        return Ok(dest);
    }
    let staging = root()?.join(format!(".{version}.partial"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let zip = staging.join("sdk.zip");
    let bare = version.trim_start_matches('v');
    crate::tools::download(
        &format!(
            "https://github.com/NVIDIA-RTX/Streamline/releases/download/v{bare}/streamline-sdk-v{bare}.zip"
        ),
        &zip,
    )?;
    let status = std::process::Command::new("bsdtar")
        .arg("-xf")
        .arg(&zip)
        .arg("-C")
        .arg(&staging)
        .args(["--include", "*bin/x64/*.dll"])
        .status()
        .context("running bsdtar")?;
    let _ = std::fs::remove_file(&zip);
    anyhow::ensure!(status.success(), "could not unpack the Streamline SDK");
    let bin = find_bin(&staging).context("SDK has no bin/x64/sl.interposer.dll")?;
    std::fs::create_dir_all(&dest)?;
    for e in std::fs::read_dir(&bin)?.flatten() {
        if e.path().is_file() {
            std::fs::rename(e.path(), dest.join(e.file_name()))?;
        }
    }
    let _ = std::fs::remove_dir_all(&staging);
    Ok(dest)
}

fn find_bin(dir: &Path) -> Option<PathBuf> {
    // Production DLLs, not the `development` subfolder.
    if dir.join("sl.interposer.dll").is_file() && !dir.ends_with("development") {
        return Some(dir.to_path_buf());
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .find_map(|e| find_bin(&e.path()))
}

/// Streamline / DLSS DLLs inside `game_dir` (relative paths) that the SDK in `sdk` also has.
pub fn replaceable(game_dir: &Path, sdk: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(game_dir, game_dir, sdk, 0, &mut out);
    out.sort();
    out
}

fn walk(base: &Path, dir: &Path, sdk: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 6 {
                walk(base, &p, sdk, depth + 1, out);
            }
            continue;
        }
        let name = e.file_name().to_string_lossy().to_lowercase();
        if (name.starts_with("sl.") || name.starts_with("nvngx_"))
            && name.ends_with(".dll")
            && sdk.join(&name).is_file()
        {
            out.push(p.strip_prefix(base).unwrap_or(&p).to_path_buf());
        }
    }
}

/// Replaces the game's Streamline / DLSS DLLs under `game_dir` with those from `sdk`.
pub fn install(
    id: &GameId,
    version: &str,
    sdk: &Path,
    game_dir: &Path,
) -> anyhow::Result<Manifest> {
    let files: Vec<(PathBuf, PathBuf)> = replaceable(game_dir, sdk)
        .into_iter()
        .map(|rel| {
            let name = rel
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            (sdk.join(name), rel)
        })
        .collect();
    anyhow::ensure!(
        !files.is_empty(),
        "this game ships no Streamline or DLSS DLLs to update"
    );
    crate::optiscaler::install_files(Mod::Streamline, id, version, files, game_dir, "")
}

pub fn manifest(id: &GameId) -> Option<Manifest> {
    Manifest::load_mod(Mod::Streamline, id).ok().flatten()
}

pub fn uninstall(id: &GameId) -> anyhow::Result<()> {
    crate::optiscaler::uninstall_mod(Mod::Streamline, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_dlls_the_game_ships() {
        let _lock = crate::CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests that change XDG dirs hold CONFIG_ENV_LOCK.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("cfg")) };
        let sdk = tmp.path().join("sdk");
        let game = tmp.path().join("game");
        std::fs::create_dir_all(&sdk).unwrap();
        std::fs::create_dir_all(game.join("Bin64")).unwrap();
        for f in [
            "sl.interposer.dll",
            "sl.dlss_g.dll",
            "nvngx_dlss.dll",
            "sl.reflex.dll",
        ] {
            std::fs::write(sdk.join(f), "new").unwrap();
        }
        std::fs::write(game.join("Bin64/sl.interposer.dll"), "old").unwrap();
        std::fs::write(game.join("Bin64/nvngx_dlss.dll"), "old").unwrap();
        std::fs::write(game.join("Bin64/game.exe"), "").unwrap();
        let id: GameId = "local:sl".parse().unwrap();
        let m = install(&id, "v1", &sdk, &game).unwrap();
        assert_eq!(m.backed_up.len(), 2);
        assert_eq!(
            std::fs::read_to_string(game.join("Bin64/nvngx_dlss.dll")).unwrap(),
            "new"
        );
        assert!(
            !game.join("Bin64/sl.reflex.dll").exists(),
            "only shipped files are replaced"
        );
        assert!(
            crate::optiscaler::installed_proxies(&id).is_empty(),
            "no proxy override"
        );
        uninstall(&id).unwrap();
        assert_eq!(
            std::fs::read_to_string(game.join("Bin64/sl.interposer.dll")).unwrap(),
            "old"
        );
        assert!(!game.join("Bin64/sl.interposer.dll.arkor-backup").exists());
    }
}
