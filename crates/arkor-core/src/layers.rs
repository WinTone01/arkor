//! Per-game switches for extra Vulkan layers (vkNemu, MAKO, vkSumi and any other implicit
//! layer the system has), turned into environment variables at launch.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layers {
    pub vknemu: VkNemu,
    pub mako: Mako,
    /// vkSumi colour grading with the user's `~/.config/vkSumi/vkSumi.conf`.
    pub vksumi: bool,
    pub afmf: Afmf,
    /// Other implicit layers to enable, as `KEY=VALUE` (their `enable_environment`).
    pub other: BTreeSet<String>,
}

/// vkNemu: drops the frame rate when there is no input for a while.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VkNemu {
    pub enabled: bool,
    /// Frame rate while idle.
    pub idle_fps: u32,
    /// Seconds without input before limiting.
    pub timeout_secs: u32,
    /// Also limit while the window is unfocused.
    pub unfocused: bool,
}

impl Default for VkNemu {
    fn default() -> Self {
        Self {
            enabled: false,
            idle_fps: 30,
            timeout_secs: 60,
            unfocused: true,
        }
    }
}

/// MAKO Renderer frame generation (and optional spatial scaling), configured by environment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mako {
    pub enabled: bool,
    pub multiplier: u32,
    /// Flow scale in percent.
    pub flow_scale: u32,
    pub performance_mode: bool,
    /// MAKO's open spatial scaler.
    pub scaling: bool,
    /// Scaling factor in percent (e.g. 150 = 1.5x).
    pub scaling_factor: u32,
}

impl Default for Mako {
    fn default() -> Self {
        Self {
            enabled: false,
            multiplier: 2,
            flow_scale: 100,
            performance_mode: false,
            scaling: false,
            scaling_factor: 150,
        }
    }
}

/// afmf-linux: AMD Fluid Motion Frames style frame generation (2x).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Afmf {
    pub enabled: bool,
    /// `auto`, `quality` or `performance` (optical flow at full or half resolution).
    pub mode: AfmfMode,
    /// Below this real frame rate no frames are generated. `None` = the layer's 30.
    pub min_fps: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AfmfMode {
    #[default]
    Auto,
    Quality,
    Performance,
}

impl AfmfMode {
    pub const ALL: [AfmfMode; 3] = [AfmfMode::Auto, AfmfMode::Quality, AfmfMode::Performance];

    pub fn as_str(self) -> &'static str {
        match self {
            AfmfMode::Auto => "auto",
            AfmfMode::Quality => "quality",
            AfmfMode::Performance => "performance",
        }
    }
}

impl Layers {
    /// Whether more than one frame generator is on (they must not be stacked).
    pub fn frame_generators(&self) -> usize {
        usize::from(self.mako.enabled) + usize::from(self.afmf.enabled)
    }

    /// Environment for the enabled layers. `lossless_dll` feeds MAKO's frame generation.
    pub fn env(&self, lossless_dll: Option<&std::path::Path>) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        let mut set = |k: &str, v: String| {
            env.insert(k.to_string(), v);
        };
        if self.vknemu.enabled {
            set("VKNEMU", "1".into());
            set("VKNEMU_IDLE_FPS", self.vknemu.idle_fps.to_string());
            set("VKNEMU_TIMEOUT", self.vknemu.timeout_secs.to_string());
            set(
                "VKNEMU_UNFOCUSED",
                u8::from(self.vknemu.unfocused).to_string(),
            );
        }
        let m = &self.mako;
        if m.enabled || m.scaling {
            set("MAKO_ENV", "1".into());
        }
        if m.enabled {
            set("ENABLE_MAKO", "1".into());
            set("MAKO_FRAME_GENERATION_ENABLED", "1".into());
            set("MAKO_MULTIPLIER", m.multiplier.to_string());
            set(
                "MAKO_FLOW_SCALE",
                format!("{:.2}", m.flow_scale as f32 / 100.),
            );
            set(
                "MAKO_PERFORMANCE_MODE",
                u8::from(m.performance_mode).to_string(),
            );
            if let Some(dll) = lossless_dll {
                set("MAKO_DLL_PATH", dll.display().to_string());
            }
        }
        if m.scaling {
            set("ENABLE_MAKO_SPATIAL_SCALING", "1".into());
            set("MAKO_SCALING_ENABLED", "1".into());
            set(
                "MAKO_SCALING_FACTOR",
                format!("{:.2}", m.scaling_factor as f32 / 100.),
            );
        }
        if self.afmf.enabled {
            set("AFMF_ENABLE", "1".into());
            set("AFMF_PERFORMANCE_MODE", self.afmf.mode.as_str().into());
            if let Some(min) = self.afmf.min_fps {
                set("AFMF_MIN_FPS", min.to_string());
            }
        }
        if self.vksumi {
            set("ENABLE_VKSUMI", "1".into());
        }
        for kv in &self.other {
            if let Some((k, v)) = kv.split_once('=') {
                set(k, v.to_string());
            }
        }
        env
    }

    pub fn toggle_other(&mut self, key: &str, value: &str) {
        let kv = format!("{key}={value}");
        if !self.other.remove(&kv) {
            self.other.insert(kv);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_only_for_enabled_layers() {
        let mut l = Layers::default();
        assert!(l.env(None).is_empty());
        l.vknemu.enabled = true;
        l.mako.enabled = true;
        l.mako.flow_scale = 75;
        l.toggle_other("ENABLE_FOO", "1");
        let env = l.env(Some(std::path::Path::new("/x/Lossless.dll")));
        assert_eq!(env["VKNEMU_IDLE_FPS"], "30");
        assert_eq!(env["MAKO_FLOW_SCALE"], "0.75");
        assert_eq!(env["MAKO_DLL_PATH"], "/x/Lossless.dll");
        assert_eq!(env["ENABLE_FOO"], "1");
        assert!(!env.contains_key("ENABLE_MAKO_SPATIAL_SCALING"));
        l.toggle_other("ENABLE_FOO", "1");
        assert!(!l.env(None).contains_key("ENABLE_FOO"));
    }
}
