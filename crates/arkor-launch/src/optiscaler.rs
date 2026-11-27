//! OptiScaler (https://github.com/optiscaler/OptiScaler): swaps a game's upscaler (DLSS, FSR,
//! XeSS) for another and can add frame generation.
//!
//! Arkor downloads a release once into `~/.local/share/arkor/tools/optiscaler/<tag>/`, then
//! installs it into a game folder on request. Every file Arkor writes is recorded in a
//! manifest and any game file it would overwrite is kept as `<name>.arkor-backup`, so
//! uninstalling restores the folder exactly. While installed, games launch with
//! `WINEDLLOVERRIDES=<proxy>=n,b` so Wine loads OptiScaler instead of its builtin DLL.
//!
//! fakenvapi (which lets AMD and Intel cards use Reflex-style latency reduction and the
//! frame generation that needs it) ships inside OptiScaler's release, often an older build.
//! The newest one from its own repository is kept in `tools/fakenvapi/<tag>/` and installed
//! in its place.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use arkor_core::GameId;
use serde::{Deserialize, Serialize};

const REPO_API: &str = "https://api.github.com/repos/optiscaler/OptiScaler/releases/latest";
const FAKENVAPI_API: &str = "https://api.github.com/repos/optiscaler/fakenvapi/releases/latest";
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

/// Where OptiScaler builds come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channel {
    /// Official releases.
    #[default]
    Stable,
    /// Daily builds of the main branch (benjamimgois/OptiScaler-builds, as Goverlay uses).
    Edge,
}

const EDGE_API: &str =
    "https://api.github.com/repos/benjamimgois/OptiScaler-builds/releases?per_page=20";
const OPTIPATCHER_URL: &str =
    "https://github.com/optiscaler/OptiPatcher/releases/download/rolling/OptiPatcher.asi";

/// Latest build on `channel`.
pub fn latest_release_in(channel: Channel) -> anyhow::Result<Release> {
    match channel {
        Channel::Stable => latest_release(),
        Channel::Edge => {
            let json = crate::tools::http_get(EDGE_API)?;
            let list: Vec<serde_json::Value> =
                serde_json::from_str(&json).context("unexpected GitHub response")?;
            let edge = list
                .into_iter()
                .find(|r| {
                    r["tag_name"]
                        .as_str()
                        .is_some_and(|t| t.starts_with("edge-"))
                })
                .context("no edge build found")?;
            parse_release(&edge.to_string())
        }
    }
}

/// Adds OptiPatcher (an ASI plugin that exposes DLSS / DLSS-FG inputs without spoofing in
/// supported games) to the game's OptiScaler install and turns on ASI loading. Blocking.
pub fn install_optipatcher(id: &GameId) -> anyhow::Result<()> {
    let mut m = Manifest::load_mod(Mod::OptiScaler, id)?.context("install OptiScaler first")?;
    let plugins = m.target_dir.join("plugins");
    let rel = if plugins.exists() {
        PathBuf::from("plugins/OptiPatcher.asi")
    } else {
        PathBuf::from("plugins")
    };
    let asi = plugins.join("OptiPatcher.asi");
    anyhow::ensure!(!asi.exists(), "{} already exists", asi.display());
    std::fs::create_dir_all(&plugins)?;
    if let Err(e) = crate::tools::download(OPTIPATCHER_URL, &asi) {
        let _ = if rel.as_os_str() == "plugins" {
            std::fs::remove_dir_all(&plugins)
        } else {
            std::fs::remove_file(&asi)
        };
        return Err(e);
    }
    m.created.push(rel);
    m.save(Mod::OptiScaler, id)?;
    set_asi_loading(id, true)
}

/// The FSR4 upscaler DLL name OptiScaler loads.
pub const FSR4_DLL: &str = "amd_fidelityfx_upscaler_dx12.dll";

/// Puts a user-supplied FSR4 build (`amd_fidelityfx_upscaler_dx12.dll`, e.g. a newer or
/// INT8 build) into the game's OptiScaler folder, keeping the bundled one as a backup.
pub fn install_fsr4(id: &GameId, dll: &Path) -> anyhow::Result<Manifest> {
    let opti = Manifest::load_mod(Mod::OptiScaler, id)?.context("install OptiScaler first")?;
    anyhow::ensure!(dll.is_file(), "{} is not a file", dll.display());
    let version = dll
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    // 0.9+ builds keep their libraries in an `OptiScaler/` subfolder.
    let rel = if opti.target_dir.join("OptiScaler").join(FSR4_DLL).is_file() {
        PathBuf::from("OptiScaler").join(FSR4_DLL)
    } else {
        PathBuf::from(FSR4_DLL)
    };
    install_files(
        Mod::Fsr4,
        id,
        &version,
        vec![(dll.to_path_buf(), rel)],
        &opti.target_dir,
        "",
    )
}

pub fn optipatcher_installed(id: &GameId) -> bool {
    Manifest::load_mod(Mod::OptiScaler, id)
        .ok()
        .flatten()
        .is_some_and(|m| m.target_dir.join("plugins/OptiPatcher.asi").is_file())
}

pub fn remove_optipatcher(id: &GameId) -> anyhow::Result<()> {
    let mut m = Manifest::load_mod(Mod::OptiScaler, id)?.context("OptiScaler is not installed")?;
    let _ = std::fs::remove_file(m.target_dir.join("plugins/OptiPatcher.asi"));
    // A `plugins` entry stays: OptiScaler's own uninstall removes the folder.
    m.created
        .retain(|c| c != Path::new("plugins/OptiPatcher.asi"));
    m.save(Mod::OptiScaler, id)?;
    set_asi_loading(id, false)
}

fn set_asi_loading(id: &GameId, on: bool) -> anyhow::Result<()> {
    if let Some(ini) = ini_path(id) {
        let text = std::fs::read_to_string(&ini)?;
        std::fs::write(
            &ini,
            ini_set(
                &text,
                "Plugins",
                "LoadAsiPlugins",
                if on { "true" } else { "auto" },
            ),
        )?;
    }
    Ok(())
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

    crate::tools::download(&release.url, &archive)?;

    let unpacked = staging.join("files");
    extract(&archive, &unpacked)?;
    // Some releases wrap everything in one folder; use the folder that has OptiScaler.dll.
    let source = find_dll_dir(&unpacked).context("archive does not contain OptiScaler.dll")?;
    std::fs::rename(&source, &dest).with_context(|| format!("moving into {}", dest.display()))?;
    let _ = std::fs::remove_dir_all(&staging);
    // Best effort: OptiScaler works with the fakenvapi it ships.
    if let Err(e) = update_fakenvapi() {
        eprintln!("arkor: newer fakenvapi not downloaded: {e:#}");
    }
    Ok(dest)
}

fn fakenvapi_root() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/tools/fakenvapi"))
}

/// The newest downloaded fakenvapi: (tag, folder holding `fakenvapi.dll`).
pub fn fakenvapi() -> Option<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(fakenvapi_root().ok()?)
        .ok()?
        .flatten()
        .filter(|e| e.path().join("fakenvapi.dll").is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    found.sort_by_key(|(tag, _)| std::cmp::Reverse(crate::start::natural_key(tag)));
    found.into_iter().next()
}

/// Downloads the latest fakenvapi when it is not there yet; its tag. Blocking.
pub fn update_fakenvapi() -> anyhow::Result<String> {
    let release = parse_release(&crate::tools::http_get(FAKENVAPI_API)?)?;
    let root = fakenvapi_root()?;
    let dest = root.join(&release.tag);
    if dest.join("fakenvapi.dll").is_file() {
        return Ok(release.tag);
    }
    let staging = root.join(format!(".{}.partial", release.tag));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let archive = staging.join("release.archive");
    crate::tools::download(&release.url, &archive)?;
    let unpacked = staging.join("files");
    extract(&archive, &unpacked)?;
    anyhow::ensure!(
        unpacked.join("fakenvapi.dll").is_file(),
        "the fakenvapi archive has no fakenvapi.dll"
    );
    std::fs::rename(&unpacked, &dest)?;
    let _ = std::fs::remove_dir_all(&staging);
    // Older copies are not needed any more.
    for e in std::fs::read_dir(&root)?.flatten() {
        if e.path() != dest {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    Ok(release.tag)
}

pub(crate) fn extract(archive: &Path, into: &Path) -> anyhow::Result<()> {
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

/// Game-folder mods Arkor installs the same way (copy, back up, record, restore).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mod {
    OptiScaler,
    DlssEnabler,
    ReShade,
    /// Newer Streamline / DLSS DLLs over the ones the game ships (no proxy DLL).
    Streamline,
    /// A user-supplied FSR4 upscaler DLL next to OptiScaler (no proxy DLL).
    Fsr4,
    /// ANR FG with Apex's own OptiScaler build.
    Anr,
}

impl Mod {
    pub const ALL: [Mod; 6] = [
        Mod::OptiScaler,
        Mod::DlssEnabler,
        Mod::ReShade,
        Mod::Streamline,
        Mod::Fsr4,
        Mod::Anr,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mod::OptiScaler => "OptiScaler",
            Mod::DlssEnabler => "DLSS Enabler",
            Mod::ReShade => "ReShade",
            Mod::Streamline => "Streamline",
            Mod::Fsr4 => "FSR4",
            Mod::Anr => "ANR FG",
        }
    }

    fn dir(self) -> &'static str {
        match self {
            Mod::OptiScaler => "optiscaler",
            Mod::DlssEnabler => "dlss-enabler",
            Mod::ReShade => "reshade",
            Mod::Streamline => "streamline",
            Mod::Fsr4 => "fsr4",
            Mod::Anr => "anr",
        }
    }
}

/// Proxy DLL names (without `.dll`) of every mod installed in a game, for WINEDLLOVERRIDES.
pub fn installed_proxies(id: &GameId) -> Vec<String> {
    Mod::ALL
        .into_iter()
        .filter_map(|m| Manifest::load_mod(m, id).ok().flatten())
        .filter(|m| !m.dll.is_empty())
        .map(|m| m.dll_stem())
        .collect()
}

impl Manifest {
    fn path_mod(m: Mod, id: &GameId) -> anyhow::Result<PathBuf> {
        Ok(arkor_core::paths::config_dir()?
            .join(m.dir())
            .join(format!("{}.toml", id.file_stem())))
    }

    /// The OptiScaler manifest of a game.
    pub fn load(id: &GameId) -> anyhow::Result<Option<Self>> {
        Self::load_mod(Mod::OptiScaler, id)
    }

    pub fn load_mod(m: Mod, id: &GameId) -> anyhow::Result<Option<Self>> {
        let path = Self::path_mod(m, id)?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(
                toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn save(&self, m: Mod, id: &GameId) -> anyhow::Result<()> {
        let path = Self::path_mod(m, id)?;
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    /// `dxgi` for `dxgi.dll`: the name WINEDLLOVERRIDES wants.
    pub fn dll_stem(&self) -> String {
        self.dll.trim_end_matches(".dll").to_string()
    }
}

/// Release files that go into the game folder: DLLs, ini files, the D3D12 runtime, the
/// `OptiScaler/` library folder of 0.9+ builds and bundled `plugins/`. Licenses, readmes and
/// the setup scripts stay out.
fn release_entries(source: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(source)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let keep = if entry.path().is_dir() {
            name.starts_with("d3d12_") || name == "optiscaler" || name == "plugins"
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
        Manifest::load_mod(Mod::Anr, id)?.is_none(),
        "ANR FG brings its own OptiScaler: remove ANR FG from this game first"
    );
    let dll = format!("{proxy}.dll");
    let newer_fakenvapi = fakenvapi().map(|(_, dir)| dir.join("fakenvapi.dll"));
    let files = release_entries(source)?
        .into_iter()
        .map(|rel| {
            let dest = if rel.as_os_str() == "OptiScaler.dll" {
                PathBuf::from(&dll)
            } else {
                rel.clone()
            };
            let src = match &newer_fakenvapi {
                Some(newer) if rel.as_os_str().eq_ignore_ascii_case("fakenvapi.dll") => {
                    newer.clone()
                }
                _ => source.join(rel),
            };
            (src, dest)
        })
        .collect();
    install_files(Mod::OptiScaler, id, version, files, target_dir, &dll)
}

/// Copies `files` (source, path relative to `target_dir`) into the game folder, moving any
/// game file it replaces aside, and records everything so `uninstall_mod` can undo it.
pub(crate) fn install_files(
    m: Mod,
    id: &GameId,
    version: &str,
    files: Vec<(PathBuf, PathBuf)>,
    target_dir: &Path,
    dll: &str,
) -> anyhow::Result<Manifest> {
    anyhow::ensure!(
        Manifest::load_mod(m, id)?.is_none(),
        "{} is already installed for this game",
        m.name()
    );
    anyhow::ensure!(
        target_dir.is_dir(),
        "{} is not a folder",
        target_dir.display()
    );
    for other in Mod::ALL.into_iter().filter(|o| *o != m) {
        if let Some(om) = Manifest::load_mod(other, id)? {
            anyhow::ensure!(
                dll.is_empty() || !om.dll.eq_ignore_ascii_case(dll),
                "{} already uses {dll}; pick another name for {}",
                other.name(),
                m.name()
            );
        }
    }

    let mut manifest = Manifest {
        version: version.to_string(),
        target_dir: target_dir.to_path_buf(),
        dll: dll.to_string(),
        created: Vec::new(),
        backed_up: Vec::new(),
    };
    let result = (|| -> anyhow::Result<()> {
        for (src, dest_name) in files {
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
            copy_recursive(&src, &dest)?;
            manifest.created.push(dest_name);
        }
        Ok(())
    })();
    if let Err(e) = result {
        // Leave the folder as we found it.
        restore(&manifest);
        return Err(e);
    }
    manifest.save(m, id)?;
    Ok(manifest)
}

/// Removes everything `install` added and puts the game's own files back.
pub fn uninstall(id: &GameId) -> anyhow::Result<()> {
    // A custom FSR4 DLL sits on top of OptiScaler's files; undo it first.
    if Manifest::load_mod(Mod::Fsr4, id)?.is_some() {
        uninstall_mod(Mod::Fsr4, id)?;
    }
    uninstall_mod(Mod::OptiScaler, id)
}

pub fn uninstall_mod(m: Mod, id: &GameId) -> anyhow::Result<()> {
    let manifest = Manifest::load_mod(m, id)?
        .with_context(|| format!("{} is not installed for this game", m.name()))?;
    restore(&manifest);
    std::fs::remove_file(Manifest::path_mod(m, id)?)?;
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
        let _lock = crate::CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests that read XDG_CONFIG_HOME hold CONFIG_ENV_LOCK.
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

/// OptiScaler.ini settings Arkor exposes: (section, key, allowed values; `auto` = OptiScaler
/// decides). Everything else in the file is left as is.
pub const INI_SETTINGS: &[(&str, &str, &[&str])] = &[
    (
        "Upscalers",
        "Dx12Upscaler",
        &["auto", "xess", "fsr21", "fsr22", "fsr31", "dlss"],
    ),
    (
        "Upscalers",
        "Dx11Upscaler",
        &[
            "auto", "fsr22", "fsr31", "xess", "xess_12", "fsr31_12", "dlss",
        ],
    ),
    (
        "Upscalers",
        "VulkanUpscaler",
        &["auto", "fsr22", "fsr31", "xess", "fsr31_12", "dlss"],
    ),
    ("FrameGen", "Enabled", &["auto", "true", "false"]),
    (
        "FrameGen",
        "FGInput",
        &[
            "auto", "nofg", "dlssg", "nukems", "fsrfg", "fsrfg30", "upscaler",
        ],
    ),
    (
        "FrameGen",
        "FGOutput",
        &["auto", "nofg", "fsrfg", "xefg", "nukems"],
    ),
    ("Spoofing", "Dxgi", &["auto", "true", "false"]),
    ("Sharpness", "OverrideSharpness", &["auto", "true", "false"]),
    (
        "Sharpness",
        "Sharpness",
        &["auto", "0.1", "0.2", "0.3", "0.5", "0.7", "1.0"],
    ),
    ("Log", "LogLevel", &["auto", "0", "1", "2", "3", "4"]),
];

/// The game's OptiScaler.ini (in the folder OptiScaler was installed to).
pub fn ini_path(id: &GameId) -> Option<PathBuf> {
    let m = Manifest::load(id).ok().flatten()?;
    let p = m.target_dir.join("OptiScaler.ini");
    p.is_file().then_some(p)
}

/// Value of `key` in `[section]`.
pub fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_section = t
                .trim_matches(|c| c == '[' || c == ']')
                .eq_ignore_ascii_case(section);
        } else if in_section
            && !t.starts_with(';')
            && let Some((k, v)) = t.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Sets `key` in `[section]`, keeping every other line (comments included) untouched;
/// appends the key (and section) when missing.
pub fn ini_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut out = Vec::new();
    let mut in_section = false;
    let mut section_seen = false;
    let mut done = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_section && !done {
                out.push(format!("{key}={value}"));
                done = true;
            }
            in_section = t
                .trim_matches(|c| c == '[' || c == ']')
                .eq_ignore_ascii_case(section);
            section_seen |= in_section;
        } else if in_section
            && !done
            && !t.starts_with(';')
            && let Some((k, _)) = t.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            // Keep the file's own spelling of the key.
            out.push(format!("{}={value}", k.trim()));
            done = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !done {
        if !section_seen {
            out.push(format!("[{section}]"));
        }
        out.push(format!("{key}={value}"));
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod ini_tests {
    use super::*;

    #[test]
    fn edits_only_the_key() {
        let ini =
            "[Upscalers]\n; pick one\nDx12Upscaler=auto\n\n[FrameGen]\n; Enabled\nEnabled=auto\n";
        assert_eq!(ini_get(ini, "FrameGen", "Enabled").as_deref(), Some("auto"));
        let out = ini_set(ini, "FrameGen", "enabled", "true");
        assert_eq!(
            out,
            "[Upscalers]\n; pick one\nDx12Upscaler=auto\n\n[FrameGen]\n; Enabled\nEnabled=true\n"
        );
        let out = ini_set(&out, "Upscalers", "VulkanUpscaler", "fsr31");
        assert_eq!(
            ini_get(&out, "Upscalers", "VulkanUpscaler").as_deref(),
            Some("fsr31")
        );
        assert!(
            out.find("VulkanUpscaler").unwrap() < out.find("[FrameGen]").unwrap(),
            "added inside its section"
        );
        let out = ini_set(&out, "Log", "LogLevel", "2");
        assert!(out.ends_with("[Log]\nLogLevel=2\n"));
    }
}
