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

/// The Steam install folder of Lossless Scaling, on any Steam library.
pub fn lossless_scaling_dir() -> Option<PathBuf> {
    let (games, _) = arkor_stores::steam::installed_games();
    let id = GameId::new(Store::Steam, LOSSLESS_SCALING_APPID);
    games.into_iter().find(|g| g.id == id)?.install_dir
}

/// `Lossless.dll` from the Steam install of Lossless Scaling (MAKO and lsfg-vk 1.x use it).
pub fn lossless_dll() -> Option<PathBuf> {
    let dll = lossless_scaling_dir()?.join("Lossless.dll");
    dll.is_file().then_some(dll)
}

/// The DLL to give lsfg-vk: `lsfg-vk.dll` (its shaders, shipped by Lossless Scaling's
/// "lsfg-vk" beta branch on Steam, which lsfg-vk 2.x needs), else `Lossless.dll`.
pub fn lsfg_dll() -> Option<PathBuf> {
    let dir = lossless_scaling_dir()?;
    ["lsfg-vk.dll", "Lossless.dll"]
        .into_iter()
        .map(|f| dir.join(f))
        .find(|p| p.is_file())
}

/// An installed lsfg-vk Vulkan layer: its library, where it was installed and its version.
#[derive(Debug, Clone, PartialEq)]
pub struct LsfgLayer {
    pub library: PathBuf,
    /// Installed in the user's folder (e.g. by lsfg-vk's install script), not by pacman.
    pub user: bool,
    /// Package version from pacman, or the layer's own implementation version.
    pub version: Option<String>,
}

pub fn lsfg_layer() -> Option<LsfgLayer> {
    const JSON: &str = "VkLayer_LSFGVK_frame_generation.json";
    let mut dirs: Vec<(PathBuf, bool)> = vec![
        ("/usr/share/vulkan/implicit_layer.d".into(), false),
        ("/etc/vulkan/implicit_layer.d".into(), false),
        ("/usr/local/share/vulkan/implicit_layer.d".into(), false),
    ];
    if let Some(data) = dirs::data_dir() {
        dirs.insert(0, (data.join("vulkan/implicit_layer.d"), true));
    }
    let (dir, user) = dirs.into_iter().find(|(d, _)| d.join(JSON).is_file())?;
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(JSON)).ok()?).ok()?;
    let lib = json["layer"]["library_path"].as_str().unwrap_or_default();
    let library = if lib.starts_with('/') {
        PathBuf::from(lib)
    } else {
        dir.join(lib)
    };
    let library = std::fs::canonicalize(&library).unwrap_or(library);
    let version = std::process::Command::new("pacman")
        .arg("-Qqo")
        .arg(&library)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| pacman_version(String::from_utf8_lossy(&o.stdout).trim()))
        .or_else(|| {
            json["layer"]["implementation_version"]
                .as_str()
                .map(|v| format!("{v}.x"))
        });
    Some(LsfgLayer {
        library,
        user,
        version,
    })
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

/// Size of `url` from its headers (after redirects), if the server says.
fn content_length(url: &str) -> Option<u64> {
    let out = std::process::Command::new("curl")
        .args(["-sIL", "--max-time", "20", "-A", "arkor", url])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<u64>().ok())?
        })
        .next_back()
}

/// The error a download returns when it was stopped (paused or cancelled).
pub const STOPPED: &str = "download stopped";

/// Downloads `url` to `dest` through a `.part` file. An earlier `.part` (a paused or broken
/// download) is continued where it ended. Progress goes to `progress` (the thread's job, or
/// an entry of its own); `progress::stop` ends it with `STOPPED`, keeping the `.part`.
pub fn download(url: &str, dest: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = dest.with_extension("part");
    let label = url
        .rsplit('/')
        .next()
        .unwrap_or(url)
        .split('?')
        .next()
        .unwrap_or(url)
        .replace("%20", " ");
    let transfer = crate::progress::Transfer::begin(&label, content_length(url));
    for attempt in 0..2 {
        let resume = attempt == 0 && part.is_file();
        let mut cmd = std::process::Command::new("curl");
        cmd.args(["-fsSL", "-A", "arkor", "-o"]).arg(&part);
        if resume {
            cmd.args(["-C", "-"]);
        } else {
            let _ = std::fs::remove_file(&part);
        }
        let mut child = cmd.arg(url).spawn().context("running curl")?;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if !transfer.update(std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)) {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!(STOPPED);
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        };
        if status.success() {
            transfer.update(std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0));
            std::fs::rename(&part, dest)?;
            return Ok(());
        }
        // A server that cannot continue: start over once.
        if !resume {
            break;
        }
    }
    let _ = std::fs::remove_file(&part);
    anyhow::bail!("download failed: {url}")
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

/// What pacman would download for `packages` (dependencies included): the start of each
/// package file's name (`name-version-`) and its size in bytes.
fn pacman_targets(packages: &[&str]) -> Vec<(String, u64)> {
    let Ok(out) = std::process::Command::new("pacman")
        .args(["-Sp", "--needed", "--print-format", "%n %v %s", "--"])
        .args(packages)
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let (name, version, size) = (w.next()?, w.next()?, w.next()?.parse().ok()?);
            Some((format!("{name}-{version}-"), size))
        })
        .collect()
}

const PACMAN_CACHE: &str = "/var/cache/pacman/pkg";

/// Bytes of `targets` already in pacman's cache (finished files and partial downloads,
/// also in the `download-*` folders pacman downloads into), each capped at its size.
fn pacman_cached(targets: &[(String, u64)]) -> u64 {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(PACMAN_CACHE) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("download-") {
                if let Ok(inner) = std::fs::read_dir(e.path()) {
                    files.extend(inner.flatten());
                }
            } else {
                files.push(e);
            }
        }
    }
    targets
        .iter()
        .map(|(prefix, size)| {
            let have = files
                .iter()
                .filter(|f| {
                    let n = f.file_name().to_string_lossy().into_owned();
                    n.starts_with(prefix.as_str()) && !n.ends_with(".sig")
                })
                .filter_map(|f| f.metadata().ok().map(|m| m.len()))
                .max()
                .unwrap_or(0);
            have.min(*size)
        })
        .sum()
}

/// Installs repository packages with pacman through pkexec (the desktop's polkit agent asks
/// for the password). The download is reported to `progress` from pacman's cache. Blocking.
pub fn install_packages(packages: &[&str]) -> anyhow::Result<()> {
    use anyhow::Context;
    anyhow::ensure!(!packages.is_empty(), "nothing to install");
    let targets = pacman_targets(packages);
    let already = pacman_cached(&targets);
    let total = targets
        .iter()
        .map(|(_, s)| s)
        .sum::<u64>()
        .saturating_sub(already);
    let log = std::env::temp_dir().join(format!("arkor-pacman-{}.log", std::process::id()));
    let out = std::fs::File::create(&log).context("creating the pacman log")?;
    let mut child = std::process::Command::new("pkexec")
        .args(["pacman", "-S", "--needed", "--noconfirm", "--"])
        .args(packages)
        .stdin(std::process::Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out)
        .spawn()
        .context("running pkexec (is polkit installed?)")?;
    let mut transfer = (total > 0).then(|| crate::progress::Transfer::begin("pacman", Some(total)));
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if let Some(t) = &transfer {
            let done = pacman_cached(&targets).saturating_sub(already);
            t.update(done.min(total));
            // Downloaded: pacman is installing now.
            if done >= total {
                transfer = None;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    };
    drop(transfer);
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    anyhow::ensure!(
        status.success(),
        "pacman failed: {}",
        text.lines()
            .filter(|l| l.contains("error"))
            .collect::<Vec<_>>()
            .join("; ")
    );
    Ok(())
}

/// An AUR helper that is installed (`paru`, `yay`…).
pub fn aur_helper() -> Option<&'static str> {
    ["paru", "yay", "pikaur", "trizen"]
        .into_iter()
        .find(|h| which(h).is_some())
}

fn which(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(name))
            .find(|f| f.is_file())
    })
}

/// A terminal that runs `bash -c script` and stays in the foreground until it ends.
fn terminal_command(script: &str) -> Option<Vec<String>> {
    let run = |prefix: &[&str]| -> Vec<String> {
        prefix
            .iter()
            .map(|s| s.to_string())
            .chain(["bash".into(), "-c".into(), script.to_string()])
            .collect()
    };
    if let Some(t) = std::env::var("TERMINAL")
        .ok()
        .filter(|t| which(t).is_some())
    {
        return Some(run(&[t.as_str(), "-e"]));
    }
    let known: [(&str, &[&str]); 9] = [
        ("konsole", &["konsole", "--nofork", "-e"]),
        ("kitty", &["kitty"]),
        ("alacritty", &["alacritty", "-e"]),
        ("foot", &["foot"]),
        ("ghostty", &["ghostty", "-e"]),
        ("wezterm", &["wezterm", "start", "--"]),
        ("gnome-terminal", &["gnome-terminal", "--wait", "--"]),
        (
            "xfce4-terminal",
            &["xfce4-terminal", "--disable-server", "-x"],
        ),
        ("xterm", &["xterm", "-e"]),
    ];
    known
        .iter()
        .find(|(bin, _)| which(bin).is_some())
        .map(|(_, prefix)| run(prefix))
}

/// The command that installs AUR `packages` with the helper (shown when no terminal is found).
pub fn aur_command(packages: &[&str]) -> Option<String> {
    Some(format!(
        "{} -S --needed {}",
        aur_helper()?,
        packages.join(" ")
    ))
}

/// Installs AUR packages with the AUR helper in a terminal window (building AUR packages
/// asks the user to review and confirm), waiting until the window closes. Blocking.
pub fn install_aur(packages: &[&str]) -> anyhow::Result<()> {
    anyhow::ensure!(!packages.is_empty(), "nothing to install");
    let command = aur_command(packages)
        .ok_or_else(|| anyhow::anyhow!("no AUR helper found (install paru or yay)"))?;
    let script = format!(
        "echo 'Arkor: {command}'; echo; {command}; echo; read -n 1 -s -r -p 'Press any key to close…'"
    );
    let argv = terminal_command(&script)
        .ok_or_else(|| anyhow::anyhow!("no terminal found; run: {command}"))?;
    let _ = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .status();
    let missing: Vec<&str> = packages
        .iter()
        .copied()
        .filter(|p| {
            !std::process::Command::new("pacman")
                .args(["-Q", "--", p])
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .collect();
    anyhow::ensure!(missing.is_empty(), "not installed: {}", missing.join(", "));
    Ok(())
}

/// GPU makers present, from the kernel's DRM devices (PCI vendor ids).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gpus {
    pub amd: bool,
    pub nvidia: bool,
    pub intel: bool,
}

pub fn gpus() -> Gpus {
    let mut g = Gpus::default();
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for e in entries.flatten() {
            match std::fs::read_to_string(e.path().join("device/vendor"))
                .map(|v| v.trim().to_string())
                .as_deref()
            {
                Ok("0x1002") => g.amd = true,
                Ok("0x10de") => g.nvidia = true,
                Ok("0x8086") => g.intel = true,
                _ => {}
            }
        }
    }
    g
}

#[cfg(test)]
mod download_tests {
    #[test]
    fn continues_a_partial_download() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("source.bin");
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&src, &data).unwrap();
        let dest = tmp.path().join("out/file.bin");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        // A paused download left the first half.
        std::fs::write(dest.with_extension("part"), &data[..100_000]).unwrap();
        super::download(&format!("file://{}", src.display()), &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(!dest.with_extension("part").exists());
    }
}
