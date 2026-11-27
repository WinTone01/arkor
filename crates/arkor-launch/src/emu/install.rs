//! Getting an emulator, the same way on every distribution:
//!
//! 1. Arkor downloads the project's own Linux build (GitHub releases, the libretro
//!    buildbot) into `<emulators>/<id>/<version>/`. AppImages are unpacked there, so they
//!    run without FUSE (which many distributions no longer install); the AppImage's name
//!    stays a small script that starts the unpacked program.
//! 2. Without such a build (Ryujinx, MAME, Dolphin, ScummVM), Flathub's, for this user only
//!    (`flatpak install --user`): Flatpak is on most distributions, and needs no password
//!    this way.
//! 3. Otherwise the user installs it from their distribution; Arkor says which program.
//!
//! An emulator already on the computer (a package, a Flatpak) is used as it is; Arkor only
//! removes what it installed.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use regex::Regex;
use serde::Deserialize;

use super::discovery::{self, Managed, compare_versions};
use super::host::Host;
use super::manifest::{EngineManifest, Source};

/// One downloadable build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub file_name: String,
    /// Where the expected SHA-256 is published, when the project publishes one.
    pub checksum_url: Option<String>,
    /// Further files of the release, unpacked beside the main one.
    pub extras: Vec<(String, String)>,
}

/// How Arkor would get an emulator here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Way {
    /// Its own Linux build, downloaded.
    Download,
    /// From Flathub, for this user.
    Flatpak(String),
    /// Only from the distribution (or by hand): these program names.
    Distribution(Vec<String>),
}

/// The archive types an emulator's download comes in that this computer can unpack.
fn can_unpack(file_pattern: &str) -> bool {
    let p = file_pattern.to_ascii_lowercase();
    if p.contains(".7z") {
        crate::start::find_in_path("7z").is_some()
            || crate::start::find_in_path("7za").is_some()
            || crate::start::find_in_path("bsdtar").is_some()
    } else if p.contains(".tar") {
        crate::start::find_in_path("tar").is_some()
    } else {
        !p.contains(".dmg")
    }
}

fn has_flatpak() -> bool {
    crate::start::find_in_path("flatpak").is_some()
}

/// How Arkor gets `m` on this computer.
pub fn way(m: &EngineManifest) -> Way {
    let host = Host::current();
    if m.sources
        .iter()
        .any(|s| s.asset_for(host).is_some_and(can_unpack))
    {
        return Way::Download;
    }
    if has_flatpak()
        && let Some(id) = m.sources.iter().find_map(|s| match s {
            Source::Flatpak { id } => Some(id.clone()),
            _ => None,
        })
    {
        return Way::Flatpak(id);
    }
    Way::Distribution(
        m.sources
            .iter()
            .find_map(|s| match s {
                Source::Path { names } => Some(names.clone()),
                _ => None,
            })
            .unwrap_or_default(),
    )
}

/// The distribution package (by Arch's name, see `crate::pkg`) of an emulator Arkor cannot
/// download, when the usual distributions have one.
pub fn distro_package(id: &str) -> Option<&'static str> {
    match id {
        "dolphin" => Some("dolphin-emu"),
        "scummvm" => Some("scummvm"),
        "mame" => Some("mame"),
        "desmume" => Some("desmume"),
        _ => None,
    }
}

/// Whether Arkor can install `m` by itself here.
pub fn can_get(m: &EngineManifest) -> bool {
    !matches!(way(m), Way::Distribution(_))
}

/// What a download needs that is missing here (`7z` for RetroArch's archive), as a tool
/// package name Arkor can install; `None` when nothing is.
pub fn missing_tool(m: &EngineManifest) -> Option<&'static str> {
    let host = Host::current();
    let pattern = m.sources.iter().find_map(|s| s.asset_for(host))?;
    (!can_unpack(pattern) && pattern.to_ascii_lowercase().contains(".7z")).then_some("7zip")
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    updated_at: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

fn date_of(stamp: Option<&str>) -> Option<String> {
    stamp.and_then(|s| s.get(..10)).map(str::to_string)
}

/// Releases in a GitHub answer that have an asset matching `pattern` (and every extra).
pub(crate) fn from_github(
    body: &str,
    pattern: &str,
    extras: &[String],
    checksums: Option<&str>,
) -> anyhow::Result<Vec<Release>> {
    let list: Vec<GhRelease> =
        serde_json::from_str(body).context("unexpected answer from GitHub")?;
    let asset_re = Regex::new(pattern)?;
    let extra_res = extras
        .iter()
        .map(|p| Regex::new(p))
        .collect::<Result<Vec<_>, _>>()?;
    let sums_re = checksums.map(Regex::new).transpose()?;
    let mut out = Vec::new();
    for r in list.into_iter().filter(|r| !r.draft && !r.prerelease) {
        let Some(asset) = r.assets.iter().find(|a| asset_re.is_match(&a.name)) else {
            continue;
        };
        let Some(extras) = extra_res
            .iter()
            .map(|re| {
                r.assets
                    .iter()
                    .find(|a| re.is_match(&a.name))
                    .map(|a| (a.browser_download_url.clone(), a.name.clone()))
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        // Rolling releases ("latest") are told apart by their build date.
        let version = if r.tag_name.chars().any(|c| c.is_ascii_digit()) {
            r.tag_name.trim_start_matches(['v', 'V', '.']).to_string()
        } else {
            date_of(asset.updated_at.as_deref())
                .or_else(|| date_of(r.published_at.as_deref()))
                .unwrap_or(r.tag_name.clone())
        };
        out.push(Release {
            version,
            url: asset.browser_download_url.clone(),
            file_name: asset.name.clone(),
            checksum_url: sums_re.as_ref().and_then(|re| {
                r.assets
                    .iter()
                    .find(|a| re.is_match(&a.name))
                    .map(|a| a.browser_download_url.clone())
            }),
            extras,
        });
    }
    Ok(out)
}

/// Versions in a buildbot folder listing.
pub(crate) fn from_buildbot(
    body: &str,
    index: &str,
    version_pattern: &str,
    template: &str,
) -> anyhow::Result<Vec<Release>> {
    let href = Regex::new(r#"href="([^"]+)""#)?;
    let version_re = Regex::new(version_pattern)?;
    let base = if index.ends_with('/') {
        index.to_string()
    } else {
        format!("{index}/")
    };
    Ok(href
        .captures_iter(body)
        .filter_map(|c| {
            let href = c.get(1)?.as_str();
            let href = href.rsplit_once(&base).map_or(href, |(_, rest)| rest);
            let href = href.trim_start_matches('/');
            let href = href.strip_prefix("stable/").unwrap_or(href);
            let version = version_re.captures(href)?.get(1)?.as_str().to_string();
            let path = template.replace("{version}", &version);
            Some(Release {
                file_name: path.rsplit('/').next().unwrap_or(&path).to_string(),
                url: format!("{base}{path}"),
                version,
                checksum_url: None,
                extras: Vec::new(),
            })
        })
        .collect())
}

/// Download links with a `version` group on a page.
pub(crate) fn from_page(body: &str, pattern: &str, base: &str) -> anyhow::Result<Vec<Release>> {
    let re = Regex::new(pattern)?;
    Ok(re
        .captures_iter(body)
        .filter_map(|c| {
            let link = c.get(0)?.as_str();
            let version = c.name("version")?.as_str().replace('_', ".");
            let url = if link.starts_with("http") {
                link.to_string()
            } else {
                format!("{base}{link}")
            };
            Some(Release {
                file_name: url.rsplit('/').next().unwrap_or(&url).to_string(),
                url,
                version,
                checksum_url: None,
                extras: Vec::new(),
            })
        })
        .collect())
}

/// The newest build of `m` for this computer. Blocking (one or two requests).
pub fn latest(m: &EngineManifest) -> anyhow::Result<Release> {
    releases(m)?
        .into_iter()
        .next()
        .with_context(|| format!("{} has no build for this computer", m.name))
}

/// The builds of `m` for this computer, newest first (a dozen at most). Blocking.
pub fn releases(m: &EngineManifest) -> anyhow::Result<Vec<Release>> {
    let host = Host::current();
    for source in &m.sources {
        let Some(pattern) = source.asset_for(host) else {
            continue;
        };
        let mut list = match source {
            Source::Github {
                repo, checksums, ..
            } => {
                let body = crate::tools::http_get(&format!(
                    "https://api.github.com/repos/{repo}/releases?per_page=15"
                ))?;
                from_github(
                    &body,
                    pattern,
                    source.extras_for(host),
                    checksums.as_deref(),
                )?
            }
            Source::Buildbot {
                index,
                version_pattern,
                ..
            } => {
                let body = crate::tools::http_get(index)?;
                from_buildbot(&body, index, version_pattern, pattern)?
            }
            Source::Page { url, base, .. } => {
                let body = crate::tools::http_get(url)?;
                from_page(&body, pattern, base.as_deref().unwrap_or(""))?
            }
            _ => continue,
        };
        // GitHub lists newest first; the others are sorted here.
        if !matches!(source, Source::Github { .. }) {
            list.sort_by(|a, b| compare_versions(&b.version, &a.version));
        }
        if !list.is_empty() {
            list.truncate(12);
            return Ok(list);
        }
    }
    bail!("{} has no build for this computer", m.name)
}

/// The newest version Arkor downloaded of `m`.
pub fn installed_version(id: &str) -> Option<String> {
    let s = discovery::state();
    let mut versions: Vec<&Managed> = s
        .managed
        .get(id)?
        .iter()
        .filter(|v| v.program.exists())
        .collect();
    versions.sort_by(|a, b| compare_versions(&b.version, &a.version));
    versions.first().map(|v| v.version.clone())
}

/// Whether Arkor installed `m` (a download or a Flatpak), so it can update and remove it.
pub fn arkor_installed(id: &str) -> bool {
    let s = discovery::state();
    s.flatpaks.contains_key(id) || installed_version(id).is_some()
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Installs (or updates) `id` the way [`way`] says; a short report. Blocking.
pub fn install(id: &str) -> anyhow::Result<String> {
    let m = super::engine(id).with_context(|| format!("unknown emulator {id}"))?;
    match way(&m) {
        Way::Download => {
            let release = latest(&m)?;
            if installed_version(id).as_deref() == Some(release.version.as_str()) {
                return Ok(format!("{} {}", m.name, release.version));
            }
            download(&m, &release)?;
            Ok(format!("{} {}", m.name, release.version))
        }
        Way::Flatpak(app) => {
            flatpak_install(&app)?;
            discovery::update_state(|s| s.flatpaks.insert(id.to_string(), app.clone()))?;
            Ok(format!("{} (Flathub)", m.name))
        }
        Way::Distribution(names) => {
            let line =
                crate::pkg::install_line(&names.iter().map(String::as_str).collect::<Vec<_>>());
            match line {
                Some(line) => bail!(
                    "Arkor cannot download {} for this computer; install it with: {line}",
                    m.name
                ),
                None => bail!(
                    "Arkor cannot download {} for this computer; install Flatpak or your distribution's {} package",
                    m.name,
                    names.first().map(String::as_str).unwrap_or(&m.id)
                ),
            }
        }
    }
}

/// `flatpak install --user` from Flathub (added for this user when missing).
fn flatpak_install(app: &str) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("flatpak")
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .context("running flatpak")?;
        anyhow::ensure!(
            out.status.success(),
            "flatpak {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(())
    };
    run(&[
        "remote-add",
        "--user",
        "--if-not-exists",
        "flathub",
        "https://dl.flathub.org/repo/flathub.flatpakrepo",
    ])?;
    let _job = crate::progress::Transfer::begin(app, None);
    run(&[
        "install",
        "--user",
        "-y",
        "--noninteractive",
        "flathub",
        app,
    ])
}

/// Downloads and unpacks `release` of `m`, then removes older versions Arkor downloaded.
pub fn download(m: &EngineManifest, release: &Release) -> anyhow::Result<PathBuf> {
    let cache = super::dirs::cache().join("downloads").join(&m.id);
    let file = cache.join(format!(
        "{}-{}",
        sanitize(&release.version),
        release.file_name
    ));
    crate::tools::download(&release.url, &file)?;
    if let Some(sums) = &release.checksum_url {
        let text = crate::tools::http_get(sums)?;
        let want = parse_sha256sums(&text, &release.file_name)
            .context("the project's checksum file does not list this download")?;
        let got = sha256(&file)?;
        if got != want {
            let _ = std::fs::remove_file(&file);
            bail!("the download of {} is damaged; try again", m.name);
        }
    }
    let dir = super::dirs::engine(&m.id).join(sanitize(&release.version));
    let staging = dir.with_extension("partial");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    unpack(&file, &release.file_name, &staging)?;
    for (url, name) in &release.extras {
        let extra = cache.join(format!("{}-{name}", sanitize(&release.version)));
        crate::tools::download(url, &extra)?;
        unpack(&extra, name, &staging)?;
        let _ = std::fs::remove_file(&extra);
    }
    unpack_appimages(&staging, 0);
    let program = discovery::locate_program(&staging, &m.binaries.linux)
        .with_context(|| format!("the {} download holds no program Arkor knows", m.name))?;
    let rel = program.strip_prefix(&staging)?.to_path_buf();
    if dir.exists() {
        arkor_core::fs_guard::remove_dir_in(&dir, &super::dirs::root())?;
    }
    std::fs::rename(&staging, &dir)?;
    let _ = std::fs::remove_file(&file);
    let size = dir_size(&dir);
    let installed = Managed {
        version: release.version.clone(),
        dir: dir.clone(),
        program: dir.join(rel),
        size,
    };
    // Older versions stay beside it (a game may need one): the newest few, and the one
    // chosen in settings.
    let pinned = arkor_core::Global::load_or_import()
        .ok()
        .and_then(|g| g.emulators.versions.get(&m.id).cloned());
    let older = discovery::update_state(|s| {
        let list = s.managed.entry(m.id.clone()).or_default();
        list.retain(|v| v.dir != installed.dir);
        list.push(installed.clone());
        list.sort_by(|a, b| compare_versions(&b.version, &a.version));
        let mut kept = 0;
        let (keep, drop): (Vec<Managed>, Vec<Managed>) = list.drain(..).partition(|v| {
            let keep = kept < KEEP_VERSIONS || pinned.as_deref() == Some(v.version.as_str());
            kept += 1;
            keep
        });
        *list = keep;
        drop
    })?;
    for v in older {
        if v.dir.starts_with(super::dirs::root()) && v.dir.exists() {
            let _ = arkor_core::fs_guard::remove_dir_in(&v.dir, &super::dirs::root());
        }
    }
    Ok(installed.program)
}

/// Versions of an emulator kept side by side (besides the one chosen in settings).
const KEEP_VERSIONS: usize = 3;

/// Every version Arkor downloaded of `id`, newest first.
pub fn installed_versions(id: &str) -> Vec<String> {
    let mut v: Vec<String> = discovery::state()
        .managed
        .get(id)
        .map(|l| {
            l.iter()
                .filter(|m| m.program.exists())
                .map(|m| m.version.clone())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| compare_versions(b, a));
    v
}

/// Downloads `version` of `id` (beside the others). Blocking.
pub fn install_version(id: &str, version: &str) -> anyhow::Result<String> {
    let m = super::engine(id).with_context(|| format!("unknown emulator {id}"))?;
    let release = releases(&m)?
        .into_iter()
        .find(|r| r.version == version)
        .with_context(|| format!("{} {version} is not offered any more", m.name))?;
    download(&m, &release)?;
    Ok(format!("{} {version}", m.name))
}

/// Removes one downloaded version of `id`.
pub fn remove_version(id: &str, version: &str) -> anyhow::Result<()> {
    let gone = discovery::update_state(|s| {
        let list = s.managed.entry(id.to_string()).or_default();
        let gone: Vec<Managed> = list
            .iter()
            .filter(|v| v.version == version)
            .cloned()
            .collect();
        list.retain(|v| v.version != version);
        gone
    })?;
    for v in gone {
        if v.dir.starts_with(super::dirs::root()) && v.dir.exists() {
            arkor_core::fs_guard::remove_dir_in(&v.dir, &super::dirs::root())?;
        }
    }
    Ok(())
}

/// Removes what Arkor installed of `id`: its downloads, and the Flatpak it installed.
pub fn remove(id: &str) -> anyhow::Result<()> {
    let (managed, flatpak) = discovery::update_state(|s| {
        (
            s.managed.remove(id).unwrap_or_default(),
            s.flatpaks.remove(id),
        )
    })?;
    for v in managed {
        if v.dir.starts_with(super::dirs::root()) && v.dir.exists() {
            arkor_core::fs_guard::remove_dir_in(&v.dir, &super::dirs::root())?;
        }
    }
    if let Some(app) = flatpak {
        let out = std::process::Command::new("flatpak")
            .args(["uninstall", "--user", "-y", "--noninteractive", &app])
            .stdin(std::process::Stdio::null())
            .output()
            .context("running flatpak")?;
        anyhow::ensure!(
            out.status.success(),
            "flatpak uninstall: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => total += e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => {}
            }
        }
    }
    total
}

/// The digest for `file_name` in a `sha256sum` listing.
pub fn parse_sha256sums(text: &str, file_name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (digest.len() == 64 && name.rsplit('/').next() == Some(file_name))
            .then(|| digest.to_ascii_lowercase())
    })
}

fn sha256(path: &Path) -> anyhow::Result<String> {
    let out = std::process::Command::new("sha256sum")
        .arg(path)
        .output()
        .context("running sha256sum")?;
    anyhow::ensure!(out.status.success(), "sha256sum failed");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .context("sha256sum said nothing")
}

/// Unpacks the download `file` (named `name`) into `dest`. Blocking.
pub fn unpack(file: &Path, name: &str, dest: &Path) -> anyhow::Result<()> {
    let lower = name.to_ascii_lowercase();
    std::fs::create_dir_all(dest)?;
    let run = |cmd: &mut std::process::Command| -> anyhow::Result<()> {
        let out = cmd
            .stdin(std::process::Stdio::null())
            .output()
            .with_context(|| format!("unpacking {name}"))?;
        anyhow::ensure!(
            out.status.success(),
            "unpacking {name}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(())
    };
    if lower.ends_with(".zip") {
        super::zip::unpack(file, dest)
    } else if lower.ends_with(".7z") {
        if let Some(seven) =
            crate::start::find_in_path("7z").or_else(|| crate::start::find_in_path("7za"))
        {
            run(std::process::Command::new(seven)
                .args(["x", "-y", "-bd"])
                .arg(format!("-o{}", dest.display()))
                .arg(file))
        } else {
            run(std::process::Command::new("bsdtar")
                .arg("-xf")
                .arg(file)
                .arg("-C")
                .arg(dest))
        }
    } else if lower.contains(".tar") || lower.ends_with(".tgz") {
        run(std::process::Command::new("tar")
            .arg("-xf")
            .arg(file)
            .arg("-C")
            .arg(dest))
    } else {
        // A program on its own (an AppImage, a single binary).
        let target = dest.join(name);
        std::fs::copy(file, &target)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
        Ok(())
    }
}

/// Unpacks every AppImage in `dir` (and one level down) beside itself, so it runs without
/// FUSE: `X.AppImage` becomes `X.AppImage.unpacked/` and `X.AppImage` a script starting
/// its `AppRun`. An AppImage that does not unpack is left as it is (it then needs FUSE).
pub fn unpack_appimages(dir: &Path, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            if depth < 2 && !name.ends_with(".unpacked") {
                unpack_appimages(&path, depth + 1);
            }
            continue;
        }
        if name.to_ascii_lowercase().ends_with(".appimage")
            && let Err(e) = unpack_appimage(&path)
        {
            eprintln!("arkor: {name} stays an AppImage: {e:#}");
        }
    }
}

/// What Arkor's starting scripts say about themselves.
const MARK: &[u8] = b"unpacked by Arkor so it runs without FUSE";

fn unpack_appimage(appimage: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let parent = appimage.parent().context("no folder")?;
    let name = appimage
        .file_name()
        .context("no name")?
        .to_string_lossy()
        .into_owned();
    // Arkor's script from an earlier unpacking.
    if std::fs::read(appimage).is_ok_and(|b| b.windows(MARK.len()).any(|w| w == MARK)) {
        return Ok(());
    }
    std::fs::set_permissions(appimage, std::fs::Permissions::from_mode(0o755))?;
    let work = parent.join(".unpacking");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let out = std::process::Command::new(appimage)
        .arg("--appimage-extract")
        .current_dir(&work)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .context("starting the AppImage")?;
    let root = work.join("squashfs-root");
    if !out.status.success() || !root.join("AppRun").exists() {
        let _ = std::fs::remove_dir_all(&work);
        bail!(
            "it did not unpack: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let unpacked = parent.join(format!("{name}.unpacked"));
    let _ = std::fs::remove_dir_all(&unpacked);
    std::fs::rename(&root, &unpacked)?;
    let _ = std::fs::remove_dir_all(&work);
    let script = format!(
        "#!/bin/sh\n# {name}, unpacked by Arkor so it runs without FUSE.\nhere=\"$(dirname \"$(readlink -f \"$0\")\")\"\nAPPDIR=\"$here/{name}.unpacked\" exec \"$here/{name}.unpacked/AppRun\" \"$@\"\n"
    );
    std::fs::write(appimage, script)?;
    std::fs::set_permissions(appimage, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// Emulators Arkor downloaded that have a newer build: (id, installed, latest). Blocking
/// (one request each).
pub fn updates() -> Vec<(String, String, String)> {
    let s = discovery::state();
    s.managed
        .keys()
        .filter_map(|id| {
            let have = installed_version(id)?;
            let m = super::engine(id)?;
            let new = latest(&m).ok()?.version;
            (compare_versions(&new, &have).is_gt()).then(|| (id.clone(), have, new))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_lists_matching_assets_and_dates_rolling_builds() {
        let body = r#"[
            {"tag_name": "latest", "published_at": "2025-10-02T00:00:00Z", "assets": [
                {"name": "DuckStation-x64.AppImage", "browser_download_url": "https://x/a", "updated_at": "2025-10-01T08:00:00Z"},
                {"name": "other.zip", "browser_download_url": "https://x/b"}]},
            {"tag_name": "v0.9", "prerelease": true, "assets": [
                {"name": "DuckStation-x64.AppImage", "browser_download_url": "https://x/c"}]}
        ]"#;
        let r = from_github(body, "^DuckStation-x64\\.AppImage$", &[], None).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].version, "2025-10-01");
        assert_eq!(r[0].url, "https://x/a");
    }

    #[test]
    fn buildbot_reads_the_directory_listing() {
        let body = r#"<a href="/stable/1.19.1/">1.19.1/</a> <a href="/stable/1.21.0/">1.21.0/</a> <a href="/stable/junk/">"#;
        let mut r = from_buildbot(
            body,
            "https://buildbot.libretro.com/stable/",
            "^(\\d+\\.\\d+\\.\\d+)/$",
            "{version}/linux/x86_64/RetroArch.7z",
        )
        .unwrap();
        r.sort_by(|a, b| compare_versions(&b.version, &a.version));
        assert_eq!(r.len(), 2);
        assert_eq!(
            r[0].url,
            "https://buildbot.libretro.com/stable/1.21.0/linux/x86_64/RetroArch.7z"
        );
        assert_eq!(r[0].file_name, "RetroArch.7z");
    }

    #[test]
    fn reads_sha256sums() {
        let text = "aa  other.zip\n".to_string() + &"b".repeat(64) + " *DuckStation-x64.AppImage\n";
        assert_eq!(
            parse_sha256sums(&text, "DuckStation-x64.AppImage").unwrap(),
            "b".repeat(64)
        );
        assert!(parse_sha256sums(&text, "missing").is_none());
    }

    #[test]
    fn every_emulator_has_a_way_here_or_names_its_program() {
        for m in super::super::engines().iter() {
            match way(m) {
                Way::Distribution(names) => assert!(!names.is_empty(), "{}", m.id),
                Way::Flatpak(id) => assert!(id.contains('.'), "{}", m.id),
                Way::Download => assert!(!m.binaries.linux.is_empty(), "{}", m.id),
            }
        }
    }

    /// An AppImage-like program: a script answering `--appimage-extract` the way AppImage
    /// runtimes do (a `squashfs-root` with an `AppRun`).
    #[test]
    fn appimages_are_unpacked_and_still_start_by_their_name() {
        use std::os::unix::fs::PermissionsExt;
        let dir = super::super::test_root("appimage");
        let app = dir.join("Emu-x86_64.AppImage");
        std::fs::write(
            &app,
            "#!/bin/sh\nif [ \"$1\" = --appimage-extract ]; then mkdir -p squashfs-root; printf '#!/bin/sh\\necho \"started $APPDIR $*\"\\n' > squashfs-root/AppRun; chmod +x squashfs-root/AppRun; fi\n",
        )
        .unwrap();
        std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
        unpack_appimages(&dir, 0);
        assert!(dir.join("Emu-x86_64.AppImage.unpacked/AppRun").exists());
        let out = std::process::Command::new(&app)
            .arg("game.rom")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(
            said.contains("started") && said.contains("game.rom"),
            "{said}"
        );
        assert!(said.contains("Emu-x86_64.AppImage.unpacked"), "{said}");
        // Again: nothing changes.
        unpack_appimages(&dir, 0);
        assert!(
            std::fs::read_to_string(&app)
                .unwrap()
                .contains("unpacked by Arkor")
        );

        // One that cannot unpack stays as it is.
        let broken = dir.join("Broken.AppImage");
        std::fs::write(&broken, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(unpack_appimage(&broken).is_err());
        assert_eq!(
            std::fs::read_to_string(&broken).unwrap(),
            "#!/bin/sh\nexit 1\n"
        );
    }
}
