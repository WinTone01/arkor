//! Extra Vulkan layers installed in user space (no root): vkSumi (colour grading), vkNemu
//! (idle FPS limiter), MAKO (frame generation / spatial scaling) and afmf-linux (AFMF-style
//! frame generation). Arkor unpacks a release
//! into `~/.local/share/arkor/layers/<id>/` and registers its layer JSON (with an absolute
//! library path) in `~/.local/share/vulkan/implicit_layer.d/`. Also lists every implicit
//! layer on the system that is switched on by an environment variable, so any of them can
//! be enabled per game.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

pub struct LayerTool {
    pub id: &'static str,
    pub name: &'static str,
    pub description_en: &'static str,
    pub description_tr: &'static str,
    releases_api: &'static str,
    asset_prefix: &'static str,
    asset_suffix: &'static str,
}

pub const TOOLS: [LayerTool; 4] = [
    LayerTool {
        id: "vksumi",
        name: "vkSumi",
        description_en: "Colour grading (vibrance, contrast, colour-blindness filters)",
        description_tr: "Renk ayarı (canlılık, kontrast, renk körlüğü filtreleri)",
        releases_api: "https://api.github.com/repos/reakjra/vkSumi/releases?per_page=20",
        asset_prefix: "vksumi-",
        asset_suffix: "x86_64.pkg.tar.zst",
    },
    LayerTool {
        id: "vknemu",
        name: "vkNemu",
        description_en: "Lowers the frame rate while the game is idle or unfocused",
        description_tr: "Oyun boştayken ya da odakta değilken kare hızını düşürür",
        releases_api: "https://api.github.com/repos/reakjra/vkNemu/releases?per_page=20",
        asset_prefix: "vknemu-",
        asset_suffix: "x86_64.pkg.tar.zst",
    },
    LayerTool {
        id: "mako",
        name: "MAKO",
        description_en: "Frame generation and spatial scaling (uses Lossless Scaling's DLL)",
        description_tr: "Kare üretimi ve uzamsal ölçekleme (Lossless Scaling'in DLL'ini kullanır)",
        releases_api: "https://api.github.com/repos/eugeniosegala/MAKO/releases?per_page=20",
        asset_prefix: "MAKO-Renderer-v",
        asset_suffix: "-linux.tar.xz",
    },
    LayerTool {
        id: "afmf",
        name: "AFMF",
        description_en: "Driver-style frame generation (AMD Fluid Motion Frames for Linux)",
        description_tr: "Sürücü tarzı kare üretimi (Linux için AMD Fluid Motion Frames)",
        releases_api: "https://api.github.com/repos/serialexperimentslainnnn/afmf-linux/releases?per_page=20",
        asset_prefix: "afmf-linux-",
        asset_suffix: "-x86_64.tar.xz",
    },
];

fn layers_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/layers"))
}

fn user_layer_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("vulkan/implicit_layer.d"))
}

/// The release Arkor installed for `id` (written at install time).
pub fn installed_version(id: &str) -> Option<String> {
    let v = std::fs::read_to_string(layers_dir().ok()?.join(id).join("version")).ok()?;
    Some(v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The newest release of `tool` with a usable build. Blocking.
pub fn latest_version(tool: &LayerTool) -> anyhow::Result<String> {
    let releases: Vec<Release> = serde_json::from_str(&crate::tools::http_get(tool.releases_api)?)
        .context("unexpected release response")?;
    releases
        .iter()
        .find(|r| {
            r.assets.iter().any(|a| {
                a.name.starts_with(tool.asset_prefix)
                    && a.name.ends_with(tool.asset_suffix)
                    && !a.name.contains("debug")
            })
        })
        .map(|r| r.tag_name.clone())
        .with_context(|| format!("no {} release found", tool.name))
}

/// Whether a layer that `enable_key` switches on is installed (Arkor's copy or the system's).
pub fn present(enable_key: &str) -> bool {
    system_layers().iter().any(|l| l.enable.0 == enable_key)
}

/// Arkor's installed copy, if any.
pub fn arkor_installed(id: &str) -> bool {
    user_layer_dir().is_ok_and(|d| {
        std::fs::read_dir(d).is_ok_and(|e| {
            e.flatten().any(|f| {
                f.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("arkor-{id}-"))
            })
        })
    })
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Downloads the latest release and registers its layers. Returns the version. Blocking.
pub fn install(tool: &LayerTool) -> anyhow::Result<String> {
    let releases: Vec<Release> = serde_json::from_str(&crate::tools::http_get(tool.releases_api)?)
        .context("unexpected release response")?;
    // Repos may publish other things too (MAKO's Decky plugin, debug and lib32 packages):
    // take the newest release that has the right build.
    let (release, asset) = releases
        .iter()
        .find_map(|r| {
            r.assets
                .iter()
                .find(|a| {
                    a.name.starts_with(tool.asset_prefix)
                        && a.name.ends_with(tool.asset_suffix)
                        && !a.name.contains("debug")
                })
                .map(|a| (r, a))
        })
        .with_context(|| {
            format!(
                "no {} release with a {} build",
                tool.name, tool.asset_suffix
            )
        })?;
    let dir = layers_dir()?.join(tool.id);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let archive = dir.join("release.archive");
    crate::tools::download(&asset.browser_download_url, &archive)?;
    let status = std::process::Command::new("bsdtar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&dir)
        .status()
        .context("running bsdtar")?;
    let _ = std::fs::remove_file(&archive);
    anyhow::ensure!(status.success(), "could not unpack {}", asset.name);
    uninstall_json(tool.id)?;
    let count = register(tool.id, &dir)?;
    anyhow::ensure!(count > 0, "{} had no Vulkan layer manifest", tool.name);
    // Remembered for update checks.
    let _ = std::fs::write(dir.join("version"), &release.tag_name);
    Ok(release.tag_name.clone())
}

/// Writes each 64-bit layer JSON under `dir` into the user layer folder with an absolute
/// library path. Returns how many were registered.
fn register(id: &str, dir: &Path) -> anyhow::Result<usize> {
    let target = user_layer_dir()?;
    std::fs::create_dir_all(&target)?;
    let mut count = 0;
    for json in find(dir, &|p| {
        // Usually under implicit_layer.d, but some archives keep it at the top level;
        // JSON files that are not layer manifests are skipped below.
        p.extension().is_some_and(|e| e == "json")
            && !p.to_string_lossy().contains("explicit_layer.d")
    }) {
        let name = json
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if name.contains(".x86.") || name.contains("32") {
            continue; // 32-bit layers would point at lib32 builds; skip them.
        }
        let mut value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json)?)?;
        let Some(layer) = value.get_mut("layer") else {
            continue;
        };
        let Some(lib) = layer.get("library_path").and_then(|v| v.as_str()) else {
            continue;
        };
        let file = Path::new(lib)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let Some(so) = find(dir, &|p| {
            p.file_name().is_some_and(|n| n.to_string_lossy() == file)
        })
        .into_iter()
        .find(|p| !p.to_string_lossy().contains("lib32")) else {
            continue;
        };
        layer["library_path"] = serde_json::Value::String(so.display().to_string());
        std::fs::write(
            target.join(format!("arkor-{id}-{name}")),
            serde_json::to_string_pretty(&value)?,
        )?;
        count += 1;
    }
    Ok(count)
}

fn find(dir: &Path, pred: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(find(&p, pred));
        } else if pred(&p) {
            out.push(p);
        }
    }
    out
}

fn uninstall_json(id: &str) -> anyhow::Result<()> {
    if let Ok(entries) = std::fs::read_dir(user_layer_dir()?) {
        for e in entries.flatten() {
            if e.file_name()
                .to_string_lossy()
                .starts_with(&format!("arkor-{id}-"))
            {
                std::fs::remove_file(e.path())?;
            }
        }
    }
    Ok(())
}

pub fn uninstall(id: &str) -> anyhow::Result<()> {
    uninstall_json(id)?;
    let _ = std::fs::remove_dir_all(layers_dir()?.join(id));
    Ok(())
}

/// An implicit layer on this system that a variable turns on.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemLayer {
    pub name: String,
    pub description: String,
    pub enable: (String, String),
}

/// Every implicit layer (user and system folders) with an `enable_environment`, deduplicated.
pub fn system_layers() -> Vec<SystemLayer> {
    let mut dirs: Vec<PathBuf> = vec![
        "/usr/share/vulkan/implicit_layer.d".into(),
        "/etc/vulkan/implicit_layer.d".into(),
    ];
    if let Ok(d) = user_layer_dir() {
        dirs.insert(0, d);
    }
    let mut out: Vec<SystemLayer> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(e.path()) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let Some(layer) = v.get("layer") else {
                continue;
            };
            let Some((k, val)) = layer
                .get("enable_environment")
                .and_then(|e| e.as_object())
                .and_then(|o| o.iter().next())
            else {
                continue;
            };
            let enable = (k.clone(), val.as_str().unwrap_or("1").to_string());
            if out.iter().any(|l| l.enable == enable) {
                continue;
            }
            out.push(SystemLayer {
                name: layer
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: layer
                    .get("description")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
                enable,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_layer_with_absolute_library() {
        let _lock = crate::CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests that change XDG dirs hold CONFIG_ENV_LOCK.
        unsafe { std::env::set_var("XDG_DATA_HOME", tmp.path()) };
        let pkg = tmp.path().join("pkg");
        std::fs::create_dir_all(pkg.join("share/vulkan/implicit_layer.d")).unwrap();
        std::fs::create_dir_all(pkg.join("lib")).unwrap();
        std::fs::create_dir_all(pkg.join("lib32")).unwrap();
        std::fs::write(pkg.join("lib/libtest.so"), "").unwrap();
        std::fs::write(pkg.join("lib32/libtest.so"), "").unwrap();
        let json = r#"{"file_format_version":"1.0.0","layer":{"name":"VK_LAYER_test","library_path":"../../../lib/libtest.so","enable_environment":{"ENABLE_TEST":"1"}}}"#;
        std::fs::write(
            pkg.join("share/vulkan/implicit_layer.d/VkLayer_test.json"),
            json,
        )
        .unwrap();
        std::fs::write(
            pkg.join("share/vulkan/implicit_layer.d/VkLayer_test.x86.json"),
            json,
        )
        .unwrap();
        assert_eq!(
            register("test", &pkg).unwrap(),
            1,
            "32-bit manifest skipped"
        );
        let written = std::fs::read_to_string(
            user_layer_dir()
                .unwrap()
                .join("arkor-test-VkLayer_test.json"),
        )
        .unwrap();
        assert!(
            written.contains(&pkg.join("lib/libtest.so").display().to_string()),
            "{written}"
        );
        assert!(arkor_installed("test"));
        assert!(
            system_layers()
                .iter()
                .any(|l| l.enable == ("ENABLE_TEST".into(), "1".into()))
        );
        uninstall("test").unwrap();
        assert!(!arkor_installed("test"));
    }
}
