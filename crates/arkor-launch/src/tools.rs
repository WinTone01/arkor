//! Host tools Arkor works with: Proton builds, lsfg-vk and Lossless Scaling.

use std::path::{Path, PathBuf};

use arkor_core::{GameId, Store};
use arkor_stores::target::compat_tool_roots;

/// Steam app id of Lossless Scaling, whose DLL lsfg-vk needs.
const LOSSLESS_SCALING_APPID: &str = "993090";

/// Installed Proton builds as (folder name, path), newest first by name.
pub fn installed_protons() -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for root in compat_tool_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Same folder name in two roots: the first root (system packages) wins.
            if path.join("proton").is_file() && !found.iter().any(|(n, _)| n == &name) {
                found.push((name, path));
            }
        }
    }
    found.sort_by_key(|(name, _)| {
        std::cmp::Reverse(crate::start::natural_key(&name.to_lowercase()))
    });
    found
}

/// A profile's `proton` value: an absolute path, or a folder name from `installed_protons`.
pub fn resolve_proton(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute() {
        return path.join("proton").is_file().then(|| path.to_path_buf());
    }
    installed_protons()
        .into_iter()
        .find(|(name, _)| name == value)
        .map(|(_, p)| p)
}

/// Whether the lsfg-vk Vulkan layer is installed (user, system or Flatpak extension).
pub fn lsfg_layer_installed() -> bool {
    const JSON: &str = "VkLayer_LSFGVK_frame_generation.json";
    let mut dirs: Vec<PathBuf> = vec![
        "/usr/share/vulkan/implicit_layer.d".into(),
        "/etc/vulkan/implicit_layer.d".into(),
        "/usr/local/share/vulkan/implicit_layer.d".into(),
    ];
    if let Some(data) = dirs::data_dir() {
        dirs.insert(0, data.join("vulkan/implicit_layer.d"));
    }
    dirs.iter().any(|d| d.join(JSON).is_file())
}

/// `Lossless.dll` from the Steam install of Lossless Scaling, on any Steam library.
pub fn lossless_dll() -> Option<PathBuf> {
    let (games, _) = arkor_stores::steam::installed_games();
    let id = GameId::new(Store::Steam, LOSSLESS_SCALING_APPID);
    let dir = games.into_iter().find(|g| g.id == id)?.install_dir?;
    let dll = dir.join("Lossless.dll");
    dll.is_file().then_some(dll)
}

/// GET `url` as text with curl (present on every Arch install).
pub fn http_get(url: &str) -> anyhow::Result<String> {
    use anyhow::Context;
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "60", "-A", "arkor", url])
        .output()
        .context("running curl")?;
    anyhow::ensure!(
        out.status.success(),
        "request failed: {url}: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Like [`http_get`] with a short timeout, for update checks before a game starts.
pub fn http_get_quick(url: &str) -> anyhow::Result<String> {
    use anyhow::Context;
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "8", "-A", "arkor", url])
        .output()
        .context("running curl")?;
    anyhow::ensure!(
        out.status.success(),
        "request failed: {url}: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Downloads `url` to `dest` (through a `.part` file, so a failed download leaves nothing).
pub fn download(url: &str, dest: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = dest.with_extension("part");
    let status = std::process::Command::new("curl")
        .args(["-fsSL", "-A", "arkor", "-o"])
        .arg(&part)
        .arg(url)
        .status()
        .context("running curl")?;
    if !status.success() {
        let _ = std::fs::remove_file(&part);
        anyhow::bail!("download failed: {url}");
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

/// Anti-cheat runtimes Proton needs for EasyAntiCheat / BattlEye games.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AntiCheat {
    pub eac: Option<PathBuf>,
    pub battleye: Option<PathBuf>,
}

const EAC_APPID: &str = "1826330";
const BATTLEYE_APPID: &str = "1161040";

/// Arkor's own copy of the runtimes (downloaded when Steam's aren't installed).
pub fn components_dir() -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/components"))
}

/// Steam's "Proton EasyAntiCheat/BattlEye Runtime" if installed, else Arkor's download.
pub fn anticheat() -> AntiCheat {
    let ours = components_dir().ok();
    let pick = |appid: &str, sub: &str| {
        arkor_stores::steam::app_dir(appid)
            .or_else(|| ours.as_ref().map(|d| d.join(sub)).filter(|p| p.is_dir()))
    };
    AntiCheat {
        eac: pick(EAC_APPID, "eac"),
        battleye: pick(BATTLEYE_APPID, "be"),
    }
}

/// Downloads both runtimes (Faugus' redistribution of them) into `components_dir`, or
/// updates them when a newer release is out. Blocking.
pub fn download_anticheat() -> anyhow::Result<AntiCheat> {
    crate::components::ensure_anticheat(true)
}

/// A tool Arkor can use, whether it's present, and the Arch package that provides it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolStatus {
    pub name: &'static str,
    /// Version (from pacman) or "installed"; `None` = missing.
    pub found: Option<String>,
    pub package: &'static str,
}

fn in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

fn layer_installed(json: &str) -> bool {
    let mut dirs: Vec<PathBuf> = vec![
        "/usr/share/vulkan/implicit_layer.d".into(),
        "/usr/share/vulkan/explicit_layer.d".into(),
        "/etc/vulkan/implicit_layer.d".into(),
    ];
    if let Some(data) = dirs::data_dir() {
        dirs.push(data.join("vulkan/implicit_layer.d"));
        dirs.push(data.join("vulkan/explicit_layer.d"));
    }
    dirs.iter().any(|d| {
        std::fs::read_dir(d).is_ok_and(|e| {
            e.flatten()
                .any(|f| f.file_name().to_string_lossy().contains(json))
        })
    })
}

fn pacman_version(pkg: &str) -> Option<String> {
    let out = std::process::Command::new("pacman")
        .args(["-Q", pkg])
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .to_string()
    })
}

/// Status of every external tool Arkor integrates with.
pub fn tool_status() -> Vec<ToolStatus> {
    let check = |name: &'static str, present: bool, package: &'static str| ToolStatus {
        name,
        found: present.then(|| pacman_version(package).unwrap_or_else(|| "installed".into())),
        package,
    };
    vec![
        check("umu-launcher", in_path("umu-run"), "umu-launcher"),
        check(
            "MangoHud",
            layer_installed("MangoHud") || in_path("mangohud"),
            "mangohud",
        ),
        check("GameMode", in_path("gamemoderun"), "gamemode"),
        check("gamescope", in_path("gamescope"), "gamescope"),
        check("lsfg-vk", lsfg_layer_installed(), "lsfg-vk"),
        check("vkBasalt", layer_installed("vkBasalt"), "vkbasalt"),
        check(
            "OBS Vulkan capture",
            layer_installed("obs_vkcapture") || layer_installed("obs-vkcapture"),
            "obs-vkcapture",
        ),
        check("winetricks", in_path("winetricks"), "winetricks"),
        check("protontricks", in_path("protontricks"), "protontricks"),
        check(
            "legendary (Epic)",
            in_path("legendary") || arkor_stores::epic::Legendary::locate().is_some(),
            "legendary",
        ),
        check(
            "game-performance (CachyOS)",
            in_path("game-performance"),
            "cachyos-settings",
        ),
        check("7-Zip", in_path("7z"), "7zip"),
        check("bsdtar", in_path("bsdtar"), "libarchive"),
        check("curl", in_path("curl"), "curl"),
    ]
}

/// (label, value) rows describing the machine.
pub fn system_info() -> Vec<(&'static str, String)> {
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    let os = read("/etc/os-release")
        .lines()
        .find_map(|l| {
            l.strip_prefix("PRETTY_NAME=")
                .map(|v| v.trim_matches('"').to_string())
        })
        .unwrap_or_default();
    let kernel = read("/proc/sys/kernel/osrelease").trim().to_string();
    let cpu = read("/proc/cpuinfo")
        .lines()
        .find_map(|l| {
            l.strip_prefix("model name")
                .map(|v| v.trim_start_matches([' ', '\t', ':']).to_string())
        })
        .unwrap_or_default();
    let threads = read("/proc/cpuinfo")
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count();
    let ram_kb: u64 = read("/proc/meminfo")
        .lines()
        .find_map(|l| {
            l.strip_prefix("MemTotal:")
                .and_then(|v| v.split_whitespace().next()?.parse().ok())
        })
        .unwrap_or(0);
    let nvidia = read("/proc/driver/nvidia/version");
    let driver = nvidia
        .lines()
        .next()
        .and_then(|l| {
            l.split_whitespace().find(|w| {
                w.chars().filter(|c| *c == '.').count() >= 1
                    && w.chars().all(|c| c.is_ascii_digit() || c == '.')
            })
        })
        .map(|v| format!("NVIDIA {v}"))
        .or_else(|| pacman_version("mesa").map(|v| format!("Mesa {v}")))
        .unwrap_or_default();
    let gpu = gpu_name().unwrap_or_default();
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let scheduler = read("/sys/kernel/sched_ext/root/ops").trim().to_string();
    vec![
        ("OS", os),
        ("Kernel", kernel),
        ("CPU", format!("{cpu} ({threads} threads)")),
        ("RAM", format!("{:.1} GiB", ram_kb as f64 / 1024.0 / 1024.0)),
        ("GPU", gpu),
        ("Driver", driver),
        ("Session", format!("{desktop} ({session})")),
        (
            "sched_ext",
            if scheduler.is_empty() {
                "-".into()
            } else {
                scheduler
            },
        ),
    ]
}

/// GPU name from `lspci` (VGA/3D controller line).
fn gpu_name() -> Option<String> {
    let out = std::process::Command::new("lspci").output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.contains("VGA compatible controller") || l.contains("3D controller"))
        .and_then(|l| l.split_once(": "))
        .map(|(_, name)| name.trim().to_string())
}

/// GET `url` with an `Authorization: Bearer` header.
pub fn http_get_bearer(url: &str, token: &str) -> anyhow::Result<String> {
    use anyhow::Context;
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "60", "-A", "arkor", "-H"])
        .arg(format!("Authorization: Bearer {token}"))
        .arg(url)
        .output()
        .context("running curl")?;
    anyhow::ensure!(
        out.status.success(),
        "request failed: {url}: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Splits `packages` into those in the configured pacman repositories and the rest (AUR or
/// not packaged for this distribution).
pub fn split_repo_packages<'a>(packages: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
    packages.iter().partition(|p| {
        std::process::Command::new("pacman")
            .args(["-Si", "--", p])
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

/// Installs repository packages with pacman through pkexec (the desktop's polkit agent asks
/// for the password). Blocking.
pub fn install_packages(packages: &[&str]) -> anyhow::Result<()> {
    use anyhow::Context;
    anyhow::ensure!(!packages.is_empty(), "nothing to install");
    let out = std::process::Command::new("pkexec")
        .args(["pacman", "-S", "--needed", "--noconfirm", "--"])
        .args(packages)
        .output()
        .context("running pkexec (is polkit installed?)")?;
    anyhow::ensure!(
        out.status.success(),
        "pacman failed: {}",
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .chain(String::from_utf8_lossy(&out.stdout).lines())
            .filter(|l| l.contains("error"))
            .collect::<Vec<_>>()
            .join("; ")
    );
    Ok(())
}
