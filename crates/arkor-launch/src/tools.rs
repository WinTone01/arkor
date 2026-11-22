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
