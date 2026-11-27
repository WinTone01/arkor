//! ReShade (reshade.me) for Windows games: the official DLL, pulled out of the setup .exe,
//! installed into the game folder under a proxy name, with crosire's shader pack and a
//! ReShade.ini pointing at it. Installed/removed like OptiScaler (manifest + backups).

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::GameId;

use crate::optiscaler::{Manifest, Mod};

const HOME: &str = "https://reshade.me";
const SHADERS: &str = "https://github.com/crosire/reshade-shaders/archive/refs/heads/slim.tar.gz";

/// DLL names ReShade can be loaded as, by API.
pub const PROXIES: [(&str, &str); 5] = [
    ("dxgi", "DirectX 10/11/12"),
    ("d3d9", "DirectX 9"),
    ("d3d11", "DirectX 11"),
    ("d3d12", "DirectX 12"),
    ("opengl32", "OpenGL"),
];

pub fn tools_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/tools/reshade"))
}

/// Latest version number on reshade.me (the non-add-on build, allowed in more online games).
pub fn latest_version() -> anyhow::Result<String> {
    let page = crate::tools::http_get(HOME)?;
    page.split("downloads/ReShade_Setup_")
        .skip(1)
        .filter_map(|rest| rest.split(".exe").next())
        .find(|v| !v.ends_with("_Addon") && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .map(str::to_string)
        .context("no ReShade download link on reshade.me")
}

/// Shader pack folder (with Shaders/ and Textures/), if downloaded.
pub fn shaders_dir() -> Option<PathBuf> {
    let dir = tools_dir().ok()?.join("shaders");
    dir.join("Shaders").is_dir().then_some(dir)
}

/// Downloaded ReShade versions, newest first.
pub fn downloaded() -> Vec<(String, PathBuf)> {
    let Ok(root) = tools_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().join("ReShade64.dll").is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    found.sort_by_key(|(v, _)| std::cmp::Reverse(crate::start::natural_key(v)));
    found
}

/// Downloads ReShade `version` and the shader pack. Blocking.
pub fn download(version: &str) -> anyhow::Result<PathBuf> {
    let root = tools_dir()?;
    let dest = root.join(version);
    if !dest.join("ReShade64.dll").is_file() {
        std::fs::create_dir_all(&dest)?;
        let setup = dest.join("setup.exe");
        crate::tools::download(
            &format!("{HOME}/downloads/ReShade_Setup_{version}.exe"),
            &setup,
        )?;
        // The setup is a self-extracting archive; 7z reads the DLLs straight out of it.
        let status = std::process::Command::new("7z")
            .args(["e", "-y"])
            .arg(format!("-o{}", dest.display()))
            .arg(&setup)
            .args(["ReShade64.dll", "ReShade32.dll"])
            .output()
            .context("running 7z (install 7zip)")?;
        let _ = std::fs::remove_file(&setup);
        anyhow::ensure!(
            status.status.success() && dest.join("ReShade64.dll").is_file(),
            "could not extract ReShade from its setup"
        );
    }
    if shaders_dir().is_none() {
        let archive = root.join("shaders.tar.gz");
        crate::tools::download(SHADERS, &archive)?;
        let unpack = root.join("shaders-unpack");
        let _ = std::fs::remove_dir_all(&unpack);
        std::fs::create_dir_all(&unpack)?;
        let status = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(&unpack)
            .status()?;
        let _ = std::fs::remove_file(&archive);
        anyhow::ensure!(status.success(), "could not unpack the shader pack");
        let inner = std::fs::read_dir(&unpack)?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.join("Shaders").is_dir())
            .context("shader pack has no Shaders folder")?;
        let target = root.join("shaders");
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(inner, &target)?;
        let _ = std::fs::remove_dir_all(&unpack);
    }
    Ok(dest)
}

/// ReShade.ini pointing at the shader pack copied next to it.
fn ini() -> String {
    "[GENERAL]\nEffectSearchPaths=.\\reshade-shaders\\Shaders\\**\nTextureSearchPaths=.\\reshade-shaders\\Textures\\**\nPresetPath=.\\ReShadePreset.ini\n\n[INPUT]\nKeyOverlay=36,0,0,0\n".into()
}

/// Installs ReShade (64- or 32-bit) into `target_dir` as `<proxy>.dll` with the shaders.
pub fn install(
    id: &GameId,
    version: &str,
    source: &Path,
    target_dir: &Path,
    proxy: &str,
    bits32: bool,
) -> anyhow::Result<Manifest> {
    anyhow::ensure!(
        PROXIES.iter().any(|(p, _)| *p == proxy),
        "unsupported proxy `{proxy}`"
    );
    let shaders = shaders_dir().context("ReShade's shader pack is not downloaded")?;
    let ini_file = tools_dir()?.join("ReShade.ini");
    std::fs::write(&ini_file, ini())?;
    let dll = format!("{proxy}.dll");
    let files = vec![
        (
            source.join(if bits32 {
                "ReShade32.dll"
            } else {
                "ReShade64.dll"
            }),
            PathBuf::from(&dll),
        ),
        (ini_file, PathBuf::from("ReShade.ini")),
        (shaders, PathBuf::from("reshade-shaders")),
    ];
    crate::optiscaler::install_files(Mod::ReShade, id, version, files, target_dir, &dll)
}

/// Keys ReShade's menu can open with: (Windows key code, name).
pub const MENU_KEYS: [(u32, &str); 8] = [
    (36, "Home"),
    (35, "End"),
    (45, "Insert"),
    (46, "Delete"),
    (33, "Page Up"),
    (34, "Page Down"),
    (113, "F2"),
    (120, "F9"),
];

/// `ini` with its menu key set to `code` (the line is added when missing).
fn with_menu_key(ini: &str, code: u32) -> String {
    let line = format!("KeyOverlay={code},0,0,0");
    let mut out = Vec::new();
    let (mut in_input, mut done, mut has_input) = (false, false, false);
    for l in ini.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            if in_input && !done {
                out.push(line.clone());
                done = true;
            }
            in_input = t.eq_ignore_ascii_case("[INPUT]");
            has_input |= in_input;
        }
        if in_input && t.to_ascii_lowercase().starts_with("keyoverlay=") {
            out.push(line.clone());
            done = true;
            continue;
        }
        out.push(l.to_string());
    }
    if !done {
        if !has_input {
            out.push(String::new());
            out.push("[INPUT]".into());
        }
        out.push(line);
    }
    out.join("\n") + "\n"
}

/// The key code ReShade's menu opens with in a game it is installed in.
pub fn menu_key(m: &Manifest) -> Option<u32> {
    let ini = std::fs::read_to_string(m.target_dir.join("ReShade.ini")).ok()?;
    ini.lines()
        .find_map(|l| l.trim().strip_prefix("KeyOverlay="))
        .and_then(|v| v.split(',').next()?.trim().parse().ok())
}

/// Sets the key ReShade's menu opens with in a game it is installed in.
pub fn set_menu_key(m: &Manifest, code: u32) -> anyhow::Result<()> {
    let path = m.target_dir.join("ReShade.ini");
    let ini = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(&path, with_menu_key(&ini, code))
        .with_context(|| format!("writing {}", path.display()))
}

pub fn uninstall(id: &GameId) -> anyhow::Result<()> {
    crate::optiscaler::uninstall_mod(Mod::ReShade, id)
}

pub fn manifest(id: &GameId) -> Option<Manifest> {
    Manifest::load_mod(Mod::ReShade, id).ok().flatten()
}

/// `.fx` effects in the shader pack (for vkBasalt's ReShade support too).
pub fn effects() -> Vec<PathBuf> {
    let Some(dir) = shaders_dir() else {
        return Vec::new();
    };
    let mut fx: Vec<PathBuf> = std::fs::read_dir(dir.join("Shaders"))
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "fx"))
                .collect()
        })
        .unwrap_or_default();
    fx.sort();
    fx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_the_menu_key() {
        let changed = with_menu_key(&ini(), 45);
        assert!(changed.contains("[INPUT]\nKeyOverlay=45,0,0,0\n"));
        assert!(!changed.contains("KeyOverlay=36"));
        let added = with_menu_key("[GENERAL]\nA=1\n", 35);
        assert!(added.ends_with("[INPUT]\nKeyOverlay=35,0,0,0\n"));
        let between = with_menu_key("[INPUT]\nKeyScreenshot=44\n[OTHER]\nB=2\n", 33);
        assert!(between.contains("KeyScreenshot=44\nKeyOverlay=33,0,0,0\n[OTHER]"));
    }
}
