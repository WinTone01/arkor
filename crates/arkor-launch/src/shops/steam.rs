//! Steam through its own client: the library is read from Steam's files, installing and
//! removing is left to Steam, and its progress is read from the game's manifest.
//!
//! To install without Steam's confirmation window, Arkor leaves a manifest that marks the
//! game as wanted and starts Steam without its window: Steam then downloads it by itself.
//! A Steam that runs a game is not closed for this; its own window is used then.

use std::time::{Duration, Instant};

use anyhow::Context;
use arkor_stores::{steam_library as lib, vdf_edit};

use super::{Installed, Order, Owned, Shop};
use crate::progress::{self, Transfer};
use crate::steam_downloads::{self, Step};
use crate::{steam_link, tools};

const CDN: &str = "https://shared.steamstatic.com/store_item_assets/steam/apps";

pub(super) fn logged_in() -> bool {
    lib::account().is_some()
}

pub(super) fn account() -> Option<String> {
    lib::account()
        .map(|(_, name)| name)
        .filter(|n| !n.is_empty())
}

pub(super) fn owned() -> anyhow::Result<Vec<Owned>> {
    Ok(lib::games()?
        .into_iter()
        .map(|(id, info)| Owned {
            id: id.to_string(),
            title: info.name,
            cover: Some(format!("{CDN}/{id}/library_600x900.jpg")),
            wide: Some(format!("{CDN}/{id}/library_hero.jpg")),
            linux: false,
            windows: true,
            namespace: None,
        })
        .collect())
}

pub(super) fn installed() -> anyhow::Result<Vec<Installed>> {
    Ok(arkor_stores::steam::installed_games()
        .0
        .into_iter()
        .map(|g| Installed {
            shop: Shop::Steam,
            id: g.id.id,
            title: g.name,
            path: g.install_dir.unwrap_or_default(),
            version: String::new(),
            linux: false,
            size: g.size_on_disk,
        })
        .collect())
}

/// Starts Steam without its window.
fn start_silent() -> anyhow::Result<()> {
    steam_link::start_silent()
}

pub(super) fn install(id: &str, order: &Order) -> anyhow::Result<()> {
    let appid: u32 = id.parse().context("not a Steam app")?;
    if lib::progress(appid).is_none() {
        // Steam is not closed under a running game: the download waits for it to end
        // (Steam's own install window would come up over the game otherwise).
        while steam_link::steam_running() && steam_link::any_game_running() {
            if progress::stop_requested() {
                anyhow::bail!(tools::STOPPED);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        {
            // Steam reads manifests when it starts: closed, given the manifest, started
            // without its window, it downloads the game without asking anything.
            steam_link::shutdown()?;
            let root = lib::root().context("Steam is not installed")?;
            let infos = arkor_stores::steam_library::parse_appinfo(
                &std::fs::read(root.join("appcache/appinfo.vdf"))?,
                &[appid].into(),
            )?;
            let info = infos.get(&appid).cloned().unwrap_or_default();
            let owner = lib::account().map(|(id, _)| id).unwrap_or_default();
            lib::write_install_manifest(&root, appid, &info, &owner)?;
            start_silent()?;
        }
    } else if !lib::progress(appid).is_some_and(|p| p.installed()) {
        if !steam_link::steam_running() {
            start_silent()?;
        } else if crate::downloads::asked_to_go_on(&super::key(Shop::Steam, id))
            && steam_downloads::items()
                .iter()
                .any(|i| i.appid == appid && i.step == Step::Paused)
        {
            // Paused in Steam, and the user wants it to go on (a list that was only loaded
            // again does not close Steam).
            go_on()?;
        }
    }
    watch(appid, &order.title)
}

/// Follows Steam's download until the game is installed. Stopping only stops following
/// (Steam goes on with it).
fn watch(appid: u32, label: &str) -> anyhow::Result<()> {
    let transfer = Transfer::begin(label, None);
    let began = Instant::now();
    let mut seen_work = false;
    loop {
        if progress::stop_requested() {
            anyhow::bail!(tools::STOPPED);
        }
        match lib::progress(appid) {
            Some(p) => {
                seen_work |= p.total > 0 || p.state & 1024 != 0;
                // Measured as it happens (the manifest is only written when Steam stops).
                let item = steam_downloads::items()
                    .into_iter()
                    .find(|i| i.appid == appid);
                let (done, total, speed) = match &item {
                    Some(i) => (i.done, i.total, i.speed),
                    None => (p.done, p.total, 0.),
                };
                if !transfer.report(done, (total > 0).then_some(total), speed) {
                    anyhow::bail!(tools::STOPPED);
                }
                if p.installed() && (seen_work || began.elapsed() > Duration::from_secs(20)) {
                    return Ok(());
                }
            }
            // Removed in Steam meanwhile.
            None if seen_work => anyhow::bail!(tools::STOPPED),
            None if began.elapsed() > Duration::from_secs(10 * 60) => {
                anyhow::bail!("Steam did not start the download (log in to Steam and try again)")
            }
            None => {}
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

pub(super) fn repair(game: &Installed) -> anyhow::Result<()> {
    steam_link::open_url(&format!("steam://validate/{}", game.id))
}

pub(super) fn uninstall(game: &Installed) -> anyhow::Result<()> {
    take_away(game.id.parse().context("not a Steam app")?)
}

/// What Arkor says when Steam cannot be closed for a moment.
pub const GAME_RUNS: &str = "a Steam game is running";

/// Removes a game, installed or only begun, without Steam's windows: with Steam closed its
/// files, what was downloaded of it and its manifest go, then Steam starts again without its
/// window. Its saves (Proton's folder for it) stay.
pub(super) fn take_away(appid: u32) -> anyhow::Result<()> {
    // One at a time: Steam is closed and started for each.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    anyhow::ensure!(!steam_link::any_game_running(), GAME_RUNS);
    let path = lib::manifest(appid).context("the game is not installed")?;
    let on = steam_link::steam_running();
    steam_link::shutdown()?;
    // Read once Steam has written it for the last time.
    let result = match lib::read_manifest(path.clone()) {
        Some(m) => remove_files(&m),
        // Steam removed it itself on its way out.
        None if !path.exists() => Ok(()),
        None => Err(anyhow::anyhow!("{} is not readable", path.display())),
    };
    if on {
        start_silent()?;
    }
    result
}

/// Removes what belongs to `m` from its library. Steam must be closed.
fn remove_files(m: &lib::Manifest) -> anyhow::Result<()> {
    let steamapps = m.path.parent().context("no library folder")?;
    let name = std::path::Path::new(&m.folder);
    // Only a folder right in the library's `common` folder is removed.
    let plain = matches!(
        name.components().collect::<Vec<_>>()[..],
        [std::path::Component::Normal(_)]
    );
    let folder = steamapps.join("common").join(name);
    if plain && folder.is_dir() {
        let real = std::fs::canonicalize(&folder)?;
        anyhow::ensure!(
            real.parent()
                == std::fs::canonicalize(steamapps.join("common"))
                    .ok()
                    .as_deref(),
            "{} is not in Steam's library",
            folder.display()
        );
        // Some games share their folder with another one (mods, sequels in one install).
        anyhow::ensure!(
            !lib::folder_shared(&m.path, &m.folder),
            "another Steam game uses the same folder: remove it in Steam"
        );
        std::fs::remove_dir_all(&real)?;
    }
    // What was downloaded and not put together yet.
    let downloading = steamapps.join("downloading");
    let begun = downloading.join(m.appid.to_string());
    if begun.is_dir() && !begun.is_symlink() {
        std::fs::remove_dir_all(&begun)?;
    }
    let state = format!("state_{}_", m.appid);
    for e in std::fs::read_dir(&downloading)
        .into_iter()
        .flatten()
        .flatten()
    {
        if e.file_name().to_string_lossy().starts_with(&state) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    std::fs::remove_file(&m.path)?;
    Ok(())
}

/// An installation was called off: what there is of it goes (an installed game stays).
pub(super) fn call_off(appid: u32) -> anyhow::Result<()> {
    match lib::progress(appid) {
        Some(p) if p.state & 4 == 0 => take_away(appid),
        _ => Ok(()),
    }
}

/// Steam goes on with what it paused when it starts anew.
pub(super) fn go_on() -> anyhow::Result<()> {
    anyhow::ensure!(!steam_link::any_game_running(), GAME_RUNS);
    steam_link::shutdown()?;
    start_silent()
}

/// What was changed to put a game first: its manifest and what stood there.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct First {
    path: std::path::PathBuf,
    before: String,
}

const BEHAVIOR: [&str; 2] = ["AppState", "AutoUpdateBehavior"];

fn first_file() -> Option<std::path::PathBuf> {
    Some(dirs::state_dir()?.join("arkor/steam-first.json"))
}

fn set_behavior(path: &std::path::Path, value: &str) -> anyhow::Result<String> {
    let text = std::fs::read_to_string(path)?;
    let before = vdf_edit::get(&text, &BEHAVIOR)?.unwrap_or_else(|| "0".into());
    if before != value {
        let tmp = path.with_extension("acf.arkor-tmp");
        std::fs::write(&tmp, vdf_edit::set(&text, &BEHAVIOR, value)?)?;
        std::fs::rename(&tmp, path)?;
    }
    Ok(before)
}

/// Puts a game before Steam's other downloads: its update behaviour becomes Steam's own
/// "high priority" (what Properties → Updates sets), and the one put first before gets its
/// own back. Steam only reads this when it starts, so it is closed for a moment.
pub(super) fn first(appid: u32) -> anyhow::Result<()> {
    anyhow::ensure!(!steam_link::any_game_running(), GAME_RUNS);
    let path = lib::manifest(appid).context("the game is not in Steam")?;
    steam_link::shutdown()?;
    let result = put_first(&path, first_file().as_deref());
    start_silent()?;
    result
}

/// The game that was put first is there (or gone): its own setting comes back.
pub(super) fn first_over() {
    let Some(note) = first_file().filter(|f| f.is_file()) else {
        return;
    };
    let old: Option<First> = std::fs::read_to_string(&note)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    if let Some(old) = old
        && let Some(m) = lib::read_manifest(old.path.clone())
    {
        if !m.progress.installed() || steam_link::steam_running() {
            return;
        }
        let _ = set_behavior(&old.path, &old.before);
    }
    let _ = std::fs::remove_file(note);
}

/// See `first`; what was changed is noted in `note`. Steam must be closed.
fn put_first(path: &std::path::Path, note: Option<&std::path::Path>) -> anyhow::Result<()> {
    let old: Option<First> = note
        .and_then(|n| std::fs::read_to_string(n).ok())
        .and_then(|t| serde_json::from_str(&t).ok());
    if let Some(old) = old
        && old.path.is_file()
    {
        set_behavior(&old.path, &old.before)?;
    }
    let before = set_behavior(path, "2")?;
    if let Some(note) = note {
        if let Some(dir) = note.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let now = First {
            path: path.to_path_buf(),
            before,
        };
        std::fs::write(note, serde_json::to_vec(&now)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_game_is_first_and_the_one_before_gets_its_own_back() {
        let (tmp, apps) = library();
        let note = tmp.path().join("state/first.json");
        let write = |id: u32, behavior: &str| {
            let path = apps.join(format!("appmanifest_{id}.acf"));
            std::fs::write(
                &path,
                format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{id}\"\n\t\"AutoUpdateBehavior\"\t\t\"{behavior}\"\n}}\n"),
            )
            .unwrap();
            path
        };
        let (a, b) = (write(10, "0"), write(11, "1"));
        let of = |p: &std::path::Path| {
            let t = std::fs::read_to_string(p).unwrap();
            vdf_edit::get(&t, &BEHAVIOR).unwrap().unwrap()
        };
        put_first(&b, Some(&note)).unwrap();
        assert_eq!((of(&a), of(&b)), ("0".into(), "2".into()));
        put_first(&a, Some(&note)).unwrap();
        assert_eq!((of(&a), of(&b)), ("2".into(), "1".into()));
        // Twice the same: its own value is not forgotten.
        put_first(&a, Some(&note)).unwrap();
        put_first(&b, Some(&note)).unwrap();
        assert_eq!((of(&a), of(&b)), ("0".into(), "2".into()));
    }

    fn library() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let apps = tmp.path().join("steamapps");
        for d in [
            "common/Game",
            "common/Other",
            "downloading/10",
            "downloading/11",
        ] {
            std::fs::create_dir_all(apps.join(d)).unwrap();
            std::fs::write(apps.join(d).join("file"), "x").unwrap();
        }
        for f in ["state_10_1.patch", "state_11_1.patch"] {
            std::fs::write(apps.join("downloading").join(f), "x").unwrap();
        }
        (tmp, apps)
    }

    fn manifest(apps: &std::path::Path, id: u32, folder: &str, state: u32) -> lib::Manifest {
        let path = apps.join(format!("appmanifest_{id}.acf"));
        std::fs::write(
            &path,
            format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"{id}\"\n\t\"name\"\t\t\"G\"\n\t\"StateFlags\"\t\t\"{state}\"\n\t\"installdir\"\t\t\"{folder}\"\n}}\n"
            ),
        )
        .unwrap();
        lib::read_manifest(path).unwrap()
    }

    #[test]
    fn a_game_goes_with_everything_of_it_and_nothing_else() {
        let (_tmp, apps) = library();
        let m = manifest(&apps, 10, "Game", 4);
        remove_files(&m).unwrap();
        for gone in [
            "common/Game",
            "downloading/10",
            "downloading/state_10_1.patch",
            "appmanifest_10.acf",
        ] {
            assert!(!apps.join(gone).exists(), "{gone}");
        }
        for stays in [
            "common/Other/file",
            "downloading/11/file",
            "downloading/state_11_1.patch",
        ] {
            assert!(apps.join(stays).exists(), "{stays}");
        }
    }

    #[test]
    fn a_game_that_was_only_begun_has_no_folder() {
        let (_tmp, apps) = library();
        let m = manifest(&apps, 11, "NotThere", 1026);
        remove_files(&m).unwrap();
        assert!(!apps.join("appmanifest_11.acf").exists());
        assert!(!apps.join("downloading/11").exists());
        assert!(apps.join("common/Game/file").exists());
    }

    #[test]
    fn a_strange_folder_name_removes_nothing_outside() {
        let (tmp, apps) = library();
        let outside = tmp.path().join("precious");
        std::fs::create_dir_all(&outside).unwrap();
        for (id, folder) in [(20, ""), (21, ".."), (22, "../../precious"), (23, "/")] {
            let m = manifest(&apps, id, folder, 1026);
            remove_files(&m).unwrap();
            assert!(outside.exists() && apps.join("common/Game/file").exists());
        }
        // A link out of the library is not followed.
        std::os::unix::fs::symlink(&outside, apps.join("common/Link")).unwrap();
        let m = manifest(&apps, 24, "Link", 4);
        assert!(remove_files(&m).is_err() && outside.exists());
        // Nor is a folder two games share removed.
        let m = manifest(&apps, 25, "Other", 4);
        manifest(&apps, 26, "Other", 4);
        assert!(remove_files(&m).is_err() && apps.join("common/Other/file").exists());
    }
}
