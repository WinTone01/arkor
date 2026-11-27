//! What console mode's quick menu changes while a game runs: its MangoHud (shown or not,
//! frame limit) through the config file MangoHud watches, and screenshots.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{GameId, paths};

/// The MangoHud config a running game reads, when Arkor gave it one.
pub fn hud_file(id: &GameId) -> Option<PathBuf> {
    let path = paths::game_config_dir(id).ok()?.join("MangoHud.conf");
    path.is_file().then_some(path)
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn key(line: &str) -> &str {
    line.split('=').next().unwrap_or(line).trim()
}

/// Written in place: MangoHud reads the file again when it changes. This game's session
/// only; the next start writes it anew from the settings.
fn write(path: &Path, lines: &[String]) -> anyhow::Result<()> {
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

pub fn hud_shown(path: &Path) -> bool {
    !lines(path).iter().any(|l| key(l) == "no_display")
}

pub fn set_hud_shown(path: &Path, shown: bool) -> anyhow::Result<()> {
    let mut l = lines(path);
    l.retain(|l| key(l) != "no_display");
    if !shown {
        l.push("no_display".into());
    }
    write(path, &l)
}

/// The frame limit in use (0: none).
pub fn fps_limit(path: &Path) -> u32 {
    lines(path)
        .iter()
        .find(|l| key(l) == "fps_limit")
        .and_then(|l| l.split_once('=')?.1.split(',').next()?.trim().parse().ok())
        .unwrap_or(0)
}

pub fn set_fps_limit(path: &Path, fps: u32) -> anyhow::Result<()> {
    let mut l = lines(path);
    l.retain(|l| key(l) != "fps_limit");
    l.push(format!("fps_limit={fps}"));
    write(path, &l)
}

/// The processes of a game started as `root` (its own and those below it).
pub fn processes(root: u32) -> Vec<u32> {
    crate::steam_link::with_descendants(&[root])
}

/// Whether one of them is gamescope (it limits frames itself).
pub fn in_gamescope(pids: &[u32]) -> bool {
    pids.iter().any(|p| {
        std::fs::read_to_string(format!("/proc/{p}/comm"))
            .is_ok_and(|c| c.trim().starts_with("gamescope"))
    })
}

/// The program that saves the screen here, and how it is told where (the file comes last).
fn shot_tool() -> Option<(&'static str, &'static [&'static str])> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_uppercase();
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    // The desktop's own one first; then what works on the session's kind of screen.
    let mut tools: Vec<(&'static str, &'static [&'static str])> = Vec::new();
    if desktop.contains("KDE") {
        tools.push(("spectacle", &["-b", "-n", "-f", "-o"]));
    }
    if desktop.contains("GNOME") {
        tools.push(("gnome-screenshot", &["-f"]));
    }
    if wayland {
        // wlroots desktops (Sway, Hyprland, niri, labwc…) and others with screencopy.
        tools.push(("grim", &[]));
    } else {
        tools.extend([
            ("maim", &[][..]),
            ("scrot", &["-o"][..]),
            ("import", &["-window", "root"][..]),
        ]);
    }
    tools.extend([
        ("spectacle", &["-b", "-n", "-f", "-o"][..]),
        ("gnome-screenshot", &["-f"][..]),
    ]);
    tools.into_iter().find(|(bin, _)| crate::system::has(bin))
}

/// Whether the screen can be saved here.
pub fn can_screenshot() -> bool {
    shot_tool().is_some()
}

/// Saves what is on the screen into `~/Pictures/Arkor` (with the desktop's tool). Blocking.
pub fn screenshot(name: &str) -> anyhow::Result<PathBuf> {
    let (tool, args) = shot_tool().context("no screenshot program is installed")?;
    let dir = dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .context("no pictures folder")?
        .join("Arkor");
    std::fs::create_dir_all(&dir)?;
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let file = dir.join(format!("{} {}.png", safe.trim(), arkor_core::stats::now()));
    let status = std::process::Command::new(tool)
        .args(args)
        .arg(&file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("{tool} could not be started"))?;
    anyhow::ensure!(
        status.success() && file.is_file(),
        "the screen could not be saved"
    );
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_what_mangohud_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("MangoHud.conf");
        std::fs::write(
            &f,
            "# Generated by Arkor\nfps\nfps_limit=144,60,0\ntoggle_hud=Shift_R+F12\n",
        )
        .unwrap();
        assert!(hud_shown(&f));
        assert_eq!(fps_limit(&f), 144);
        set_hud_shown(&f, false).unwrap();
        assert!(!hud_shown(&f));
        set_fps_limit(&f, 60).unwrap();
        assert_eq!(fps_limit(&f), 60);
        set_hud_shown(&f, true).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert_eq!(
            text,
            "# Generated by Arkor\nfps\ntoggle_hud=Shift_R+F12\nfps_limit=60\n"
        );
        assert!(processes(std::process::id()).contains(&std::process::id()));
    }
}
