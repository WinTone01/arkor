//! OptiScaler (https://github.com/optiscaler/OptiScaler): swaps a game's upscaler (DLSS, FSR,
//! XeSS) for another and can add frame generation.
//!
//! Arkor downloads a release once into `~/.local/share/arkor/tools/optiscaler/<tag>/`, then
//! installs it into a game folder on request. Every file Arkor writes is recorded in a
//! manifest and any game file it would overwrite is kept as `<name>.arkor-backup`, so
//! uninstalling restores the folder exactly. While installed, games launch with
//! `WINEDLLOVERRIDES=<proxy>=n,b` so Wine loads OptiScaler instead of its builtin DLL.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use arkor_core::GameId;
use serde::{Deserialize, Serialize};

const REPO_API: &str = "https://api.github.com/repos/optiscaler/OptiScaler/releases/latest";
const BACKUP_SUFFIX: &str = ".arkor-backup";

/// DLL names Windows games load that OptiScaler can pose as. `dxgi` works for most
/// DirectX 11/12 games; others help when a game or another mod already uses `dxgi`.
pub const PROXY_DLLS: [&str; 6] = ["dxgi", "winmm", "version", "dbghelp", "wininet", "winhttp"];

pub fn tools_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/tools/optiscaler"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub size: u64,
}

/// Asks GitHub for the latest release (uses `curl`, present on every Arch install).
pub fn latest_release() -> anyhow::Result<Release> {
    let out = Command::new("curl")
        .args(["-fsSL", "-A", "arkor", REPO_API])
        .output()
        .context("running curl")?;
    anyhow::ensure!(
        out.status.success(),
        "GitHub request failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    parse_release(&String::from_utf8_lossy(&out.stdout))
}

fn parse_release(json: &str) -> anyhow::Result<Release> {
    #[derive(Deserialize)]
    struct Gh {
        tag_name: String,
        assets: Vec<Asset>,
    }
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        size: u64,
        browser_download_url: String,
    }
    let gh: Gh = serde_json::from_str(json).context("unexpected GitHub response")?;
    let asset = gh
        .assets
        .into_iter()
        .find(|a| a.name.ends_with(".7z") || a.name.ends_with(".zip"))
        .context("release has no .7z/.zip archive")?;
    Ok(Release {
        tag: gh.tag_name,
        url: asset.browser_download_url,
        size: asset.size,
    })
}

/// Downloaded releases as (tag, folder holding OptiScaler.dll), newest first.
pub fn downloaded() -> Vec<(String, PathBuf)> {
    let Ok(root) = tools_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().join("OptiScaler.dll").is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    found.sort_by_key(|(tag, _)| std::cmp::Reverse(crate::start::natural_key(tag)));
    found
}

/// Downloads and unpacks `release` (skipped if already there). Blocking; run off the UI thread.
pub fn download(release: &Release) -> anyhow::Result<PathBuf> {
    let root = tools_dir()?;
    let dest = root.join(&release.tag);
    if dest.join("OptiScaler.dll").is_file() {
        return Ok(dest);
    }
    std::fs::create_dir_all(&root)?;
    let staging = root.join(format!(".{}.partial", release.tag));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let archive = staging.join("release.archive");

    let status = Command::new("curl")
        .args(["-fsSL", "-A", "arkor", "-o"])
        .arg(&archive)
        .arg(&release.url)
        .status()
        .context("running curl")?;
    anyhow::ensure!(status.success(), "download failed: {}", release.url);

    let unpacked = staging.join("files");
    extract(&archive, &unpacked)?;
    // Some releases wrap everything in one folder; use the folder that has OptiScaler.dll.
    let source = find_dll_dir(&unpacked).context("archive does not contain OptiScaler.dll")?;
    std::fs::rename(&source, &dest).with_context(|| format!("moving into {}", dest.display()))?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(dest)
}

fn extract(archive: &Path, into: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(into)?;
    // 7z handles every 7z filter; bsdtar (libarchive, part of Arch's base) is the fallback.
    let seven = Command::new("7z")
        .arg("x")
        .arg("-y")
        .arg(format!("-o{}", into.display()))
        .arg(archive)
        .output();
    if let Ok(out) = seven
        && out.status.success()
    {
        return Ok(());
    }
    let status = Command::new("bsdtar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .status()
        .context("neither 7z nor bsdtar could be run (install p7zip or libarchive)")?;
    anyhow::ensure!(status.success(), "could not unpack {}", archive.display());
    Ok(())
}

fn find_dll_dir(dir: &Path) -> Option<PathBuf> {
    if dir.join("OptiScaler.dll").is_file() {
        return Some(dir.to_path_buf());
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .find_map(|e| find_dll_dir(&e.path()))
}

/// What Arkor installed into a game folder, so it can be removed exactly.
/// Stored as `~/.config/arkor/optiscaler/<store>-<id>.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub target_dir: PathBuf,
    /// Proxy DLL file name OptiScaler.dll was installed as, e.g. `dxgi.dll`.
    pub dll: String,
    /// Files and folders Arkor created (relative to `target_dir`).
    pub created: Vec<PathBuf>,
    /// Game files Arkor moved aside (relative to `target_dir`); `<name>.arkor-backup` exists.
    pub backed_up: Vec<PathBuf>,
}

impl Manifest {
    fn path(id: &GameId) -> anyhow::Result<PathBuf> {
        Ok(arkor_core::paths::config_dir()?
            .join("optiscaler")
            .join(format!("{}.toml", id.file_stem())))
    }

    pub fn load(id: &GameId) -> anyhow::Result<Option<Self>> {
        let path = Self::path(id)?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(
                toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn save(&self, id: &GameId) -> anyhow::Result<()> {
        let path = Self::path(id)?;
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    /// `dxgi` for `dxgi.dll`: the name WINEDLLOVERRIDES wants.
    pub fn dll_stem(&self) -> String {
        self.dll.trim_end_matches(".dll").to_string()
    }
}

/// Release files that go into the game folder: DLLs, ini files and the D3D12 runtime.
/// Licenses, readmes and the setup scripts stay out.
fn release_entries(source: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(source)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let keep = if entry.path().is_dir() {
            name.starts_with("d3d12_")
        } else {
            name.ends_with(".dll") || name.ends_with(".ini")
        };
        if keep {
            entries.push(PathBuf::from(entry.file_name()));
        }
    }
    entries.sort();
    Ok(entries)
}

/// Installs OptiScaler from `source` (a downloaded release) into `target_dir` as `<proxy>.dll`.
pub fn install(
    id: &GameId,
    version: &str,
    source: &Path,
    target_dir: &Path,
    proxy: &str,
) -> anyhow::Result<Manifest> {
    anyhow::ensure!(
        PROXY_DLLS.contains(&proxy),
        "unsupported proxy DLL `{proxy}`"
    );
    anyhow::ensure!(
        Manifest::load(id)?.is_none(),
        "OptiScaler is already installed for this game"
    );
    anyhow::ensure!(
        target_dir.is_dir(),
        "{} is not a folder",
        target_dir.display()
    );

    let dll = format!("{proxy}.dll");
    let mut manifest = Manifest {
        version: version.to_string(),
        target_dir: target_dir.to_path_buf(),
        dll: dll.clone(),
        created: Vec::new(),
        backed_up: Vec::new(),
    };
    let result = (|| -> anyhow::Result<()> {
        for rel in release_entries(source)? {
            let dest_name = if rel.as_os_str() == "OptiScaler.dll" {
                PathBuf::from(&dll)
            } else {
                rel.clone()
            };
            let dest = target_dir.join(&dest_name);
            if dest.exists() {
                let backup = backup_path(&dest);
                anyhow::ensure!(
                    !backup.exists(),
                    "{} already exists; remove it first",
                    backup.display()
                );
                std::fs::rename(&dest, &backup)
                    .with_context(|| format!("backing up {}", dest.display()))?;
                manifest.backed_up.push(dest_name.clone());
            }
            copy_recursive(&source.join(&rel), &dest)?;
            manifest.created.push(dest_name);
        }
        Ok(())
    })();
    if let Err(e) = result {
        // Leave the folder as we found it.
        restore(&manifest);
        return Err(e);
    }
    manifest.save(id)?;
    Ok(manifest)
}

/// Removes everything `install` added and puts the game's own files back.
pub fn uninstall(id: &GameId) -> anyhow::Result<()> {
    let manifest = Manifest::load(id)?.context("OptiScaler is not installed for this game")?;
    restore(&manifest);
    std::fs::remove_file(Manifest::path(id)?)?;
    Ok(())
}

fn restore(manifest: &Manifest) {
    for rel in &manifest.created {
        let path = manifest.target_dir.join(rel);
        let _ = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
    }
    for rel in &manifest.backed_up {
        let path = manifest.target_dir.join(rel);
        let _ = std::fs::rename(backup_path(&path), &path);
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(BACKUP_SUFFIX);
    path.with_file_name(name)
}

fn copy_recursive(from: &Path, to: &Path) -> anyhow::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)?.flatten() {
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to).with_context(|| format!("copying to {}", to.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkor_core::Store;

    fn touch(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn parses_github_release() {
        let r = parse_release(
            r#"{"tag_name":"v0.9.5","assets":[{"name":"Optiscaler_0.9.5.7z","size":70,"browser_download_url":"https://x/o.7z"}]}"#,
        )
        .unwrap();
        assert_eq!(
            r,
            Release {
                tag: "v0.9.5".into(),
                url: "https://x/o.7z".into(),
                size: 70
            }
        );
    }

    #[test]
    fn install_and_uninstall_restore_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests in this crate do not read XDG_CONFIG_HOME concurrently with this one.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("config")) };
        let src = tmp.path().join("release");
        touch(&src.join("OptiScaler.dll"), "opti");
        touch(&src.join("OptiScaler.ini"), "ini");
        touch(&src.join("libxess.dll"), "new xess");
        touch(&src.join("D3D12_Optiscaler/D3D12Core.dll"), "core");
        touch(&src.join("Licenses/XeSS_LICENSE.txt"), "lic");
        touch(&src.join("setup_linux.sh"), "sh");
        let game = tmp.path().join("game");
        touch(&game.join("Game.exe"), "exe");
        touch(&game.join("libxess.dll"), "game's own xess");

        let id = GameId::new(Store::Local, "opti-test");
        let m = install(&id, "v1", &src, &game, "dxgi").unwrap();
        assert_eq!(
            std::fs::read_to_string(game.join("dxgi.dll")).unwrap(),
            "opti"
        );
        assert_eq!(
            std::fs::read_to_string(game.join("libxess.dll")).unwrap(),
            "new xess"
        );
        assert_eq!(
            std::fs::read_to_string(game.join("libxess.dll.arkor-backup")).unwrap(),
            "game's own xess"
        );
        assert!(game.join("D3D12_Optiscaler/D3D12Core.dll").is_file());
        assert!(!game.join("Licenses").exists() && !game.join("setup_linux.sh").exists());
        assert_eq!(m.dll_stem(), "dxgi");
        assert_eq!(Manifest::load(&id).unwrap(), Some(m));
        assert!(
            install(&id, "v1", &src, &game, "dxgi").is_err(),
            "no double install"
        );

        uninstall(&id).unwrap();
        let mut left: Vec<_> = std::fs::read_dir(&game)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        left.sort();
        assert_eq!(left, ["Game.exe", "libxess.dll"]);
        assert_eq!(
            std::fs::read_to_string(game.join("libxess.dll")).unwrap(),
            "game's own xess"
        );
        assert_eq!(Manifest::load(&id).unwrap(), None);
    }
}
