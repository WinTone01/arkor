use std::path::PathBuf;

use anyhow::Context;

/// `~/.config/arkor`
pub fn config_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("no XDG config dir")?
        .join("arkor"))
}

/// `~/.local/share/arkor/stores`: store logins, what is installed and the helper programs.
/// Not in the config folder, so backups never carry logins.
pub fn stores_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/stores"))
}

/// `~/.local/share/arkor/saves/<id>`: copies of a game's saves, and how they stand against
/// the store's cloud.
pub fn saves_dir(id: &crate::GameId) -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/saves")
        .join(id.file_stem()))
}

/// `~/.config/arkor/profiles`
pub fn profiles_dir() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("profiles"))
}

/// `~/.config/arkor/games/<id>`: generated per-game tool configs (MangoHud.conf, ...).
pub fn game_config_dir(id: &crate::GameId) -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("games").join(id.file_stem()))
}

/// MangoHud's own per-user config (`~/.config/MangoHud/MangoHud.conf`), if it exists.
/// Arkor only reads it (to import the user's layout); it never writes it.
pub fn mangohud_user_config() -> Option<PathBuf> {
    let p = dirs::config_dir()?.join("MangoHud/MangoHud.conf");
    p.is_file().then_some(p)
}

/// `~/.local/state/arkor/logs/<id>.log`: output of the last launch of a game.
pub fn launch_log(id: &crate::GameId) -> anyhow::Result<PathBuf> {
    let state = dirs::state_dir().context("no XDG state dir")?;
    Ok(state
        .join("arkor/logs")
        .join(format!("{}.log", id.file_stem())))
}
