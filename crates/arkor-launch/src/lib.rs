//! The single launch pipeline every game goes through:
//! `arkor run <store:id> -- <command...>` (for Steam: launch options `arkor run steam:<appid> -- %command%`).
//!
//! `plan` is pure (profile + command in, final argv + env out) so it can be unit-tested;
//! `exec` performs side effects (writes generated configs, replaces the process).
//! `start` (see [`start`]) is the store-aware entry point behind `arkor launch` / Play.

pub mod start;

use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{GameId, Global, Profile, paths};

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Generated files to write before launching: (path, contents).
    pub files: Vec<(PathBuf, String)>,
}

pub fn plan(
    profile: &Profile,
    global: &Global,
    game_dir: &Path,
    command: &[String],
) -> anyhow::Result<Plan> {
    anyhow::ensure!(
        !command.is_empty(),
        "no command to run (did you forget `-- %command%`?)"
    );

    let mut env = BTreeMap::new();
    let mut files = Vec::new();
    let mut argv = Vec::new();

    let hud = profile.effective_hud(global);
    match hud {
        Some(settings) => {
            let conf = game_dir.join("MangoHud.conf");
            env.insert("MANGOHUD_CONFIGFILE".into(), conf.display().to_string());
            files.push((conf, settings.render()));
            if profile.gamescope.enabled {
                // Inside gamescope, gamescope's --mangoapp draws the HUD; the Vulkan layer
                // must stay off or a system-wide MANGOHUD=1 would draw a second one.
                disable_layer(&mut env);
            } else {
                env.insert("MANGOHUD".into(), "1".into());
            }
        }
        // Explicitly off, so a system-wide MANGOHUD=1 (e.g. environment.d) cannot re-enable it.
        None => disable_layer(&mut env),
    }

    if profile.gamemode {
        argv.push("gamemoderun".to_string());
    }

    if profile.gamescope.enabled {
        argv.push("gamescope".to_string());
        argv.extend(profile.gamescope.args.iter().cloned());
        if hud.is_some() && !profile.gamescope.args.iter().any(|a| a == "--mangoapp") {
            argv.push("--mangoapp".to_string());
        }
        argv.push("--".to_string());
    }

    argv.extend(command.iter().cloned());

    // User env last so it can override anything above.
    env.extend(profile.env.clone());

    Ok(Plan { argv, env, files })
}

fn disable_layer(env: &mut BTreeMap<String, String>) {
    env.insert("MANGOHUD".into(), "0".into());
    env.insert("DISABLE_MANGOHUD".into(), "1".into());
}

/// Loads the game's profile, writes generated configs and replaces this process with the game.
/// Only returns on error.
pub fn exec(id: &GameId, command: &[String]) -> anyhow::Result<std::convert::Infallible> {
    exec_with(id, command, &BTreeMap::new(), None)
}

/// Like [`exec`], with environment set before the profile is applied (so the profile can still
/// override it) and an optional working directory.
pub fn exec_with(
    id: &GameId,
    command: &[String],
    base_env: &BTreeMap<String, String>,
    working_dir: Option<&Path>,
) -> anyhow::Result<std::convert::Infallible> {
    let profile = Profile::load(id)?;
    let global = Global::load_or_import()?;
    let game_dir = paths::game_config_dir(id)?;
    let plan = plan(&profile, &global, &game_dir, command)?;

    for (path, contents) in &plan.files {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))?;
    }

    let mut cmd = std::process::Command::new(&plan.argv[0]);
    cmd.args(&plan.argv[1..]).envs(base_env).envs(&plan.env);
    // Arkor's own UI sets DISABLE_MANGOHUD for itself; it must not leak into a game that
    // is supposed to show the HUD.
    if !plan.env.contains_key("DISABLE_MANGOHUD") {
        cmd.env_remove("DISABLE_MANGOHUD");
    }
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    // exec (not spawn): Steam tracks the game by our PID, and we add no overhead while it runs.
    let err = cmd.exec();
    Err(err).with_context(|| format!("failed to exec `{}`", plan.argv[0]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkor_core::HudMode;

    fn cmd() -> Vec<String> {
        vec!["proton".into(), "run".into(), "game.exe".into()]
    }

    fn global(enabled: bool) -> Global {
        let mut g = Global::default();
        g.mangohud.enabled = enabled;
        g
    }

    #[test]
    fn no_hud_forces_layer_off() {
        let p = plan(&Profile::default(), &global(false), Path::new("/g"), &cmd()).unwrap();
        assert_eq!(p.argv, cmd());
        assert_eq!(p.env["MANGOHUD"], "0");
        assert_eq!(p.env["DISABLE_MANGOHUD"], "1");
        assert!(p.files.is_empty());
    }

    #[test]
    fn global_hud_uses_layer_and_global_layout() {
        let g = global(true);
        let p = plan(&Profile::default(), &g, Path::new("/g"), &cmd()).unwrap();
        assert_eq!(p.env["MANGOHUD"], "1");
        assert_eq!(p.env["MANGOHUD_CONFIGFILE"], "/g/MangoHud.conf");
        assert!(!p.env.contains_key("DISABLE_MANGOHUD"));
        assert_eq!(
            p.files,
            vec![(
                PathBuf::from("/g/MangoHud.conf"),
                g.mangohud.settings.render()
            )]
        );
    }

    #[test]
    fn game_off_beats_global_on() {
        let mut prof = Profile::default();
        prof.mangohud.mode = HudMode::Off;
        let p = plan(&prof, &global(true), Path::new("/g"), &cmd()).unwrap();
        assert_eq!(p.env["DISABLE_MANGOHUD"], "1");
        assert!(p.files.is_empty());
    }

    #[test]
    fn full_stack_order_and_env() {
        let mut prof = Profile {
            gamemode: true,
            ..Default::default()
        };
        prof.mangohud.mode = HudMode::Custom;
        prof.gamescope.enabled = true;
        prof.gamescope.args = vec!["-f".into()];
        prof.env.insert("FOO".into(), "bar".into());

        let p = plan(&prof, &global(false), Path::new("/g"), &cmd()).unwrap();
        assert_eq!(
            p.argv,
            [
                "gamemoderun",
                "gamescope",
                "-f",
                "--mangoapp",
                "--",
                "proton",
                "run",
                "game.exe"
            ]
        );
        assert_eq!(
            p.env["MANGOHUD"], "0",
            "layer off under gamescope, mangoapp draws it"
        );
        assert_eq!(p.env["MANGOHUD_CONFIGFILE"], "/g/MangoHud.conf");
        assert_eq!(p.env["FOO"], "bar");
    }

    #[test]
    fn user_env_overrides() {
        let mut prof = Profile::default();
        prof.env.insert("MANGOHUD".into(), "1".into());
        let p = plan(&prof, &global(false), Path::new("/g"), &cmd()).unwrap();
        assert_eq!(p.env["MANGOHUD"], "1");
    }

    #[test]
    fn empty_command_is_error() {
        assert!(plan(&Profile::default(), &global(false), Path::new("/g"), &[]).is_err());
    }
}
