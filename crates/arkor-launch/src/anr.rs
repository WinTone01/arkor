//! ANR FG (Apex Neural Rendering, https://github.com/apex-compute/ANR): neural frame
//! generation for DirectX 12 and Vulkan games on NVIDIA, AMD and Intel cards. It works as
//! OptiScaler's DLSS frame generation provider and needs Apex's own OptiScaler build, so
//! Arkor installs the release's `full` package (that OptiScaler, its `OptiScaler.ini` and
//! `OptiScaler/custom/Apex Neural Rendering FG/`) instead of adding to a stock OptiScaler.
//!
//! The files go in like Arkor's other game-folder mods (`optiscaler::install_files`): each
//! one recorded, a game file it replaces kept as a backup, everything undone on removal. The
//! proxy DLL is `dxgi.dll`, or `winmm.dll` in a Vulkan game that never loads DXGI (as the
//! package's own installer decides); launches get `WINEDLLOVERRIDES=<proxy>=n,b` like every
//! other proxy mod.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::GameId;

use crate::optiscaler::{Manifest, Mod};

const RELEASES: &str = "https://api.github.com/repos/apex-compute/ANR/releases?per_page=10";
/// The folder ANR FG lives in, under the game's `OptiScaler/custom/`.
pub const PROVIDER_DIR: &str = "OptiScaler/custom/Apex Neural Rendering FG";
/// The settings file a player may have changed; kept over an update.
const SETTINGS: &str = "dlssg/nr_fg.ini";
/// The names Apex's OptiScaler can load as.
pub const PROXIES: [&str; 3] = ["dxgi", "winmm", "version"];

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// `V.B20`
    pub version: String,
    pub url: String,
    pub size: u64,
    /// Expected SHA-256 of the package, from the release's `SHA256SUMS.txt`.
    pub sha256: Option<String>,
}

pub fn tools_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/tools/anr"))
}

/// The newest release with a `full` package (from GitHub's release list JSON).
fn parse_releases(json: &str) -> anyhow::Result<(Release, Option<String>)> {
    let list: Vec<serde_json::Value> =
        serde_json::from_str(json).context("unexpected GitHub response")?;
    list.iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .find_map(|r| {
            let tag = r["tag_name"].as_str()?;
            let assets = r["assets"].as_array()?;
            let full = assets.iter().find(|a| {
                a["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("anr-fg-") && n.ends_with("-full.zip"))
            })?;
            let sums = assets
                .iter()
                .find(|a| a["name"].as_str() == Some("SHA256SUMS.txt"))
                .and_then(|a| a["browser_download_url"].as_str())
                .map(str::to_string);
            Some((
                Release {
                    version: tag.trim_start_matches("FG-").to_string(),
                    url: full["browser_download_url"].as_str()?.to_string(),
                    size: full["size"].as_u64().unwrap_or(0),
                    sha256: None,
                },
                sums.map(|s| format!("{s}\n{}", full["name"].as_str().unwrap_or_default())),
            ))
        })
        .context("no ANR FG release with a full package found")
}

/// The hash `SHA256SUMS.txt` gives for `file`.
fn sum_of(sums: &str, file: &str) -> Option<String> {
    crate::emu::install::parse_sha256sums(sums, file)
}

/// Asks GitHub for the newest ANR FG release. Blocking.
pub fn latest() -> anyhow::Result<Release> {
    let (mut release, sums) = parse_releases(&crate::tools::http_get(RELEASES)?)?;
    if let Some((url, file)) = sums.as_deref().and_then(|s| s.split_once('\n')) {
        release.sha256 = crate::tools::http_get(url)
            .ok()
            .and_then(|text| sum_of(&text, file));
    }
    Ok(release)
}

/// Downloaded packages as (version, folder holding `game/`), newest first.
pub fn downloaded() -> Vec<(String, PathBuf)> {
    let Ok(entries) = tools_dir().and_then(|d| Ok(std::fs::read_dir(d)?)) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().join("game/dxgi.dll").is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    found.sort_by_key(|(v, _)| std::cmp::Reverse(crate::start::natural_key(v)));
    found
}

/// Downloads, checks and unpacks a release (skipped when it is there). Blocking.
pub fn download(release: &Release) -> anyhow::Result<PathBuf> {
    let root = tools_dir()?;
    let dest = root.join(&release.version);
    if dest.join("game/dxgi.dll").is_file() {
        return Ok(dest);
    }
    let staging = root.join(format!(".{}.partial", release.version));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let zip = staging.join("package.zip");
    crate::tools::download(&release.url, &zip)?;
    if let Some(want) = &release.sha256 {
        let got = crate::emu::install::sha256(&zip)?;
        anyhow::ensure!(
            &got == want,
            "the ANR FG download is damaged (SHA-256 {got}, expected {want})"
        );
    }
    let files = staging.join("files");
    crate::optiscaler::extract(&zip, &files)?;
    anyhow::ensure!(
        files.join("game/dxgi.dll").is_file() && files.join("game").join(PROVIDER_DIR).is_dir(),
        "the ANR FG package does not have the expected files"
    );
    std::fs::rename(&files, &dest)?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(dest)
}

/// Whether the game in `dir` is a Vulkan game that never loads DXGI: its largest `.exe`
/// names `vulkan-1.dll` and none of `dxgi.dll`, `d3d11.dll`, `d3d12.dll` (in ASCII or
/// UTF-16), the same test as ANR's installer.
pub fn vulkan_only(dir: &Path) -> bool {
    let Some(exe) = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
    else {
        return false;
    };
    let Ok(bytes) = std::fs::read(exe) else {
        return false;
    };
    names_only_vulkan(&bytes)
}

fn names_only_vulkan(bytes: &[u8]) -> bool {
    let text: Vec<u8> = bytes
        .iter()
        .filter(|b| **b != 0)
        .map(u8::to_ascii_lowercase)
        .collect();
    let has = |n: &[u8]| text.windows(n.len()).any(|w| w == n);
    has(b"vulkan-1.dll") && !has(b"dxgi.dll") && !has(b"d3d11.dll") && !has(b"d3d12.dll")
}

/// The proxy name for a game folder when the player did not choose one.
pub fn default_proxy(dir: &Path) -> &'static str {
    if vulkan_only(dir) { "winmm" } else { "dxgi" }
}

pub fn manifest(id: &GameId) -> Option<Manifest> {
    Manifest::load_mod(Mod::Anr, id).ok().flatten()
}

/// Installs ANR FG with Apex's OptiScaler into `target_dir` as `<proxy>.dll`. Blocking.
pub fn install(
    id: &GameId,
    version: &str,
    source: &Path,
    target_dir: &Path,
    proxy: &str,
) -> anyhow::Result<Manifest> {
    anyhow::ensure!(
        PROXIES.contains(&proxy),
        "Apex's OptiScaler loads as dxgi, winmm or version, not {proxy}"
    );
    for (m, what) in [
        (Mod::OptiScaler, "OptiScaler"),
        (Mod::DlssEnabler, "DLSS Enabler"),
    ] {
        anyhow::ensure!(
            Manifest::load_mod(m, id)?.is_none(),
            "ANR FG brings its own OptiScaler: remove {what} from this game first"
        );
    }
    anyhow::ensure!(
        crate::bundles::dlssnr(id).is_none(),
        "ANR FG brings its own OptiScaler: remove DLSS-NR from this game first"
    );
    let game = source.join("game");
    let dll = format!("{proxy}.dll");
    let files = vec![
        (game.join("dxgi.dll"), PathBuf::from(&dll)),
        (game.join("OptiScaler.ini"), PathBuf::from("OptiScaler.ini")),
        (game.join("OptiScaler"), PathBuf::from("OptiScaler")),
    ];
    crate::optiscaler::install_files(Mod::Anr, id, version, files, target_dir, &dll)
}

pub fn uninstall(id: &GameId) -> anyhow::Result<()> {
    crate::optiscaler::uninstall_mod(Mod::Anr, id)
}

/// Puts the newest downloaded package in place of the installed one, keeping the proxy
/// name and the player's `nr_fg.ini`. Blocking.
pub fn update(id: &GameId) -> anyhow::Result<Manifest> {
    let old = manifest(id).context("ANR FG is not installed in this game")?;
    let (version, source) = downloaded()
        .into_iter()
        .next()
        .context("no ANR FG package downloaded")?;
    let settings = old.target_dir.join(PROVIDER_DIR).join(SETTINGS);
    let kept = std::fs::read(&settings).ok();
    uninstall(id)?;
    let m = install(id, &version, &source, &old.target_dir, &old.dll_stem())?;
    if let Some(bytes) = kept {
        std::fs::write(m.target_dir.join(PROVIDER_DIR).join(SETTINGS), bytes)?;
    }
    Ok(m)
}

/// The newest `nr_fg-*.log` of the game, the file ANR's developers ask for with a problem.
pub fn latest_log(m: &Manifest) -> Option<PathBuf> {
    let dir = m.target_dir.join(PROVIDER_DIR).join("dlssg");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("nr_fg-") && n.ends_with(".log"))
        })
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_release_list_and_checksums() {
        let json = r#"[
          {"tag_name": "FG-V.B20", "draft": false, "assets": [
            {"name": "anr-fg-V.B20-addon.zip", "size": 1, "browser_download_url": "https://x/addon.zip"},
            {"name": "anr-fg-V.B20-full.zip", "size": 11729994, "browser_download_url": "https://x/full.zip"},
            {"name": "SHA256SUMS.txt", "size": 283, "browser_download_url": "https://x/SHA256SUMS.txt"}
          ]}
        ]"#;
        let (r, sums) = parse_releases(json).unwrap();
        assert_eq!(r.version, "V.B20");
        assert_eq!(r.url, "https://x/full.zip");
        assert_eq!(
            sums.as_deref(),
            Some("https://x/SHA256SUMS.txt\nanr-fg-V.B20-full.zip")
        );
        let text = "750885d982b0e58e815004da698999f3b85f20c84a28c33cc15ae54e555f70ae  anr-fg-V.B20-addon.zip\n999e8b08ac177749880d75018bbe02f6d184730bbd3bb13b68633860bf491598  anr-fg-V.B20-full.zip\n";
        assert_eq!(
            sum_of(text, "anr-fg-V.B20-full.zip").as_deref(),
            Some("999e8b08ac177749880d75018bbe02f6d184730bbd3bb13b68633860bf491598")
        );
        assert!(sum_of(text, "other.zip").is_none());
    }

    #[test]
    fn tells_vulkan_only_games() {
        let utf16 = |s: &str| s.bytes().flat_map(|b| [b, 0]).collect::<Vec<u8>>();
        assert!(names_only_vulkan(b"MZ..VULKAN-1.dll..kernel32.dll"));
        assert!(names_only_vulkan(&utf16("vulkan-1.dll")));
        assert!(!names_only_vulkan(b"vulkan-1.dll dxgi.dll"));
        assert!(!names_only_vulkan(b"d3d12.dll"));
    }
}
