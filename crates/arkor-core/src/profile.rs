use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::mangohud::HudSettings;
use crate::{GameId, paths};

/// Per-game launch profile, stored as `~/.config/arkor/profiles/<store>-<id>.toml`.
/// Every field is optional so an empty file means "follow the global settings".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    /// Extra environment variables, applied last (they override everything Arkor sets).
    pub env: BTreeMap<String, String>,
    pub mangohud: GameHud,
    /// No longer used: Arkor does not start games through Feral GameMode (it clashes with
    /// the tuning of distributions such as CachyOS). Read so older files still load.
    #[serde(skip_serializing)]
    pub gamemode: bool,
    pub gamescope: Gamescope,
    /// Proton build for this game: a folder name from a compatibility tools dir
    /// (e.g. `GE-Proton10-25`) or an absolute path. `None` = automatic.
    /// Not used for Steam games, where Steam picks the Proton.
    pub proton: Option<String>,
    pub lsfg: Lsfg,
    /// Steam-style launch options: `KEY=VALUE` words become environment, other words wrap
    /// the game (e.g. `strace -f %command% -dx12`); `%command%` marks where the game goes.
    pub launch_options: String,
    /// Shell command run before the game starts (waited for).
    pub pre_launch: String,
    /// Shell command run after the game exits.
    pub post_launch: String,
    pub extra_app: ExtraApp,
    /// Keep the system from sleeping while the game runs (systemd-inhibit).
    pub no_sleep: bool,
    /// Write Proton's own log (PROTON_LOG) next to Arkor's launch log.
    pub proton_log: bool,
    /// umu database id (e.g. `umu-1086940`) so Proton applies the game's protonfixes.
    pub umu_id: Option<String>,
    /// Linux games only: the Steam Linux Runtime they run in through umu (`umu-steamrt4`,
    /// `umu-sniper`, `umu-soldier`, `umu-scout`), or `disable-runtime` to run directly.
    /// `None` = the default, Steam Runtime 4 (as Faugus Launcher does).
    pub linux_runtime: Option<String>,
    /// Enabled one-click switches, by id (see `tweaks::TWEAKS`).
    pub tweaks: std::collections::BTreeSet<String>,
    pub vkbasalt: crate::vkbasalt::VkBasalt,
    pub layers: crate::layers::Layers,
    /// Steam games only: what Arkor sets in Steam for the game.
    pub steam: SteamSettings,
    /// Whether the game's saves follow the store's cloud. `None` = as in Settings.
    pub cloud_saves: Option<bool>,
}

/// What Arkor writes into Steam's own settings for a Steam game (Steam starts it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SteamSettings {
    /// Compatibility tool, by Steam's name for it (`proton_experimental`, `GE-Proton10-25`).
    /// `None` leaves Steam's own choice; `Some("")` = Steam's default for the game.
    pub compat_tool: Option<String>,
    /// Leave Arkor out of the game's launch options: Steam gets only the launch options
    /// written here, and Arkor's other settings (MangoHud, lsfg-vk, environment) do not apply.
    pub no_arkor: bool,
}

/// A second program started together with the game (e.g. a trainer or mod manager).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExtraApp {
    pub path: Option<PathBuf>,
    /// Seconds to wait between the game and the extra app.
    pub delay_secs: u32,
    /// Start the extra app first, then the game.
    pub before_game: bool,
}

/// Split launch options into (environment, words before the game, words after the game).
pub fn parse_launch_options(text: &str) -> (Vec<(String, String)>, Vec<String>, Vec<String>) {
    let mut env = Vec::new();
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut seen_command = false;
    let mut in_env_section = true;
    for word in shell_words(text) {
        if word == "%command%" {
            seen_command = true;
            in_env_section = false;
            continue;
        }
        if in_env_section
            && let Some((k, v)) = word.split_once('=')
            && !k.is_empty()
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            env.push((k.to_string(), v.to_string()));
            continue;
        }
        in_env_section = false;
        if seen_command {
            after.push(word)
        } else {
            before.push(word)
        }
    }
    (env, before, after)
}

/// Minimal POSIX-like word splitting: spaces separate words, single/double quotes group,
/// backslash escapes the next character.
pub fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut has_word = false;
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') | (None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '\'') | (None, '"') => {
                quote = Some(c);
                has_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if has_word || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    has_word = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if has_word || !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// lsfg-vk (Lossless Scaling frame generation on Linux) for this game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Lsfg {
    pub mode: LsfgMode,
    /// Generated frames per real frame + 1: 2 doubles the frame rate.
    pub multiplier: u32,
    /// Motion estimation resolution in percent (25-100). Lower is faster, less accurate.
    pub flow_scale: u32,
    /// Lighter model: faster, slightly lower quality.
    pub performance_mode: bool,
    /// Executable names the game runs as (e.g. `FarCry6.exe`). When set, Arkor gives lsfg-vk a
    /// per-game config whose profile matches only these processes, so a launcher started
    /// first (Ubisoft Connect, EA app, ...) is left alone and the game itself gets frame
    /// generation. Empty = every Vulkan process of the launch.
    pub processes: Vec<String>,
    /// Frame pacing: `vsync` (default) or `none`.
    pub pacing: Option<String>,
    /// Let lsfg-vk choose the present mode (recommended).
    pub override_present_mode: bool,
    /// Half-precision math: faster, may glitch on some GPUs.
    pub fp16: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LsfgMode {
    /// No frame generation, even if lsfg-vk's own config file has a profile for this game.
    /// ("default", the old "leave it to lsfg-vk's config" choice, reads as this: Arkor
    /// manages lsfg-vk for its games.)
    #[default]
    #[serde(alias = "default")]
    Off,
    /// Arkor's settings, given to lsfg-vk for this game only.
    On,
}

pub const LSFG_MULTIPLIER_RANGE: (u32, u32) = (2, 5);
pub const LSFG_FLOW_SCALE_RANGE: (u32, u32) = (25, 100);

impl Default for Lsfg {
    fn default() -> Self {
        Self {
            mode: LsfgMode::Off,
            multiplier: 2,
            flow_scale: 100,
            performance_mode: false,
            processes: Vec::new(),
            pacing: None,
            override_present_mode: true,
            fp16: true,
        }
    }
}

/// `linux_runtime` value that runs a Linux game directly, without a runtime container.
pub const NO_RUNTIME: &str = "disable-runtime";
/// The runtime Linux games use unless chosen otherwise.
pub const DEFAULT_RUNTIME: &str = "umu-steamrt4";

impl Profile {
    /// The runtime a Linux game runs in, `None` when it runs directly.
    pub fn runtime(&self) -> Option<&str> {
        match self.linux_runtime.as_deref() {
            None | Some("") => Some(DEFAULT_RUNTIME),
            Some(NO_RUNTIME) => None,
            Some(r) => Some(r),
        }
    }
}

impl Lsfg {
    /// lsfg-vk 2.x config with one profile active only in `processes`; `None` if no process
    /// names are set.
    pub fn render_config(&self, dll: Option<&Path>) -> Option<String> {
        let names: Vec<String> = self
            .processes
            .iter()
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .flat_map(|p| {
                // lsfg-vk compares names as written; cover the common case variants.
                let mut v = vec![p.to_string()];
                if p.to_lowercase() != p {
                    v.push(p.to_lowercase());
                }
                v
            })
            .collect();
        if names.is_empty() {
            return None;
        }
        let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let mut out = String::from(
            "# Generated by Arkor for one game. Change it from Arkor.\nversion = 2\n\n[global]\n",
        );
        if let Some(dll) = dll {
            out.push_str(&format!("dll = {}\n", q(&dll.display().to_string())));
        }
        out.push_str(&format!(
            "allow_fp16 = {}\n\n[[profile]]\nname = \"arkor\"\n",
            self.fp16
        ));
        out.push_str(&format!(
            "active_in = [{}]\n",
            names.iter().map(|n| q(n)).collect::<Vec<_>>().join(", ")
        ));
        out.push_str(&format!("multiplier = {}\n", self.multiplier));
        out.push_str(&format!(
            "flow_scale = {:.2}\n",
            self.flow_scale as f32 / 100.0
        ));
        out.push_str(&format!("performance_mode = {}\n", self.performance_mode));
        out.push_str(&format!(
            "override_present_mode = {}\n",
            self.override_present_mode
        ));
        if let Some(p) = &self.pacing {
            out.push_str(&format!("pacing_mode = {}\n", q(p)));
        }
        Some(out)
    }

    pub fn step_multiplier(&mut self, up: bool) {
        let (min, max) = LSFG_MULTIPLIER_RANGE;
        self.multiplier = if up {
            self.multiplier + 1
        } else {
            self.multiplier.saturating_sub(1)
        }
        .clamp(min, max);
    }

    pub fn step_flow_scale(&mut self, up: bool) {
        let (min, max) = LSFG_FLOW_SCALE_RANGE;
        self.flow_scale = if up {
            self.flow_scale + 5
        } else {
            self.flow_scale.saturating_sub(5)
        }
        .clamp(min, max);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HudMode {
    /// Use the global HUD (shown only if it is enabled globally).
    #[default]
    Global,
    /// Use this game's own `settings`.
    Custom,
    /// Never show MangoHud for this game, even if MANGOHUD=1 is set system-wide.
    Off,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameHud {
    pub mode: HudMode,
    /// Only used in `Custom` mode; kept when switching away so it is not lost.
    pub settings: Option<HudSettings>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gamescope {
    pub enabled: bool,
    /// Output (window/screen) size.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Size the game renders at; gamescope upscales it to the output size.
    pub game_width: Option<u32>,
    pub game_height: Option<u32>,
    pub refresh: Option<u32>,
    pub fullscreen: bool,
    pub borderless: bool,
    /// Upscaling filter: `fsr`, `nis`, `linear`, `nearest` or `pixel`.
    pub filter: Option<String>,
    pub hdr: bool,
    /// Extra raw gamescope arguments.
    pub args: Vec<String>,
}

pub const GAMESCOPE_FILTERS: [&str; 5] = ["fsr", "nis", "linear", "nearest", "pixel"];

impl Gamescope {
    pub fn to_args(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut num = |flag: &str, v: Option<u32>| {
            if let Some(v) = v.filter(|v| *v > 0) {
                out.push(flag.to_string());
                out.push(v.to_string());
            }
        };
        num("-W", self.width);
        num("-H", self.height);
        num("-w", self.game_width);
        num("-h", self.game_height);
        num("-r", self.refresh);
        if self.fullscreen {
            out.push("-f".into());
        }
        if self.borderless {
            out.push("-b".into());
        }
        if let Some(f) = &self.filter {
            out.push("-F".into());
            out.push(f.clone());
        }
        if self.hdr {
            out.push("--hdr-enabled".into());
        }
        out.extend(self.args.iter().cloned());
        out
    }
}

/// Settings that apply to every game unless the game's profile overrides them.
/// Stored as `~/.config/arkor/global.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Global {
    pub language: Language,
    pub mangohud: GlobalHud,
    /// Where per-game Wine prefixes are created. `None` = `~/Games/Arkor/Prefixes`.
    pub prefixes_dir: Option<PathBuf>,
    /// Where store games are installed. `None` = `~/Games/Arkor/Games`.
    pub games_dir: Option<PathBuf>,
    /// Open in console mode (the full-screen gamepad interface).
    pub start_in_console: bool,
    /// Games started from console mode run as they are (not full screen in gamescope).
    pub console_windowed: bool,
    /// Console mode opens without its start-up animation.
    pub console_no_intro: bool,
    /// Steam's downloads are controlled through its developer port (off unless asked for:
    /// any program on the computer can use that port while it is open).
    pub steam_control: bool,
    /// The pad's middle button brings Steam forward (Steam's own setting, set through its
    /// port while it is controlled).
    pub guide_opens_steam: bool,
    /// Whose button marks console mode shows: `xbox`, `playstation`, `nintendo`
    /// (`None`: as the gamepad in use is).
    pub pad_glyphs: Option<String>,
    /// Saves of Epic and GOG games do not follow the store's cloud (they do unless turned off).
    pub no_cloud_saves: bool,
    /// Copies of every game's saves after it is played are switched off (written only when
    /// set, so older Arkor versions still read the file).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub no_save_backups: bool,
    /// How loud console mode's own sounds are, in percent (0 = silent, `None` = 70).
    pub console_volume: Option<u8>,
    /// Console mode darkens after this many minutes without input (`None`: 5, 0: never).
    pub console_dim_mins: Option<u32>,
    /// The computer sleeps after this many minutes without input in console mode, when
    /// nothing plays or downloads (`None` or 0: never).
    pub console_sleep_mins: Option<u32>,
    /// Environment for every game (each game's own `env` overrides it).
    pub env: BTreeMap<String, String>,
    /// Close Arkor's window after starting a game.
    pub close_on_launch: bool,
    /// Show a small "Starting <game>…" window while a game starts.
    pub splash: bool,
    /// Keep running in the system tray when the window is closed.
    pub tray: bool,
    /// Start with the desktop session (minimized to the tray).
    pub autostart: bool,
    pub discord: Discord,
    /// SteamGridDB API key (steamgriddb.com/profile/preferences/api) for cover art.
    pub steamgriddb_key: String,
    pub backup: crate::backup::BackupSettings,
    pub appearance: Appearance,
    /// Older switch: no automatic updates at all. Read as `UpdateInterval::Never`.
    pub no_auto_updates: bool,
    /// How often Arkor looks for new versions of what it downloaded (umu-launcher, the
    /// Steam Runtime, anti-cheat, "Latest" Protons, tools and Arkor itself).
    pub update_interval: UpdateInterval,
    /// When it last looked (Unix seconds).
    pub last_update_check: Option<u64>,
    /// What new games start with, and the Proton for games left on "Automatic".
    pub defaults: GameDefaults,
}

/// How often to look for updates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateInterval {
    #[default]
    Daily,
    Weekly,
    /// Before each game (like Faugus Launcher).
    OnGameStart,
    Never,
}

impl UpdateInterval {
    pub const ALL: [UpdateInterval; 4] = [
        UpdateInterval::Daily,
        UpdateInterval::Weekly,
        UpdateInterval::OnGameStart,
        UpdateInterval::Never,
    ];
}

impl Global {
    /// The update interval, with the older "no automatic updates" switch respected.
    pub fn update_interval(&self) -> UpdateInterval {
        if self.no_auto_updates {
            UpdateInterval::Never
        } else {
            self.update_interval
        }
    }

    /// Whether a check for updates is due at `now` (Unix seconds): daily or weekly since
    /// the last one. "On game start" and "Never" are not checked on a timer.
    pub fn updates_due(&self, now: u64) -> bool {
        let period = match self.update_interval() {
            UpdateInterval::Daily => 86_400,
            UpdateInterval::Weekly => 7 * 86_400,
            UpdateInterval::OnGameStart | UpdateInterval::Never => return false,
        };
        self.last_update_check
            .is_none_or(|last| now.saturating_sub(last) >= period)
    }

    /// Whether to look for updates right before a game starts.
    pub fn updates_on_launch(&self) -> bool {
        self.update_interval() == UpdateInterval::OnGameStart
    }
}

/// Defaults for games (Settings → Game defaults). `proton` applies at launch to every game
/// whose own Proton is "Automatic"; the rest is copied into a game's profile when it is
/// added (and can be copied into every game with "Apply to all games").
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameDefaults {
    /// Proton build name (as in `Profile::proton`). `None` = pick automatically.
    pub proton: Option<String>,
    /// No longer used (see `Profile::gamemode`); read so older files still load.
    #[serde(skip_serializing)]
    pub gamemode: bool,
    pub no_sleep: bool,
    /// Tweak ids switched on for new games (see `tweaks::TWEAKS`).
    pub tweaks: std::collections::BTreeSet<String>,
    pub launch_options: String,
}

impl GameDefaults {
    /// Copies the defaults into a game's profile (its other settings stay).
    pub fn apply_to(&self, profile: &mut Profile) {
        profile.no_sleep = self.no_sleep;
        profile.tweaks.extend(self.tweaks.iter().cloned());
        if profile.launch_options.trim().is_empty() {
            profile.launch_options = self.launch_options.clone();
        }
    }
}

/// Look and feel of Arkor's window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub theme: Theme,
    /// Accent colour as 0xRRGGBB. `None` = Arkor's default violet.
    pub accent: Option<u32>,
    pub grid_size: GridSize,
    /// Short sounds when a game starts or stops, a download finishes, or something fails.
    pub sounds: bool,
    /// How the framed panels and cards look.
    pub glass: Glass,
    /// What is drawn behind the panels.
    pub backdrop: Backdrop,
    /// Base text size in pixels (every size in the window follows it).
    pub font_size: Option<u8>,
    /// Stop the animated backgrounds and other movement.
    pub reduce_motion: bool,
    /// Picture for `Backdrop::Image`.
    pub backdrop_image: Option<PathBuf>,
}

/// What a new install looks like: dark, violet, Crystal panels over the Drift background,
/// with sounds.
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            accent: None,
            grid_size: GridSize::Medium,
            sounds: true,
            glass: Glass::Crystal,
            backdrop: Backdrop::Drift,
            font_size: None,
            reduce_motion: false,
            backdrop_image: None,
        }
    }
}

impl Appearance {
    pub const FONT_SIZES: [u8; 4] = [13, 14, 15, 16];

    pub fn font_size(&self) -> u8 {
        self.font_size
            .filter(|s| Self::FONT_SIZES.contains(s))
            .unwrap_or(14)
    }
}

/// Panel and card style (fill, opacity, border, corner radius).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Glass {
    Acrylic,
    #[default]
    Crystal,
    Smoked,
    Soft,
}

impl Glass {
    pub const ALL: [Glass; 4] = [Glass::Acrylic, Glass::Crystal, Glass::Smoked, Glass::Soft];
}

/// Window background behind the panels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    /// A glow rising behind the window.
    Nebula,
    Eclipse,
    Pulse,
    Grid,
    /// Animated.
    Aurora,
    /// Animated.
    Tide,
    /// Animated.
    #[default]
    Drift,
    /// Plain background colour.
    Plain,
    /// The user's own picture (`Appearance::backdrop_image`).
    Image,
}

impl Backdrop {
    pub const ALL: [Backdrop; 9] = [
        Backdrop::Nebula,
        Backdrop::Eclipse,
        Backdrop::Pulse,
        Backdrop::Grid,
        Backdrop::Aurora,
        Backdrop::Tide,
        Backdrop::Drift,
        Backdrop::Plain,
        Backdrop::Image,
    ];

    pub fn animated(self) -> bool {
        matches!(self, Backdrop::Aurora | Backdrop::Tide | Backdrop::Drift)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    /// Follow the desktop's light / dark setting.
    System,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GridSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl GridSize {
    pub const ALL: [GridSize; 3] = [GridSize::Small, GridSize::Medium, GridSize::Large];

    /// Cover width in pixels (covers are 2:3).
    pub fn cover_width(self) -> f32 {
        match self {
            GridSize::Small => 110.,
            GridSize::Medium => 150.,
            GridSize::Large => 210.,
        }
    }
}

/// Discord "Playing …" activity while a game runs, through Discord's local IPC socket.
/// Needs a Discord application id (create one at discord.com/developers).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Discord {
    pub enabled: bool,
    pub app_id: String,
}

/// UI language. English is the default; Turkish is the other supported language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Tr,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GlobalHud {
    pub enabled: bool,
    pub settings: HudSettings,
}

impl Profile {
    /// Environment variables of the enabled tweaks.
    pub fn tweaks_env(&self) -> Vec<(String, String)> {
        self.tweaks
            .iter()
            .filter_map(|id| crate::tweaks::find(id))
            .flat_map(|t| t.env.iter().map(|(k, v)| (k.to_string(), v.to_string())))
            .collect()
    }

    /// Wrapper commands of the enabled tweaks (e.g. `game-performance`).
    pub fn tweak_wrappers(&self) -> Vec<String> {
        self.tweaks
            .iter()
            .filter_map(|id| crate::tweaks::find(id)?.wrapper)
            .map(String::from)
            .collect()
    }

    pub fn path_for(id: &GameId) -> anyhow::Result<std::path::PathBuf> {
        Ok(paths::profiles_dir()?.join(format!("{}.toml", id.file_stem())))
    }

    /// Loads the profile for `id`, or the default one if it does not exist yet.
    pub fn load(id: &GameId) -> anyhow::Result<Self> {
        Ok(read_toml(&Self::path_for(id)?)?.unwrap_or_default())
    }

    pub fn save(&self, id: &GameId) -> anyhow::Result<()> {
        write_toml(&Self::path_for(id)?, self)
    }

    /// The HUD this game should run with, or `None` for no HUD.
    pub fn effective_hud<'a>(&'a self, global: &'a Global) -> Option<&'a HudSettings> {
        match self.mangohud.mode {
            HudMode::Off => None,
            HudMode::Global => global.mangohud.enabled.then_some(&global.mangohud.settings),
            // Custom without settings yet behaves like the global layout, shown.
            HudMode::Custom => Some(
                self.mangohud
                    .settings
                    .as_ref()
                    .unwrap_or(&global.mangohud.settings),
            ),
        }
    }
}

impl Global {
    pub fn prefixes_dir(&self) -> PathBuf {
        self.prefixes_dir.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("Games/Arkor/Prefixes")
        })
    }

    /// The folder store games are installed in (one subfolder per store).
    pub fn games_dir(&self) -> PathBuf {
        self.games_dir.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("Games/Arkor/Games")
        })
    }

    pub fn path() -> anyhow::Result<std::path::PathBuf> {
        Ok(paths::config_dir()?.join("global.toml"))
    }

    /// Loads global settings. On first run there is no file yet: Arkor adopts the user's
    /// existing setup (MangoHud.conf layout, MANGOHUD=1 in the environment) and saves it,
    /// so nothing changes visually when they start launching through Arkor.
    pub fn load_or_import() -> anyhow::Result<Self> {
        let path = Self::path()?;
        if let Some(global) = read_toml(&path)? {
            return Ok(global);
        }
        let mut global = Self::default();
        global.mangohud.enabled = std::env::var("MANGOHUD").is_ok_and(|v| v == "1");
        if let Some(conf) =
            paths::mangohud_user_config().and_then(|p| std::fs::read_to_string(p).ok())
        {
            global.mangohud.settings = HudSettings::import(&conf);
        }
        global.save()?;
        Ok(global)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        write_toml(&Self::path()?, self)
    }
}

fn read_toml<T: DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text)
            .map(Some)
            .with_context(|| format!("invalid {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Writes via temp file + rename so a crash never leaves a half-written file.
pub(crate) fn write_toml<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let dir = path.parent().context("path has no parent")?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string_pretty(value)?)?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn linux_games_run_in_steam_runtime_4_by_default() {
        let mut p = Profile::default();
        assert_eq!(
            p.runtime(),
            Some(DEFAULT_RUNTIME),
            "like Faugus: SteamRT4 unless chosen"
        );
        p.linux_runtime = Some("umu-sniper".into());
        assert_eq!(p.runtime(), Some("umu-sniper"));
        p.linux_runtime = Some(NO_RUNTIME.into());
        assert_eq!(p.runtime(), None, "run directly");
    }

    #[test]
    fn update_interval_timing() {
        let mut g = Global::default();
        assert!(g.updates_due(1_000_000), "never checked: due");
        g.last_update_check = Some(1_000_000);
        assert!(!g.updates_due(1_000_000 + 3_600));
        assert!(g.updates_due(1_000_000 + 86_400));
        g.update_interval = UpdateInterval::Weekly;
        assert!(!g.updates_due(1_000_000 + 86_400));
        g.update_interval = UpdateInterval::OnGameStart;
        assert!(!g.updates_due(9_999_999) && g.updates_on_launch());
        g.update_interval = UpdateInterval::Daily;
        g.no_auto_updates = true;
        assert_eq!(g.update_interval(), UpdateInterval::Never);
        assert!(!g.updates_due(9_999_999) && !g.updates_on_launch());
    }

    #[test]
    fn old_lsfg_default_reads_as_off() {
        // "default" (leave it to lsfg-vk's own config) is gone: Arkor manages lsfg-vk.
        let p: Profile = toml::from_str("[lsfg]\nmode = \"default\"\n").unwrap();
        assert_eq!(p.lsfg.mode, LsfgMode::Off);
        assert_eq!(Profile::default().lsfg.mode, LsfgMode::Off);
    }

    use super::*;
    use crate::mangohud::Metric;

    #[test]
    fn defaults_fill_a_new_profile_without_losing_its_own_settings() {
        let defaults = GameDefaults {
            proton: Some("Proton-CachyOS Latest".into()),
            gamemode: true,
            no_sleep: true,
            tweaks: ["wayland".to_string()].into(),
            launch_options: "%command% -dx12".into(),
        };
        let mut fresh = Profile::default();
        defaults.apply_to(&mut fresh);
        assert!(fresh.no_sleep);
        assert!(!fresh.gamemode, "GameMode is no longer used");
        assert!(fresh.tweaks.contains("wayland"));
        assert_eq!(fresh.launch_options, "%command% -dx12");
        // The Proton default applies at launch, not by writing it into the game.
        assert_eq!(fresh.proton, None);

        let mut own = Profile {
            launch_options: "VAR=1 %command%".into(),
            tweaks: ["hdr".to_string()].into(),
            ..Default::default()
        };
        defaults.apply_to(&mut own);
        assert_eq!(own.launch_options, "VAR=1 %command%");
        assert!(own.tweaks.contains("hdr") && own.tweaks.contains("wayland"));
    }

    #[test]
    fn parses_partial_profile() {
        let p: Profile = toml::from_str(
            r#"
            gamemode = true
            [env]
            PROTON_ENABLE_WAYLAND = "1"
            [mangohud]
            mode = "custom"
            settings = { metrics = ["fps"], fps_limit = 144 }
            "#,
        )
        .unwrap();
        // An old file with `gamemode` still loads; it is not written back.
        assert!(!toml::to_string(&p).unwrap().contains("gamemode"));
        assert_eq!(p.env["PROTON_ENABLE_WAYLAND"], "1");
        let hud = p.mangohud.settings.as_ref().unwrap();
        assert_eq!(hud.metrics, [Metric::Fps].into());
        assert_eq!(hud.font_size, 24, "missing fields take defaults");
    }

    #[test]
    fn launch_options_split() {
        let (env, before, after) = parse_launch_options(
            r#"DXVK_ASYNC=1 PROTON_LOG="1 2" strace -f %command% -dx12 'a b'"#,
        );
        assert_eq!(
            env,
            [
                ("DXVK_ASYNC".into(), "1".into()),
                ("PROTON_LOG".into(), "1 2".into())
            ]
        );
        assert_eq!(before, ["strace", "-f"]);
        assert_eq!(after, ["-dx12", "a b"]);
        let (env, before, after) = parse_launch_options("gamemoderun FOO=1");
        assert!(env.is_empty() && after.is_empty());
        assert_eq!(
            before,
            ["gamemoderun", "FOO=1"],
            "KEY=VALUE after a command word is an argument"
        );
        assert_eq!(shell_words(r#"a\ b "c\"d" ''"#), ["a b", "c\"d", ""]);
    }

    #[test]
    fn lsfg_config_targets_game_process() {
        let mut l = Lsfg {
            multiplier: 3,
            flow_scale: 75,
            processes: vec!["FarCry6.exe".into(), " ".into()],
            ..Lsfg::default()
        };
        let text = l
            .render_config(Some(Path::new("/ls/Lossless.dll")))
            .unwrap();
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(parsed["version"].as_integer(), Some(2));
        assert_eq!(parsed["global"]["dll"].as_str(), Some("/ls/Lossless.dll"));
        let profile = &parsed["profile"].as_array().unwrap()[0];
        let active: Vec<_> = profile["active_in"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(active, ["FarCry6.exe", "farcry6.exe"]);
        assert_eq!(profile["multiplier"].as_integer(), Some(3));
        assert_eq!(profile["flow_scale"].as_float(), Some(0.75));
        l.processes.clear();
        assert!(l.render_config(None).is_none());
    }

    #[test]
    fn gamescope_args_and_tweaks() {
        let g = Gamescope {
            width: Some(2560),
            height: Some(1440),
            game_width: Some(1920),
            game_height: Some(1080),
            fullscreen: true,
            filter: Some("fsr".into()),
            args: vec!["--expose-wayland".into()],
            ..Default::default()
        };
        assert_eq!(
            g.to_args().join(" "),
            "-W 2560 -H 1440 -w 1920 -h 1080 -f -F fsr --expose-wayland"
        );
        let mut p = Profile::default();
        p.tweaks.insert("hdr".into());
        p.tweaks.insert("game_performance".into());
        p.tweaks.insert("unknown".into());
        assert!(p.tweaks_env().contains(&("DXVK_HDR".into(), "1".into())));
        assert_eq!(p.tweak_wrappers(), ["game-performance"]);
    }

    #[test]
    fn rejects_typos() {
        assert!(toml::from_str::<Profile>("gamemod = true").is_err());
    }

    #[test]
    fn effective_hud_resolution() {
        let mut global = Global::default();
        let mut p = Profile::default();
        assert!(p.effective_hud(&global).is_none(), "global disabled");
        global.mangohud.enabled = true;
        assert_eq!(p.effective_hud(&global), Some(&global.mangohud.settings));

        p.mangohud.mode = HudMode::Off;
        assert!(p.effective_hud(&global).is_none());

        global.mangohud.enabled = false;
        p.mangohud.mode = HudMode::Custom;
        let own = HudSettings {
            fps_limit: 60,
            ..Default::default()
        };
        p.mangohud.settings = Some(own.clone());
        assert_eq!(
            p.effective_hud(&global),
            Some(&own),
            "custom ignores global switch"
        );
    }
}

#[cfg(test)]
mod appearance_default_tests {
    use super::*;

    #[test]
    fn new_installs_look_like_this() {
        let g: Global = toml::from_str("").unwrap();
        let a = g.appearance;
        assert_eq!(
            (a.theme, a.glass, a.backdrop, a.grid_size),
            (
                Theme::Dark,
                Glass::Crystal,
                Backdrop::Drift,
                GridSize::Medium
            )
        );
        assert!(a.sounds && a.accent.is_none() && !a.reduce_motion);
    }
}
