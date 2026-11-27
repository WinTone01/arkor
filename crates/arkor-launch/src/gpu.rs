//! Choosing the graphics card a game renders on, on computers with more than one (a laptop's
//! integrated and dedicated cards, a desktop with two). Mesa's drivers take `DRI_PRIME` and
//! `MESA_VK_DEVICE_SELECT`; NVIDIA's own driver its PRIME render offload variables.

use std::path::Path;

/// A graphics card of this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gpu {
    /// PCI address, `0000:03:00.0`.
    pub slot: String,
    /// `0x10de`, `0x1002`, `0x8086`.
    pub vendor: String,
    pub device: String,
    /// The kernel driver: `nvidia`, `amdgpu`, `i915`, `xe`, `nouveau`.
    pub driver: String,
    /// The card the computer started with (usually the one the screen is on).
    pub boot: bool,
}

impl Gpu {
    pub fn vendor_name(&self) -> &'static str {
        match self.vendor.as_str() {
            "0x10de" => "NVIDIA",
            "0x1002" => "AMD",
            "0x8086" => "Intel",
            _ => "GPU",
        }
    }

    /// "NVIDIA (0000:01:00.0)".
    pub fn label(&self) -> String {
        format!("{} ({})", self.vendor_name(), self.slot)
    }

    /// The environment that makes a game render on this card.
    pub fn env(&self) -> Vec<(String, String)> {
        if self.driver == "nvidia" {
            return vec![
                ("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into()),
                ("__GLX_VENDOR_LIBRARY_NAME".into(), "nvidia".into()),
                ("__VK_LAYER_NV_optimus".into(), "NVIDIA_only".into()),
            ];
        }
        let hex = |s: &str| s.trim_start_matches("0x").to_string();
        vec![
            (
                "DRI_PRIME".into(),
                format!("pci-{}", self.slot.replace([':', '.'], "_")),
            ),
            (
                "MESA_VK_DEVICE_SELECT".into(),
                format!("{}:{}", hex(&self.vendor), hex(&self.device)),
            ),
        ]
    }
}

/// The graphics cards under `drm` (`/sys/class/drm`), each once.
pub fn list_in(drm: &Path) -> Vec<Gpu> {
    let mut out: Vec<Gpu> = Vec::new();
    let Ok(entries) = std::fs::read_dir(drm) else {
        return out;
    };
    let mut cards: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("card") && !n.contains('-'))
        })
        .collect();
    cards.sort();
    for card in cards {
        let dev = card.join("device");
        let read = |f: &str| {
            std::fs::read_to_string(dev.join(f))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        let Some(slot) = std::fs::canonicalize(&dev)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        else {
            continue;
        };
        if out.iter().any(|g| g.slot == slot) {
            continue;
        }
        let driver = read("uevent")
            .lines()
            .find_map(|l| l.strip_prefix("DRIVER=").map(str::to_string))
            .unwrap_or_default();
        out.push(Gpu {
            slot,
            vendor: read("vendor"),
            device: read("device"),
            driver,
            boot: read("boot_vga") == "1",
        });
    }
    out
}

/// This computer's graphics cards.
pub fn list() -> Vec<Gpu> {
    list_in(Path::new("/sys/class/drm"))
}

/// The environment for the card at `slot`, when it is still there.
pub fn env_for(slot: &str) -> Vec<(String, String)> {
    list()
        .into_iter()
        .find(|g| g.slot == slot)
        .map(|g| g.env())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cards_and_their_environment() {
        let root = tempfile::tempdir().unwrap();
        let drm = root.path().join("drm");
        for (card, slot, vendor, device, driver, boot) in [
            ("card0", "0000:00:02.0", "0x8086", "0x46a6", "i915", "1"),
            ("card1", "0000:01:00.0", "0x10de", "0x2504", "nvidia", "0"),
            ("card2", "0000:03:00.0", "0x1002", "0x73bf", "amdgpu", "0"),
        ] {
            let dev = root.path().join("devices").join(slot);
            std::fs::create_dir_all(&dev).unwrap();
            std::fs::write(dev.join("vendor"), vendor).unwrap();
            std::fs::write(dev.join("device"), device).unwrap();
            std::fs::write(dev.join("boot_vga"), boot).unwrap();
            std::fs::write(dev.join("uevent"), format!("DRIVER={driver}\n")).unwrap();
            std::fs::create_dir_all(drm.join(card)).unwrap();
            std::os::unix::fs::symlink(&dev, drm.join(card).join("device")).unwrap();
        }
        std::fs::create_dir_all(drm.join("card1-DP-1")).unwrap();
        let gpus = list_in(&drm);
        assert_eq!(gpus.len(), 3);
        assert!(gpus[0].boot && gpus[0].vendor_name() == "Intel");
        assert!(
            gpus[1]
                .env()
                .iter()
                .any(|(k, v)| k == "__NV_PRIME_RENDER_OFFLOAD" && v == "1")
        );
        let amd = gpus[2].env();
        assert!(amd.contains(&("DRI_PRIME".into(), "pci-0000_03_00_0".into())));
        assert!(amd.contains(&("MESA_VK_DEVICE_SELECT".into(), "1002:73bf".into())));
    }
}
