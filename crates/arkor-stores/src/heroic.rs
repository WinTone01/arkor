//! Heroic Games Launcher interop: reuse what Heroic already set up for a game.

use std::path::PathBuf;

use serde::Deserialize;

const HEROIC_CONFIGS: &[&str] = &[
    ".config/heroic",
    ".var/app/com.heroicgameslauncher.hgl/config/heroic",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameConfig {
    #[serde(default)]
    wine_prefix: Option<PathBuf>,
}

/// The Wine prefix Heroic uses for `app_name` (Epic app name or GOG id), if Heroic has one.
/// Launching with the same prefix keeps the game's saves and settings.
pub fn wine_prefix(app_name: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    HEROIC_CONFIGS.iter().find_map(|cfg| {
        let path = home
            .join(cfg)
            .join("GamesConfig")
            .join(format!("{app_name}.json"));
        let text = std::fs::read_to_string(path).ok()?;
        // File layout: { "<app_name>": { ...settings }, "version": ..., "explicit": ... }
        let mut root: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(&text).ok()?;
        let config: GameConfig = serde_json::from_value(root.remove(app_name)?).ok()?;
        config.wine_prefix.filter(|p| p.is_dir())
    })
}
