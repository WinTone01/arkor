//! `arkor launch <id>`: start a library game the way its kind needs, with its profile applied.
//!
//! - Steam: Arkor's settings are written into Steam's (see `steam_link`), then Steam starts
//!   the game (`steam://rungameid`) and Arkor waits while it runs.
//! - Epic: `legendary launch` (login/ownership), with `arkor run <id> -- umu-run` as wrapper.
//! - Amazon: `nile launch`, the same way.
//! - Windows: `umu-run <exe>` through `arkor run`, in the game's prefix with its Proton.
//! - Linux: the executable itself, optionally inside a Steam Linux Runtime through umu.
//!
//! Also the prefix tools (winecfg, winetricks, running a file in the prefix) and stopping a
//! running game.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::library::{GameKind, Library, LibraryGame};
use arkor_core::{GameId, Global, Profile, Store};
use arkor_stores::epic;
use arkor_stores::target::ExeTarget;

/// Starts the game; returns its exit code when Arkor waited for it (see `crate::run`).
pub fn start(id: &GameId) -> anyhow::Result<i32> {
    match id.store {
        // Legacy ids from before the library: still startable directly.
        Store::Steam => {
            crate::steam_link::open_url(&format!("steam://rungameid/{}", id.id))?;
            Ok(0)
        }
        Store::Local => {
            let global = Global::load_or_import()?;
            let library = Library::load()?;
            let game = library
                .get(&id.id)
                .with_context(|| format!("`{id}` is not in the library"))?
                .clone();
            // Newer saves from the cloud (Arkor's window did that itself).
            if std::env::var_os("ARKOR_SAVES_DONE").is_none() {
                crate::saves::before_launch(id);
            }
            // What the game's settings use but is missing (a layer, a "Latest" Proton).
            crate::downloads::ensure_needs(&Profile::load(id).unwrap_or_default());
            match &game.kind {
                GameKind::Steam { appid } => crate::steam_link::launch(appid, id),
                GameKind::Epic { app_name } => epic_game(id, &game, app_name, &global),
                GameKind::Amazon { id: game_id } => amazon_game(id, &game, game_id, &global),
                GameKind::Windows | GameKind::Linux => {
                    let target = arkor_stores::launch_target(id, &global.prefixes_dir())?;
                    run_target(id, target)
                }
                GameKind::Emulated { system, emulator } => {
                    emulated_game(id, &game, system, emulator.as_deref(), &global)
                }
            }
        }
        _ => anyhow::bail!("`{id}` is not a library game: add it to the library first"),
    }
}

fn epic_game(
    id: &GameId,
    game: &LibraryGame,
    app_name: &str,
    global: &Global,
) -> anyhow::Result<i32> {
    crate::shops::vault::settle(crate::shops::Shop::Epic);
    let legendary = epic::Legendary::locate_for(app_name)
        .context("legendary not found: log in to Epic on Arkor's Stores page")?;
    anyhow::ensure!(
        legendary.is_logged_in(),
        "not logged in to Epic: log in on Arkor's Stores page"
    );
    let command = legendary.launch_command(app_name, &umu_wrapper(id)?);
    through_client(id, game, "egs", command, global)
}

fn amazon_game(
    id: &GameId,
    game: &LibraryGame,
    game_id: &str,
    global: &Global,
) -> anyhow::Result<i32> {
    crate::shops::vault::settle(crate::shops::Shop::Amazon);
    let nile = arkor_stores::amazon::Nile::locate_for(game_id)
        .context("this Amazon game is not installed, or nile is missing")?;
    let command = nile.launch_command(game_id, &umu_wrapper(id)?);
    through_client(id, game, "amazon", command, global)
}

/// `arkor run <id> -- umu-run`, which a store's client puts in front of the game.
fn umu_wrapper(id: &GameId) -> anyhow::Result<Vec<String>> {
    let me = std::env::current_exe().context("locating the arkor binary")?;
    Ok(vec![
        me.display().to_string(),
        "run".into(),
        id.to_string(),
        "--".into(),
        umu_run()?,
    ])
}

/// Becomes the store client that starts the game, in the game's prefix with its Proton.
fn through_client(
    id: &GameId,
    game: &LibraryGame,
    umu_store: &str,
    (argv, mut env): (Vec<String>, BTreeMap<String, String>),
    global: &Global,
) -> anyhow::Result<i32> {
    let (proton, latest) = chosen_proton(id, game.source_proton.clone());
    crate::components::before_launch(global.updates_on_launch(), latest);
    let prefix = game.effective_prefix(&global.prefixes_dir());
    std::fs::create_dir_all(&prefix)
        .with_context(|| format!("creating prefix {}", prefix.display()))?;
    env.extend(proton_env(Some(umu_store), &prefix, proton, id));
    if !global.updates_on_launch() {
        env.insert("UMU_RUNTIME_UPDATE".into(), "0".into());
    }
    let code = wait_for_game(
        std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .envs(&env),
    )
    .with_context(|| format!("failed to start `{}`", argv[0]));
    crate::shops::vault::settle_all();
    code
}

/// Runs a store's client and waits for the game it starts: legendary ends as soon as the
/// game runs, which would look like the game had ended.
fn wait_for_game(client: &mut std::process::Command) -> anyhow::Result<i32> {
    // What the client leaves behind becomes a child of this process.
    // SAFETY: plain syscall.
    unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) };
    let mut client = client.spawn()?;
    // The client renewed the login at its start: sealed right away, not after hours of play.
    std::thread::spawn(|| {
        for wait in [5, 20, 60] {
            std::thread::sleep(std::time::Duration::from_secs(wait));
            crate::shops::vault::settle_all();
        }
    });
    let status = client.wait()?;
    let mut code = status.code().unwrap_or(1);
    // `arkor run`, which waits for the game itself.
    let me = std::env::current_exe().ok();
    while let Some(pid) = own_child(me.as_deref()) {
        let mut status = 0;
        // SAFETY: waits for a child of this process.
        if unsafe { libc::waitpid(pid, &mut status, 0) } == pid {
            code = if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status)
            } else {
                1
            };
        }
    }
    Ok(code)
}

/// A child of this process that is `arkor run`.
fn own_child(me: Option<&Path>) -> Option<i32> {
    let parent = std::process::id().to_string();
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let pid: i32 = e.file_name().to_str()?.parse().ok()?;
        let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
        // "pid (name) state parent ...": the name may hold spaces and brackets.
        let mut rest = stat.rsplit_once(')')?.1.split_whitespace();
        let (state, of) = (rest.next()?, rest.next()?);
        let ours = of == parent && state != "Z";
        let exe = std::fs::read_link(e.path().join("exe")).ok();
        // Not the one that uploads the saves afterwards.
        let run = std::fs::read(e.path().join("cmdline"))
            .is_ok_and(|c| c.split(|b| *b == 0).nth(1) == Some(b"run"));
        (ours && run && exe.is_some() && exe.as_deref() == me).then_some(pid)
    })
}

/// A game an emulator plays: what it needs and lacks is downloaded first (the emulator, a
/// RetroArch core), then the emulator starts it like a Linux game.
fn emulated_game(
    id: &GameId,
    game: &LibraryGame,
    system: &str,
    emulator: Option<&str>,
    global: &Global,
) -> anyhow::Result<i32> {
    anyhow::ensure!(
        game.exe.exists(),
        "{} not found (is its drive mounted?)",
        game.exe.display()
    );
    let settings = &global.emulators;
    let command = match crate::emu::launch::command(&game.exe, system, emulator, settings) {
        Ok(c) => c,
        Err(e) => {
            let missing = e.downcast::<crate::emu::launch::NotInstalled>()?;
            for key in &missing.keys {
                eprintln!(
                    "arkor: installing {}, which this game uses",
                    crate::downloads::label(key)
                );
                crate::downloads::run(key)?;
            }
            crate::emu::launch::command(&game.exe, system, emulator, settings)?
        }
    };
    eprintln!("arkor: {} plays {}", command.emulator, game.name);
    let mut argv = command.argv;
    argv.extend(game.args.iter().cloned());
    crate::run(id, &argv, &command.env, Some(&command.cwd))
}

fn run_target(id: &GameId, target: ExeTarget) -> anyhow::Result<i32> {
    anyhow::ensure!(
        target.needs_start() || target.exe.is_file(),
        "{} not found (is its drive mounted?)",
        target.exe.display()
    );
    let update = Global::load_or_import()
        .map(|g| g.updates_on_launch())
        .unwrap_or(false);
    if target.native {
        let profile = Profile::load(id).unwrap_or_default();
        let mut command = Vec::new();
        let mut env = BTreeMap::new();
        if let Some(runtime) = profile.runtime().map(str::to_string) {
            // umu runs native games inside the chosen Steam Linux Runtime container.
            crate::components::before_launch(update, None);
            command.push(umu_run()?);
            env.insert("PROTONPATH".into(), runtime);
            env.insert("GAMEID".into(), crate::components::umu_game_id(&id.id));
            env.insert("UMU_NO_PROTON".into(), "1".into());
            if !update {
                env.insert("UMU_RUNTIME_UPDATE".into(), "0".into());
            }
        }
        command.push(target.exe.display().to_string());
        command.extend(target.args);
        return crate::run(id, &command, &env, Some(&target.working_dir));
    }
    let prefix = target
        .prefix
        .clone()
        .context("Windows game without a prefix")?;
    std::fs::create_dir_all(&prefix)
        .with_context(|| format!("creating prefix {}", prefix.display()))?;
    if crate::ea_fix::apply(&prefix, &target.exe).unwrap_or(false) {
        eprintln!("arkor: updated the EA app's launcher path");
    }
    let (proton, latest) = chosen_proton(id, target.proton.clone());
    crate::components::before_launch(update, latest);
    let mut command = vec![umu_run()?];
    if target.needs_start() {
        // Shortcuts open through Wine's `start`, which resolves .lnk/.url inside the prefix.
        // Its full path lets umu find it and Proton pick a game drive (a bare "start.exe"
        // gives "Executable not found" and "unable to use parent for game drive").
        let start = prefix.join("drive_c/windows/system32/start.exe");
        command.push(if start.exists() {
            start.display().to_string()
        } else {
            "start.exe".into()
        });
        command.push("/unix".into());
    }
    command.push(target.exe.display().to_string());
    command.extend(target.args.iter().cloned());
    let mut env = proton_env(None, &prefix, proton, id);
    if !update {
        env.insert("UMU_RUNTIME_UPDATE".into(), "0".into());
    }
    crate::run(id, &command, &env, Some(&target.working_dir))
}

/// Older profile value for "newest GE-Proton" (umu's token); now the auto-updated
/// `Proton-GE Latest` build (see [`crate::components::Latest`]).
pub const GE_LATEST: &str = "GE-Proton";

/// The game's Proton: the profile's choice if set and installed, else the source launcher's.
/// A "Latest" build is also returned as such, so it can be updated before the game starts.
fn chosen_proton(
    id: &GameId,
    from_source: Option<PathBuf>,
) -> (Option<PathBuf>, Option<crate::components::Latest>) {
    // An unreadable profile must not lose the source launcher's Proton.
    let wanted = Profile::load(id).ok().and_then(|p| p.proton);
    // "Automatic": the source launcher's choice, else the default from Settings.
    let wanted = match (wanted, &from_source) {
        (None, None) => Global::load_or_import()
            .ok()
            .and_then(|g| g.defaults.proton),
        (w, _) => w,
    };
    match wanted {
        Some(name) => {
            if let Some(latest) = crate::components::Latest::from_value(&name) {
                return (latest.path().ok(), Some(latest));
            }
            match crate::tools::resolve_proton(&name) {
                Some(path) => (Some(path), None),
                None => {
                    eprintln!("arkor: Proton `{name}` is not installed, using the default");
                    (from_source, None)
                }
            }
        }
        None => (from_source, None),
    }
}

/// Arkor's auto-updated umu-run (Faugus' build) if downloaded, else the system's.
pub(crate) fn umu_run() -> anyhow::Result<String> {
    crate::components::umu_run_path()
        .ok()
        .filter(|p| p.is_file())
        .or_else(|| find_in_path("umu-run"))
        .map(|p| p.display().to_string())
        .context("umu-run not found: install umu-launcher")
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Environment for umu-launcher. Anything here can be overridden from the game's profile
/// (`[env]`), e.g. `PROTONPATH` to pick another Proton build.
pub fn proton_env(
    umu_store: Option<&str>,
    prefix: &Path,
    proton: Option<PathBuf>,
    id: &GameId,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("WINEPREFIX".into(), prefix.display().to_string());
    if let Some(store) = umu_store {
        env.insert("STORE".into(), store.into());
    }
    // umu requires a GAMEID; one per game (the profile's umu id for protonfixes replaces it).
    env.insert("GAMEID".into(), crate::components::umu_game_id(&id.id));
    // As Faugus runs umu: through Steam's runtime when installed, entering the container
    // of an already running game instead of starting a second one.
    env.insert("UMU_USE_STEAM".into(), "1".into());
    env.insert("UMU_CONTAINER_NSENTER".into(), "1".into());
    if std::env::var_os("PROTONPATH").is_none() {
        let proton = proton
            .map(|p| p.display().to_string())
            .unwrap_or_else(default_proton);
        env.insert("PROTONPATH".into(), proton);
    }
    env
}

/// What a prefix tool runs.
#[derive(Debug, Clone, PartialEq)]
pub enum PrefixTool {
    Winecfg,
    /// winetricks with these verbs (none = its GUI).
    Winetricks(Vec<String>),
    /// Any .exe/.msi/.bat/.reg file (installers, patches, registry files).
    Run(PathBuf),
}

/// Environment and prefix a library game's tools run with.
pub fn game_wine_env(id: &GameId) -> anyhow::Result<(PathBuf, BTreeMap<String, String>)> {
    let global = Global::load_or_import()?;
    let library = Library::load()?;
    let game = library
        .get(&id.id)
        .with_context(|| format!("`{id}` is not in the library"))?;
    anyhow::ensure!(game.needs_prefix(), "{} has no Wine prefix", game.name);
    let prefix = game.effective_prefix(&global.prefixes_dir());
    let (proton, _) = chosen_proton(id, game.source_proton.clone());
    let env = proton_env(None, &prefix, proton, id);
    Ok((prefix, env))
}

/// Runs a prefix tool for a library game and waits for it. Blocking.
pub fn run_prefix_tool(id: &GameId, tool: &PrefixTool) -> anyhow::Result<std::process::ExitStatus> {
    let (prefix, mut env) = game_wine_env(id)?;
    std::fs::create_dir_all(&prefix)?;
    let mut cmd = std::process::Command::new(umu_run()?);
    match tool {
        PrefixTool::Winecfg => {
            cmd.arg("winecfg");
        }
        PrefixTool::Winetricks(verbs) => {
            // umu special-cases GAMEID=winetricks: it fetches and runs winetricks itself.
            env.insert("GAMEID".into(), "winetricks".into());
            cmd.arg("winetricks").args(verbs);
        }
        PrefixTool::Run(file) => {
            let ext = file
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            match ext.as_str() {
                "msi" => cmd.args(["msiexec", "/i"]).arg(file),
                "reg" => cmd.args(["regedit", "/S"]).arg(file),
                "bat" | "cmd" | "lnk" | "url" => {
                    cmd.args(["start.exe", "/wait", "/unix"]).arg(file)
                }
                _ => cmd.arg(file),
            };
            if let Some(dir) = file.parent() {
                cmd.current_dir(dir);
            }
        }
    }
    cmd.envs(&env)
        .env_remove("DISABLE_MANGOHUD")
        .env("MANGOHUD", "0");
    cmd.status().context("running umu-run")
}

/// Lets Wine set the game's prefix up again (`wineboot -u`); what is in it stays. Blocking.
pub fn update_prefix(id: &GameId) -> anyhow::Result<()> {
    let (prefix, env) = game_wine_env(id)?;
    std::fs::create_dir_all(&prefix)?;
    let status = std::process::Command::new(umu_run()?)
        .args(["wineboot", "-u"])
        .envs(&env)
        .env_remove("DISABLE_MANGOHUD")
        .env("MANGOHUD", "0")
        .status()
        .context("running umu-run")?;
    anyhow::ensure!(status.success(), "wineboot ended with {status}");
    Ok(())
}

/// Stops everything running in a Windows game's prefix (`wineserver -k`).
pub fn kill_prefix(id: &GameId) -> anyhow::Result<()> {
    let (prefix, env) = game_wine_env(id)?;
    let proton = env.get("PROTONPATH").map(PathBuf::from);
    let wineserver = proton
        .map(|p| p.join("files/bin/wineserver"))
        .filter(|p| p.is_file())
        .or_else(|| find_in_path("wineserver"))
        .context("wineserver not found")?;
    // Proton keeps the actual Wine prefix in `pfx/` inside the compat data folder.
    let wine_prefix = if prefix.join("pfx").is_dir() {
        prefix.join("pfx")
    } else {
        prefix
    };
    std::process::Command::new(wineserver)
        .arg("-k")
        .env("WINEPREFIX", wine_prefix)
        .status()?;
    Ok(())
}

/// Picks a Proton build: an installed one if any (system packages first, e.g. CachyOS's
/// proton-cachyos), otherwise `GE-Proton`, which umu downloads and keeps up to date.
fn default_proton() -> String {
    let candidates: Vec<PathBuf> = crate::tools::installed_protons()
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    pick_proton(&candidates).map_or_else(|| "GE-Proton".into(), |p| p.display().to_string())
}

/// Preference: proton-cachyos, then GE-Proton (newest name first), then any other Proton.
fn pick_proton(candidates: &[PathBuf]) -> Option<&Path> {
    let name = |p: &&PathBuf| {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase()
    };
    let mut usable: Vec<&PathBuf> = candidates
        .iter()
        .filter(|p| p.join("proton").is_file())
        .collect();
    usable.sort_by_key(|p| std::cmp::Reverse(natural_key(&name(p))));
    let rank = |p: &&PathBuf| {
        let n = name(p);
        if n.starts_with("proton-cachyos") {
            0
        } else if n.starts_with("ge-proton") {
            1
        } else {
            2
        }
    };
    usable.into_iter().min_by_key(rank).map(|p| p.as_path())
}

/// Sort key that compares digit runs as numbers, so "GE-Proton10-1" > "GE-Proton9-20".
pub(crate) fn natural_key(s: &str) -> Vec<(u64, String)> {
    let mut key = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        let digit = c.is_ascii_digit();
        let mut run = String::new();
        while let Some(&c) = chars.peek().filter(|c| c.is_ascii_digit() == digit) {
            run.push(c);
            chars.next();
        }
        // Numbers sort by value; text runs carry u64::MAX so they compare as text among themselves.
        key.push(if digit {
            (run.parse().unwrap_or(u64::MAX), String::new())
        } else {
            (u64::MAX, run)
        });
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_cachyos_then_newest_ge() {
        let tmp = tempfile::tempdir().unwrap();
        let mk = |name: &str, with_proton: bool| {
            let dir = tmp.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            if with_proton {
                std::fs::write(dir.join("proton"), "").unwrap();
            }
            dir
        };
        let ge9 = mk("GE-Proton9-20", true);
        let ge10 = mk("GE-Proton10-25", true);
        let broken = mk("proton-cachyos-broken", false);
        let other = mk("Proton-Experimental", true);
        assert_eq!(
            pick_proton(&[ge9.clone(), ge10.clone(), broken.clone(), other.clone()]),
            Some(ge10.as_path())
        );

        let cachy = mk("proton-cachyos-slr", true);
        assert_eq!(
            pick_proton(&[ge10, cachy.clone(), other]),
            Some(cachy.as_path())
        );
        assert_eq!(pick_proton(&[broken]), None);
    }

    #[test]
    fn natural_order() {
        let mut v = ["ge-proton9-20", "ge-proton10-3", "ge-proton10-25"];
        v.sort_by_key(|s| natural_key(s));
        assert_eq!(v, ["ge-proton9-20", "ge-proton10-3", "ge-proton10-25"]);
    }

    #[test]
    fn proton_env_sets_umu_vars() {
        let id: GameId = "local:far-cry-6".parse().unwrap();
        let env = proton_env(
            Some("gog"),
            Path::new("/pfx"),
            Some("/tools/GE".into()),
            &id,
        );
        assert_eq!(env["STORE"], "gog");
        assert_eq!(env["GAMEID"], crate::components::umu_game_id("far-cry-6"));
        assert_eq!(env["WINEPREFIX"], "/pfx");
        if std::env::var_os("PROTONPATH").is_none() {
            assert_eq!(env["PROTONPATH"], "/tools/GE");
        }
        assert!(!proton_env(None, Path::new("/p"), None, &id).contains_key("STORE"));
    }
}
