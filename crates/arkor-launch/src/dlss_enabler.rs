//! DLSS Enabler: a `version.dll` proxy that brings DLSS (including frame generation through
//! FSR 3) to GPUs and games that lack it. Builds come from the OptiScaler-builds mirror that
//! Goverlay also uses. Installed and removed like OptiScaler (see `optiscaler::install_files`).

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::GameId;
use serde::Deserialize;

use crate::optiscaler::{Manifest, Mod};

const LISTING: &str =
    "https://api.github.com/repos/benjamimgois/OptiScaler-builds/contents/de?ref=nightly-action";
pub const DLL: &str = "version.dll";

#[derive(Debug, Clone, PartialEq)]
pub struct Build {
    /// e.g. `4.9.0 STABLE`
    pub version: String,
    pub stable: bool,
    pub url: String,
    pub size: u64,
}

pub fn tools_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/tools/dlss-enabler"))
}

pub fn builds() -> anyhow::Result<Vec<Build>> {
    parse_listing(&crate::tools::http_get(LISTING)?)
}

fn parse_listing(json: &str) -> anyhow::Result<Vec<Build>> {
    #[derive(Deserialize)]
    struct Item {
        name: String,
        size: u64,
        download_url: Option<String>,
    }
    let items: Vec<Item> = serde_json::from_str(json).context("unexpected listing")?;
    let mut builds: Vec<Build> = items
        .into_iter()
        .filter(|i| i.name.starts_with("DLSS Enabler") && i.name.ends_with(".zip"))
        .filter_map(|i| {
            // "DLSS Enabler 4.9.0 STABLE 757 …zip" -> "4.9.0 STABLE"
            let mut words = i
                .name
                .trim_start_matches("DLSS Enabler")
                .trim_end_matches(".zip")
                .split_whitespace();
            let number = words.next()?.to_string();
            let channel = words.next().unwrap_or("").to_string();
            Some(Build {
                stable: channel.eq_ignore_ascii_case("STABLE"),
                version: format!("{number} {channel}").trim().to_string(),
                url: i.download_url?,
                size: i.size,
            })
        })
        .collect();
    // Stable first, then by version number, newest first.
    builds.sort_by_key(|b| {
        (
            !b.stable,
            std::cmp::Reverse(crate::start::natural_key(&b.version)),
        )
    });
    Ok(builds)
}

pub(crate) fn folder_name(version: &str) -> String {
    version.replace(' ', "-")
}

/// Downloaded builds as (version, folder with version.dll), stable/newest first.
pub fn downloaded() -> Vec<(String, PathBuf)> {
    let Ok(root) = tools_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().join(DLL).is_file())
        .map(|e| (e.file_name().to_string_lossy().replace('-', " "), e.path()))
        .collect();
    found.sort_by_key(|(v, _)| {
        (
            !v.contains("STABLE"),
            std::cmp::Reverse(crate::start::natural_key(v)),
        )
    });
    found
}

/// Downloads and unpacks a build. Blocking.
pub fn download(build: &Build) -> anyhow::Result<PathBuf> {
    let dest = tools_dir()?.join(folder_name(&build.version));
    if dest.join(DLL).is_file() {
        return Ok(dest);
    }
    std::fs::create_dir_all(&dest)?;
    let zip = dest.join("build.zip");
    crate::tools::download(&build.url, &zip)?;
    let status = std::process::Command::new("bsdtar")
        .arg("-xf")
        .arg(&zip)
        .arg("-C")
        .arg(&dest)
        .status()
        .context("running bsdtar")?;
    let _ = std::fs::remove_file(&zip);
    anyhow::ensure!(
        status.success() && dest.join(DLL).is_file(),
        "the archive had no {DLL}"
    );
    Ok(dest)
}

pub fn install(
    id: &GameId,
    version: &str,
    source: &Path,
    target_dir: &Path,
) -> anyhow::Result<Manifest> {
    anyhow::ensure!(
        Manifest::load_mod(Mod::Anr, id)?.is_none(),
        "ANR FG is installed in this game; remove it first"
    );
    let files = vec![(source.join(DLL), PathBuf::from(DLL))];
    crate::optiscaler::install_files(Mod::DlssEnabler, id, version, files, target_dir, DLL)
}

pub fn uninstall(id: &GameId) -> anyhow::Result<()> {
    crate::optiscaler::uninstall_mod(Mod::DlssEnabler, id)
}

pub fn manifest(id: &GameId) -> Option<Manifest> {
    Manifest::load_mod(Mod::DlssEnabler, id).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mirror_listing_stable_first() {
        let builds = parse_listing(
            r#"[{"name":"DLSS Enabler 4.10.0 TRUNK.zip","size":8,"download_url":"u/trunk"},
                {"name":"DLSS Enabler 4.9.0 STABLE 757 4.9.0 2026-08-31.zip","size":8,"download_url":"u/stable"},
                {"name":"README.md","size":1,"download_url":"u/readme"}]"#,
        )
        .unwrap();
        let v: Vec<_> = builds
            .iter()
            .map(|b| (b.version.as_str(), b.stable, b.url.as_str()))
            .collect();
        assert_eq!(
            v,
            [
                ("4.9.0 STABLE", true, "u/stable"),
                ("4.10.0 TRUNK", false, "u/trunk")
            ]
        );
    }

    #[test]
    fn conflicts_with_optiscaler_on_same_dll() {
        let _lock = crate::CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests that read XDG_CONFIG_HOME hold CONFIG_ENV_LOCK.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("config")) };
        let game = tmp.path().join("game");
        std::fs::create_dir_all(&game).unwrap();
        let src = tmp.path().join("de");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join(DLL), "de").unwrap();
        let opti = tmp.path().join("opti");
        std::fs::create_dir_all(&opti).unwrap();
        std::fs::write(opti.join("OptiScaler.dll"), "o").unwrap();

        let id = GameId::new(arkor_core::Store::Local, "de-conflict");
        crate::optiscaler::install(&id, "v1", &opti, &game, "version").unwrap();
        let err = install(&id, "4.9.0", &src, &game).unwrap_err().to_string();
        assert!(err.contains("already uses version.dll"), "{err}");
        crate::optiscaler::uninstall(&id).unwrap();

        install(&id, "4.9.0", &src, &game).unwrap();
        assert_eq!(std::fs::read_to_string(game.join(DLL)).unwrap(), "de");
        assert_eq!(crate::optiscaler::installed_proxies(&id), ["version"]);
        uninstall(&id).unwrap();
        assert!(!game.join(DLL).exists());
    }
}
