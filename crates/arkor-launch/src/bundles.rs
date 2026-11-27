//! AMD upscaler bundles that come with their own Linux installers: HelixSR (HLX DLSS and
//! HLX FSR 4 on top of OptiScaler) and DLSSNR-AMD (DLSS 5's neural rendering model in Vulkan).
//! Arkor downloads them and drives their installers instead of copying their files itself.
//! HelixSR's setup builds its networks on this PC from NVIDIA's and AMD's files after the
//! user agrees to their terms, so it runs in a terminal where those questions can be answered.

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
}

impl Bundle {
    pub const ALL: [Bundle; 3] = [Bundle::HelixSr, Bundle::DlssNrRdna4, Bundle::DlssNrRdna3];

    pub fn name(self) -> &'static str {
        match self {
            Bundle::HelixSr => "HelixSR",
            Bundle::DlssNrRdna4 => "DLSS-NR (RDNA4)",
            Bundle::DlssNrRdna3 => "DLSS-NR (RDNA3)",
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
        }
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
        Bundle::DlssNrRdna4 => {
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
                n.contains("Linux") && n.contains("x86_64") && n.ends_with(".tar.gz")
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

/// Networks DLSSNR-AMD can start with.
pub const DLSSNR_MODELS: [&str; 4] = [
    "CC_SMALL_FP8",
    "CC_SMALL_FP8_EXPERIMENTAL",
    "CC_SILVER_AARDWOLD",
    "CC_SILVER_AARDWOLD_OPTIMIZED",
];

/// What Arkor did with DLSSNR-AMD for a game: `~/.config/arkor/dlssnr/<store>-<id>.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DlssNr {
    pub bundle: Bundle,
    pub version: String,
    pub installer: PathBuf,
    pub game_dir: PathBuf,
    /// DLL names the installer asked to load native first (e.g. `dxgi`).
    pub overrides: Vec<String>,
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

/// Installs DLSSNR-AMD (OptiScaler route) into `game_dir` with the user's NVIDIA
/// `nvngx_dlssnr` file. Returns the installer's output. Blocking.
pub fn dlssnr_install(
    id: &GameId,
    bundle: Bundle,
    version: &str,
    dir: &Path,
    game_dir: &Path,
    nvidia_dll: &Path,
    model: &str,
) -> anyhow::Result<String> {
    anyhow::ensure!(bundle != Bundle::HelixSr, "not a DLSS-NR bundle");
    anyhow::ensure!(DLSSNR_MODELS.contains(&model), "unknown network {model}");
    anyhow::ensure!(
        crate::optiscaler::Manifest::load(id)?.is_none(),
        "DLSS-NR brings its own OptiScaler: remove Arkor's OptiScaler from this game first"
    );
    let installer = dir.join("install.sh");
    let out = Command::new("bash")
        .arg(&installer)
        .arg(game_dir)
        .arg("optiscaler")
        .arg("--dll")
        .arg(nvidia_dll)
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
