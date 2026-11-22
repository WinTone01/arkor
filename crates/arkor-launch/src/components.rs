//! Components that update themselves before a game starts, the way Faugus Launcher does it:
//!
//! - `umu-run`: Faugus' build of umu-launcher (github.com/Faugus/umu-launcher), kept in
//!   Arkor's data folder. The system's `umu-run` is used when it cannot be downloaded.
//! - The EasyAntiCheat and BattlEye runtimes (github.com/Faugus/components).
//! - "Latest" Proton builds: one fixed folder per build in Steam's `compatibilitytools.d`
//!   (`Proton-GE Latest`, `Proton-CachyOS Latest`, …), replaced in place when a new release
//!   is out. The folder names are Faugus', so both launchers share the same copy.
//!
//! Every check is a quick request to the release API; nothing is downloaded when the
//! installed version is current. A failed check never stops a game from starting.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use crate::proton_dl::{self, Variant};
use crate::tools;

const UMU_API: &str = "https://api.github.com/repos/Faugus/umu-launcher/releases";
const UMU_URL: &str = "https://github.com/Faugus/umu-launcher/releases/download/{}/umu-run";
const COMPONENTS_API: &str = "https://api.github.com/repos/Faugus/components/releases/latest";

/// A Proton build that is always kept at its newest release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Latest {
    Ge,
    CachyOs,
    Dw,
    Wineland,
}

impl Latest {
    pub const ALL: [Latest; 4] = [Latest::CachyOs, Latest::Ge, Latest::Dw, Latest::Wineland];

    /// Folder name in `compatibilitytools.d`, also the profile's `proton` value and the
    /// name Steam shows.
    pub fn dir_name(self) -> &'static str {
        match self {
            Latest::Ge => "Proton-GE Latest",
            Latest::CachyOs => "Proton-CachyOS Latest",
            Latest::Dw => "DW-Proton Latest",
            Latest::Wineland => "Proton-Wineland Latest",
        }
    }

    pub fn variant(self) -> Variant {
        match self {
            Latest::Ge => Variant::Ge,
            Latest::CachyOs => Variant::CachyOs,
            Latest::Dw => Variant::Dw,
            Latest::Wineland => Variant::Wineland,
        }
    }

    fn api(self) -> &'static str {
        match self {
            Latest::Ge => {
                "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases/latest"
            }
            Latest::CachyOs => {
                "https://api.github.com/repos/CachyOS/proton-cachyos/releases/latest"
            }
            Latest::Dw => "https://dawn.wine/api/v1/repos/dawn-winery/dwproton/releases/latest",
            Latest::Wineland => {
                "https://api.github.com/repos/nanomatters/proton-cachyos/releases/latest"
            }
        }
    }

    /// The build a profile value names (`"Proton-GE Latest"`; the older umu token
    /// `"GE-Proton"` means the GE one too).
    pub fn from_value(value: &str) -> Option<Latest> {
        if value == crate::start::GE_LATEST {
            return Some(Latest::Ge);
        }
        Latest::ALL.into_iter().find(|l| l.dir_name() == value)
    }

    pub fn path(self) -> anyhow::Result<PathBuf> {
        Ok(proton_dl::install_root()?.join(self.dir_name()))
    }

    /// The installed release, from the folder's `version` file (`<timestamp> <tag>`).
    pub fn installed_version(self) -> Option<String> {
        let path = self.path().ok()?;
        if !path.join("proton").is_file() {
            return None;
        }
        let text = std::fs::read_to_string(path.join("version")).ok()?;
        let mut parts = text.split_whitespace();
        let first = parts.next()?;
        Some(parts.next().unwrap_or(first).to_string())
    }
}

/// Makes the release tags and `version` files of one build comparable (Faugus' rules).
fn normalize_version(v: &str) -> String {
    v.trim_start_matches('v')
        .replace("GE-Proton", "")
        .replace("cachyos-wineland-", "")
        .replace("wineland-", "")
        .replace("cachyos-", "")
        .replace("dwproton-", "")
        .replace("DW-Proton-", "")
        .trim_end_matches('+')
        .trim_matches('-')
        .to_string()
}

/// Installs or updates a "Latest" build; returns its folder. With `update` false an
/// installed copy is used as it is (no request). Blocking.
pub fn ensure_latest(latest: Latest, update: bool) -> anyhow::Result<PathBuf> {
    let dest = latest.path()?;
    let installed = latest.installed_version();
    if installed.is_some() && !update {
        return Ok(dest);
    }
    let json = tools::http_get_quick(latest.api())?;
    // The list parser picks the asset for this CPU and its checksum.
    let release = proton_dl::parse_list(latest.variant(), &format!("[{json}]"))?
        .into_iter()
        .next()
        .with_context(|| format!("no {} release found", latest.dir_name()))?;
    if installed
        .as_deref()
        .is_some_and(|v| normalize_version(v) == normalize_version(&release.tag))
    {
        return Ok(dest);
    }
    eprintln!("arkor: installing {} ({})", latest.dir_name(), release.tag);
    proton_dl::install_into(&release, &dest)?;
    write_compat_vdf(&dest, latest.dir_name())?;
    Ok(dest)
}

/// Steam lists a compatibility tool by the name in its `compatibilitytool.vdf`.
fn write_compat_vdf(dir: &Path, name: &str) -> anyhow::Result<()> {
    std::fs::write(
        dir.join("compatibilitytool.vdf"),
        format!(
            "\"compatibilitytools\"\n{{\n  \"compat_tools\"\n  {{\n    \"{name}\"\n    {{\n      \"install_path\" \".\"\n      \"display_name\" \"{name}\"\n      \"from_oslist\"  \"windows\"\n      \"to_oslist\"    \"linux\"\n    }}\n  }}\n}}\n"
        ),
    )?;
    Ok(())
}

fn data_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir().context("no XDG data dir")?.join("arkor"))
}

/// Arkor's copy of Faugus' umu-run (may not exist yet).
pub fn umu_run_path() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("umu-run"))
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Version of Arkor's umu-run, if downloaded.
pub fn umu_version() -> Option<String> {
    let dir = data_dir().ok()?;
    umu_run_path()
        .ok()
        .filter(|p| p.is_file())
        .and_then(|_| read_trimmed(&dir.join("umu-version.txt")))
}

/// Downloads umu-run when missing or (with `update`) when a newer release is out. Blocking.
pub fn ensure_umu(update: bool) -> anyhow::Result<PathBuf> {
    #[derive(Deserialize)]
    struct Rel {
        tag_name: String,
    }
    let path = umu_run_path()?;
    let version_file = data_dir()?.join("umu-version.txt");
    let current = umu_version();
    if current.is_some() && !update {
        return Ok(path);
    }
    let releases: Vec<Rel> = serde_json::from_str(&tools::http_get_quick(UMU_API)?)
        .context("unexpected umu-launcher release list")?;
    let latest = releases
        .into_iter()
        .next()
        .context("no umu-launcher release")?
        .tag_name;
    if current.as_deref() == Some(latest.as_str()) {
        return Ok(path);
    }
    eprintln!("arkor: updating umu-launcher to {latest}");
    tools::download(&UMU_URL.replace("{}", &latest), &path)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(version_file, &latest)?;
    Ok(path)
}

/// Version of the downloaded anti-cheat runtimes, if any.
pub fn anticheat_version() -> Option<String> {
    let dir = tools::components_dir().ok()?;
    (dir.join("eac").is_dir() && dir.join("be").is_dir())
        .then(|| read_trimmed(&dir.join("version.txt")))
        .flatten()
}

/// Downloads EasyAntiCheat and BattlEye when missing or (with `update`) outdated. Blocking.
pub fn ensure_anticheat(update: bool) -> anyhow::Result<tools::AntiCheat> {
    #[derive(Deserialize)]
    struct Rel {
        tag_name: String,
        assets: Vec<Asset>,
    }
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }
    let current = anticheat_version();
    if current.is_some() && !update {
        return Ok(tools::anticheat());
    }
    let rel: Rel = serde_json::from_str(&tools::http_get_quick(COMPONENTS_API)?)
        .context("unexpected components release")?;
    if current.as_deref() == Some(rel.tag_name.as_str()) {
        return Ok(tools::anticheat());
    }
    eprintln!(
        "arkor: updating EasyAntiCheat and BattlEye ({})",
        rel.tag_name
    );
    let dir = tools::components_dir()?;
    // Download into a fresh folder next to the old one, so a failure keeps the old runtimes.
    let fresh = dir.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(&fresh)?;
    for name in ["eac.tar.gz", "be.tar.gz"] {
        let asset = rel
            .assets
            .iter()
            .find(|a| a.name == name)
            .with_context(|| format!("{name} missing in release"))?;
        let archive = fresh.join(name);
        tools::download(&asset.browser_download_url, &archive)?;
        let status = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(&fresh)
            .status()?;
        let _ = std::fs::remove_file(&archive);
        anyhow::ensure!(status.success(), "could not unpack {name}");
    }
    std::fs::write(fresh.join("version.txt"), &rel.tag_name)?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&fresh, &dir)?;
    Ok(tools::anticheat())
}

/// The Steam Linux Runtime umu runs games in (Faugus' default too).
pub const RUNTIME: &str = "steamrt4";

/// The installed Steam Linux Runtime's version (its depot version), if any.
pub fn runtime_version() -> Option<String> {
    let dir = dirs::data_dir()?.join("umu").join(RUNTIME);
    std::fs::read_to_string(dir.join("VERSIONS.txt"))
        .ok()?
        .lines()
        .find_map(|l| {
            l.strip_prefix("depot")
                .map(|v| v.split_whitespace().next().unwrap_or("").to_string())
        })
        .filter(|v| !v.is_empty())
}

/// Downloads (or with `update` refreshes) the Steam Linux Runtime, the container umu starts
/// every game in, so the first game does not wait for it. umu-run does the download, as
/// it would before a game. Blocking.
pub fn ensure_runtime(update: bool) -> anyhow::Result<()> {
    if runtime_version().is_some() && !update {
        return Ok(());
    }
    let umu = crate::start::umu_run()?;
    let out = std::process::Command::new(umu)
        .arg("/usr/bin/true")
        .env("PROTONPATH", format!("umu-{RUNTIME}"))
        .env("GAMEID", "umu-arkor-runtime")
        .env("UMU_NO_PROTON", "1")
        .env("UMU_RUNTIME_UPDATE", if update { "" } else { "0" })
        .stdin(std::process::Stdio::null())
        .output()
        .context("running umu-run")?;
    anyhow::ensure!(
        runtime_version().is_some(),
        "the Steam Linux Runtime could not be installed: {}",
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .last()
            .unwrap_or("")
    );
    Ok(())
}

/// Before a game that runs through umu: refresh umu-run and the anti-cheat runtimes, and
/// the game's "Latest" Proton if it uses one. `update` false (automatic updates off) only
/// downloads what is missing. Problems are printed, never fatal: the game still starts.
pub fn before_launch(update: bool, latest: Option<Latest>) {
    if let Err(e) = ensure_umu(update) {
        eprintln!("arkor: umu-launcher not updated: {e:#}");
    }
    if let Err(e) = ensure_anticheat(update) {
        eprintln!("arkor: anti-cheat runtimes not updated: {e:#}");
    }
    // umu updates the runtime itself when the game starts; this only fills in a missing one.
    if let Err(e) = ensure_runtime(false) {
        eprintln!("arkor: Steam Linux Runtime not installed: {e:#}");
    }
    if let Some(latest) = latest
        && let Err(e) = ensure_latest(latest, update)
    {
        eprintln!("arkor: {} not updated: {e:#}", latest.dir_name());
    }
}

/// Whether any Proton build is installed (a "Latest" one counts).
pub fn any_proton() -> bool {
    !tools::installed_protons().is_empty()
}

/// The build installed when there is no Proton at all: the default from Settings if it is
/// a "Latest" one, else Proton-CachyOS Latest (Faugus' default).
pub fn first_proton(default: Option<&str>) -> Latest {
    default
        .and_then(Latest::from_value)
        .unwrap_or(Latest::CachyOs)
}

/// What every Windows game needs, checked when Arkor starts: umu-launcher, the anti-cheat
/// runtimes and at least one Proton build. `step` is told what is being done. Blocking.
pub fn ensure_required(
    update: bool,
    default_proton: Option<&str>,
    step: &dyn Fn(&str),
) -> anyhow::Result<()> {
    step("umu-launcher");
    ensure_umu(update)?;
    step("EasyAntiCheat & BattlEye");
    ensure_anticheat(update)?;
    step("Steam Linux Runtime");
    ensure_runtime(update)?;
    if !any_proton() {
        let latest = first_proton(default_proton);
        step(latest.dir_name());
        ensure_latest(latest, true)?;
    }
    Ok(())
}

/// umu's GAMEID for a library game without its own umu id: stable per game, so each game
/// gets its own window class (`steam_app_…`), like Faugus' `umu-faugus<crc32>`.
pub fn umu_game_id(game_id: &str) -> String {
    format!("umu-arkor{:08x}", crc32(game_id.as_bytes()))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_across_tag_styles() {
        assert_eq!(normalize_version("GE-Proton10-25"), "10-25");
        assert_eq!(
            normalize_version("cachyos-10.0-20260101-slr"),
            normalize_version("proton-cachyos-10.0-20260101-slr".trim_start_matches("proton-"))
        );
        assert_eq!(normalize_version("v1.2+"), "1.2");
    }

    #[test]
    fn crc32_matches_zlib() {
        // The standard CRC-32 check value (same as Python's zlib.crc32, which Faugus uses).
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(umu_game_id("x").len(), "umu-arkor".len() + 8);
    }

    #[test]
    fn profile_values_name_builds() {
        assert_eq!(
            Latest::from_value("Proton-CachyOS Latest"),
            Some(Latest::CachyOs)
        );
        assert_eq!(Latest::from_value("GE-Proton"), Some(Latest::Ge));
        assert_eq!(Latest::from_value("GE-Proton10-25"), None);
    }
}

/// Asks the real release APIs (no downloads). `cargo test -p arkor-launch -- --ignored`.
#[cfg(test)]
mod network {
    use super::*;

    #[test]
    #[ignore = "needs the network"]
    fn latest_releases_resolve() {
        for l in Latest::ALL {
            let json = tools::http_get_quick(l.api()).unwrap();
            let r = proton_dl::parse_list(l.variant(), &format!("[{json}]")).unwrap();
            let r = r
                .first()
                .unwrap_or_else(|| panic!("{l:?}: no usable asset"));
            println!(
                "{}: {} -> {} (checksum: {})",
                l.dir_name(),
                r.tag,
                r.url,
                r.checksum_url.is_some()
            );
        }
        let umu = tools::http_get_quick(UMU_API).unwrap();
        assert!(umu.contains("tag_name"));
        let comp = tools::http_get_quick(COMPONENTS_API).unwrap();
        assert!(comp.contains("eac.tar.gz") && comp.contains("be.tar.gz"));
    }
}

#[cfg(test)]
mod network_install {
    use super::*;

    /// Downloads umu-run and the anti-cheat runtimes into `$XDG_DATA_HOME` twice: the
    /// second run must find them current. Run with a scratch XDG_DATA_HOME.
    #[test]
    #[ignore = "needs the network; downloads into XDG_DATA_HOME"]
    fn umu_and_anticheat_install_then_stay() {
        let umu = ensure_umu(true).unwrap();
        assert!(umu.is_file());
        let v = umu_version().unwrap();
        let ac = ensure_anticheat(true).unwrap();
        let av = anticheat_version().unwrap();
        println!("umu {v} at {}, anti-cheat {av}: {ac:?}", umu.display());
        let modified = std::fs::metadata(&umu).unwrap().modified().unwrap();
        ensure_umu(true).unwrap();
        ensure_anticheat(true).unwrap();
        assert_eq!(
            std::fs::metadata(&umu).unwrap().modified().unwrap(),
            modified
        );
    }
}
