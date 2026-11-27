//! Settings in console mode, in categories as on the desktop: a screen lists the categories,
//! a category its settings. A switch flips with A; a choice turns with left and right, or
//! opens as a list with A; a text opens the keyboard.

use arkor_core::library::GameKind;
use arkor_core::mangohud::{HudSettings, Metric, Orientation, Position, Preset};
use arkor_core::profile::{Theme, UpdateInterval};
use arkor_core::tweaks::Category;
use arkor_core::{HudMode, LsfgMode};
use arkor_launch::shops::Shop;

use super::Library;
use super::console::{Act, Item, Kind, Opt, Row, Screen, Value};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum GameOpt {
    Hud,
    HudPreset,
    HudPosition,
    HudOrientation,
    HudFont,
    HudAlpha,
    FpsLimit,
    Metric(Metric),
    Gamescope,
    GsOutput,
    GsGame,
    GsRefresh,
    GsFilter,
    GsFullscreen,
    GsHdr,
    Lsfg,
    LsfgMultiplier,
    LsfgFlow,
    LsfgPerformance,
    Mako,
    Afmf,
    NoSleep,
    VkBasalt,
    Cas,
    VkSumi,
    Proton,
    ProtonLog,
    LinuxRuntime,
    /// The emulator an emulator game plays with.
    Emulator,
    /// The graphics card the game renders on.
    Gpu,
    Winecfg,
    Winetricks,
    Tweak(&'static str),
    LaunchOptions,
    PreLaunch,
    PostLaunch,
    Hidden,
    Category,
    /// Typed on the keyboard: the game's name, a category of its own.
    Name,
    NewCategory,
}

impl GameOpt {
    /// Edited as text on the keyboard.
    pub(super) fn is_text(self) -> bool {
        matches!(
            self,
            GameOpt::LaunchOptions
                | GameOpt::PreLaunch
                | GameOpt::PostLaunch
                | GameOpt::Name
                | GameOpt::NewCategory
        )
    }
}

/// A setting on its line: what it is, its icon and its name.
type Setting = (GameOpt, &'static str, String);

/// The categories of a game's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum GameCat {
    /// More FPS, smoother motion, less delay (the desktop's "Smoother & faster").
    Smooth,
    Picture,
    Overlay,
    Compatibility,
    Starting,
    Game,
}

impl GameCat {
    pub(super) const ALL: [GameCat; 6] = [
        GameCat::Smooth,
        GameCat::Picture,
        GameCat::Overlay,
        GameCat::Compatibility,
        GameCat::Starting,
        GameCat::Game,
    ];
}

/// The categories of Arkor's own settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum AppCat {
    /// Arkor's own updates.
    Arkor,
    General,
    /// The computer: its network, Bluetooth, sound, screens and power.
    Network,
    Bluetooth,
    Sound,
    Display,
    Power,
    /// Gamepads: which are there, their buttons.
    Controllers,
    Looks,
    Games,
    /// Emulators and the BIOS folder.
    Emulators,
    Stores,
    Proton,
    Tools,
    System,
}

impl AppCat {
    pub(super) const ALL: [AppCat; 15] = [
        AppCat::Arkor,
        AppCat::General,
        AppCat::Network,
        AppCat::Bluetooth,
        AppCat::Controllers,
        AppCat::Sound,
        AppCat::Display,
        AppCat::Power,
        AppCat::Looks,
        AppCat::Games,
        AppCat::Emulators,
        AppCat::Proton,
        AppCat::Stores,
        AppCat::Tools,
        AppCat::System,
    ];
}

/// The measures offered in console mode (the desktop's editor has them all).
const METRICS: [Metric; 13] = [
    Metric::Fps,
    Metric::FrameTiming,
    Metric::GpuStats,
    Metric::GpuTemp,
    Metric::GpuPower,
    Metric::Vram,
    Metric::CpuStats,
    Metric::CpuTemp,
    Metric::Ram,
    Metric::Resolution,
    Metric::ShowFpsLimit,
    Metric::Fsr,
    Metric::Hdr,
];

const RUNTIMES: [&str; 5] = [
    "umu-steamrt4",
    "umu-sniper",
    "umu-soldier",
    "umu-scout",
    "disable-runtime",
];

const RESOLUTIONS: [&str; 6] = [
    "1280x720",
    "1600x900",
    "1920x1080",
    "2560x1440",
    "3440x1440",
    "3840x2160",
];
const REFRESH: [u32; 7] = [60, 75, 90, 120, 144, 165, 240];
const FPS: [u32; 8] = [30, 40, 45, 60, 90, 120, 144, 165];

fn size_text(w: Option<u32>, h: Option<u32>) -> Option<String> {
    Some(format!("{}x{}", w?, h?))
}

fn parse_size(text: &str) -> (Option<u32>, Option<u32>) {
    match text.split_once('x') {
        Some((w, h)) => (w.parse().ok(), h.parse().ok()),
        None => (None, None),
    }
}

impl Library {
    fn position_name(&self, p: Position) -> &'static str {
        match p {
            Position::TopLeft => self.tr("Top left", "Sol üst"),
            Position::TopCenter => self.tr("Top", "Üst orta"),
            Position::TopRight => self.tr("Top right", "Sağ üst"),
            Position::MiddleLeft => self.tr("Left", "Sol orta"),
            Position::MiddleRight => self.tr("Right", "Sağ orta"),
            Position::BottomLeft => self.tr("Bottom left", "Sol alt"),
            Position::BottomCenter => self.tr("Bottom", "Alt orta"),
            Position::BottomRight => self.tr("Bottom right", "Sağ alt"),
        }
    }

    fn preset_name(&self, p: Preset) -> &'static str {
        match p {
            Preset::FpsOnly => self.tr("FPS only", "Yalnızca FPS"),
            Preset::Basic => self.tr("Basic", "Temel"),
            Preset::BasicHorizontal => self.tr("Basic, in a line", "Temel, yatay"),
            Preset::Full => self.tr("Everything", "Her şey"),
        }
    }

    /// The layout the game shows: its own, or the shared one.
    fn hud(&self) -> &HudSettings {
        match self.profile.mangohud.mode {
            HudMode::Custom => self
                .profile
                .mangohud
                .settings
                .as_ref()
                .unwrap_or(&self.global.mangohud.settings),
            _ => &self.global.mangohud.settings,
        }
    }

    /// Changes the layout the game shows; the shared one is saved with the settings.
    fn edit_shown_hud(&mut self, f: impl FnOnce(&mut HudSettings)) {
        if self.profile.mangohud.mode == HudMode::Custom {
            let global = self.global.mangohud.settings.clone();
            f(self.profile.mangohud.settings.get_or_insert(global));
        } else {
            f(&mut self.global.mangohud.settings);
            let r = self.global.save();
            self.report(r);
        }
    }

    /// The choices of `o` as (label, key); `None` for a switch or a text.
    pub(super) fn game_opt_choices(&self, o: GameOpt) -> Option<Vec<(String, String)>> {
        let s = |v: &str| v.to_string();
        let native = self.tr("Native", "Ekranın kendisi");
        Some(match o {
            GameOpt::Hud => vec![
                (s(self.tr("Off", "Kapalı")), s("off")),
                (s(self.tr("Shared layout", "Ortak düzen")), s("global")),
                (s(self.tr("Its own layout", "Kendi düzeni")), s("custom")),
            ],
            GameOpt::HudPreset => Preset::ALL
                .into_iter()
                .enumerate()
                .map(|(i, p)| (s(self.preset_name(p)), i.to_string()))
                .collect(),
            GameOpt::HudPosition => [
                Position::TopLeft,
                Position::TopCenter,
                Position::TopRight,
                Position::MiddleLeft,
                Position::MiddleRight,
                Position::BottomLeft,
                Position::BottomCenter,
                Position::BottomRight,
            ]
            .into_iter()
            .map(|p| (s(self.position_name(p)), s(p.key())))
            .collect(),
            GameOpt::FpsLimit => std::iter::once((s(self.tr("No limit", "Sınırsız")), s("0")))
                .chain(FPS.iter().map(|n| (n.to_string(), n.to_string())))
                .collect(),
            GameOpt::GsOutput => std::iter::once((s(native), String::new()))
                .chain(RESOLUTIONS.iter().map(|r| (s(r), s(r))))
                .collect(),
            GameOpt::GsGame => std::iter::once((
                s(self.tr("Same as the output", "Çıkışla aynı")),
                String::new(),
            ))
            .chain(RESOLUTIONS.iter().map(|r| (s(r), s(r))))
            .collect(),
            GameOpt::GsRefresh => std::iter::once((s(native), String::new()))
                .chain(REFRESH.iter().map(|n| (format!("{n} Hz"), n.to_string())))
                .collect(),
            GameOpt::GsFilter => std::iter::once((s(self.tr("None", "Yok")), String::new()))
                .chain(
                    arkor_core::profile::GAMESCOPE_FILTERS
                        .iter()
                        .map(|f| (f.to_uppercase(), s(f))),
                )
                .collect(),
            GameOpt::HudOrientation => vec![
                (s(self.tr("Down", "Dikey")), s("v")),
                (s(self.tr("Across", "Yatay")), s("h")),
            ],
            GameOpt::HudFont => (16..=36)
                .step_by(2)
                .map(|n| (n.to_string(), n.to_string()))
                .collect(),
            GameOpt::HudAlpha => (0..=100)
                .step_by(10)
                .map(|n| (format!("%{n}"), n.to_string()))
                .collect(),
            GameOpt::Cas => (0..=100)
                .step_by(10)
                .map(|n| (format!("%{n}"), n.to_string()))
                .collect(),
            GameOpt::Emulator => self
                .entry()
                .map(|e| self.emu_game_choices(e))
                .unwrap_or_default(),
            GameOpt::Gpu => std::iter::once((s(self.tr("Automatic", "Otomatik")), String::new()))
                .chain(self.tools.cards.iter().map(|c| (c.label(), c.slot.clone())))
                .collect(),
            GameOpt::LinuxRuntime => std::iter::once((
                s(self.tr("Steam Runtime 4 (default)", "Steam Runtime 4 (varsayılan)")),
                String::new(),
            ))
            .chain(RUNTIMES.iter().skip(1).map(|r| {
                let label = match *r {
                    "disable-runtime" => self.tr("None (run directly)", "Yok (doğrudan çalıştır)"),
                    "umu-sniper" => "Steam Runtime 3 (sniper)",
                    "umu-soldier" => "Steam Runtime 2 (soldier)",
                    _ => "Steam Runtime 1 (scout)",
                };
                (s(label), s(r))
            }))
            .collect(),
            GameOpt::Category => {
                let mut out = vec![(s(self.tr("None", "Yok")), String::new())];
                out.extend(self.categories().into_iter().map(|c| (c.clone(), c)));
                out
            }
            GameOpt::LsfgMultiplier => (2..=4).map(|n| (format!("{n}×"), n.to_string())).collect(),
            GameOpt::LsfgFlow => [25u32, 40, 50, 60, 75, 85, 100]
                .iter()
                .map(|n| (format!("%{n}"), n.to_string()))
                .collect(),
            GameOpt::Proton => {
                let mut out = vec![(s(self.tr("Automatic", "Otomatik")), String::new())];
                for l in arkor_launch::components::Latest::ALL {
                    out.push((s(l.dir_name()), s(l.dir_name())));
                }
                for (name, _) in &self.tools.protons {
                    if !out.iter().any(|(_, k)| k == name) {
                        out.push((name.clone(), name.clone()));
                    }
                }
                out
            }
            _ => return None,
        })
    }

    /// The current key of a choice.
    fn game_opt_key(&self, o: GameOpt) -> String {
        let p = &self.profile;
        let g = &p.gamescope;
        match o {
            GameOpt::Hud => match p.mangohud.mode {
                HudMode::Off => "off",
                HudMode::Global => "global",
                HudMode::Custom => "custom",
            }
            .into(),
            GameOpt::HudPreset => Preset::ALL
                .iter()
                .position(|pr| pr.metrics() == self.hud().metrics)
                .map(|i| i.to_string())
                .unwrap_or_default(),
            GameOpt::HudPosition => self.hud().position.key().into(),
            GameOpt::FpsLimit => self.hud().fps_limit.to_string(),
            GameOpt::GsOutput => size_text(g.width, g.height).unwrap_or_default(),
            GameOpt::GsGame => size_text(g.game_width, g.game_height).unwrap_or_default(),
            GameOpt::GsRefresh => g.refresh.map(|r| r.to_string()).unwrap_or_default(),
            GameOpt::GsFilter => g.filter.clone().unwrap_or_default(),
            GameOpt::HudOrientation => match self.hud().orientation {
                Orientation::Vertical => "v".into(),
                Orientation::Horizontal => "h".into(),
            },
            GameOpt::HudFont => self.hud().font_size.to_string(),
            GameOpt::HudAlpha => (self.hud().background_alpha / 10 * 10).to_string(),
            GameOpt::Cas => (p.vkbasalt.cas_sharpness / 10 * 10).to_string(),
            GameOpt::LinuxRuntime => p.linux_runtime.clone().unwrap_or_default(),
            GameOpt::Gpu => p.gpu.clone().unwrap_or_default(),
            GameOpt::Emulator => match self.entry().map(|e| &e.kind) {
                Some(GameKind::Emulated { emulator, .. }) => emulator.clone().unwrap_or_default(),
                _ => String::new(),
            },
            GameOpt::Category => self
                .entry()
                .and_then(|e| e.category.clone())
                .unwrap_or_default(),
            GameOpt::LsfgMultiplier => p.lsfg.multiplier.to_string(),
            GameOpt::LsfgFlow => p.lsfg.flow_scale.to_string(),
            GameOpt::Proton => p.proton.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }

    pub(super) fn game_opt_value(&self, o: GameOpt) -> Value {
        let p = &self.profile;
        match o {
            GameOpt::Gamescope => Value::Switch(p.gamescope.enabled),
            GameOpt::GsFullscreen => Value::Switch(p.gamescope.fullscreen),
            GameOpt::GsHdr => Value::Switch(p.gamescope.hdr),
            GameOpt::Lsfg => Value::Switch(p.lsfg.mode == LsfgMode::On),
            GameOpt::LsfgPerformance => Value::Switch(p.lsfg.performance_mode),
            GameOpt::NoSleep => Value::Switch(p.no_sleep),
            GameOpt::VkBasalt => Value::Switch(p.vkbasalt.enabled),
            GameOpt::ProtonLog => Value::Switch(p.proton_log),
            GameOpt::Tweak(id) => Value::Switch(p.tweaks.contains(id)),
            GameOpt::Metric(m) => Value::Switch(self.hud().metrics.contains(&m)),
            GameOpt::Mako => Value::Switch(p.layers.mako.enabled),
            GameOpt::Afmf => Value::Switch(p.layers.afmf.enabled),
            GameOpt::VkSumi => Value::Switch(p.layers.vksumi),
            GameOpt::Hidden => Value::Switch(self.entry().is_some_and(|e| e.hidden)),
            GameOpt::Winecfg | GameOpt::Winetricks => Value::None,
            GameOpt::LaunchOptions | GameOpt::PreLaunch | GameOpt::PostLaunch => {
                let text = self.game_opt_text(o);
                Value::Text(if text.trim().is_empty() {
                    "—".into()
                } else {
                    text
                })
            }
            _ => {
                let key = self.game_opt_key(o);
                let label = self
                    .game_opt_choices(o)
                    .and_then(|c| c.into_iter().find(|(_, k)| *k == key).map(|(l, _)| l))
                    .unwrap_or_else(|| match o {
                        // Metrics picked by hand on the desktop.
                        GameOpt::HudPreset => self.tr("Custom", "Kendine özgü").into(),
                        _ if key.is_empty() => "—".into(),
                        _ => key.clone(),
                    });
                Value::Choice(label)
            }
        }
    }

    /// Flips a switch.
    pub(super) fn game_opt_toggle(&mut self, o: GameOpt) {
        let p = &mut self.profile;
        match o {
            GameOpt::Gamescope => p.gamescope.enabled = !p.gamescope.enabled,
            GameOpt::GsFullscreen => p.gamescope.fullscreen = !p.gamescope.fullscreen,
            GameOpt::GsHdr => p.gamescope.hdr = !p.gamescope.hdr,
            GameOpt::Lsfg => {
                p.lsfg.mode = if p.lsfg.mode == LsfgMode::On {
                    LsfgMode::Off
                } else {
                    LsfgMode::On
                };
                if p.lsfg.multiplier < 2 {
                    p.lsfg.multiplier = 2;
                }
            }
            GameOpt::LsfgPerformance => p.lsfg.performance_mode = !p.lsfg.performance_mode,
            GameOpt::NoSleep => p.no_sleep = !p.no_sleep,
            GameOpt::VkBasalt => p.vkbasalt.enabled = !p.vkbasalt.enabled,
            GameOpt::ProtonLog => p.proton_log = !p.proton_log,
            GameOpt::Tweak(id) => {
                if !p.tweaks.remove(id) {
                    p.tweaks.insert(id.to_string());
                }
            }
            GameOpt::Mako => p.layers.mako.enabled = !p.layers.mako.enabled,
            GameOpt::Afmf => p.layers.afmf.enabled = !p.layers.afmf.enabled,
            GameOpt::VkSumi => p.layers.vksumi = !p.layers.vksumi,
            GameOpt::Metric(m) => {
                self.edit_shown_hud(|h| {
                    if !h.metrics.remove(&m) {
                        h.metrics.insert(m);
                    }
                });
            }
            GameOpt::Hidden => {
                if let Some(e) = self.entry_mut() {
                    e.hidden = !e.hidden;
                }
                self.save_library();
                self.apply_filter();
                return;
            }
            _ => return,
        }
        self.save();
    }

    /// Sets a choice by its key.
    pub(super) fn game_opt_set(&mut self, o: GameOpt, key: &str) {
        let num = key.parse::<u32>().ok();
        match o {
            GameOpt::Hud => {
                let mode = match key {
                    "off" => HudMode::Off,
                    "custom" => HudMode::Custom,
                    _ => HudMode::Global,
                };
                if mode == HudMode::Custom && self.profile.mangohud.settings.is_none() {
                    self.profile.mangohud.settings = Some(self.global.mangohud.settings.clone());
                }
                // The shared layout shows nothing while MangoHud is off for all games.
                if mode == HudMode::Global {
                    self.global.mangohud.enabled = true;
                    let r = self.global.save();
                    self.report(r);
                }
                self.profile.mangohud.mode = mode;
            }
            GameOpt::HudPreset => {
                if let Some(p) = num.and_then(|i| Preset::ALL.get(i as usize).copied()) {
                    self.edit_shown_hud(|h| h.apply_preset(p));
                }
            }
            GameOpt::HudPosition => {
                let pos = [
                    Position::TopLeft,
                    Position::TopCenter,
                    Position::TopRight,
                    Position::MiddleLeft,
                    Position::MiddleRight,
                    Position::BottomLeft,
                    Position::BottomCenter,
                    Position::BottomRight,
                ]
                .into_iter()
                .find(|p| p.key() == key);
                if let Some(pos) = pos {
                    self.edit_shown_hud(|h| h.position = pos);
                }
            }
            GameOpt::FpsLimit => {
                let n = num.unwrap_or(0);
                self.edit_shown_hud(|h| h.fps_limit = n);
            }
            GameOpt::GsOutput => {
                let (w, h) = parse_size(key);
                let g = &mut self.profile.gamescope;
                (g.width, g.height) = (w, h);
            }
            GameOpt::GsGame => {
                let (w, h) = parse_size(key);
                let g = &mut self.profile.gamescope;
                (g.game_width, g.game_height) = (w, h);
            }
            GameOpt::GsRefresh => self.profile.gamescope.refresh = num,
            GameOpt::GsFilter => {
                self.profile.gamescope.filter = Some(key.to_string()).filter(|k| !k.is_empty())
            }
            GameOpt::HudOrientation => {
                let o = if key == "h" {
                    Orientation::Horizontal
                } else {
                    Orientation::Vertical
                };
                self.edit_shown_hud(|h| h.orientation = o);
            }
            GameOpt::HudFont => {
                if let Some(n) = num {
                    self.edit_shown_hud(|h| h.font_size = n);
                }
            }
            GameOpt::HudAlpha => {
                if let Some(n) = num {
                    self.edit_shown_hud(|h| h.background_alpha = n);
                }
            }
            GameOpt::Cas => {
                if let Some(n) = num {
                    self.profile.vkbasalt.cas_sharpness = n;
                    let effects = &mut self.profile.vkbasalt.effects;
                    if !effects.iter().any(|e| e == "cas") {
                        effects.push("cas".into());
                    }
                }
            }
            GameOpt::LinuxRuntime => {
                self.profile.linux_runtime = Some(key.to_string()).filter(|k| !k.is_empty())
            }
            GameOpt::Gpu => self.profile.gpu = Some(key.to_string()).filter(|k| !k.is_empty()),
            GameOpt::Emulator => {
                if let Some(e) = self.entry_mut()
                    && let GameKind::Emulated { emulator, .. } = &mut e.kind
                {
                    emulator.clone_from(&Some(key.to_string()).filter(|k| !k.is_empty()));
                }
                self.save_library();
                return;
            }
            GameOpt::Category => {
                if let Some(e) = self.entry_mut() {
                    e.category = Some(key.to_string()).filter(|k| !k.is_empty());
                }
                self.save_library();
                self.apply_filter();
                return;
            }
            GameOpt::LsfgMultiplier => {
                if let Some(n) = num {
                    self.profile.lsfg.multiplier = n;
                }
            }
            GameOpt::LsfgFlow => {
                if let Some(n) = num {
                    self.profile.lsfg.flow_scale = n;
                }
            }
            GameOpt::Proton => {
                self.profile.proton = Some(key.to_string()).filter(|k| !k.is_empty())
            }
            _ => return,
        }
        self.save();
    }

    /// The text a text setting holds.
    pub(super) fn game_opt_text(&self, o: GameOpt) -> String {
        if let Some(text) = self.library_text_of(o) {
            return text;
        }
        let p = &self.profile;
        match o {
            GameOpt::LaunchOptions => p.launch_options.clone(),
            GameOpt::PreLaunch => p.pre_launch.clone(),
            GameOpt::PostLaunch => p.post_launch.clone(),
            _ => String::new(),
        }
    }

    /// Keeps what was typed for a text setting.
    pub(super) fn game_opt_write(&mut self, o: GameOpt, text: &str) {
        let text = text.trim().to_string();
        let p = &mut self.profile;
        match o {
            GameOpt::LaunchOptions => p.launch_options = text,
            GameOpt::PreLaunch => p.pre_launch = text,
            GameOpt::PostLaunch => p.post_launch = text,
            GameOpt::Name | GameOpt::NewCategory => return self.library_text(o, &text),
            _ => return,
        }
        self.save();
    }

    /// What a game setting does, in one short sentence (under its line).
    fn game_opt_help(&self, o: GameOpt) -> &'static str {
        let t = |en, tr| self.tr(en, tr);
        match o {
            GameOpt::Tweak(id) => arkor_core::tweaks::find(id).map_or("", |tw| {
                if self.global.language == arkor_core::Language::Tr {
                    tw.help_tr
                } else {
                    tw.help_en
                }
            }),
            GameOpt::Hud => t(
                "FPS, temperatures and usage on top of the game.",
                "Oyunun üstünde FPS, sıcaklık ve kullanım.",
            ),
            GameOpt::HudPreset => t("How much it shows.", "Ne kadar bilgi gösterileceği."),
            GameOpt::HudPosition => t("Where on the screen.", "Ekranın neresinde duracağı."),
            GameOpt::HudOrientation => t("A column or a line.", "Sütun mu, satır mı."),
            GameOpt::HudFont => t("How big the text is.", "Yazının ne kadar büyük olacağı."),
            GameOpt::HudAlpha => t(
                "How dark the box behind it is.",
                "Arkasındaki kutunun ne kadar koyu olacağı.",
            ),
            GameOpt::FpsLimit => t(
                "Caps the frame rate: less heat and noise, steadier motion.",
                "Kare hızını sınırlar: daha az ısı ve gürültü, daha düzenli hareket.",
            ),
            GameOpt::Metric(_) => "",
            GameOpt::Gamescope => t(
                "Valve's game window: force a resolution, upscale, fullscreen, HDR.",
                "Valve'ın oyun penceresi: çözünürlüğü zorla, ölçekle, tam ekran, HDR.",
            ),
            GameOpt::GsOutput => t(
                "The size of the window on your screen.",
                "Pencerenin ekrandaki boyutu.",
            ),
            GameOpt::GsGame => t(
                "Lower than the output gives more FPS; it is scaled up.",
                "Çıkıştan düşük olursa FPS artar; görüntü büyütülür.",
            ),
            GameOpt::GsRefresh => t(
                "How many times a second the screen updates.",
                "Ekranın saniyede kaç kez yenileneceği.",
            ),
            GameOpt::GsFilter => t(
                "How the picture is scaled up (FSR is the sharpest).",
                "Görüntünün nasıl büyütüleceği (en keskini FSR).",
            ),
            GameOpt::GsFullscreen => t("No window frame.", "Pencere çerçevesi olmadan."),
            GameOpt::GsHdr => t("For HDR screens.", "HDR ekranlar için."),
            GameOpt::Lsfg => t(
                "Frame generation from the Lossless Scaling app (buy it on Steam).",
                "Lossless Scaling uygulamasının kare üretimi (Steam'den satın alınır).",
            ),
            GameOpt::LsfgMultiplier => t(
                "2x doubles the frame rate, 3x triples it.",
                "2x kare hızını ikiye, 3x üçe katlar.",
            ),
            GameOpt::LsfgFlow => t(
                "Lower is faster, higher looks better.",
                "Düşük daha hızlı, yüksek daha güzel görünür.",
            ),
            GameOpt::LsfgPerformance => t(
                "Lighter, for weaker graphics cards.",
                "Daha hafif, zayıf ekran kartları için.",
            ),
            GameOpt::Mako => t(
                "Free frame generation and upscaling.",
                "Ücretsiz kare üretimi ve ölçekleme.",
            ),
            GameOpt::Afmf => t(
                "AMD's frame generation for any game.",
                "AMD'nin her oyunda çalışan kare üretimi.",
            ),
            GameOpt::NoSleep => t(
                "The screen does not dim or sleep while you play.",
                "Oynarken ekran kararmaz ya da uykuya geçmez.",
            ),
            GameOpt::VkBasalt => t(
                "Sharpens the picture and adds effects (Vulkan and DXVK games).",
                "Görüntüyü keskinleştirir ve efekt ekler (Vulkan ve DXVK oyunları).",
            ),
            GameOpt::Cas => t(
                "How much sharper the picture gets.",
                "Görüntünün ne kadar keskinleşeceği.",
            ),
            GameOpt::VkSumi => t(
                "More vivid colours and contrast.",
                "Daha canlı renkler ve kontrast.",
            ),
            GameOpt::Proton => t(
                "What runs the Windows game. Automatic picks a good one.",
                "Windows oyununu çalıştıran yazılım. Otomatik iyi bir tanesini seçer.",
            ),
            GameOpt::ProtonLog => t(
                "Writes a detailed log, for finding problems.",
                "Sorun bulmak için ayrıntılı kayıt tutar.",
            ),
            GameOpt::LinuxRuntime => t(
                "A container that gives the game the libraries it expects.",
                "Oyuna beklediği kütüphaneleri veren bir kap.",
            ),
            GameOpt::Emulator => t(
                "Which emulator plays it.",
                "Oyunu hangi emülatörün oynatacağı.",
            ),
            GameOpt::Gpu => t(
                "Which graphics card the game uses.",
                "Oyunun hangi ekran kartını kullanacağı.",
            ),
            GameOpt::Winecfg => t(
                "Wine's own settings window.",
                "Wine'ın kendi ayar penceresi.",
            ),
            GameOpt::Winetricks => t(
                "Installs Windows parts some games need.",
                "Bazı oyunların istediği Windows parçalarını kurar.",
            ),
            GameOpt::LaunchOptions => t(
                "Extra words when the game starts, e.g. -dx11.",
                "Oyun başlarken ek kelimeler, ör. -dx11.",
            ),
            GameOpt::PreLaunch => t(
                "A command that runs before the game.",
                "Oyundan önce çalışan bir komut.",
            ),
            GameOpt::PostLaunch => t(
                "A command that runs after the game closes.",
                "Oyun kapandıktan sonra çalışan bir komut.",
            ),
            GameOpt::Hidden | GameOpt::Category | GameOpt::Name | GameOpt::NewCategory => "",
        }
    }

    fn game_cat_title(&self, c: GameCat) -> (&'static str, &'static str) {
        match c {
            GameCat::Smooth => ("zap", self.tr("Smoother & faster", "Daha akıcı ve hızlı")),
            GameCat::Picture => ("palette", self.tr("Picture", "Görüntü")),
            GameCat::Overlay => (
                "gauge",
                self.tr("Performance overlay", "Performans göstergesi"),
            ),
            GameCat::Compatibility => ("wine", self.tr("Compatibility", "Uyumluluk")),
            GameCat::Starting => ("terminal", self.tr("Starting", "Başlatma")),
            GameCat::Game => ("gamepad-2", self.tr("Game", "Oyun")),
        }
    }

    /// The categories that have something for this game.
    fn game_cats(&self) -> Vec<GameCat> {
        GameCat::ALL
            .into_iter()
            .filter(|c| !self.game_cat_rows(*c).is_empty())
            .collect()
    }

    /// A game's settings: its categories, with what is switched on in each.
    pub(super) fn game_setting_rows(&self, id: &str) -> Vec<Row> {
        if self.entry().is_none_or(|e| e.id != id) {
            return Vec::new();
        }
        let mut rows: Vec<Row> = self
            .game_cats()
            .into_iter()
            .map(|c| {
                let (icon, title) = self.game_cat_title(c);
                let on = self
                    .game_cat_rows(c)
                    .iter()
                    // The measures shown on the overlay are not switched-on features.
                    .filter(|r| !matches!(r.items[0].act, Act::Opt(Opt::Game(GameOpt::Metric(_)))))
                    .filter(|r| {
                        matches!(
                            r.items[0].look,
                            super::console::Look::Line {
                                value: Value::Switch(true),
                                ..
                            }
                        )
                    })
                    .count();
                let summary = match on {
                    0 => String::new(),
                    n => format!("{n} {}", self.tr("on", "açık")),
                };
                Row::new(
                    Kind::Line,
                    vec![Item::line(
                        Act::Screen(Screen::GameCat(id.to_string(), c)),
                        icon,
                        title,
                        Value::Text(summary),
                    )],
                )
            })
            .collect();
        rows.extend(self.emu_achievement_row(id));
        rows.push(Row::new(
            Kind::Line,
            vec![Item::line(
                Act::Screen(Screen::Trouble(id.to_string())),
                "stethoscope",
                self.tr("It does not start?", "Açılmıyor mu?"),
                Value::None,
            )],
        ));
        rows
    }

    /// The settings of one category.
    pub(super) fn game_cat_rows(&self, c: GameCat) -> Vec<Row> {
        let Some(g) = self.entry() else {
            return Vec::new();
        };
        let steam = matches!(g.kind, GameKind::Steam { .. });
        let linux = g.kind == GameKind::Linux;
        let prefix = g.needs_prefix() && !steam;
        let p = &self.profile;
        let t = |en, tr| self.tr(en, tr);
        let tr = self.global.language == arkor_core::Language::Tr;
        let gpus = self.tools.gpus;
        let tweaks = |c: Category| -> Vec<(GameOpt, &'static str, String)> {
            self.tweaks_of(c)
                .into_iter()
                .map(|tw| {
                    let label = if tr { tw.label_tr } else { tw.label_en };
                    (GameOpt::Tweak(tw.id), "toggle-right", label.to_string())
                })
                .collect()
        };
        // The title of a category, as on the desktop.
        let gt = |key: &str| {
            super::features::group(key).map(|g| self.tr(g.name.0, g.name.1).to_string())
        };
        let o = |opt: GameOpt, icon: &'static str, label: &str| (opt, icon, label.to_string());
        // (setting, icon, label) with the title of the group it begins.
        let mut groups: Vec<(Option<String>, Vec<Setting>)> = Vec::new();
        match c {
            GameCat::Overlay => {
                let mut v = vec![o(GameOpt::Hud, "gauge", "MangoHud")];
                if p.mangohud.mode != HudMode::Off {
                    v.extend([
                        o(GameOpt::HudPreset, "layout-list", t("Layout", "Düzen")),
                        o(GameOpt::HudPosition, "move", t("Position", "Konum")),
                        o(GameOpt::HudOrientation, "columns-2", t("Direction", "Yön")),
                        o(GameOpt::HudFont, "type", t("Text size", "Yazı boyutu")),
                        o(GameOpt::HudAlpha, "square", t("Background", "Arka plan")),
                        o(GameOpt::FpsLimit, "timer", t("FPS limit", "FPS sınırı")),
                    ]);
                }
                groups.push((None, v));
                if p.mangohud.mode != HudMode::Off {
                    let names = [
                        "FPS",
                        t("Frame time graph", "Kare süresi grafiği"),
                        t("GPU load", "GPU kullanımı"),
                        t("GPU temperature", "GPU sıcaklığı"),
                        t("GPU power", "GPU gücü"),
                        "VRAM",
                        t("CPU load", "CPU kullanımı"),
                        t("CPU temperature", "CPU sıcaklığı"),
                        "RAM",
                        t("Resolution", "Çözünürlük"),
                        t("FPS limit", "FPS sınırı"),
                        "FSR",
                        "HDR",
                    ];
                    groups.push((
                        Some(t("Shown", "Gösterilenler").to_string()),
                        METRICS
                            .iter()
                            .zip(names)
                            .map(|(m, n)| o(GameOpt::Metric(*m), "activity", n))
                            .collect(),
                    ));
                }
            }
            GameCat::Smooth => {
                if prefix || steam {
                    groups.push((gt("upscale"), tweaks(Category::Upscaling)));
                }
                let mut fg = vec![o(GameOpt::Lsfg, "layers", "Lossless Scaling (lsfg-vk)")];
                if p.lsfg.mode == LsfgMode::On {
                    fg.extend([
                        o(GameOpt::LsfgMultiplier, "layers", t("Multiplier", "Çarpan")),
                        o(GameOpt::LsfgFlow, "gauge", t("Flow scale", "Akış ölçeği")),
                        o(
                            GameOpt::LsfgPerformance,
                            "zap",
                            t("Performance mode", "Performans modu"),
                        ),
                    ]);
                }
                fg.push(o(GameOpt::Mako, "layers", "MAKO"));
                if gpus.amd {
                    fg.push(o(GameOpt::Afmf, "layers", "AFMF"));
                }
                fg.extend(tweaks(Category::FrameGen));
                groups.push((gt("framegen"), fg));
                groups.push((gt("latency"), tweaks(Category::Latency)));
                let mut st = vec![o(
                    GameOpt::NoSleep,
                    "coffee",
                    t("Keep the computer awake", "Uykuyu engelle"),
                )];
                st.extend(tweaks(Category::Stutter));
                groups.push((gt("stutter"), st));
            }
            GameCat::Picture => {
                let mut v = vec![o(GameOpt::VkBasalt, "wand-sparkles", "vkBasalt")];
                if p.vkbasalt.enabled {
                    v.push(o(
                        GameOpt::Cas,
                        "sparkles",
                        t("Sharpening (CAS)", "Keskinleştirme (CAS)"),
                    ));
                }
                v.push(o(
                    GameOpt::VkSumi,
                    "palette",
                    t("Colour grading (vkSumi)", "Renk düzenleme (vkSumi)"),
                ));
                groups.push((gt("effects"), v));
                let mut sc = vec![o(GameOpt::Gamescope, "monitor", "Gamescope")];
                if p.gamescope.enabled {
                    sc.extend([
                        o(
                            GameOpt::GsOutput,
                            "monitor",
                            t("Output size", "Çıkış çözünürlüğü"),
                        ),
                        o(
                            GameOpt::GsGame,
                            "scaling",
                            t("Game renders at", "Oyunun çözünürlüğü"),
                        ),
                        o(
                            GameOpt::GsRefresh,
                            "activity",
                            t("Refresh rate", "Yenileme hızı"),
                        ),
                        o(
                            GameOpt::GsFilter,
                            "sparkles",
                            t("Upscaling filter", "Ölçekleme filtresi"),
                        ),
                        o(
                            GameOpt::GsFullscreen,
                            "maximize",
                            t("Fullscreen", "Tam ekran"),
                        ),
                        o(GameOpt::GsHdr, "sun", "HDR"),
                    ]);
                }
                if prefix || steam {
                    sc.extend(tweaks(Category::Display));
                }
                groups.push((gt("screen"), sc));
            }
            GameCat::Compatibility => {
                let mut v = Vec::new();
                if prefix {
                    v.extend([
                        o(GameOpt::Proton, "wine", "Proton"),
                        o(
                            GameOpt::ProtonLog,
                            "logs",
                            t("Proton log", "Proton günlüğü"),
                        ),
                    ]);
                }
                if matches!(g.kind, GameKind::Emulated { .. }) {
                    v.push(o(GameOpt::Emulator, "joystick", t("Emulator", "Emülatör")));
                }
                if linux {
                    v.push(o(
                        GameOpt::LinuxRuntime,
                        "package",
                        t("Runtime", "Çalışma ortamı"),
                    ));
                }
                groups.push((gt("proton"), v));
                if prefix || steam {
                    groups.push((gt("fixes"), tweaks(Category::Fixes)));
                }
                let mut gpu = Vec::new();
                if self.tools.cards.len() > 1 {
                    gpu.push(o(GameOpt::Gpu, "cpu", t("Graphics card", "Ekran kartı")));
                }
                gpu.extend(tweaks(Category::Gpu));
                groups.push((gt("gpu"), gpu));
                if prefix || steam {
                    groups.push((gt("pads"), tweaks(Category::Input)));
                }
                let mut tools = Vec::new();
                if prefix {
                    tools.extend([
                        o(GameOpt::Winecfg, "settings", "winecfg"),
                        o(GameOpt::Winetricks, "wrench", "winetricks"),
                    ]);
                }
                tools.extend(tweaks(Category::Sharing));
                groups.push((gt("tools"), tools));
            }
            GameCat::Starting => groups.push((
                None,
                vec![
                    o(
                        GameOpt::LaunchOptions,
                        "terminal",
                        t("Launch options", "Başlatma seçenekleri"),
                    ),
                    o(
                        GameOpt::PreLaunch,
                        "arrow-right-to-line",
                        t("Command before", "Önce çalışacak komut"),
                    ),
                    o(
                        GameOpt::PostLaunch,
                        "arrow-right-from-line",
                        t("Command after", "Sonra çalışacak komut"),
                    ),
                ],
            )),
            GameCat::Game => groups.push((
                None,
                vec![
                    o(GameOpt::Category, "folder", t("Category", "Kategori")),
                    o(
                        GameOpt::Hidden,
                        "eye-off",
                        t("Hidden in the library", "Kütüphanede gizli"),
                    ),
                ],
            )),
        }
        let mut rows = Vec::new();
        for (title, items) in groups {
            for (n, (opt, icon, label)) in items.into_iter().enumerate() {
                let mut item = Item::line(
                    Act::Opt(Opt::Game(opt)),
                    icon,
                    label,
                    self.game_opt_value(opt),
                );
                item.sub = self.game_opt_help(opt).to_string();
                let mut row = Row::new(Kind::Line, vec![item]);
                if n == 0 {
                    row.title = title.clone();
                }
                rows.push(row);
            }
        }
        if c == GameCat::Game {
            rows.push(Row::new(
                Kind::Line,
                vec![Item::line(
                    Act::DesktopPage(g.game_id().to_string()),
                    "monitor",
                    t(
                        "Everything else on the desktop",
                        "Geri kalan her şey masaüstünde",
                    ),
                    Value::None,
                )],
            ));
        }
        rows
    }

    pub(super) fn game_cat_name(&self, c: GameCat) -> &'static str {
        self.game_cat_title(c).1
    }

    // ---- Arkor's own settings ----

    fn app_cat_title(&self, c: AppCat) -> (&'static str, &'static str) {
        match c {
            AppCat::Arkor => ("sparkles", self.tr("Arkor update", "Arkor güncellemesi")),
            AppCat::General => ("settings", self.tr("General", "Genel")),
            AppCat::Network => ("wifi", self.tr("Network", "Ağ")),
            AppCat::Bluetooth => ("bluetooth", "Bluetooth"),
            AppCat::Controllers => ("gamepad-2", self.tr("Gamepads", "Kollar")),
            AppCat::Sound => ("volume-2", self.tr("Sound", "Ses")),
            AppCat::Display => ("monitor", self.tr("Screen", "Ekran")),
            AppCat::Power => ("gauge", self.tr("Power", "Güç")),
            AppCat::Looks => ("palette", self.tr("Looks", "Görünüm")),
            AppCat::Games => ("gamepad-2", self.tr("Games", "Oyunlar")),
            AppCat::Emulators => ("joystick", self.tr("Emulators", "Emülatörler")),
            AppCat::Stores => ("store", self.tr("Stores", "Mağazalar")),
            AppCat::Proton => ("wine", self.tr("Proton & Wine", "Proton ve Wine")),
            AppCat::Tools => ("wrench", self.tr("Tools", "Araçlar")),
            AppCat::System => ("cpu", self.tr("System", "Sistem")),
        }
    }

    /// Arkor's settings: its categories.
    pub(super) fn setting_rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = AppCat::ALL
            .into_iter()
            // Screens are set through KDE's tool; on other desktops their own settings do it.
            .filter(|c| {
                *c != AppCat::Display || cfg!(test) || arkor_launch::system::has("kscreen-doctor")
            })
            .map(|c| {
                let (icon, title) = self.app_cat_title(c);
                Row::new(
                    Kind::Line,
                    vec![Item::line(
                        Act::Screen(Screen::SettingsCat(c)),
                        icon,
                        title,
                        match c {
                            AppCat::Arkor => Value::Text(self.console_arkor_state()),
                            c if super::console_system::sys_cat(c) => self.sys_summary(c),
                            AppCat::Controllers => self.pads_summary(),
                            _ => Value::None,
                        },
                    )],
                )
            })
            .collect();
        rows.push(Row::new(
            Kind::Line,
            vec![Item::line(
                Act::Desktop,
                "monitor",
                self.tr("Leave console mode", "Konsol modundan çık"),
                Value::None,
            )],
        ));
        rows
    }

    /// The settings of one of Arkor's categories.
    pub(super) fn app_cat_rows(&self, c: AppCat) -> Vec<Row> {
        let g = &self.global;
        let t = |en, tr| self.tr(en, tr);
        let line = |opt: Opt, icon, label: &str| {
            let value = self.opt_value(&opt);
            Row::new(
                Kind::Line,
                vec![Item::line(Act::Opt(opt), icon, label, value)],
            )
        };
        match c {
            AppCat::Arkor => {
                let mut rows = self.console_arkor_rows();
                rows.push(line(
                    Opt::Updates,
                    "refresh-cw",
                    t("Look for updates by itself", "Kendiliğinden denetle"),
                ));
                rows
            }
            AppCat::General => vec![
                line(Opt::Language, "languages", t("Language", "Dil")),
                line(
                    Opt::Autostart,
                    "power",
                    t("Start with the computer", "Bilgisayar açılınca başlat"),
                ),
                line(
                    Opt::StartHere,
                    "tv",
                    t("Start in console mode", "Konsol modunda başlat"),
                ),
                line(
                    Opt::Tray,
                    "panel-bottom",
                    t("Keep running in the tray", "Kapatınca tepside çalışsın"),
                ),
            ],
            AppCat::Looks => vec![
                line(Opt::Theme, "moon", t("Theme", "Tema")),
                line(Opt::Accent, "palette", t("Accent colour", "Vurgu rengi")),
                line(Opt::Motion, "zap", t("Reduce motion", "Hareketi azalt")),
                line(Opt::Sounds, "bell", t("Sounds", "Sesler")),
                line(
                    Opt::ConsoleVolume,
                    "volume-2",
                    t("Console sounds", "Konsol sesleri"),
                ),
                line(
                    Opt::ConsoleIntro,
                    "sparkles",
                    t("Start-up animation", "Giriş animasyonu"),
                ),
            ],
            AppCat::Games => {
                let with = |mut row: Row, sub: &str| {
                    row.items[0].sub = sub.to_string();
                    row
                };
                let mut rows = vec![
                    with(
                        line(
                            Opt::GlobalHud,
                            "gauge",
                            t("FPS overlay in every game", "Her oyunda FPS göstergesi"),
                        ),
                        t(
                            "MangoHud shows FPS and temperatures on top of games.",
                            "MangoHud oyunların üstünde FPS ve sıcaklığı gösterir.",
                        ),
                    ),
                    with(
                        line(
                            Opt::FullScreen,
                            "maximize",
                            t(
                                "Keep games full screen (gamescope)",
                                "Oyunları tam ekranda tut (gamescope)",
                            ),
                        ),
                        t(
                            "Games open full screen in Valve's game window, as on the Steam Deck.",
                            "Oyunlar Steam Deck'teki gibi Valve'ın oyun penceresinde tam ekran açılır.",
                        ),
                    ),
                    with(
                        line(
                            Opt::CloudSaves,
                            "cloud",
                            t(
                                "Saves follow the stores' clouds",
                                "Kayıtlar mağaza bulutuyla eşitlensin",
                            ),
                        ),
                        t(
                            "Your saves go to Epic's or GOG's cloud and come back on another computer.",
                            "Kayıtların Epic ya da GOG bulutuna gider, başka bir bilgisayarda geri gelir.",
                        ),
                    ),
                    with(
                        line(
                            Opt::DefaultProton,
                            "wine",
                            t("Proton for new games", "Yeni oyunlar için Proton"),
                        ),
                        t(
                            "What runs new Windows games. Automatic picks a good one.",
                            "Yeni Windows oyunlarını ne çalıştıracak. Otomatik iyi bir tanesini seçer.",
                        ),
                    ),
                    Row::new(
                        Kind::Line,
                        vec![Item::line(
                            Act::Folder(super::console::FolderFor::Games),
                            "folder",
                            t("Games folder", "Oyun klasörü"),
                            Value::Text(g.games_dir().display().to_string()),
                        )],
                    ),
                ];
                rows.extend(self.clip_rows());
                rows
            }
            AppCat::Emulators => self.emulator_rows(),
            AppCat::Proton => self.console_proton_rows(),
            AppCat::Tools => self.console_program_rows(),
            AppCat::System => self.console_system_rows(),
            AppCat::Network
            | AppCat::Bluetooth
            | AppCat::Sound
            | AppCat::Display
            | AppCat::Power => self.sys_rows(c),
            AppCat::Controllers => self.pads_rows(),
            AppCat::Stores => {
                let mut rows: Vec<Row> = Shop::ALL
                    .into_iter()
                    .map(|shop| {
                        let logged = self.shops.logged_in(shop);
                        let value = match self.shops.account(shop) {
                            Some(name) if shop == Shop::Steam => name.to_string(),
                            Some(name) => format!("{name} · {}", t("log out", "çıkış yap")),
                            None if logged => t("Log out", "Çıkış yap").to_string(),
                            None => t("Log in", "Giriş yap").to_string(),
                        };
                        let act = if shop == Shop::Steam {
                            Act::Nothing
                        } else if logged {
                            Act::Logout(shop)
                        } else {
                            Act::Login(shop)
                        };
                        Row::new(
                            Kind::Line,
                            vec![Item::line(
                                act,
                                super::shops_page::shop_icon(shop),
                                shop.name(),
                                Value::Text(value),
                            )],
                        )
                    })
                    .collect();
                let mut control = line(
                    Opt::SteamControl,
                    "sliders-horizontal",
                    t("Control Steam", "Steam'i yönet"),
                );
                control.items[0].sub = t(
                    "Downloads, installing, moving, achievements; opens Steam's developer port",
                    "İndirme, kurma, taşıma, başarımlar; Steam'in geliştirici kapısını açar",
                )
                .into();
                rows.push(control);
                if g.steam_control {
                    let mut guide = line(
                        Opt::GuideSteam,
                        "gamepad-2",
                        t(
                            "The middle button opens Steam",
                            "Orta tuş Steam'i öne getirsin",
                        ),
                    );
                    guide.items[0].sub = t(
                        "Off: it stays Arkor's, also in Steam's settings",
                        "Kapalı: Arkor'da kalır; Steam ayarlarında da kapanır",
                    )
                    .into();
                    rows.push(guide);
                }
                rows
            }
        }
    }

    // ---- Any setting, by what it is ----

    /// The choices of a setting as (label, key); `None` for switches and texts.
    pub(super) fn opt_choices(&self, o: &Opt) -> Option<Vec<(String, String)>> {
        if let Some(c) = self
            .sys_opt_choices(o)
            .or_else(|| self.quick_opt_choices(o))
            .or_else(|| self.pads_opt_choices(o))
            .or_else(|| self.idle_opt_choices(o))
            .or_else(|| self.clips_opt_choices(o))
        {
            return Some(c);
        }
        let s = |v: &str| v.to_string();
        Some(match o {
            Opt::Game(g) => return self.game_opt_choices(*g),
            Opt::Language => super::i18n::LANGUAGES
                .into_iter()
                .map(|(_, n)| (s(n), s(n)))
                .collect(),
            Opt::Theme => vec![
                (s(self.tr("Dark", "Koyu")), s("dark")),
                (s(self.tr("Light", "Açık")), s("light")),
                (
                    s(self.tr("Like the desktop", "Masaüstüyle aynı")),
                    s("system"),
                ),
            ],
            Opt::Accent => super::style::pal::ACCENTS
                .iter()
                .map(|(c, en, tr)| (s(self.tr(en, tr)), format!("{c:06x}")))
                .collect(),
            Opt::SteamFolder => {
                let mut out: Vec<(String, String)> = self
                    .console
                    .steam_folders
                    .iter()
                    .filter(|f| f.mounted)
                    .map(|f| (self.steam_folder_name(f), f.index.to_string()))
                    .collect();
                let new = match self.shops.game.as_ref().and_then(|g| g.steam_new.as_ref()) {
                    Some(dir) => dir.display().to_string(),
                    None => self.tr("Another folder…", "Başka bir klasör…").to_string(),
                };
                out.push((new, super::console::NEW_FOLDER.into()));
                out
            }
            Opt::ConsoleVolume => [0, 20, 40, 55, 70, 85, 100]
                .into_iter()
                .map(|v| {
                    let label = match v {
                        0 => s(self.tr("Off", "Kapalı")),
                        v => self.percent(v as f32 / 100.),
                    };
                    (label, v.to_string())
                })
                .collect(),
            Opt::Updates => vec![
                (s(self.tr("Every day", "Her gün")), s("daily")),
                (s(self.tr("Every week", "Her hafta")), s("weekly")),
                (
                    s(self.tr("Before each game", "Her oyundan önce")),
                    s("game"),
                ),
                (s(self.tr("Never", "Hiçbir zaman")), s("never")),
            ],
            Opt::InstallProton => {
                let auto = match &self.global.defaults.proton {
                    Some(d) => format!("{} ({d})", self.tr("Default", "Varsayılan")),
                    None => s(self.tr("Automatic", "Otomatik")),
                };
                let mut out = vec![(auto, String::new())];
                for l in arkor_launch::components::Latest::ALL {
                    out.push((s(l.dir_name()), s(l.dir_name())));
                }
                for (name, _) in &self.tools.protons {
                    if !out.iter().any(|(_, k)| k == name) {
                        out.push((name.clone(), name.clone()));
                    }
                }
                out
            }
            Opt::DefaultProton => {
                let mut out = vec![(s(self.tr("Automatic", "Otomatik")), String::new())];
                for l in arkor_launch::components::Latest::ALL {
                    out.push((s(l.dir_name()), s(l.dir_name())));
                }
                for (name, _) in &self.tools.protons {
                    if !out.iter().any(|(_, k)| k == name) {
                        out.push((name.clone(), name.clone()));
                    }
                }
                out
            }
            _ => return None,
        })
    }

    pub(super) fn opt_key(&self, o: &Opt) -> String {
        if let Some(k) = self
            .sys_opt_key(o)
            .or_else(|| self.quick_opt_key(o))
            .or_else(|| self.pads_opt_key(o))
            .or_else(|| self.idle_opt_key(o))
            .or_else(|| self.clips_opt_key(o))
        {
            return k;
        }
        let g = &self.global;
        match o {
            Opt::Game(x) => self.game_opt_key(*x),
            Opt::Language => super::i18n::LANGUAGES
                .into_iter()
                .find(|(l, _)| *l == g.language)
                .map(|(_, n)| n.to_string())
                .unwrap_or_default(),
            Opt::Theme => match g.appearance.theme {
                Theme::Dark => "dark",
                Theme::Light => "light",
                Theme::System => "system",
            }
            .into(),
            Opt::Accent => format!(
                "{:06x}",
                g.appearance
                    .accent
                    .unwrap_or(super::style::pal::DEFAULT_ACCENT)
            ),
            Opt::SteamFolder => {
                if self
                    .shops
                    .game
                    .as_ref()
                    .is_some_and(|g| g.steam_new.is_some())
                {
                    return super::console::NEW_FOLDER.into();
                }
                let chosen = self.shops.game.as_ref().and_then(|g| g.steam_folder);
                let usual = self.console.steam_folders.iter().find(|f| f.default);
                chosen
                    .or(usual.map(|f| f.index))
                    .unwrap_or_default()
                    .to_string()
            }
            Opt::ConsoleVolume => g.console_volume.unwrap_or(70).to_string(),
            Opt::Updates => match g.update_interval() {
                UpdateInterval::Daily => "daily",
                UpdateInterval::Weekly => "weekly",
                UpdateInterval::OnGameStart => "game",
                UpdateInterval::Never => "never",
            }
            .into(),
            Opt::DefaultProton => g.defaults.proton.clone().unwrap_or_default(),
            Opt::InstallProton => self
                .shops
                .game
                .as_ref()
                .and_then(|g| g.proton.clone())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// How a setting is shown at the end of its line.
    pub(super) fn opt_value(&self, o: &Opt) -> Value {
        if let Some(v) = self
            .sys_opt_value(o)
            .or_else(|| self.quick_opt_value(o))
            .or_else(|| self.clips_opt_value(o))
        {
            return v;
        }
        let g = &self.global;
        match o {
            Opt::Game(x) => self.game_opt_value(*x),
            Opt::StartHere => Value::Switch(g.start_in_console),
            Opt::Autostart => Value::Switch(g.autostart),
            Opt::Tray => Value::Switch(g.tray),
            Opt::Motion => Value::Switch(g.appearance.reduce_motion),
            Opt::Sounds => Value::Switch(g.appearance.sounds),
            Opt::ConsoleIntro => Value::Switch(!g.console_no_intro),
            Opt::SteamControl => Value::Switch(g.steam_control),
            Opt::GlobalHud => Value::Switch(g.mangohud.enabled),
            Opt::FullScreen => Value::Switch(!g.console_windowed),
            Opt::CloudSaves => Value::Switch(!g.no_cloud_saves),
            Opt::GuideSteam => Value::Switch(g.guide_opens_steam),
            Opt::InstallMenu => Value::Switch(self.shops.game.as_ref().is_some_and(|g| g.menu)),
            Opt::InstallDesktop => {
                Value::Switch(self.shops.game.as_ref().is_some_and(|g| g.desktop))
            }
            _ => {
                let key = self.opt_key(o);
                let label = self
                    .opt_choices(o)
                    .and_then(|c| c.into_iter().find(|(_, k)| *k == key).map(|(l, _)| l))
                    .unwrap_or(key);
                Value::Choice(label)
            }
        }
    }

    /// Sets a choice of any setting.
    pub(super) fn opt_set(&mut self, o: &Opt, key: &str) {
        if self.sys_opt_set(o, key)
            || self.quick_opt_set(o, key)
            || self.pads_opt_set(o, key)
            || self.idle_opt_set(o, key)
            || self.clips_opt_set(o, key)
        {
            return;
        }
        if *o == Opt::InstallProton {
            if let Some(g) = self.shops.game.as_mut() {
                g.proton = Some(key.to_string()).filter(|k| !k.is_empty());
            }
            return;
        }
        if *o == Opt::SteamFolder {
            if let Some(g) = self.shops.game.as_mut() {
                g.steam_folder = key.parse().ok();
                if g.steam_folder.is_some() {
                    g.steam_new = None;
                }
            }
            return;
        }
        let g = &mut self.global;
        match o {
            Opt::Game(x) => return self.game_opt_set(*x, key),
            Opt::Language => {
                if let Some((l, _)) = super::i18n::LANGUAGES.into_iter().find(|(_, n)| *n == key) {
                    g.language = l;
                    self.fields.clear();
                }
            }
            Opt::Theme => {
                g.appearance.theme = match key {
                    "light" => Theme::Light,
                    "system" => Theme::System,
                    _ => Theme::Dark,
                }
            }
            Opt::Accent => g.appearance.accent = u32::from_str_radix(key, 16).ok(),
            Opt::ConsoleVolume => g.console_volume = key.parse().ok(),
            Opt::Updates => {
                g.no_auto_updates = false;
                g.update_interval = match key {
                    "weekly" => UpdateInterval::Weekly,
                    "game" => UpdateInterval::OnGameStart,
                    "never" => UpdateInterval::Never,
                    _ => UpdateInterval::Daily,
                }
            }
            Opt::DefaultProton => {
                g.defaults.proton = Some(key.to_string()).filter(|k| !k.is_empty())
            }
            _ => return,
        }
        let r = self.global.save();
        self.report(r);
    }

    /// Flips a switch of Arkor's settings.
    pub(super) fn opt_toggle(&mut self, o: &Opt) {
        if self.sys_opt_toggle(o) || self.quick_opt_toggle(o) || self.clips_opt_toggle(o) {
            return;
        }
        if let Some(g) = self.shops.game.as_mut() {
            match o {
                Opt::InstallMenu => return g.menu = !g.menu,
                Opt::InstallDesktop => return g.desktop = !g.desktop,
                _ => {}
            }
        }
        let g = &mut self.global;
        match o {
            Opt::Game(x) => return self.game_opt_toggle(*x),
            Opt::StartHere => g.start_in_console = !g.start_in_console,
            Opt::Autostart => {
                g.autostart = !g.autostart;
                let on = g.autostart;
                if let Err(e) = super::tray::set_autostart(on) {
                    self.status = Some(format!("{e:#}").into());
                }
            }
            Opt::Tray => g.tray = !g.tray,
            Opt::Motion => g.appearance.reduce_motion = !g.appearance.reduce_motion,
            Opt::Sounds => g.appearance.sounds = !g.appearance.sounds,
            Opt::ConsoleIntro => g.console_no_intro = !g.console_no_intro,
            Opt::GlobalHud => g.mangohud.enabled = !g.mangohud.enabled,
            Opt::FullScreen => g.console_windowed = !g.console_windowed,
            Opt::CloudSaves => g.no_cloud_saves = !g.no_cloud_saves,
            Opt::GuideSteam => {
                g.guide_opens_steam = !g.guide_opens_steam;
                self.console.guide_told = false;
            }
            _ => return,
        }
        let r = self.global.save();
        self.report(r);
    }

    /// Turns a choice one step; `false` when it is no choice.
    pub(super) fn opt_turn(&mut self, o: &Opt, delta: i32) -> bool {
        if !Self::sys_turns(o) {
            return false;
        }
        let Some(choices) = self.opt_choices(o) else {
            return false;
        };
        let key = self.opt_key(o);
        let at = choices.iter().position(|(_, k)| *k == key).unwrap_or(0) as i32;
        let next = (at + delta).clamp(0, choices.len() as i32 - 1) as usize;
        if next as i32 != at {
            let key = choices[next].1.clone();
            self.opt_set(o, &key);
        }
        true
    }
}
