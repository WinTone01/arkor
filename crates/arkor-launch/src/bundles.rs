//! AMD upscaler bundles that come with their own Linux installers: HelixSR (HLX DLSS and
//! HLX FSR 4 on top of OptiScaler) and DLSSNR-AMD (DLSS 5's neural rendering model in Vulkan).
//! Arkor downloads them and drives their installers instead of copying their files itself.
//! HelixSR's setup builds its networks on this PC from NVIDIA's and AMD's files after the
//! user agrees to their terms, so it runs in a terminal where those questions can be answered.
//!
//! DLSSNR-AMD installs by one of four routes: through OptiScaler (games with a DLSS, FSR or
//! XeSS option, 64-bit only) or through ReShade (DirectX 10-12 games without an upscaler,
//! Vulkan and DirectX 9 games, old DirectX 9 games). 32-bit games take the i686 package.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use arkor_core::GameId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Bundle {
    HelixSr,
    /// DLSSNR-AMD for RX 9000 (RDNA4), from its GitLab project.
    DlssNrRdna4,
    /// The RDNA3 (RX 7000) port.
    DlssNrRdna3,
    /// DLSSNR-AMD's i686 package, for 32-bit games (ReShade routes only).
    DlssNrRdna4X32,
}

impl Bundle {
    pub const ALL: [Bundle; 4] = [
        Bundle::HelixSr,
        Bundle::DlssNrRdna4,
        Bundle::DlssNrRdna3,
        Bundle::DlssNrRdna4X32,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Bundle::HelixSr => "HelixSR",
            Bundle::DlssNrRdna4 => "DLSS-NR (RDNA4)",
            Bundle::DlssNrRdna3 => "DLSS-NR (RDNA3)",
            Bundle::DlssNrRdna4X32 => "DLSS-NR (RDNA4, 32-bit)",
        }
    }

    /// Id for progress and download lists.
    pub fn key(self) -> &'static str {
        self.dir()
    }

    fn dir(self) -> &'static str {
        match self {
            Bundle::HelixSr => "helixsr",
            Bundle::DlssNrRdna4 => "dlssnr-rdna4",
            Bundle::DlssNrRdna3 => "dlssnr-rdna3",
            Bundle::DlssNrRdna4X32 => "dlssnr-rdna4-i686",
        }
    }

    /// The networks its installer takes (the RDNA3 port still has the older names).
    pub fn models(self) -> &'static [&'static str] {
        match self {
            Bundle::DlssNrRdna3 => &[
                "CC_SMALL_FP8",
                "CC_SMALL_FP8_EXPERIMENTAL",
                "CC_SILVER_AARDWOLD",
                "CC_SILVER_AARDWOLD_OPTIMIZED",
            ],
            _ => &DLSSNR_MODELS,
        }
    }

    /// Whether it is a package for 32-bit games.
    pub fn is_32bit(self) -> bool {
        self == Bundle::DlssNrRdna4X32
    }

    /// File inside the unpacked bundle that marks it complete.
    fn marker(self) -> &'static str {
        match self {
            Bundle::HelixSr => "helixsr-setup.sh",
            _ => "install.sh",
        }
    }
}

fn root(b: Bundle) -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/tools")
        .join(b.dir()))
}

/// (version, download URL) of the newest release.
pub fn latest(b: Bundle) -> anyhow::Result<(String, String)> {
    let pick = |json: &str,
                tag_key: &str,
                assets: &dyn Fn(&serde_json::Value) -> Vec<(String, String)>,
                want: &dyn Fn(&str) -> bool|
     -> anyhow::Result<(String, String)> {
        let list: Vec<serde_json::Value> =
            serde_json::from_str(json).context("unexpected release response")?;
        list.iter()
            .find_map(|r| {
                let tag = r[tag_key].as_str()?.to_string();
                assets(r)
                    .into_iter()
                    .find(|(name, _)| want(name))
                    .map(|(_, url)| (tag, url))
            })
            .context("no matching release found")
    };
    let github = |r: &serde_json::Value| -> Vec<(String, String)> {
        r["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| {
                Some((
                    a["name"].as_str()?.to_string(),
                    a["browser_download_url"].as_str()?.to_string(),
                ))
            })
            .collect()
    };
    match b {
        Bundle::HelixSr => {
            let json = crate::tools::http_get(
                "https://api.github.com/repos/lonewolf0622/HelixSR/releases?per_page=10",
            )?;
            pick(&json, "tag_name", &github, &|n| {
                n.starts_with("HelixSR-") && n.ends_with(".zip")
            })
        }
        Bundle::DlssNrRdna3 => {
            let json = crate::tools::http_get(
                "https://api.github.com/repos/saaaaaaaaaaaaaaaaaas/DLSSNR-RDNA3/releases?per_page=10",
            )?;
            pick(&json, "tag_name", &github, &|n| {
                n.contains("x86_64") && n.ends_with(".tar.gz")
            })
        }
        Bundle::DlssNrRdna4 | Bundle::DlssNrRdna4X32 => {
            let arch = if b == Bundle::DlssNrRdna4X32 {
                "i686"
            } else {
                "x86_64"
            };
            let json = crate::tools::http_get(
                "https://gitlab.com/api/v4/projects/mochizuki0323%2FDLSSNR-AMD/releases?per_page=10",
            )?;
            let gitlab = |r: &serde_json::Value| -> Vec<(String, String)> {
                r["assets"]["links"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|a| {
                        Some((
                            a["name"].as_str()?.to_string(),
                            a["url"].as_str()?.to_string(),
                        ))
                    })
                    .collect()
            };
            pick(&json, "tag_name", &gitlab, &|n| {
                n.contains("Linux") && n.contains(arch) && n.ends_with(".tar.gz")
            })
        }
    }
}

/// Downloaded copies as (version, unpacked folder), newest first.
pub fn downloaded(b: Bundle) -> Vec<(String, PathBuf)> {
    let Ok(entries) = root(b).and_then(|r| Ok(std::fs::read_dir(r)?)) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            Some((
                e.file_name().to_string_lossy().into_owned(),
                find_marker(&e.path(), b.marker(), 2)?,
            ))
        })
        .collect();
    out.sort_by_key(|(v, _)| std::cmp::Reverse(crate::start::natural_key(v)));
    out
}

/// Folder holding `marker`, at most `depth` levels down (archives may wrap a top folder).
fn find_marker(dir: &Path, marker: &str, depth: usize) -> Option<PathBuf> {
    if dir.join(marker).is_file() {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .find_map(|e| find_marker(&e.path(), marker, depth - 1))
}

/// Downloads and unpacks the newest release (a few hundred MB). Blocking.
pub fn download(b: Bundle) -> anyhow::Result<(String, PathBuf)> {
    let (version, url) = latest(b)?;
    if let Some(found) = downloaded(b).into_iter().find(|(v, _)| *v == version) {
        return Ok(found);
    }
    let dest = root(b)?.join(&version);
    let staging = root(b)?.join(format!(".{version}.partial"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let archive = staging.join("bundle.archive");
    crate::tools::download(&url, &archive)?;
    let status = Command::new("bsdtar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .status()
        .context("running bsdtar")?;
    let _ = std::fs::remove_file(&archive);
    anyhow::ensure!(status.success(), "could not unpack {}", b.name());
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&staging, &dest)?;
    let dir = find_marker(&dest, b.marker(), 2)
        .with_context(|| format!("{} archive has no {}", b.name(), b.marker()))?;
    Ok((version, dir))
}

/// Runs `script args…` with bash in a terminal window (kept open until Enter is pressed), so
/// the user can read and answer what it asks.
fn run_in_terminal(script: &Path, args: &[&Path]) -> anyhow::Result<()> {
    let dir = quote(
        &script
            .parent()
            .unwrap_or(Path::new("/"))
            .display()
            .to_string(),
    );
    let mut line = format!("cd {dir} && bash {}", quote(&script.display().to_string()));
    for a in args {
        line.push(' ');
        line.push_str(&quote(&a.display().to_string()));
    }
    line.push_str("; echo; read -r -p 'Press Enter to close this window… ' _");
    let argv = crate::tools::terminal_command(&line)
        .context("no terminal emulator found (install konsole, ptyxis or xterm)")?;
    Command::new(&argv[0])
        .args(&argv[1..])
        .spawn()
        .with_context(|| format!("starting {}", argv[0]))?;
    Ok(())
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Opens HelixSR's setup for `game_dir` in a terminal. It builds HelixSR the first time
/// (asking before it downloads NVIDIA's and AMD's files) and then installs it into the game.
pub fn helixsr_setup(bundle: &Path, game_dir: &Path) -> anyhow::Result<()> {
    run_in_terminal(&bundle.join("helixsr-setup.sh"), &[game_dir])
}

/// Opens HelixSR's uninstaller for `game_dir` in a terminal (it puts the game's files back).
pub fn helixsr_remove(bundle: &Path, game_dir: &Path) -> anyhow::Result<()> {
    run_in_terminal(&bundle.join("helixsr-uninstall.sh"), &[game_dir])
}

/// Whether HelixSR is installed in `game_dir`.
pub fn helixsr_installed(game_dir: &Path) -> bool {
    game_dir.join("HelixSR").is_dir() || game_dir.join("HelixSR-Scaler-remove.sh").is_file()
}

/// Networks DLSSNR-AMD (0.1+) can start with.
pub const DLSSNR_MODELS: [&str; 4] = [
    "CC_SMALL_FP8",
    "CC_SMALL_FP8_OPTIMIZED",
    "CC_SILVER_AARDWOLD",
    "CC_SILVER_AARDWOLD_OPTIMIZED",
];

/// How DLSSNR-AMD hooks into a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NrRoute {
    /// The game has a DLSS, FSR or XeSS option: OptiScaler does the upscaling (64-bit only).
    #[default]
    Optiscaler,
    /// DirectX 10/11/12 games without a usable upscaler, through ReShade.
    ReshadeFeeder,
    /// Vulkan games and DirectX 9 games (DXVK turns them into Vulkan), through ReShade.
    ReshadeFeederVulkan,
    /// As the Vulkan route, with the depth direction old DirectX 9 games use.
    ReshadeFeederDx9,
}

impl NrRoute {
    pub const ALL: [NrRoute; 4] = [
        NrRoute::Optiscaler,
        NrRoute::ReshadeFeeder,
        NrRoute::ReshadeFeederVulkan,
        NrRoute::ReshadeFeederDx9,
    ];

    /// The installer's name for it.
    pub fn arg(self) -> &'static str {
        match self {
            NrRoute::Optiscaler => "optiscaler",
            NrRoute::ReshadeFeeder => "reshade-feeder",
            NrRoute::ReshadeFeederVulkan => "reshade-feeder-vulkan",
            NrRoute::ReshadeFeederDx9 => "reshade-feeder-dx9",
        }
    }

    pub fn from_arg(s: &str) -> Option<NrRoute> {
        NrRoute::ALL.into_iter().find(|r| r.arg() == s)
    }
}

/// What Arkor did with DLSSNR-AMD for a game: `~/.config/arkor/dlssnr/<store>-<id>.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DlssNr {
    pub bundle: Bundle,
    pub version: String,
    pub installer: PathBuf,
    pub game_dir: PathBuf,
    /// DLL names the installer asked to load native first (e.g. `dxgi`).
    pub overrides: Vec<String>,
    #[serde(default)]
    pub route: NrRoute,
    /// Other variables its launch line sets (the Vulkan layer of the ReShade routes).
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

fn dlssnr_path(id: &GameId) -> anyhow::Result<PathBuf> {
    Ok(arkor_core::paths::config_dir()?
        .join("dlssnr")
        .join(format!("{}.toml", id.file_stem())))
}

pub fn dlssnr(id: &GameId) -> Option<DlssNr> {
    toml::from_str(&std::fs::read_to_string(dlssnr_path(id).ok()?).ok()?).ok()
}

/// DLL names from the `WINEDLLOVERRIDES="…"` line the installer prints.
fn parse_overrides(output: &str) -> Vec<String> {
    let Some(start) = output.rfind("WINEDLLOVERRIDES=") else {
        return Vec::new();
    };
    let value = output[start + "WINEDLLOVERRIDES=".len()..].trim_start_matches(['"', '\'']);
    let value = value.split(['"', '\'', ' ', '\n']).next().unwrap_or("");
    value
        .split(';')
        .filter_map(|part| part.split_once('=').map(|(names, _)| names))
        .flat_map(|names| names.split(','))
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

/// Variables other than `WINEDLLOVERRIDES` on the launch line the installer prints
/// (`Steam launch options:  VK_ADD_LAYER_PATH="…" VK_INSTANCE_LAYERS=… WINEDLLOVERRIDES=…`).
fn parse_launch_env(output: &str) -> Vec<(String, String)> {
    let Some(line) = output
        .lines()
        .find_map(|l| l.trim().strip_prefix("Steam launch options:"))
    else {
        return Vec::new();
    };
    arkor_core::profile::shell_words(line)
        .into_iter()
        .take_while(|w| w != "%command%")
        .filter_map(|w| {
            let (k, v) = w.split_once('=')?;
            (k != "WINEDLLOVERRIDES" && !k.is_empty()).then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// Whether the package already holds the networks taken from NVIDIA's file at an earlier
/// install (then no file needs to be chosen again).
pub fn dlssnr_has_models(dir: &Path) -> bool {
    ["CC_SMALL_FP8.bin", "CC_SILVER_AARDWOLD.bin"]
        .iter()
        .any(|m| dir.join("dlssnr-amd").join(m).is_file())
}

/// Installs DLSSNR-AMD into `game_dir` by `route`, with the user's NVIDIA `nvngx_dlssnr`
/// file (`None` when the package already holds the networks). Returns the installer's
/// output. Blocking.
#[allow(clippy::too_many_arguments)]
pub fn dlssnr_install(
    id: &GameId,
    bundle: Bundle,
    version: &str,
    dir: &Path,
    game_dir: &Path,
    nvidia_dll: Option<&Path>,
    model: &str,
    route: NrRoute,
) -> anyhow::Result<String> {
    anyhow::ensure!(bundle != Bundle::HelixSr, "not a DLSS-NR bundle");
    anyhow::ensure!(bundle.models().contains(&model), "unknown network {model}");
    anyhow::ensure!(
        !(bundle.is_32bit() && route == NrRoute::Optiscaler),
        "the OptiScaler route is for 64-bit games; pick a ReShade route for a 32-bit game"
    );
    anyhow::ensure!(
        nvidia_dll.is_some() || dlssnr_has_models(dir),
        "choose NVIDIA's nvngx_dlssnr.dll the first time"
    );
    if route == NrRoute::Optiscaler {
        anyhow::ensure!(
            crate::optiscaler::Manifest::load(id)?.is_none(),
            "DLSS-NR brings its own OptiScaler: remove Arkor's OptiScaler from this game first"
        );
    } else {
        anyhow::ensure!(
            crate::reshade::manifest(id).is_none(),
            "DLSS-NR brings its own ReShade: remove Arkor's ReShade from this game first"
        );
    }
    let installer = dir.join("install.sh");
    let mut cmd = Command::new("bash");
    cmd.arg(&installer).arg(game_dir).arg(route.arg());
    if let Some(dll) = nvidia_dll {
        cmd.arg("--dll").arg(dll);
    }
    if route != NrRoute::Optiscaler {
        cmd.arg("--no-lumenite");
    }
    let out = cmd
        .args(["--model", model])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .context("running the DLSS-NR installer")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    anyhow::ensure!(
        out.status.success(),
        "the DLSS-NR installer failed:\n{}",
        text.lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    );
    let record = DlssNr {
        bundle,
        version: version.to_string(),
        installer,
        game_dir: game_dir.to_path_buf(),
        overrides: parse_overrides(&text),
        route,
        env: parse_launch_env(&text),
    };
    let path = dlssnr_path(id)?;
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    std::fs::write(&path, toml::to_string_pretty(&record)?)?;
    Ok(text)
}

/// Runs the installer's `remove` route for the game and forgets it. Blocking.
pub fn dlssnr_remove(id: &GameId) -> anyhow::Result<()> {
    let record = dlssnr(id).context("DLSS-NR is not installed for this game")?;
    if record.installer.is_file() {
        let out = Command::new("bash")
            .arg(&record.installer)
            .arg(&record.game_dir)
            .arg("remove")
            .stdin(std::process::Stdio::null())
            .output()
            .context("running the DLSS-NR installer")?;
        anyhow::ensure!(
            out.status.success(),
            "the DLSS-NR uninstall failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    std::fs::remove_file(dlssnr_path(id)?)?;
    Ok(())
}

/// Whether the GPU is AMD (these bundles need RDNA3 / RDNA4).
pub fn amd_gpu() -> bool {
    std::fs::read_dir("/sys/class/drm").is_ok_and(|d| {
        d.flatten().any(|e| {
            std::fs::read_to_string(e.path().join("device/vendor"))
                .is_ok_and(|v| v.trim() == "0x1002")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_overrides_from_installer_output() {
        let out = "Done.\nSteam launch options:\n    WINEDLLOVERRIDES=\"dxgi=n,b\" %command%\n";
        assert_eq!(parse_overrides(out), ["dxgi"]);
        assert_eq!(
            parse_overrides("WINEDLLOVERRIDES=\"winmm,version=n,b;d3d12=n\" %command%"),
            ["winmm", "version", "d3d12"]
        );
        assert!(parse_overrides("nothing here").is_empty());
    }

    #[test]
    fn reads_the_vulkan_layer_of_the_launch_line() {
        let out = "Steam launch options:  VK_ADD_LAYER_PATH=\"/g/My Game/dlssnr-amd/int4/layer\" VK_INSTANCE_LAYERS=VK_LAYER_DLSSNR_int4 WINEDLLOVERRIDES=\"d3d9=n,b\" %command%\n";
        assert_eq!(
            parse_launch_env(out),
            [
                (
                    "VK_ADD_LAYER_PATH".to_string(),
                    "/g/My Game/dlssnr-amd/int4/layer".to_string()
                ),
                (
                    "VK_INSTANCE_LAYERS".to_string(),
                    "VK_LAYER_DLSSNR_int4".to_string()
                ),
            ]
        );
        assert_eq!(parse_overrides(out), ["d3d9"]);
        assert!(parse_launch_env("Steam launch options:  WINEDLLOVERRIDES=\"dxgi=n,b\" %command%").is_empty());
        assert_eq!(NrRoute::from_arg("reshade-feeder-dx9"), Some(NrRoute::ReshadeFeederDx9));
        assert!(Bundle::DlssNrRdna3.models().contains(&"CC_SMALL_FP8_EXPERIMENTAL"));
    }

    #[test]
    fn finds_wrapped_bundle_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("HelixSR-3.4.1")).unwrap();
        std::fs::write(tmp.path().join("HelixSR-3.4.1/helixsr-setup.sh"), "").unwrap();
        assert_eq!(
            find_marker(tmp.path(), "helixsr-setup.sh", 2),
            Some(tmp.path().join("HelixSR-3.4.1"))
        );
    }
}
