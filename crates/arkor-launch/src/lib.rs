//! The single launch pipeline every game goes through:
//! `arkor run <store:id> -- <command...>` (for Steam: launch options `arkor run <id> -- %command%`).
//!
//! `plan` is pure (profile + command in, final argv + env out) so it can be unit-tested;
//! `run` performs side effects: writes generated configs, runs hooks and the extra app,
//! and either replaces the process with the game or waits for it.
//! `start` (see [`start`]) is the library-aware entry point behind `arkor launch` / Play.

pub mod admin;
pub mod bundles;
pub mod compat;
pub mod components;
pub mod covers;
pub mod discord;
pub mod dlss_enabler;
pub mod downloads;
pub mod ea_fix;
pub mod launchers;
pub mod layers;
pub mod mods;
pub mod optiscaler;
pub mod progress;
pub mod proton_dl;
pub mod quick;
pub mod reshade;
pub mod saves;
pub mod self_update;
pub mod shops;
pub mod shortcuts;
pub mod start;
pub mod steam_api;
pub mod steam_control;
pub mod steam_downloads;
pub mod steam_link;
pub mod streamline;
pub mod system;
pub mod tools;
pub mod trouble;
pub mod umu_db;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::profile::parse_launch_options;
use arkor_core::{GameId, Global, LsfgMode, Profile, paths};

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Generated files to write before launching: (path, contents).
    pub files: Vec<(PathBuf, String)>,
}

/// Facts about this machine and game that `plan` needs, gathered by the caller so `plan`
/// stays pure and testable.
#[derive(Debug, Clone, Default)]
pub struct Host {
    /// Lossless Scaling's `Lossless.dll` (lsfg-vk needs it), if found.
    pub lossless_dll: Option<PathBuf>,
    /// The DLL lsfg-vk loads its shaders from (`lsfg-vk.dll` for lsfg-vk 2.x).
    pub lsfg_dll: Option<PathBuf>,
    /// Proxy DLL names of game-folder mods installed in this game (e.g. `dxgi`, `version`).
    pub proxy_dlls: Vec<String>,
    /// `WINEDLLOVERRIDES` already in the environment, kept when Arkor adds its own.
    pub inherited_dll_overrides: Option<String>,
    /// What the game's mod loaders need from Wine (Amethyst's overrides, `(dll, n,b)`).
    pub mod_dll_overrides: Vec<(String, String)>,
    /// Folder for Proton's own logs when the profile asks for them.
    pub log_dir: Option<PathBuf>,
    pub anticheat: tools::AntiCheat,
}

impl Host {
    pub fn detect(id: &GameId) -> Self {
        Self {
            lossless_dll: tools::lossless_dll(),
            lsfg_dll: tools::lsfg_dll(),
            proxy_dlls: {
                let mut dlls = optiscaler::installed_proxies(id);
                // DLSS-NR's installer names the DLLs that must load from the game folder.
                dlls.extend(bundles::dlssnr(id).map(|d| d.overrides).unwrap_or_default());
                dlls
            },
            inherited_dll_overrides: std::env::var("WINEDLLOVERRIDES")
                .ok()
                .filter(|v| !v.is_empty()),
            mod_dll_overrides: mods::link::launch_overrides(id),
            log_dir: paths::launch_log(id)
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf)),
            anticheat: tools::anticheat(),
        }
    }
}

pub fn plan(
    profile: &Profile,
    global: &Global,
    host: &Host,
    game_dir: &Path,
    command: &[String],
) -> anyhow::Result<Plan> {
    anyhow::ensure!(
        !command.is_empty(),
        "no command to run (did you forget `-- %command%`?)"
    );

    // Lowest priority first: global env, Arkor's own settings, launch options, game env.
    let mut env: BTreeMap<String, String> = global.env.clone();
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
                env.remove("DISABLE_MANGOHUD");
            }
        }
        // Explicitly off, so a system-wide MANGOHUD=1 (e.g. environment.d) cannot re-enable it.
        None => disable_layer(&mut env),
    }

    match profile.lsfg.mode {
        LsfgMode::On => {
            let lsfg = &profile.lsfg;
            env.insert("ENABLE_LSFGVK".into(), "1".into());
            match lsfg.render_config(host.lsfg_dll.as_deref()) {
                // A per-game config whose profile only matches the game's own process, so a
                // launcher started first (e.g. Ubisoft Connect) does not get frame generation
                // and the game itself does.
                Some(config) => {
                    let path = game_dir.join("lsfg-vk.toml");
                    env.insert("LSFGVK_CONFIG".into(), path.display().to_string());
                    files.push((path, config));
                }
                // Process unknown: lsfg-vk's environment profile, applied to every process.
                None => {
                    env.insert("LSFGVK_ENV".into(), "1".into());
                    env.insert("LSFGVK_MULTIPLIER".into(), lsfg.multiplier.to_string());
                    env.insert(
                        "LSFGVK_FLOW_SCALE".into(),
                        format!("{:.2}", lsfg.flow_scale as f32 / 100.0),
                    );
                    env.insert(
                        "LSFGVK_PERFORMANCE_MODE".into(),
                        if lsfg.performance_mode { "1" } else { "0" }.into(),
                    );
                    if let Some(dll) = &host.lsfg_dll {
                        env.insert("LSFGVK_DLL_PATH".into(), dll.display().to_string());
                    }
                }
            }
        }
        LsfgMode::Off => {
            env.insert("DISABLE_LSFGVK".into(), "1".into());
        }
    }

    if !host.proxy_dlls.is_empty() || !host.mod_dll_overrides.is_empty() {
        // Wine must load the mods' proxy DLLs from the game folder instead of its builtins;
        // the mod loaders' come first, so Arkor's own win for the same DLL.
        let ours = host
            .mod_dll_overrides
            .iter()
            .filter(|(dll, _)| !host.proxy_dlls.contains(dll))
            .map(|(dll, order)| format!("{dll}={order}"))
            .chain(host.proxy_dlls.iter().map(|d| format!("{d}=n,b")))
            .collect::<Vec<_>>()
            .join(";");
        let overrides = match &host.inherited_dll_overrides {
            Some(existing) => format!("{existing};{ours}"),
            None => ours,
        };
        env.insert("WINEDLLOVERRIDES".into(), overrides);
    }

    if profile.proton_log
        && let Some(dir) = &host.log_dir
    {
        env.insert("PROTON_LOG".into(), "1".into());
        env.insert("PROTON_LOG_DIR".into(), dir.display().to_string());
    }
    // Proton loads these only for games that ship EasyAntiCheat / BattlEye. A runtime the
    // caller already set (Steam sets its own) is kept.
    if let Some(eac) = &host.anticheat.eac
        && std::env::var_os("PROTON_EAC_RUNTIME").is_none()
    {
        env.insert("PROTON_EAC_RUNTIME".into(), eac.display().to_string());
    }
    if let Some(be) = &host.anticheat.battleye
        && std::env::var_os("PROTON_BATTLEYE_RUNTIME").is_none()
    {
        env.insert("PROTON_BATTLEYE_RUNTIME".into(), be.display().to_string());
    }
    if let Some(umu_id) = profile.umu_id.as_ref().filter(|id| !id.is_empty()) {
        env.insert("GAMEID".into(), umu_id.clone());
    }
    env.extend(profile.tweaks_env());
    env.extend(profile.layers.env(host.lossless_dll.as_deref()));
    if profile.layers.afmf.enabled && profile.gamescope.enabled {
        // afmf-linux needs to know it runs inside gamescope (more images, skip gamescope itself).
        env.insert("AFMF_GAMESCOPE".into(), "1".into());
    }
    if profile.vkbasalt.enabled {
        let conf = game_dir.join("vkBasalt.conf");
        env.insert("ENABLE_VKBASALT".into(), "1".into());
        env.insert("VKBASALT_CONFIG_FILE".into(), conf.display().to_string());
        files.push((conf, profile.vkbasalt.render()));
    }

    let (option_env, before, after) = parse_launch_options(&profile.launch_options);

    if profile.no_sleep {
        argv.extend(
            [
                "systemd-inhibit",
                "--what=idle:sleep",
                "--who=Arkor",
                "--why=Playing a game",
                "--",
            ]
            .map(String::from),
        );
    }
    argv.extend(profile.tweak_wrappers());
    argv.extend(before);
    if profile.gamescope.enabled {
        argv.push("gamescope".to_string());
        let args = profile.gamescope.to_args();
        let has_mangoapp = args.iter().any(|a| a == "--mangoapp");
        argv.extend(args);
        if hud.is_some() && !has_mangoapp {
            argv.push("--mangoapp".to_string());
        }
        argv.push("--".to_string());
    }
    argv.extend(command.iter().cloned());
    argv.extend(after);

    env.extend(option_env);
    // The game's own env last so it can override anything above.
    env.extend(profile.env.clone());

    Ok(Plan { argv, env, files })
}

fn disable_layer(env: &mut BTreeMap<String, String>) {
    env.insert("MANGOHUD".into(), "0".into());
    env.insert("DISABLE_MANGOHUD".into(), "1".into());
}

/// Loads the game's profile, writes generated configs, runs the game and waits for it
/// (hooks, extra app, Discord, play time). Returns the game's exit code.
pub fn exec(id: &GameId, command: &[String]) -> anyhow::Result<i32> {
    run(id, command, &BTreeMap::new(), None)
}

/// Like [`exec`], with environment set before the profile is applied (so the profile can still
/// override it) and an optional working directory.
/// Where console mode notes a game it starts (for games Steam starts, which do not inherit
/// Arkor's environment).
fn console_marker() -> Option<PathBuf> {
    Some(dirs::state_dir()?.join("arkor/console-launch"))
}

/// Notes that console mode starts `id` on a screen of `w`×`h` pixels.
pub fn note_console_launch(id: &GameId, (w, h): (u32, u32)) {
    if let Some(path) = console_marker() {
        let _ = path.parent().map(std::fs::create_dir_all);
        let now = arkor_core::stats::now();
        let _ = std::fs::write(path, format!("{id}\n{w}x{h}\n{now}\n"));
    }
}

/// The screen size when `id` was started from console mode (lately).
fn console_screen(id: &GameId) -> Option<(u32, u32)> {
    let parse = |s: &str| {
        let (w, h) = s.trim().split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    };
    if let Ok(size) = std::env::var("ARKOR_CONSOLE_SIZE") {
        return parse(&size);
    }
    let text = std::fs::read_to_string(console_marker()?).ok()?;
    let mut lines = text.lines();
    let (who, size, at) = (lines.next()?, lines.next()?, lines.next()?);
    let age = arkor_core::stats::now().saturating_sub(at.parse().ok()?);
    (who == id.to_string() && age < 15 * 60)
        .then(|| parse(size))
        .flatten()
}

pub fn run(
    id: &GameId,
    command: &[String],
    base_env: &BTreeMap<String, String>,
    working_dir: Option<&Path>,
) -> anyhow::Result<i32> {
    let mut profile = Profile::load(id)?;
    let mut global = Global::load_or_import()?;
    let trial = trouble::Trial::from_env();
    trial.apply(id, &mut profile, &mut global);
    // Started from console mode: the game is kept full screen in gamescope, so no panel
    // or other window shows over (or next to) it.
    if !global.console_windowed
        && !profile.gamescope.enabled
        && !trial.plain
        && let Some((w, h)) = console_screen(id)
        && start::find_in_path("gamescope").is_some()
    {
        let g = &mut profile.gamescope;
        g.enabled = true;
        (g.width, g.height) = (Some(w), Some(h));
        g.fullscreen = true;
    }
    // Mods kept out of the game folder go in now (before the overrides they need are read).
    let mods_placed = if trial.plain {
        None
    } else {
        let folder = arkor_core::library::Library::load()
            .ok()
            .and_then(|l| l.get(&id.id).cloned())
            .and_then(|g| mods::link::place(&g, &global.prefixes_dir()))
            .map(|p| p.game_dir);
        mods::play::before(id, folder.as_deref())
    };
    let game_dir = paths::game_config_dir(id)?;
    let plan = plan(&profile, &global, &Host::detect(id), &game_dir, command)?;

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

    let discord = global.discord.enabled && !global.discord.app_id.trim().is_empty();
    let shell_env: BTreeMap<String, String> = base_env
        .iter()
        .chain(&plan.env)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if !profile.pre_launch.trim().is_empty() {
        let status = shell(&profile.pre_launch, &shell_env, working_dir)?;
        anyhow::ensure!(status.success(), "pre-launch command failed ({status})");
    }

    let extra = profile
        .extra_app
        .path
        .clone()
        .map(|path| (path, profile.extra_app.clone()));
    let mut extra_child = None;
    if let Some((path, cfg)) = &extra
        && cfg.before_game
    {
        extra_child = spawn_extra(path, &shell_env)?;
        std::thread::sleep(std::time::Duration::from_secs(cfg.delay_secs.into()));
    }

    // MangoHud only writes benchmark logs into a folder that exists.
    if let Some(dir) = arkor_core::bench::default_dir() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Arkor waits for the game (instead of exec'ing it) to run the hooks and count play time.
    let started = arkor_core::stats::now();
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to start `{}`", plan.argv[0]))?;
    let presence = discord
        .then(|| discord::Presence::start(&global.discord.app_id, &game_name(id), child.id()));

    if let Some((path, cfg)) = &extra
        && !cfg.before_game
    {
        std::thread::sleep(std::time::Duration::from_secs(cfg.delay_secs.into()));
        extra_child = spawn_extra(path, &shell_env)?;
    }

    let status = child.wait()?;
    drop(presence);
    if let Err(e) = arkor_core::stats::Stats::record(id, started, arkor_core::stats::now()) {
        eprintln!("arkor: could not save play time: {e:#}");
    }
    if let Some(mut extra) = extra_child {
        let _ = extra.kill();
    }
    drop(mods_placed);
    upload_saves(id);
    if !profile.post_launch.trim().is_empty() {
        shell(&profile.post_launch, &shell_env, working_dir)?;
    }
    Ok(status.code().unwrap_or(1))
}

/// After the game ended, in a process of its own (whoever started the game need not wait for
/// it, and stopping the game does not stop it): uploads what it saved to the store's cloud,
/// and keeps a copy of its saves here (any game).
fn upload_saves(id: &GameId) {
    use std::os::unix::process::CommandExt;
    let Ok(me) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(me);
    cmd.args(["saves", "after"])
        .arg(id.to_string())
        .stdin(std::process::Stdio::null());
    // SAFETY: only an async-signal-safe syscall between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    if let Err(e) = cmd.spawn() {
        eprintln!("arkor: the saves were not uploaded: {e}");
    }
}

fn shell(
    command: &str,
    env: &BTreeMap<String, String>,
    dir: Option<&Path>,
) -> anyhow::Result<std::process::ExitStatus> {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(command).envs(env);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    cmd.status().with_context(|| format!("running `{command}`"))
}

/// Windows programs (.exe/.bat/.msi) go through umu-run with the game's environment
/// (same prefix and Proton); anything else runs directly.
fn spawn_extra(
    path: &Path,
    env: &BTreeMap<String, String>,
) -> anyhow::Result<Option<std::process::Child>> {
    let windows = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["exe", "bat", "msi"].contains(&e.to_ascii_lowercase().as_str()));
    let mut cmd = if windows {
        let mut c = std::process::Command::new(start::umu_run()?);
        c.arg(path);
        c
    } else {
        std::process::Command::new(path)
    };
    cmd.envs(env);
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    Ok(Some(
        cmd.spawn()
            .with_context(|| format!("starting {}", path.display()))?,
    ))
}

fn game_name(id: &GameId) -> String {
    arkor_core::library::Library::load()
        .ok()
        .and_then(|l| l.get(&id.id).map(|g| g.name.clone()))
        .unwrap_or_else(|| id.to_string())
}

/// Tests that point XDG_CONFIG_HOME somewhere else take this, so they don't race.
#[cfg(test)]
pub(crate) static CONFIG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        let p = plan(
            &Profile::default(),
            &global(false),
            &Host::default(),
            Path::new("/g"),
            &cmd(),
        )
        .unwrap();
        assert_eq!(p.argv, cmd());
        assert_eq!(p.env["MANGOHUD"], "0");
        assert_eq!(p.env["DISABLE_MANGOHUD"], "1");
        assert!(p.files.is_empty());
    }

    #[test]
    fn global_hud_uses_layer_and_global_layout() {
        // The layout's output folder comes from XDG_DATA_HOME, which other tests change.
        let _lock = CONFIG_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let g = global(true);
        let p = plan(
            &Profile::default(),
            &g,
            &Host::default(),
            Path::new("/g"),
            &cmd(),
        )
        .unwrap();
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
        let p = plan(
            &prof,
            &global(true),
            &Host::default(),
            Path::new("/g"),
            &cmd(),
        )
        .unwrap();
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

        let p = plan(
            &prof,
            &global(false),
            &Host::default(),
            Path::new("/g"),
            &cmd(),
        )
        .unwrap();
        assert_eq!(
            p.argv,
            [
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
        let p = plan(
            &prof,
            &global(false),
            &Host::default(),
            Path::new("/g"),
            &cmd(),
        )
        .unwrap();
        assert_eq!(p.env["MANGOHUD"], "1");
    }

    #[test]
    fn lsfg_modes() {
        let mut prof = Profile::default();
        let host = Host {
            lossless_dll: Some("/ls/Lossless.dll".into()),
            lsfg_dll: Some("/ls/lsfg-vk.dll".into()),
            ..Default::default()
        };
        let p = plan(&prof, &global(false), &host, Path::new("/g"), &cmd()).unwrap();
        assert_eq!(
            p.env.get("DISABLE_LSFGVK").map(String::as_str),
            Some("1"),
            "off by default: Arkor manages lsfg-vk, its own config file cannot turn it on"
        );

        prof.lsfg.mode = LsfgMode::On;
        prof.lsfg.multiplier = 3;
        prof.lsfg.flow_scale = 75;
        let p = plan(&prof, &global(false), &host, Path::new("/g"), &cmd()).unwrap();
        assert_eq!(p.env["ENABLE_LSFGVK"], "1");
        assert_eq!(p.env["LSFGVK_ENV"], "1");
        assert_eq!(p.env["LSFGVK_MULTIPLIER"], "3");
        assert_eq!(p.env["LSFGVK_FLOW_SCALE"], "0.75");
        assert_eq!(p.env["LSFGVK_PERFORMANCE_MODE"], "0");
        // lsfg-vk 2.x gets the beta branch's shader DLL, not Lossless.dll.
        assert_eq!(p.env["LSFGVK_DLL_PATH"], "/ls/lsfg-vk.dll");

        prof.lsfg.mode = LsfgMode::Off;
        let p = plan(&prof, &global(false), &host, Path::new("/g"), &cmd()).unwrap();
        assert_eq!(p.env["DISABLE_LSFGVK"], "1");
        assert!(!p.env.contains_key("ENABLE_LSFGVK"));
    }

    #[test]
    fn optiscaler_override_keeps_existing() {
        let host = Host {
            proxy_dlls: vec!["dxgi".into(), "version".into()],
            inherited_dll_overrides: Some("d3d11=n".into()),
            ..Default::default()
        };
        let p = plan(
            &Profile::default(),
            &global(false),
            &host,
            Path::new("/g"),
            &cmd(),
        )
        .unwrap();
        assert_eq!(p.env["WINEDLLOVERRIDES"], "d3d11=n;dxgi=n,b;version=n,b");
    }

    #[test]
    fn mod_loaders_get_their_dlls_and_arkors_own_win() {
        let host = Host {
            proxy_dlls: vec!["version".into()],
            mod_dll_overrides: vec![
                ("version".into(), "b".into()),
                ("winhttp".into(), "n,b".into()),
            ],
            ..Default::default()
        };
        let p = plan(
            &Profile::default(),
            &global(false),
            &host,
            Path::new("/g"),
            &cmd(),
        )
        .unwrap();
        assert_eq!(p.env["WINEDLLOVERRIDES"], "winhttp=n,b;version=n,b");
        let only_mods = Host {
            mod_dll_overrides: vec![("winhttp".into(), "n,b".into())],
            ..Default::default()
        };
        let p = plan(
            &Profile::default(),
            &global(false),
            &only_mods,
            Path::new("/g"),
            &cmd(),
        )
        .unwrap();
        assert_eq!(p.env["WINEDLLOVERRIDES"], "winhttp=n,b");
    }

    #[test]
    fn lsfg_with_game_process_uses_per_game_config() {
        let mut prof = Profile::default();
        prof.lsfg.mode = LsfgMode::On;
        prof.lsfg.processes = vec!["FarCry6.exe".into()];
        let p = plan(
            &prof,
            &global(false),
            &Host::default(),
            Path::new("/g"),
            &cmd(),
        )
        .unwrap();
        assert_eq!(p.env["ENABLE_LSFGVK"], "1");
        assert_eq!(p.env["LSFGVK_CONFIG"], "/g/lsfg-vk.toml");
        assert!(
            !p.env.contains_key("LSFGVK_ENV"),
            "env profile would hit the launcher too"
        );
        let (path, text) = p
            .files
            .iter()
            .find(|(path, _)| path.ends_with("lsfg-vk.toml"))
            .unwrap();
        assert_eq!(path, &PathBuf::from("/g/lsfg-vk.toml"));
        assert!(
            text.contains(r#"active_in = ["FarCry6.exe", "farcry6.exe"]"#),
            "{text}"
        );
    }

    #[test]
    fn options_wrappers_and_env_priority() {
        let mut g = global(false);
        g.env.insert("A".into(), "global".into());
        g.env.insert("B".into(), "global".into());
        let mut prof = Profile {
            no_sleep: true,
            gamemode: true,
            ..Default::default()
        };
        prof.tweaks.insert("game_performance".into());
        prof.tweaks.insert("wayland".into());
        prof.launch_options = "B=options C=options strace %command% -dx12".into();
        prof.env.insert("C".into(), "game".into());
        prof.umu_id = Some("umu-1086940".into());
        let p = plan(&prof, &g, &Host::default(), Path::new("/g"), &cmd()).unwrap();
        assert_eq!(
            p.argv,
            [
                "systemd-inhibit",
                "--what=idle:sleep",
                "--who=Arkor",
                "--why=Playing a game",
                "--",
                "game-performance",
                "strace",
                "proton",
                "run",
                "game.exe",
                "-dx12"
            ]
        );
        assert_eq!(
            (
                p.env["A"].as_str(),
                p.env["B"].as_str(),
                p.env["C"].as_str()
            ),
            ("global", "options", "game")
        );
        assert_eq!(p.env["PROTON_ENABLE_WAYLAND"], "1");
        assert_eq!(p.env["GAMEID"], "umu-1086940");
    }

    #[test]
    fn empty_command_is_error() {
        assert!(
            plan(
                &Profile::default(),
                &global(false),
                &Host::default(),
                Path::new("/g"),
                &[]
            )
            .is_err()
        );
    }
}
