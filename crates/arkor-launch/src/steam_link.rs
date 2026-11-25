//! Steam games stay Steam's: Steam starts them, with its own Proton and launch options.
//! Arkor writes its choices into Steam's settings instead:
//!
//! - the compatibility tool (Proton) for the game, in `config/config.vdf`
//!   (`CompatToolMapping`), as Steam's Properties → Compatibility does;
//! - the launch options, in `userdata/<account>/config/localconfig.vdf`, set to
//!   `arkor run <id> -- %command%` so Arkor's settings (MangoHud, lsfg-vk, environment,
//!   its launch options) apply when Steam starts the game, from Arkor or from Steam.
//!
//! Steam keeps these files in memory and writes them back when it exits, so they are only
//! changed while Steam is closed; a game whose settings changed while Steam runs gets them
//! when it is started from Arkor (Steam is closed and started again first). Files are edited
//! in place (`vdf_edit`), and backed up once (`<file>.arkor-backup`) before the first change.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context;
use arkor_core::{GameId, Profile};
use arkor_stores::vdf_edit;

const MAPPING: [&str; 5] = [
    "InstallConfigStore",
    "Software",
    "Valve",
    "Steam",
    "CompatToolMapping",
];
const APPS: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];
/// SteamID64 of account id 0.
const STEAMID64_BASE: u64 = 76_561_197_960_265_728;

/// Steam's folder (native Steam; the Flatpak one is not changed).
pub fn root() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    [".local/share/Steam", ".steam/steam"]
        .iter()
        .map(|p| home.join(p))
        .find(|p| p.join("config").is_dir())
}

fn config_file(root: &Path) -> PathBuf {
    root.join("config/config.vdf")
}

/// The signed-in (most recent) account's `localconfig.vdf`.
fn local_config(root: &Path) -> Option<PathBuf> {
    let users = std::fs::read_to_string(root.join("config/loginusers.vdf")).unwrap_or_default();
    let ids = vdf_edit::keys(&users, &["users"]).unwrap_or_default();
    let recent = ids
        .iter()
        .max_by_key(|id| {
            let get = |k: &str| {
                vdf_edit::get(&users, &["users", id, k])
                    .ok()
                    .flatten()
                    .unwrap_or_default()
            };
            (
                get("MostRecent") == "1",
                get("timestamp").parse::<u64>().unwrap_or(0),
            )
        })
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(|id| id.checked_sub(STEAMID64_BASE));
    let file = |account: &str| {
        root.join("userdata")
            .join(account)
            .join("config/localconfig.vdf")
    };
    if let Some(account) = recent
        && file(&account.to_string()).is_file()
    {
        return Some(file(&account.to_string()));
    }
    // Otherwise the account used last on this machine.
    std::fs::read_dir(root.join("userdata"))
        .ok()?
        .flatten()
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "0" && n.chars().all(|c| c.is_ascii_digit()))
        .map(|n| file(&n))
        .filter(|f| f.is_file())
        .max_by_key(|f| f.metadata().and_then(|m| m.modified()).ok())
}

fn read(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// Steam's compatibility tool for `appid` (`None`: Steam's default).
pub fn compat_tool(appid: &str) -> Option<String> {
    let text = read(&config_file(&root()?)).ok()?;
    let mut path = MAPPING.to_vec();
    path.extend([appid, "name"]);
    vdf_edit::get(&text, &path)
        .ok()
        .flatten()
        .filter(|n| !n.is_empty())
}

/// The launch options Steam has for `appid`.
pub fn launch_options(appid: &str) -> Option<String> {
    let text = read(&local_config(&root()?)?).ok()?;
    let mut path = APPS.to_vec();
    path.extend([appid, "LaunchOptions"]);
    vdf_edit::get(&text, &path).ok().flatten()
}

/// A compatibility tool Steam offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    /// Steam's name for it (what `CompatToolMapping` stores).
    pub name: String,
    pub display: String,
}

/// Steam's name for one of Valve's Proton apps: "Proton 9.0" → `proton_9`,
/// "Proton 5.13" → `proton_513`, "Proton Experimental" → `proton_experimental`.
fn valve_tool_name(display: &str) -> Option<String> {
    let rest = display.strip_prefix("Proton ")?;
    match rest {
        "Experimental" => return Some("proton_experimental".into()),
        "Hotfix" => return Some("proton_hotfix".into()),
        _ => {}
    }
    let (major, minor) = rest.split_once('.').unwrap_or((rest, "0"));
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !digits(major) || !digits(minor) {
        return None; // e.g. "Proton EasyAntiCheat Runtime"
    }
    Some(if minor == "0" {
        format!("proton_{major}")
    } else {
        format!("proton_{major}{minor}")
    })
}

/// Every compatibility tool Steam can use: Valve's Proton apps installed in a library, and
/// tools in `compatibilitytools.d` (Arkor's downloads, GE-Proton, system packages).
pub fn tools() -> Vec<Tool> {
    let mut out: Vec<Tool> = Vec::new();
    let Some(root) = root() else { return out };
    for lib in arkor_stores::steam::library_folders(&root).unwrap_or_default() {
        let Ok(dir) = std::fs::read_dir(lib.join("steamapps")) else {
            continue;
        };
        for f in dir.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(f.path()) else {
                continue;
            };
            if let Some(display) = vdf_edit::get(&text, &["AppState", "name"]).ok().flatten()
                && let Some(name) = valve_tool_name(&display)
            {
                out.push(Tool { name, display });
            }
        }
    }
    let mut dirs = vec![root.join("compatibilitytools.d")];
    dirs.extend(
        [
            "/usr/share/steam/compatibilitytools.d",
            "/usr/local/share/steam/compatibilitytools.d",
        ]
        .map(PathBuf::from),
    );
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(e.path().join("compatibilitytool.vdf")) else {
                continue;
            };
            let base = ["compatibilitytools", "compat_tools"];
            for name in vdf_edit::keys(&text, &base).unwrap_or_default() {
                let get = |k: &str| {
                    vdf_edit::get(&text, &[base[0], base[1], &name, k])
                        .ok()
                        .flatten()
                };
                // Only tools that run Windows games (not runtimes for Linux ones).
                if get("from_oslist").is_some_and(|o| !o.contains("windows")) {
                    continue;
                }
                let display = get("display_name").unwrap_or_else(|| name.clone());
                out.push(Tool { name, display });
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(t.name.clone()));
    out.sort_by(|a, b| {
        crate::start::natural_key(&b.display).cmp(&crate::start::natural_key(&a.display))
    });
    out
}

pub fn steam_running() -> bool {
    crate::shortcuts::steam_running()
}

/// The Arkor program Steam should call: the installed one when there is one, so a test build
/// started from elsewhere does not end up in Steam's settings.
fn arkor_program() -> PathBuf {
    let current = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("arkor"));
    let installed = [
        dirs::executable_dir().map(|d| d.join("arkor")),
        Some(PathBuf::from("/usr/bin/arkor")),
    ];
    if installed.iter().flatten().any(|p| *p == current) {
        return current;
    }
    installed
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .unwrap_or(current)
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:+@%".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// `arkor run <id> -- %command%`: what makes Steam start the game through Arkor.
pub fn wrapper(id: &GameId) -> String {
    format!(
        "{} run {id} -- %command%",
        shell_quote(&arkor_program().display().to_string())
    )
}

/// Whether Steam's launch options for the game go through Arkor.
pub fn is_wrapper(options: &str, id: &GameId) -> bool {
    options.contains(&format!(" run {id} -- %command%"))
}

/// What Arkor wants in Steam for a game.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    /// `None`: leave Steam's choice; `Some(None)`: Steam's default; `Some(Some(name))`.
    pub tool: Option<Option<String>>,
    pub launch_options: String,
}

pub fn wanted(id: &GameId, profile: &Profile) -> Wanted {
    Wanted {
        tool: profile
            .steam
            .compat_tool
            .clone()
            .map(|t| Some(t).filter(|t| !t.is_empty())),
        // Through Arkor, Arkor's own launch options are applied by `arkor run`.
        launch_options: if profile.steam.no_arkor {
            profile.launch_options.trim().to_string()
        } else {
            wrapper(id)
        },
    }
}

/// What differs between Steam's settings and Arkor's for the game ("Proton", "launch options").
pub fn out_of_sync(appid: &str, id: &GameId, profile: &Profile) -> Vec<&'static str> {
    let want = wanted(id, profile);
    let mut out = Vec::new();
    if let Some(tool) = want.tool
        && tool != compat_tool(appid)
    {
        out.push("Proton");
    }
    if launch_options(appid).unwrap_or_default().trim() != want.launch_options {
        out.push("launch options");
    }
    out
}

/// Takes the launch options the user had in Steam into Arkor (once, when Arkor has none),
/// so putting Arkor in front of them loses nothing. Returns whether the profile changed.
pub fn adopt_launch_options(appid: &str, id: &GameId, profile: &mut Profile) -> bool {
    let Some(steam) = launch_options(appid) else {
        return false;
    };
    let steam = steam.trim();
    if steam.is_empty() || is_wrapper(steam, id) || !profile.launch_options.trim().is_empty() {
        return false;
    }
    profile.launch_options = steam.to_string();
    true
}

/// Copies `file` to `<file>.arkor-backup` the first time Arkor changes it.
fn backup_once(file: &Path) -> anyhow::Result<()> {
    let backup = PathBuf::from(format!("{}.arkor-backup", file.display()));
    if !backup.exists() {
        std::fs::copy(file, &backup).with_context(|| format!("backing up {}", file.display()))?;
    }
    Ok(())
}

/// Writes through a temporary file, so a crash never leaves half a file.
fn write_atomic(file: &Path, text: &str) -> anyhow::Result<()> {
    let tmp = PathBuf::from(format!("{}.arkor-tmp", file.display()));
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, file).with_context(|| format!("replacing {}", file.display()))?;
    Ok(())
}

/// Writes Arkor's settings for the game into Steam's files under `root`. Steam must be closed.
pub fn apply_in(root: &Path, appid: &str, id: &GameId, profile: &Profile) -> anyhow::Result<()> {
    let want = wanted(id, profile);
    if let Some(tool) = &want.tool {
        let file = config_file(root);
        let text = read(&file)?;
        let mut app = MAPPING.to_vec();
        app.push(appid);
        let new = match tool {
            Some(name) => {
                let mut t = text.clone();
                for (key, value) in [("name", name.as_str()), ("config", ""), ("priority", "250")] {
                    let mut path = app.clone();
                    path.push(key);
                    t = vdf_edit::set(&t, &path, value)?;
                }
                t
            }
            None => vdf_edit::remove(&text, &app)?,
        };
        if new != text {
            backup_once(&file)?;
            write_atomic(&file, &new)?;
        }
    }
    let file = local_config(root).context("no Steam account found (sign in to Steam once)")?;
    let text = read(&file)?;
    let mut path = APPS.to_vec();
    path.extend([appid, "LaunchOptions"]);
    let new = vdf_edit::set(&text, &path, &want.launch_options)?;
    if new != text {
        backup_once(&file)?;
        write_atomic(&file, &new)?;
    }
    Ok(())
}

/// Closes Steam (`steam -shutdown`) and waits until it is gone.
pub fn shutdown() -> anyhow::Result<()> {
    if !steam_running() {
        return Ok(());
    }
    let _ = std::process::Command::new("steam")
        .arg("-shutdown")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let start = Instant::now();
    while steam_running() {
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(60),
            "Steam did not close; close it and try again"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    // Steam writes its files on the way out; let it finish.
    std::thread::sleep(Duration::from_secs(1));
    Ok(())
}

/// Brings Steam's settings for the game in line with Arkor's, closing Steam first when it
/// runs and something differs. Returns what was changed.
pub fn sync(appid: &str, id: &GameId, profile: &Profile) -> anyhow::Result<Vec<&'static str>> {
    let changes = out_of_sync(appid, id, profile);
    if changes.is_empty() {
        return Ok(changes);
    }
    let root = root().context("Steam is not installed")?;
    if steam_running() {
        eprintln!(
            "arkor: closing Steam to apply this game's {}",
            changes.join(" and ")
        );
        shutdown()?;
    }
    apply_in(&root, appid, id, profile)?;
    Ok(changes)
}

/// Asks Steam to start `appid`. Steam is started on its own (not as a child of Arkor).
pub fn open_url(url: &str) -> anyhow::Result<()> {
    steam_command(Some(url))
}

/// Starts Steam without any window (nothing when it runs already).
pub fn start_silent() -> anyhow::Result<()> {
    if steam_running() {
        return Ok(());
    }
    steam_command(None)
}

fn steam_command(url: Option<&str>) -> anyhow::Result<()> {
    let program = if crate::start::find_in_path("steam").is_some() {
        "steam"
    } else {
        "xdg-open"
    };
    let mut cmd = std::process::Command::new(program);
    // Steam that is not running yet starts without its window (and its friends and chat
    // windows), so nothing of it shows over a game or Arkor.
    if program == "steam" && !steam_running() {
        cmd.args(["-silent", "-nofriendsui", "-nochatui"]);
    }
    cmd.args(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: setsid is async-signal-safe; the new session keeps Steam out of Arkor's group.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().with_context(|| format!("starting {program}"))?;
    // Reap it in the background (it exits at once when Steam is already running).
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Whether Steam runs any game now.
pub fn any_game_running() -> bool {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .any(|p| {
            p.file_name()
                .to_str()
                .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
                && std::fs::read(p.path().join("cmdline"))
                    .is_ok_and(|c| c.split(|b| *b == 0).any(|a| a == b"SteamLaunch"))
        })
}

/// Processes Steam started for `appid` (its `reaper SteamLaunch AppId=<appid>`).
pub fn game_pids(appid: &str) -> Vec<u32> {
    let want = format!("AppId={appid}");
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    procs
        .flatten()
        .filter_map(|p| p.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| {
                let args: Vec<&[u8]> = c.split(|b| *b == 0).collect();
                args.contains(&&b"SteamLaunch"[..]) && args.contains(&want.as_bytes())
            })
        })
        .collect()
}

/// Every process below `roots` (children of children…), roots included.
fn with_descendants(roots: &[u32]) -> Vec<u32> {
    let parents: Vec<(u32, u32)> = std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|p| {
            let pid = p.file_name().to_str()?.parse::<u32>().ok()?;
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // Field 4 (ppid) comes after the `(name)`, which may itself hold spaces.
            let ppid = stat
                .rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()?;
            Some((pid, ppid))
        })
        .collect();
    let mut out = roots.to_vec();
    let mut i = 0;
    while i < out.len() {
        let parent = out[i];
        out.extend(
            parents
                .iter()
                .filter(|(_, pp)| *pp == parent)
                .map(|(p, _)| *p),
        );
        i += 1;
    }
    out
}

/// Ends a running Steam game: SIGTERM to the game's processes (Steam sees it as closed).
pub fn stop_game(appid: &str) {
    for pid in with_descendants(&game_pids(appid)).into_iter().rev() {
        // SAFETY: plain syscall.
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }
}

/// `arkor launch` for a Steam game: applies Arkor's settings to Steam, asks Steam to start
/// the game, then waits while it runs (so Arkor shows it as running and can stop it).
pub fn launch(appid: &str, id: &GameId) -> anyhow::Result<i32> {
    let profile = Profile::load(id).unwrap_or_default();
    match sync(appid, id, &profile) {
        Ok(changed) if !changed.is_empty() => {
            eprintln!("arkor: set this game's {} in Steam", changed.join(" and "))
        }
        Ok(_) => {}
        // The game still starts, with what Steam has.
        Err(e) => eprintln!("arkor: could not apply settings to Steam: {e:#}"),
    }
    open_url(&format!("steam://rungameid/{appid}"))?;
    // Steam may need to start, sign in or update the game first.
    let mut start = Instant::now();
    let began = Instant::now();
    while game_pids(appid).is_empty() {
        // While Steam updates the game it is not late.
        if began.elapsed() < Duration::from_secs(12 * 3600)
            && appid
                .parse()
                .is_ok_and(|id| crate::steam_downloads::pending(id).is_some())
        {
            start = Instant::now();
        }
        if start.elapsed() > Duration::from_secs(15 * 60) {
            eprintln!("arkor: Steam did not start the game (is it installed and up to date?)");
            return Ok(0);
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    eprintln!("arkor: Steam started the game");
    while !game_pids(appid).is_empty() {
        std::thread::sleep(Duration::from_secs(2));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_valve_protons() {
        assert_eq!(valve_tool_name("Proton 9.0").as_deref(), Some("proton_9"));
        assert_eq!(
            valve_tool_name("Proton 5.13").as_deref(),
            Some("proton_513")
        );
        assert_eq!(
            valve_tool_name("Proton Experimental").as_deref(),
            Some("proton_experimental")
        );
        assert_eq!(valve_tool_name("Proton EasyAntiCheat Runtime"), None);
        assert_eq!(valve_tool_name("Steam Linux Runtime 3.0 (sniper)"), None);
    }

    #[test]
    fn writes_tool_and_launch_options_into_a_steam_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("userdata/42/config")).unwrap();
        let config = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"AutoUpdateWindowEnabled\"\t\t\"0\"\n\t\t\t}\n\t\t}\n\t}\n}\n";
        let local = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"730\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";
        std::fs::write(root.join("config/config.vdf"), config).unwrap();
        std::fs::write(root.join("userdata/42/config/localconfig.vdf"), local).unwrap();
        let id: GameId = "local:cs2".parse().unwrap();
        let mut profile = Profile::default();
        profile.steam.compat_tool = Some("proton_experimental".into());
        apply_in(root, "730", &id, &profile).unwrap();
        let c = std::fs::read_to_string(root.join("config/config.vdf")).unwrap();
        let mut p = MAPPING.to_vec();
        p.extend(["730", "name"]);
        assert_eq!(
            vdf_edit::get(&c, &p).unwrap().as_deref(),
            Some("proton_experimental")
        );
        let l = std::fs::read_to_string(root.join("userdata/42/config/localconfig.vdf")).unwrap();
        let mut p = APPS.to_vec();
        p.extend(["730", "LaunchOptions"]);
        let options = vdf_edit::get(&l, &p).unwrap().unwrap();
        assert!(is_wrapper(&options, &id), "{options}");
        assert!(l.contains("\"LastPlayed\"\t\t\"1\""));
        // Backups hold the files as they were.
        assert_eq!(
            std::fs::read_to_string(root.join("config/config.vdf.arkor-backup")).unwrap(),
            config
        );
        // Back to Steam's default and without Arkor: mapping removed, own options written.
        profile.steam.compat_tool = Some(String::new());
        profile.steam.no_arkor = true;
        profile.launch_options = "PROTON_ENABLE_WAYLAND=1 %command%".into();
        apply_in(root, "730", &id, &profile).unwrap();
        let c = std::fs::read_to_string(root.join("config/config.vdf")).unwrap();
        assert!(!c.contains("\"730\""), "{c}");
        let l = std::fs::read_to_string(root.join("userdata/42/config/localconfig.vdf")).unwrap();
        assert_eq!(
            vdf_edit::get(&l, &p).unwrap().as_deref(),
            Some("PROTON_ENABLE_WAYLAND=1 %command%")
        );
    }
}

#[cfg(test)]
mod real_steam {
    /// Read-only look at this machine's Steam.
    #[test]
    #[ignore]
    fn reads_this_machines_steam() {
        let root = super::root().unwrap();
        println!("root {}", root.display());
        println!("account file {:?}", super::local_config(&root));
        for t in super::tools() {
            println!("tool {} = {}", t.name, t.display);
        }
        for app in ["230410", "730"] {
            println!(
                "{app}: tool {:?} options {:?}",
                super::compat_tool(app),
                super::launch_options(app)
            );
        }
        let id: arkor_core::GameId = "local:warframe".parse().unwrap();
        println!("wrapper {}", super::wrapper(&id));
        println!(
            "out of sync {:?}",
            super::out_of_sync("230410", &id, &arkor_core::Profile::default())
        );
        println!("running {}", super::steam_running());
    }
}
