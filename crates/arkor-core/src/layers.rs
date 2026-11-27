//! Per-game switches for extra Vulkan layers (vkNemu, MAKO, vkSumi and any other implicit
//! layer the system has), turned into environment variables at launch.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layers {
    pub vknemu: VkNemu,
    pub mako: Mako,
    /// vkSumi colour grading, with `sumi`'s values (a per-game `vkSumi.conf`).
    pub vksumi: bool,
    pub sumi: Sumi,
    pub afmf: Afmf,
    /// Other implicit layers to enable, as `KEY=VALUE` (their `enable_environment`).
    pub other: BTreeSet<String>,
}

/// One of vkSumi's colour controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SumiKnob {
    Brightness,
    Contrast,
    Exposure,
    Gamma,
    Shadows,
    Midtones,
    Highlights,
    Saturation,
    Vibrance,
    Hue,
    Temperature,
    Tint,
    RedGain,
    GreenGain,
    BlueGain,
}

impl SumiKnob {
    pub const ALL: [SumiKnob; 15] = [
        SumiKnob::Brightness,
        SumiKnob::Contrast,
        SumiKnob::Exposure,
        SumiKnob::Gamma,
        SumiKnob::Shadows,
        SumiKnob::Midtones,
        SumiKnob::Highlights,
        SumiKnob::Saturation,
        SumiKnob::Vibrance,
        SumiKnob::Hue,
        SumiKnob::Temperature,
        SumiKnob::Tint,
        SumiKnob::RedGain,
        SumiKnob::GreenGain,
        SumiKnob::BlueGain,
    ];

    /// The key in `vkSumi.conf`.
    pub fn key(self) -> &'static str {
        match self {
            SumiKnob::Brightness => "brightness",
            SumiKnob::Contrast => "contrast",
            SumiKnob::Exposure => "exposure",
            SumiKnob::Gamma => "gamma",
            SumiKnob::Shadows => "shadows",
            SumiKnob::Midtones => "midtones",
            SumiKnob::Highlights => "highlights",
            SumiKnob::Saturation => "saturation",
            SumiKnob::Vibrance => "vibrance",
            SumiKnob::Hue => "hue_deg",
            SumiKnob::Temperature => "temperature",
            SumiKnob::Tint => "tint",
            SumiKnob::RedGain => "red_gain",
            SumiKnob::GreenGain => "green_gain",
            SumiKnob::BlueGain => "blue_gain",
        }
    }

    /// (lowest, highest, one step), in hundredths (degrees for the hue).
    pub fn range(self) -> (i32, i32, i32) {
        match self {
            SumiKnob::Exposure => (-300, 300, 10),
            SumiKnob::Hue => (-180, 180, 5),
            _ => (-100, 100, 5),
        }
    }

    /// The value as `vkSumi.conf` and the screen show it.
    pub fn show(self, v: i32) -> String {
        if self == SumiKnob::Hue {
            format!("{v}°")
        } else {
            format!("{:.2}", v as f32 / 100.)
        }
    }
}

/// Colour-blindness correction vkSumi can apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Daltonize {
    #[default]
    Off,
    Protan,
    Deutan,
    Tritan,
}

impl Daltonize {
    pub const ALL: [Daltonize; 4] = [
        Daltonize::Off,
        Daltonize::Protan,
        Daltonize::Deutan,
        Daltonize::Tritan,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Daltonize::Off => "off",
            Daltonize::Protan => "protan",
            Daltonize::Deutan => "deutan",
            Daltonize::Tritan => "tritan",
        }
    }
}

/// vkSumi's colour grading for a game. Every value 0 is "no change"; values are hundredths
/// (exposure -3..3, others -1..1), the hue in degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sumi {
    pub values: BTreeMap<String, i32>,
    /// Left half graded, right half as the game draws it, to compare.
    pub compare: bool,
    pub daltonize: Daltonize,
    /// Key that turns the grading off and on in game.
    pub toggle_key: String,
}

impl Default for Sumi {
    fn default() -> Self {
        Self {
            values: BTreeMap::new(),
            compare: false,
            daltonize: Daltonize::Off,
            toggle_key: "Shift_R+F9".into(),
        }
    }
}

impl Sumi {
    pub fn get(&self, k: SumiKnob) -> i32 {
        self.values.get(k.key()).copied().unwrap_or(0)
    }

    pub fn set(&mut self, k: SumiKnob, v: i32) {
        let (lo, hi, _) = k.range();
        let v = v.clamp(lo, hi);
        if v == 0 {
            self.values.remove(k.key());
        } else {
            self.values.insert(k.key().into(), v);
        }
    }

    /// One step up (`up`) or down.
    pub fn step(&mut self, k: SumiKnob, up: bool) {
        let step = k.range().2;
        self.set(k, self.get(k) + if up { step } else { -step });
    }

    /// Every value back to "no change".
    pub fn reset(&mut self) {
        self.values.clear();
    }

    /// The `vkSumi.conf` for these values.
    pub fn render(&self) -> String {
        let mut out = String::from(
            "# Written by Arkor for this game; changes here are replaced at the next start.\n\
             PER_GAME_CONFIG_CREATION = false\n\nenabled = true\n",
        );
        let key = self.toggle_key.trim();
        if !key.is_empty() {
            out.push_str(&format!("toggle_keys = {key}\n"));
        }
        out.push_str(&format!(
            "compare = {}\ndaltonize = {}\n\n",
            self.compare,
            self.daltonize.key()
        ));
        for k in SumiKnob::ALL {
            let v = self.get(k);
            let text = if k == SumiKnob::Hue {
                format!("{v}.0")
            } else {
                format!("{:.2}", v as f32 / 100.)
            };
            out.push_str(&format!("{} = {text}\n", k.key()));
        }
        out
    }
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
    /// GPU capacity kept free (0, 5, 10, 15 or 20 %) so frame pacing stays even when the
    /// card would otherwise run at 100 % (MAKO 4.6+).
    pub gpu_headroom: u32,
    /// Let MAKO brighten HDR pictures itself (off: the game's own HDR brightness is kept).
    pub hdr_exposure: bool,
}

/// The GPU headroom steps MAKO takes.
pub const MAKO_HEADROOM: [u32; 5] = [0, 5, 10, 15, 20];

impl Default for Mako {
    fn default() -> Self {
        Self {
            enabled: false,
            multiplier: 2,
            flow_scale: 100,
            performance_mode: false,
            scaling: false,
            scaling_factor: 150,
            gpu_headroom: 0,
            hdr_exposure: false,
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
            if !m.hdr_exposure {
                set("MAKO_DISABLE_HDR_EXPOSURE", "1".into());
            }
            if m.gpu_headroom > 0 {
                set("MAKO_GPU_HEADROOM_PERCENT", m.gpu_headroom.to_string());
            }
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
        assert_eq!(env["MAKO_DISABLE_HDR_EXPOSURE"], "1");
        assert!(!env.contains_key("MAKO_GPU_HEADROOM_PERCENT"));
        l.mako.gpu_headroom = 10;
        assert_eq!(l.env(None)["MAKO_GPU_HEADROOM_PERCENT"], "10");
        l.toggle_other("ENABLE_FOO", "1");
        assert!(!l.env(None).contains_key("ENABLE_FOO"));
    }

    #[test]
    fn sumi_values_stay_in_range_and_render() {
        let mut s = Sumi::default();
        s.set(SumiKnob::Exposure, 999);
        assert_eq!(s.get(SumiKnob::Exposure), 300);
        s.step(SumiKnob::Hue, false);
        assert_eq!(s.get(SumiKnob::Hue), -5);
        s.step(SumiKnob::Saturation, true);
        s.daltonize = Daltonize::Deutan;
        let conf = s.render();
        assert!(conf.contains("exposure = 3.00\n"));
        assert!(conf.contains("hue_deg = -5.0\n"));
        assert!(conf.contains("saturation = 0.05\n"));
        assert!(conf.contains("contrast = 0.00\n"));
        assert!(conf.contains("daltonize = deutan\n"));
        assert!(conf.contains("toggle_keys = Shift_R+F9\n"));
        s.set(SumiKnob::Saturation, 0);
        assert!(!s.values.contains_key("saturation"));
        let back: Layers = toml::from_str("vksumi = true\n[sumi]\ncompare = true\n").unwrap();
        assert!(back.sumi.compare && back.sumi.toggle_key == "Shift_R+F9");
    }
}
