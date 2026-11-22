use std::collections::BTreeMap;
use std::path::Path;

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
    pub gamemode: bool,
    pub gamescope: Gamescope,
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
    /// Raw gamescope arguments, e.g. `["-W", "2560", "-H", "1440", "-f"]`.
    pub args: Vec<String>,
}

/// Settings that apply to every game unless the game's profile overrides them.
/// Stored as `~/.config/arkor/global.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Global {
    pub language: Language,
    pub mangohud: GlobalHud,
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
    use super::*;
    use crate::mangohud::Metric;

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
        assert!(p.gamemode);
        assert_eq!(p.env["PROTON_ENABLE_WAYLAND"], "1");
        let hud = p.mangohud.settings.as_ref().unwrap();
        assert_eq!(hud.metrics, [Metric::Fps].into());
        assert_eq!(hud.font_size, 24, "missing fields take defaults");
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
