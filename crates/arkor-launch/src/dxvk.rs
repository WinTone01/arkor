//! A DXVK or VKD3D-Proton version of the user's choice for one Windows game, instead of the
//! one its Proton brings.
//!
//! Proton keeps its own copies in the prefix; Wine loads a DLL from the game's own folder
//! first, so the chosen version's DLLs go there, beside the game's program, just before it
//! starts (only where the game has no file of that name) and are taken out again when it
//! ends. A mark (`dxvk.placed`, in the game's config folder) lists them while they are there;
//! when the computer went off meanwhile, [`leftovers`] takes them out at the next start.
//!
//! Releases come from the projects' GitHub pages (doitsujin/dxvk,
//! HansKristian-Work/vkd3d-proton) into `~/.local/share/arkor/dxvk/<kind>/<version>/`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::GameId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dxvk,
    Vkd3d,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Dxvk, Kind::Vkd3d];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Dxvk => "DXVK",
            Kind::Vkd3d => "VKD3D-Proton",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::Dxvk => "dxvk",
            Kind::Vkd3d => "vkd3d",
        }
    }

    fn repo(self) -> &'static str {
        match self {
            Kind::Dxvk => "doitsujin/dxvk",
            Kind::Vkd3d => "HansKristian-Work/vkd3d-proton",
        }
    }

    /// The DLLs it replaces.
    pub fn dlls(self) -> &'static [&'static str] {
        match self {
            Kind::Dxvk => &["d3d8", "d3d9", "d3d10core", "d3d11", "dxgi"],
            Kind::Vkd3d => &["d3d12", "d3d12core"],
        }
    }

    /// The folder of 64-bit and of 32-bit DLLs in its archive.
    fn arch_dirs(self) -> (&'static str, &'static str) {
        match self {
            Kind::Dxvk => ("x64", "x32"),
            Kind::Vkd3d => ("x64", "x86"),
        }
    }
}

fn root(kind: Kind) -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/dxvk")
        .join(kind.key()))
}

/// Versions downloaded, newest first.
pub fn installed(kind: Kind) -> Vec<String> {
    let Ok(dir) = root(kind) else {
        return Vec::new();
    };
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("x64").is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort_by(|a, b| crate::emu::discovery::compare_versions(b, a));
    v
}

/// The versions offered (newest first): (version, archive address).
pub fn releases(kind: Kind) -> anyhow::Result<Vec<(String, String)>> {
    let body = crate::tools::http_get(&format!(
        "https://api.github.com/repos/{}/releases?per_page=10",
        kind.repo()
    ))?;
    let list: serde_json::Value = serde_json::from_str(&body)?;
    let prefix = match kind {
        Kind::Dxvk => "dxvk-",
        Kind::Vkd3d => "vkd3d-proton-",
    };
    Ok(list
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|r| r["prerelease"] != true)
                .filter_map(|r| {
                    let asset = r["assets"].as_array()?.iter().find(|x| {
                        x["name"].as_str().is_some_and(|n| {
                            n.starts_with(prefix) && !n.contains("native") && n.contains(".tar")
                        })
                    })?;
                    Some((
                        r["tag_name"].as_str()?.trim_start_matches('v').to_string(),
                        asset["browser_download_url"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Downloads `version` (the newest when `None`); the version. Blocking.
pub fn install(kind: Kind, version: Option<&str>) -> anyhow::Result<String> {
    let list = releases(kind)?;
    let (v, url) = match version {
        Some(v) => list
            .into_iter()
            .find(|(r, _)| r == v)
            .with_context(|| format!("{} {v} is not offered", kind.name()))?,
        None => list
            .into_iter()
            .next()
            .with_context(|| format!("no {} release found", kind.name()))?,
    };
    let dest = root(kind)?.join(&v);
    if dest.join("x64").is_dir() {
        return Ok(v);
    }
    let name = url.rsplit('/').next().unwrap_or("archive.tar");
    let archive = root(kind)?.join(format!(".{name}"));
    crate::tools::download(&url, &archive)?;
    let staging = root(kind)?.join(format!(".{v}.partial"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let out = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .output()
        .context("running tar")?;
    let _ = std::fs::remove_file(&archive);
    anyhow::ensure!(
        out.status.success(),
        "unpacking {name}: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    // The archive holds one folder (`dxvk-3.2/x64/…`).
    let inner = std::fs::read_dir(&staging)?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("x64").is_dir())
        .context("the archive holds no x64 folder")?;
    std::fs::rename(&inner, &dest)?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(v)
}

/// Removes a downloaded version.
pub fn remove(kind: Kind, version: &str) -> anyhow::Result<()> {
    let root = root(kind)?;
    let dir = root.join(version);
    if dir.exists() {
        arkor_core::fs_guard::remove_dir_in(&dir, &root)?;
    }
    Ok(())
}

/// Whether the Windows program at `exe` is 64-bit (its PE header says x86-64).
pub fn is_64bit(exe: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let read = || -> std::io::Result<bool> {
        let mut f = std::fs::File::open(exe)?;
        let mut mz = [0u8; 64];
        f.read_exact(&mut mz)?;
        let pe = u32::from_le_bytes([mz[60], mz[61], mz[62], mz[63]]) as u64;
        f.seek(SeekFrom::Start(pe))?;
        let mut head = [0u8; 6];
        f.read_exact(&mut head)?;
        Ok(&head[..4] == b"PE\0\0" && u16::from_le_bytes([head[4], head[5]]) == 0x8664)
    };
    read().unwrap_or(true)
}

const MARK: &str = "dxvk.placed";

/// DLLs placed for one launch; dropping it takes them out.
#[derive(Debug)]
pub struct Placed {
    files: Vec<PathBuf>,
    mark: Option<PathBuf>,
}

impl Drop for Placed {
    fn drop(&mut self) {
        for f in &self.files {
            let _ = std::fs::remove_file(f);
        }
        if let Some(m) = &self.mark {
            let _ = std::fs::remove_file(m);
        }
    }
}

/// Before the game starts: the chosen versions' DLLs beside the game's program `exe`, and
/// the Wine overrides that make them load. `None` when nothing is chosen. Blocking.
pub fn before(
    id: &GameId,
    exe: &Path,
    choices: &[(Kind, String)],
) -> Option<(Placed, Vec<String>)> {
    leftovers();
    if choices.is_empty() {
        return None;
    }
    let dir = exe.parent()?;
    let bits64 = is_64bit(exe);
    let mut files = Vec::new();
    let mut overrides = Vec::new();
    for (kind, version) in choices {
        let Ok(src_root) = root(*kind).map(|r| r.join(version)) else {
            continue;
        };
        let (x64, x32) = kind.arch_dirs();
        let src = src_root.join(if bits64 { x64 } else { x32 });
        if !src.is_dir() {
            eprintln!("arkor: {} {version} is not downloaded", kind.name());
            continue;
        }
        for dll in kind.dlls() {
            let from = src.join(format!("{dll}.dll"));
            let to = dir.join(format!("{dll}.dll"));
            // The game's own file (a mod's proxy, its own DXVK) is left alone.
            if !from.is_file() || to.exists() {
                continue;
            }
            match std::fs::copy(&from, &to) {
                Ok(_) => {
                    files.push(to);
                    overrides.push(format!("{dll}=n,b"));
                }
                Err(e) => eprintln!("arkor: {}: {e}", to.display()),
            }
        }
    }
    if files.is_empty() {
        return None;
    }
    let mark = arkor_core::paths::game_config_dir(id)
        .ok()
        .map(|d| d.join(MARK));
    if let Some(m) = &mark {
        if let Some(d) = m.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        // This boot and `arkor run`'s process first (see `mods::play`), then the files.
        let me = std::process::id();
        let mut text = format!(
            "{} {me} {}\n",
            crate::mods::play::boot_id(),
            crate::mods::play::started(me).unwrap_or_default()
        );
        for f in &files {
            text.push_str(&format!("{}\n", f.display()));
        }
        let _ = std::fs::write(m, text);
    }
    Some((Placed { files, mark }, overrides))
}

/// Takes out DLLs a launch left beside a game (the computer went off, or `arkor run` was
/// killed). Blocking.
pub fn leftovers() {
    let Ok(games) = arkor_core::paths::config_dir().map(|d| d.join("games")) else {
        return;
    };
    let boot = crate::mods::play::boot_id();
    for dir in std::fs::read_dir(games).into_iter().flatten().flatten() {
        let mark = dir.path().join(MARK);
        let Ok(text) = std::fs::read_to_string(&mark) else {
            continue;
        };
        let mut lines = text.lines();
        let head = lines.next().unwrap_or_default();
        if crate::mods::play::still_placed(head, &boot) {
            continue;
        }
        for line in lines {
            let _ = std::fs::remove_file(line);
        }
        let _ = std::fs::remove_file(&mark);
    }
}

/// The Windows program a launch command starts (`umu-run … game.exe`), if any.
pub fn exe_of(command: &[String]) -> Option<PathBuf> {
    command
        .iter()
        .map(PathBuf::from)
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) && p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_pe_machine() {
        let dir = tempfile::tempdir().unwrap();
        let mut pe = vec![0u8; 0x100];
        pe[0..2].copy_from_slice(b"MZ");
        pe[60..64].copy_from_slice(&0x80u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        let exe = dir.path().join("game.exe");
        std::fs::write(&exe, &pe).unwrap();
        assert!(is_64bit(&exe));
        pe[0x84..0x86].copy_from_slice(&0x014cu16.to_le_bytes());
        std::fs::write(&exe, &pe).unwrap();
        assert!(!is_64bit(&exe));
        assert_eq!(
            exe_of(&["umu-run".into(), exe.display().to_string(), "-dx11".into()]),
            Some(exe.clone())
        );
    }
}
