//! Mods only while the game runs: `arkor run` places them in the game folder just before the
//! game starts and takes them out when it ends, so the folder is otherwise the store's own
//! (its updates and file checks see no mods, and nothing is left behind).
//!
//! While they are placed, a mark (`mods.placed`: this boot and `arkor run`'s process) stays
//! in the game's config folder. When the computer went off or `arkor run` was killed with the
//! game, nothing took them out: [`leftovers`] finds such marks (another boot, or a process
//! that is gone) and takes those mods out, when Arkor starts and before every launch.

use std::path::{Path, PathBuf};

use arkor_core::GameId;
use arkor_core::library::{GameKind, LibraryGame};

use super::{Link, amethyst, link};

/// Whether `game` starts through `arkor run`: every game Arkor starts does, and a Steam game
/// does when Steam's launch options for it are Arkor's (Steam starts it, from Arkor or not).
pub fn through_arkor(id: &GameId, game: &LibraryGame) -> bool {
    match &game.kind {
        GameKind::Steam { appid } => crate::steam_link::launch_options(appid)
            .is_some_and(|o| crate::steam_link::is_wrapper(&o, id)),
        _ => true,
    }
}

/// Before the game starts: its mods are placed in `game_dir` (when they are kept out of it
/// otherwise) and what their loaders need from Wine is read again, for this launch.
/// Returns what takes them out again when it is dropped (also when the launch fails before
/// the game starts).
pub fn before(id: &GameId, game_dir: Option<&Path>) -> Option<Placed> {
    // A launch that never ended (the computer went off) left mods behind: out first.
    leftovers();
    let mut link = link::load(id).filter(|l| l.while_playing)?;
    let placed = amethyst::with(|c| {
        super::deploy(c, &link)?;
        link::refresh(c, id, &mut link)
    });
    // Not needed while the game runs.
    amethyst::release_now();
    if let Err(e) = placed {
        eprintln!("arkor: the mods were not placed: {e:#}");
        return None;
    }
    let mark = arkor_core::paths::game_config_dir(id)
        .ok()
        .map(|d| d.join(MARK));
    if let Some(mark) = &mark
        && let Err(e) = std::fs::write(
            mark,
            format!(
                "{} {} {}\n",
                boot_id(),
                std::process::id(),
                started(std::process::id()).unwrap_or_default()
            ),
        )
    {
        eprintln!("arkor: {}: {e}", mark.display());
    }
    let missing = game_dir.and_then(super::missing_loader).map(str::to_string);
    if missing != link.loader_missing {
        link.loader_missing = missing;
        let _ = link::save(id, &link);
    }
    Some(Placed { link, mark })
}

/// A game's mods placed for one launch; dropping it takes them out of the game folder.
#[derive(Debug)]
pub struct Placed {
    link: Link,
    mark: Option<PathBuf>,
}

impl Drop for Placed {
    fn drop(&mut self) {
        match amethyst::with(|c| super::restore(c, &self.link)) {
            Ok(()) => {
                if let Some(mark) = &self.mark {
                    let _ = std::fs::remove_file(mark);
                }
            }
            Err(e) => eprintln!("arkor: the mods were not taken out of the game folder: {e:#}"),
        }
        amethyst::release_now();
    }
}

/// The mark of mods placed for a launch, in the game's config folder.
const MARK: &str = "mods.placed";

/// This boot of the computer.
pub(crate) fn boot_id() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// When a process started (clock ticks after boot), which tells it from a later process
/// given the same number.
pub(crate) fn started(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // After the name in brackets (which may hold spaces): state is field 3, start time 22.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

/// Whether a mark's launch is still going: the same boot, and its process alive.
pub(crate) fn still_placed(mark: &str, boot: &str) -> bool {
    let mut words = mark.split_whitespace();
    let (Some(at), Some(pid), Some(start)) = (words.next(), words.next(), words.next()) else {
        return false;
    };
    let Ok(pid) = pid.parse::<u32>() else {
        return false;
    };
    at == boot && started(pid).is_some_and(|s| s.to_string() == start)
}

/// Takes out the mods a launch left in game folders (the computer went off, or `arkor run`
/// was killed); how many games. Blocking.
pub fn leftovers() -> usize {
    let Ok(games) = arkor_core::paths::config_dir().map(|d| d.join("games")) else {
        return 0;
    };
    let boot = boot_id();
    let left: Vec<PathBuf> = std::fs::read_dir(games)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|dir| {
            std::fs::read_to_string(dir.join(MARK)).is_ok_and(|m| !still_placed(&m, &boot))
        })
        .collect();
    let mut done = 0;
    for dir in left {
        let mark = dir.join(MARK);
        let Some(link) = std::fs::read_to_string(dir.join("mods.toml"))
            .ok()
            .and_then(|t| toml::from_str::<Link>(&t).ok())
        else {
            let _ = std::fs::remove_file(&mark);
            continue;
        };
        match amethyst::with(|c| super::restore(c, &link)) {
            Ok(()) => {
                let _ = std::fs::remove_file(&mark);
                done += 1;
            }
            Err(e) => eprintln!("arkor: mods left in {}: {e:#}", link.game),
        }
    }
    if done > 0 {
        amethyst::release_now();
    }
    done
}

/// Turns "only while playing" on or off for a game, and puts the folder in the state that
/// goes with it: mods out (on, and the game starts through Arkor) or in.
pub fn set(id: &GameId, on: bool, through: bool) -> anyhow::Result<()> {
    let mut link = link::load(id).ok_or_else(|| anyhow::anyhow!("no mods for {id}"))?;
    link.while_playing = on;
    link::save(id, &link)?;
    link.while_playing = on && through;
    amethyst::with(|c| super::settle(c, &link))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_a_finished_launch_from_a_running_one() {
        let boot = "b1";
        let me = std::process::id();
        let at = started(me).unwrap();
        assert!(
            still_placed(&format!("b1 {me} {at}\n"), boot),
            "this process runs"
        );
        assert!(
            !still_placed(&format!("b0 {me} {at}"), boot),
            "another boot"
        );
        assert!(
            !still_placed(&format!("b1 {me} {}", at + 1), boot),
            "another process with the same number"
        );
        assert!(
            !still_placed("b1 4294967 1", boot),
            "a process that is gone"
        );
        assert!(!still_placed(&format!("b1 {me}"), boot), "an old mark");
        assert!(!still_placed("", boot));
    }
}
