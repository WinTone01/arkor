//! Proton downloader: lists releases of popular Proton builds and installs them into Steam's
//! `compatibilitytools.d` (so Steam, umu and Arkor all see them). Downloads are checked
//! against the release's `.sha512sum` when it has one.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Ge,
    CachyOs,
    Dw,
    Wineland,
}

impl Variant {
    pub const ALL: [Variant; 4] = [
        Variant::Ge,
        Variant::CachyOs,
        Variant::Dw,
        Variant::Wineland,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Variant::Ge => "GE-Proton",
            Variant::CachyOs => "Proton-CachyOS",
            Variant::Dw => "DW-Proton",
            Variant::Wineland => "Proton-Wineland",
        }
    }

    fn api(self) -> &'static str {
        match self {
            Variant::Ge => {
                "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases?per_page=15"
            }
            Variant::CachyOs => {
                "https://api.github.com/repos/CachyOS/proton-cachyos/releases?per_page=15"
            }
            Variant::Dw => "https://dawn.wine/api/v1/repos/dawn-winery/dwproton/releases?limit=15",
            Variant::Wineland => {
                "https://api.github.com/repos/nanomatters/proton-cachyos/releases?per_page=15"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub variant: Variant,
    pub tag: String,
    pub url: String,
    pub size: u64,
    pub checksum_url: Option<String>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    #[serde(default)]
    size: u64,
    browser_download_url: String,
}

pub fn list(variant: Variant) -> anyhow::Result<Vec<Release>> {
    let json = crate::tools::http_get(variant.api())?;
    parse(variant, &json, cpu_has_v3())
}

/// Parses a release list (newest first) the way `list` does, for this CPU.
pub fn parse_list(variant: Variant, json: &str) -> anyhow::Result<Vec<Release>> {
    parse(variant, json, cpu_has_v3())
}

fn parse(variant: Variant, json: &str, x86_64_v3: bool) -> anyhow::Result<Vec<Release>> {
    let releases: Vec<ApiRelease> =
        serde_json::from_str(json).context("unexpected release list")?;
    Ok(releases
        .into_iter()
        .filter_map(|r| {
            let archive = |a: &&ApiAsset| {
                a.name.ends_with(".tar.gz")
                    || a.name.ends_with(".tar.xz")
                    || a.name.ends_with(".tar.zst")
            };
            let x86 = |a: &&ApiAsset| !a.name.contains("aarch64") && !a.name.contains("arm64");
            let v3 = |a: &&ApiAsset| a.name.contains("x86_64_v3");
            let candidates: Vec<&ApiAsset> = r.assets.iter().filter(archive).filter(x86).collect();
            // Prefer the x86-64-v3 optimized build when the CPU supports it.
            let asset = candidates
                .iter()
                .find(|a| x86_64_v3 && v3(a))
                .or_else(|| candidates.iter().find(|a| !v3(a)))
                .or_else(|| candidates.first())?;
            let checksum_url = r
                .assets
                .iter()
                // Exact stem match: `x86_64.sha512sum` must not pair with `x86_64_v3.tar.xz`.
                .find(|c| c.name.strip_suffix(".sha512sum") == asset.name.split(".tar").next())
                .map(|c| c.browser_download_url.clone());
            Some(Release {
                variant,
                tag: r.tag_name.clone(),
                url: asset.browser_download_url.clone(),
                size: asset.size,
                checksum_url,
            })
        })
        .collect())
}

fn cpu_has_v3() -> bool {
    std::fs::read_to_string("/proc/cpuinfo")
        .is_ok_and(|c| c.contains(" avx2") && c.contains(" bmi2") && c.contains(" fma"))
}

/// Where downloaded builds go: Steam's user compatibility tools folder.
pub fn install_root() -> anyhow::Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("no home dir")?
        .join(".local/share/Steam/compatibilitytools.d"))
}

/// Downloads `release` into the cache and checks it against its checksum; returns the archive.
fn fetch_verified(release: &Release) -> anyhow::Result<PathBuf> {
    let cache = dirs::cache_dir()
        .context("no cache dir")?
        .join("arkor/proton");
    let file_name = release.url.rsplit('/').next().unwrap_or("proton.tar");
    let archive = cache.join(file_name);
    crate::tools::download(&release.url, &archive)?;

    if let Some(url) = &release.checksum_url {
        let expected = crate::tools::http_get(url)?;
        let expected = expected
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        let out = std::process::Command::new("sha512sum")
            .arg(&archive)
            .output()
            .context("running sha512sum")?;
        let actual = String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        if expected.is_empty() || actual != expected {
            let _ = std::fs::remove_file(&archive);
            anyhow::bail!("checksum mismatch for {file_name}; download removed");
        }
    }
    Ok(archive)
}

fn untar(archive: &Path, into: &Path) -> anyhow::Result<()> {
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .status()
        .context("running tar")?;
    let _ = std::fs::remove_file(archive);
    anyhow::ensure!(status.success(), "could not unpack {}", archive.display());
    Ok(())
}

/// Downloads, verifies and unpacks `release`; returns the installed folder. Blocking.
pub fn install(release: &Release) -> anyhow::Result<PathBuf> {
    let root = install_root()?;
    std::fs::create_dir_all(&root)?;
    let archive = fetch_verified(release)?;
    let file_name = archive
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let before = folders(&root);
    untar(&archive, &root)?;
    let created = folders(&root).into_iter().find(|f| !before.contains(f));
    created
        .or_else(|| {
            let stem = file_name.split(".tar").next().unwrap_or(&file_name);
            Some(root.join(stem)).filter(|p| p.is_dir())
        })
        .context("archive did not contain a Proton folder")
}

/// Like [`install`], but the build ends up in `dest` (replacing what is there), whatever
/// the archive's folder is called. Used for the auto-updated "Latest" builds.
pub fn install_into(release: &Release, dest: &Path) -> anyhow::Result<()> {
    let root = dest.parent().context("destination without a parent")?;
    std::fs::create_dir_all(root)?;
    let archive = fetch_verified(release)?;
    let tmp = root.join("__arkor_tmp__");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let result = (|| {
        untar(&archive, &tmp)?;
        let extracted = folders(&tmp)
            .into_iter()
            .find(|p| p.is_dir())
            .context("archive did not contain a Proton folder")?;
        if dest.exists() {
            std::fs::remove_dir_all(dest)?;
        }
        std::fs::rename(&extracted, dest)?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn folders(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .map(|e| e.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Removes a build Arkor (or the user) put in the user compatibility tools folder.
/// System packages (`/usr/share/...`) are never touched.
pub fn uninstall(path: &Path) -> anyhow::Result<()> {
    let root = install_root()?;
    anyhow::ensure!(
        path.starts_with(&root) && path != root,
        "only builds in {} can be removed",
        root.display()
    );
    std::fs::remove_dir_all(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"[{"tag_name":"cachyos-11.0-20261015-slr","assets":[
        {"name":"proton-cachyos-11.0-slr-arm64.tar.xz","size":1,"browser_download_url":"u/arm"},
        {"name":"proton-cachyos-11.0-slr-x86_64.sha512sum","size":1,"browser_download_url":"u/x86.sum"},
        {"name":"proton-cachyos-11.0-slr-x86_64.tar.xz","size":5,"browser_download_url":"u/x86"},
        {"name":"proton-cachyos-11.0-slr-x86_64_v3.sha512sum","size":1,"browser_download_url":"u/v3.sum"},
        {"name":"proton-cachyos-11.0-slr-x86_64_v3.tar.xz","size":4,"browser_download_url":"u/v3"}]},
        {"tag_name":"notes-only","assets":[]}]"#;

    #[test]
    fn picks_cpu_matching_asset_and_checksum() {
        let v3 = parse(Variant::CachyOs, JSON, true).unwrap();
        assert_eq!(v3.len(), 1);
        assert_eq!(
            (v3[0].url.as_str(), v3[0].checksum_url.as_deref()),
            ("u/v3", Some("u/v3.sum"))
        );
        let plain = parse(Variant::CachyOs, JSON, false).unwrap();
        assert_eq!(
            (plain[0].url.as_str(), plain[0].checksum_url.as_deref()),
            ("u/x86", Some("u/x86.sum"))
        );
    }

    #[test]
    fn rejects_non_list_responses() {
        assert!(parse(Variant::Wineland, r#"{"message":"Not Found"}"#, true).is_err());
    }
}
