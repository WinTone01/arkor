//! MangoHud HUD model: what to show, where and how. Rendered to a `MangoHud.conf`
//! (with `legacy_layout=0`, so the order keys are written in is the order MangoHud draws them).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Variant order is the on-screen order (see module docs), so keep it grouped by hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    GpuStats,
    GpuTemp,
    GpuCoreClock,
    GpuMemClock,
    GpuPower,
    Vram,
    CpuStats,
    CpuTemp,
    CpuMhz,
    CpuPower,
    Ram,
    Battery,
    Fps,
    Frametime,
    FrameTiming,
    Resolution,
    Wine,
    EngineVersion,
    Time,
}

impl Metric {
    pub const ALL: [Metric; 19] = [
        Metric::GpuStats,
        Metric::GpuTemp,
        Metric::GpuCoreClock,
        Metric::GpuMemClock,
        Metric::GpuPower,
        Metric::Vram,
        Metric::CpuStats,
        Metric::CpuTemp,
        Metric::CpuMhz,
        Metric::CpuPower,
        Metric::Ram,
        Metric::Battery,
        Metric::Fps,
        Metric::Frametime,
        Metric::FrameTiming,
        Metric::Resolution,
        Metric::Wine,
        Metric::EngineVersion,
        Metric::Time,
    ];

    /// The MangoHud config key.
    pub fn key(self) -> &'static str {
        match self {
            Metric::GpuStats => "gpu_stats",
            Metric::GpuTemp => "gpu_temp",
            Metric::GpuCoreClock => "gpu_core_clock",
            Metric::GpuMemClock => "gpu_mem_clock",
            Metric::GpuPower => "gpu_power",
            Metric::Vram => "vram",
            Metric::CpuStats => "cpu_stats",
            Metric::CpuTemp => "cpu_temp",
            Metric::CpuMhz => "cpu_mhz",
            Metric::CpuPower => "cpu_power",
            Metric::Ram => "ram",
            Metric::Battery => "battery",
            Metric::Fps => "fps",
            Metric::Frametime => "frametime",
            Metric::FrameTiming => "frame_timing",
            Metric::Resolution => "resolution",
            Metric::Wine => "wine",
            Metric::EngineVersion => "engine_version",
            Metric::Time => "time",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Position {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Position {
    pub const ALL: [Position; 8] = [
        Position::TopLeft,
        Position::TopCenter,
        Position::TopRight,
        Position::MiddleLeft,
        Position::MiddleRight,
        Position::BottomLeft,
        Position::BottomCenter,
        Position::BottomRight,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Position::TopLeft => "top-left",
            Position::TopCenter => "top-center",
            Position::TopRight => "top-right",
            Position::MiddleLeft => "middle-left",
            Position::MiddleRight => "middle-right",
            Position::BottomLeft => "bottom-left",
            Position::BottomCenter => "bottom-center",
            Position::BottomRight => "bottom-right",
        }
    }

    /// (row, column) on a 3x3 grid; the centre cell is not a MangoHud position.
    pub fn cell(self) -> (u8, u8) {
        match self {
            Position::TopLeft => (0, 0),
            Position::TopCenter => (0, 1),
            Position::TopRight => (0, 2),
            Position::MiddleLeft => (1, 0),
            Position::MiddleRight => (1, 2),
            Position::BottomLeft => (2, 0),
            Position::BottomCenter => (2, 1),
            Position::BottomRight => (2, 2),
        }
    }

    pub fn from_cell(row: u8, col: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.cell() == (row, col))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Orientation {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    FpsOnly,
    Basic,
    BasicHorizontal,
    Full,
}

impl Preset {
    pub const ALL: [Preset; 4] = [
        Preset::FpsOnly,
        Preset::Basic,
        Preset::BasicHorizontal,
        Preset::Full,
    ];

    pub fn metrics(self) -> BTreeSet<Metric> {
        use Metric::*;
        match self {
            Preset::FpsOnly => [Fps].into(),
            Preset::Basic => [
                GpuStats,
                GpuTemp,
                CpuStats,
                CpuTemp,
                Vram,
                Ram,
                Fps,
                FrameTiming,
            ]
            .into(),
            Preset::BasicHorizontal => [GpuStats, CpuStats, Vram, Ram, Fps, FrameTiming].into(),
            Preset::Full => Metric::ALL
                .into_iter()
                .filter(|m| !matches!(m, Battery | Time))
                .collect(),
        }
    }

    pub fn orientation(self) -> Orientation {
        match self {
            Preset::FpsOnly | Preset::BasicHorizontal => Orientation::Horizontal,
            Preset::Basic | Preset::Full => Orientation::Vertical,
        }
    }
}

/// Selectable FPS caps; 0 means unlimited.
pub const FPS_LIMITS: [u32; 10] = [0, 30, 60, 75, 90, 120, 144, 165, 180, 240];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HudSettings {
    pub metrics: BTreeSet<Metric>,
    pub orientation: Orientation,
    pub position: Position,
    pub font_size: u32,
    /// Background opacity, 0-100.
    pub background_alpha: u32,
    /// 0 = unlimited.
    pub fps_limit: u32,
    /// Any other MangoHud keys (colors, toggle keys, blacklist, ...), written verbatim.
    /// A value of `""` writes a bare flag.
    pub extra: BTreeMap<String, String>,
}

impl Default for HudSettings {
    fn default() -> Self {
        let mut s = Self {
            metrics: BTreeSet::new(),
            orientation: Orientation::Vertical,
            position: Position::TopLeft,
            font_size: 24,
            background_alpha: 40,
            fps_limit: 0,
            extra: [("toggle_hud".to_string(), "Shift_R+F12".to_string())].into(),
        };
        s.apply_preset(Preset::Basic);
        s
    }
}

pub const FONT_SIZE_RANGE: (u32, u32) = (12, 48);

impl HudSettings {
    /// Presets only choose *what* is shown and the orientation; position, size etc. are kept.
    pub fn apply_preset(&mut self, preset: Preset) {
        self.metrics = preset.metrics();
        self.orientation = preset.orientation();
    }

    pub fn matching_preset(&self) -> Option<Preset> {
        Preset::ALL
            .into_iter()
            .find(|p| p.metrics() == self.metrics && p.orientation() == self.orientation)
    }

    pub fn toggle_metric(&mut self, metric: Metric) {
        if !self.metrics.remove(&metric) {
            self.metrics.insert(metric);
        }
    }

    pub fn step_font_size(&mut self, up: bool) {
        let (min, max) = FONT_SIZE_RANGE;
        self.font_size = if up {
            self.font_size + 2
        } else {
            self.font_size.saturating_sub(2)
        }
        .clamp(min, max);
    }

    pub fn step_alpha(&mut self, up: bool) {
        self.background_alpha = if up {
            self.background_alpha + 10
        } else {
            self.background_alpha.saturating_sub(10)
        }
        .min(100);
    }

    pub fn step_fps_limit(&mut self, up: bool) {
        let last = FPS_LIMITS.len() - 1;
        // First entry >= current; `len` if current is above the whole list (e.g. imported 300).
        let ix = FPS_LIMITS
            .iter()
            .position(|&l| l >= self.fps_limit)
            .unwrap_or(FPS_LIMITS.len());
        let exact = FPS_LIMITS.get(ix) == Some(&self.fps_limit);
        let ix = match (up, exact) {
            (true, true) => (ix + 1).min(last),
            (true, false) => ix.min(last),
            (false, _) => ix.saturating_sub(1),
        };
        self.fps_limit = FPS_LIMITS[ix];
    }

    /// Renders the `MangoHud.conf` contents.
    pub fn render(&self) -> String {
        let mut out = String::from(
            "# Generated by Arkor. Change it from Arkor, edits here are overwritten.\n",
        );
        let mut line = |k: &str, v: &str| {
            if v.is_empty() {
                out.push_str(k);
            } else {
                out.push_str(&format!("{k}={v}"));
            }
            out.push('\n');
        };
        line("legacy_layout", "0");
        line("position", self.position.key());
        line("font_size", &self.font_size.to_string());
        line(
            "background_alpha",
            &format!("{:.1}", self.background_alpha as f32 / 100.0),
        );
        if self.orientation == Orientation::Horizontal {
            line("horizontal", "");
        }
        if self.fps_limit > 0 {
            line("fps_limit", &self.fps_limit.to_string());
        }
        for (k, v) in &self.extra {
            line(k, v);
        }
        for m in &self.metrics {
            line(m.key(), "");
        }
        out
    }

    /// Imports an existing MangoHud.conf (e.g. one written by Goverlay).
    /// Keys Arkor models become fields; everything else is kept in `extra`.
    pub fn import(conf: &str) -> Self {
        let mut s = Self {
            metrics: BTreeSet::new(),
            extra: BTreeMap::new(),
            ..Self::default()
        };
        for raw in conf.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = match line.split_once('=') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => (line, ""),
            };
            if let Some(m) = Metric::from_key(key) {
                // `fps=0` style disables are legal in MangoHud.
                if value != "0" {
                    s.metrics.insert(m);
                }
                continue;
            }
            match key {
                "legacy_layout" | "full" | "no_display" => {}
                "horizontal" => {
                    if value != "0" {
                        s.orientation = Orientation::Horizontal;
                    }
                }
                "position" => {
                    if let Some(p) = Position::ALL.into_iter().find(|p| p.key() == value) {
                        s.position = p;
                    }
                }
                "font_size" => s.font_size = value.parse().unwrap_or(s.font_size),
                "background_alpha" => {
                    if let Ok(a) = value.parse::<f32>() {
                        s.background_alpha = ((a * 100.0).round() as u32).min(100);
                    }
                }
                // MangoHud allows a list (`60,144`) cycled with a hotkey; Arkor keeps the first.
                "fps_limit" => {
                    s.fps_limit = value
                        .split(',')
                        .next()
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0)
                }
                _ => {
                    s.extra.insert(key.to_string(), value.to_string());
                }
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_detected() {
        let mut s = HudSettings::default();
        assert_eq!(s.matching_preset(), Some(Preset::Basic));
        s.apply_preset(Preset::FpsOnly);
        assert_eq!(s.matching_preset(), Some(Preset::FpsOnly));
        s.toggle_metric(Metric::Ram);
        assert_eq!(s.matching_preset(), None);
    }

    #[test]
    fn render_orders_metrics_by_hardware() {
        let mut s = HudSettings {
            extra: BTreeMap::new(),
            ..Default::default()
        };
        s.metrics = [Metric::Fps, Metric::CpuStats, Metric::GpuStats].into();
        s.orientation = Orientation::Horizontal;
        s.fps_limit = 144;
        let body: Vec<_> = s.render().lines().skip(1).map(str::to_owned).collect();
        assert_eq!(
            body,
            [
                "legacy_layout=0",
                "position=top-left",
                "font_size=24",
                "background_alpha=0.4",
                "horizontal",
                "fps_limit=144",
                "gpu_stats",
                "cpu_stats",
                "fps",
            ]
        );
    }

    #[test]
    fn imports_goverlay_config() {
        let s = HudSettings::import(
            "#### Goverlay ####\nlegacy_layout=0\nbackground_alpha=0.6\nfont_size=20\nposition=bottom-right\n\
             gpu_stats\ngpu_color=2E9762\ncpu_stats\nfps\nfps_limit=0,144\nblacklist=zenity,lact\nram=0\n",
        );
        assert_eq!(
            s.metrics,
            [Metric::GpuStats, Metric::CpuStats, Metric::Fps].into()
        );
        assert_eq!((s.font_size, s.background_alpha, s.fps_limit), (20, 60, 0));
        assert_eq!(s.position, Position::BottomRight);
        assert_eq!(s.extra["gpu_color"], "2E9762");
        assert_eq!(s.extra["blacklist"], "zenity,lact");
        assert!(!s.extra.contains_key("legacy_layout"));
    }

    #[test]
    fn steppers_clamp() {
        let mut s = HudSettings {
            fps_limit: 0,
            ..Default::default()
        };
        s.step_fps_limit(false);
        assert_eq!(s.fps_limit, 0);
        s.step_fps_limit(true);
        assert_eq!(s.fps_limit, 30);
        s.fps_limit = 100; // imported, not in the list
        s.step_fps_limit(true);
        assert_eq!(s.fps_limit, 120);
        s.fps_limit = 300;
        s.step_fps_limit(false);
        assert_eq!(s.fps_limit, 240);
        s.font_size = 48;
        s.step_font_size(true);
        assert_eq!(s.font_size, 48);
        s.background_alpha = 100;
        s.step_alpha(true);
        assert_eq!(s.background_alpha, 100);
    }

    #[test]
    fn toml_roundtrip() {
        let s = HudSettings::default();
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("\"gpu_stats\""), "{text}");
        assert_eq!(toml::from_str::<HudSettings>(&text).unwrap(), s);
    }
}
