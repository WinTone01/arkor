//! RetroArch specifics: libretro cores from the libretro buildbot (the files RetroArch's own
//! online updater fetches), and the settings file Arkor hands RetroArch so BIOS files,
//! saves and cores are found without the user opening RetroArch's menus.
//!
//! Cores live in Arkor's folder, whichever RetroArch runs them, so a Flatpak or a
//! distribution's RetroArch plays with the same cores as a downloaded one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::Value;

use super::discovery::{self, CoreInstall};

const BUILDBOT: &str = "https://buildbot.libretro.com/nightly";

/// The buildbot folder of this computer's cores.
fn platform_dir() -> Option<&'static str> {
    if cfg!(target_arch = "aarch64") {
        Some("linux/aarch64/latest")
    } else if cfg!(target_arch = "x86_64") {
        Some("linux/x86_64/latest")
    } else {
        None
    }
}

/// Where cores are kept.
pub fn cores_dir() -> PathBuf {
    super::dirs::engine("retroarch").join("cores")
}

/// Where Arkor keeps RetroArch's settings, saves and states.
pub fn home() -> PathBuf {
    super::dirs::home("retroarch")
}

/// The file name of `core` (`mgba_libretro.so`).
pub fn core_file(core: &str) -> String {
    format!("{core}_libretro.so")
}

/// The installed library of `core`, if any.
pub fn installed_core(core: &str) -> Option<PathBuf> {
    discovery::state()
        .cores
        .get(core)
        .map(|c| c.path.clone())
        .filter(|p| p.exists())
        .or_else(|| {
            let p = cores_dir().join(core_file(core));
            p.exists().then_some(p)
        })
}

/// One line of the buildbot's `.index-extended`: date, the CRC-32 of the library inside the
/// zip, and the zip's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub date: String,
    pub crc32: String,
    pub file: String,
}

pub fn parse_index(text: &str) -> Vec<IndexEntry> {
    text.lines()
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            Some(IndexEntry {
                date: p.next()?.to_string(),
                crc32: p.next()?.to_ascii_lowercase(),
                file: p.next()?.to_string(),
            })
        })
        .collect()
}

/// Downloads `core` (checked against the buildbot's CRC-32) into [`cores_dir`]; its build
/// date. Blocking.
pub fn install_core(core: &str) -> anyhow::Result<String> {
    let dir = platform_dir().context("libretro has no cores for this computer")?;
    let base = format!("{BUILDBOT}/{dir}");
    let zip_name = format!("{}.zip", core_file(core));
    let index = parse_index(&crate::tools::http_get(&format!("{base}/.index-extended"))?);
    let entry = index
        .into_iter()
        .find(|e| e.file == zip_name)
        .with_context(|| format!("the libretro buildbot has no {core} core for this computer"))?;
    let zip = super::dirs::cache()
        .join("downloads/cores")
        .join(&entry.file);
    crate::tools::download(&format!("{base}/{zip_name}"), &zip)?;
    let staging = cores_dir().join(format!(".{core}.partial"));
    let _ = std::fs::remove_dir_all(&staging);
    super::zip::unpack(&zip, &staging)?;
    let unpacked = discovery::locate_program(&staging, &[core_file(core)])
        .with_context(|| format!("the {core} download holds no {}", core_file(core)))?;
    let actual = crc32fast::hash(&std::fs::read(&unpacked)?);
    if format!("{actual:08x}") != entry.crc32 {
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::remove_file(&zip);
        bail!("the {core} core is damaged (its checksum does not match); try again");
    }
    let path = cores_dir().join(core_file(core));
    std::fs::rename(&unpacked, &path).with_context(|| format!("saving {}", path.display()))?;
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_file(&zip);
    let installed = CoreInstall {
        size: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        path,
        date: entry.date.clone(),
    };
    let core = core.to_string();
    discovery::update_state(|s| s.cores.insert(core, installed))?;
    Ok(entry.date)
}

/// Removes a core Arkor downloaded.
pub fn remove_core(core: &str) -> anyhow::Result<()> {
    if let Some(path) = installed_core(core)
        && path.starts_with(cores_dir())
    {
        std::fs::remove_file(&path).with_context(|| format!("deleting {}", path.display()))?;
    }
    let core = core.to_string();
    discovery::update_state(|s| s.cores.remove(&core))?;
    Ok(())
}

/// Installed cores whose buildbot build is at least two weeks newer (cores are rebuilt
/// nightly; anything closer would be an update every day). Blocking (one request).
pub fn outdated_cores() -> Vec<String> {
    let installed = discovery::state().cores;
    if installed.is_empty() {
        return Vec::new();
    }
    let Some(dir) = platform_dir() else {
        return Vec::new();
    };
    let Ok(text) = crate::tools::http_get(&format!("{BUILDBOT}/{dir}/.index-extended")) else {
        return Vec::new();
    };
    let index = parse_index(&text);
    let day = |d: &str| -> Option<i64> {
        let mut p = d.split('-').map(|x| x.parse::<i64>().ok());
        Some(p.next()?? * 365 + p.next()?? * 30 + p.next()??)
    };
    installed
        .iter()
        .filter(|(core, have)| {
            index
                .iter()
                .find(|e| e.file == format!("{}.zip", core_file(core)))
                .is_some_and(|e| match (day(&have.date), day(&e.date)) {
                    (Some(h), Some(o)) => o - h >= 14,
                    (None, Some(_)) => true,
                    _ => false,
                })
        })
        .map(|(c, _)| c.clone())
        .collect()
}

/// The settings Arkor manages in RetroArch's file, from the emulator's option `values`.
pub fn managed_settings(values: &BTreeMap<String, Value>) -> Vec<(&'static str, String)> {
    let flag = |id: &str, default: bool| {
        values
            .get(id)
            .and_then(Value::as_bool)
            .unwrap_or(default)
            .to_string()
    };
    let home = home();
    let mut out = vec![
        (
            "system_directory",
            super::dirs::system().display().to_string(),
        ),
        ("libretro_directory", cores_dir().display().to_string()),
        (
            "savefile_directory",
            home.join("saves").display().to_string(),
        ),
        (
            "savestate_directory",
            home.join("states").display().to_string(),
        ),
        (
            "screenshot_directory",
            home.join("screenshots").display().to_string(),
        ),
        ("video_fullscreen", flag("fullscreen", true)),
        ("video_vsync", flag("vsync", true)),
        ("fps_show", flag("show_fps", false)),
        ("pause_nonactive", flag("pause_in_background", true)),
        // Start + Select opens the menu on any controller; Escape twice quits; the
        // controller's guide button belongs to Arkor's quick menu.
        ("input_menu_toggle_gamepad_combo", "4".to_string()),
        ("quit_press_twice", "true".to_string()),
        ("config_save_on_exit", "true".to_string()),
    ];
    if let Some(driver) = values.get("video_driver").and_then(Value::as_str)
        && !driver.is_empty()
    {
        out.push(("video_driver", driver.to_string()));
    }
    out
}

/// Writes `settings` into the RetroArch settings file at `path`, keeping its other lines.
pub fn write_config(path: &Path, settings: &[(&str, String)]) -> anyhow::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    for (key, value) in settings {
        let line = format!("{key} = \"{}\"", value.replace('"', ""));
        let found = lines
            .iter_mut()
            .find(|l| l.split_once('=').is_some_and(|(k, _)| k.trim() == *key));
        match found {
            Some(l) => *l = line,
            None => lines.push(line),
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_buildbot_index() {
        let index = parse_index(
            "2026-10-30 AAAA mgba_libretro.so.zip\n2026-10-31 bbbb snes9x_libretro.so.zip\nbad\n",
        );
        assert_eq!(index.len(), 2);
        assert_eq!(index[0].crc32, "aaaa");
        assert_eq!(index[1].file, "snes9x_libretro.so.zip");
    }

    #[test]
    fn config_keeps_other_lines_and_replaces_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("retroarch.cfg");
        std::fs::write(
            &cfg,
            "video_fullscreen = \"false\"\nmenu_driver = \"ozone\"\n",
        )
        .unwrap();
        let mut values = BTreeMap::new();
        values.insert("fullscreen".to_string(), Value::Bool(true));
        write_config(&cfg, &managed_settings(&values)).unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("video_fullscreen = \"true\""));
        assert!(text.contains("menu_driver = \"ozone\""));
        assert!(text.contains("libretro_directory = "));
        assert_eq!(text.matches("video_fullscreen").count(), 1);
    }
}
