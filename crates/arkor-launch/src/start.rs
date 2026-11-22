//! `arkor launch <store:id>`: start a game the way its store needs, with the profile applied.
//!
//! - Steam: hand off to Steam (`steam://rungameid`); the profile applies through the
//!   `arkor run ... %command%` launch option.
//! - Epic: `legendary launch` (login/ownership), with `arkor run <id> -- umu-run` as wrapper.
//! - GOG: DRM-free, so `arkor run` directly: `umu-run <exe>` or the native `start.sh`.
//!
//! Windows games run through umu-launcher with a Proton build (see [`proton_env`]).

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{GameId, Store};
use arkor_stores::{epic, gog, heroic};

pub fn start(id: &GameId) -> anyhow::Result<Infallible> {
    match id.store {
        Store::Steam => {
            let err = std::process::Command::new("steam")
                .arg(format!("steam://rungameid/{}", id.id))
                .exec();
            Err(err).context("failed to start Steam")
        }
        Store::Epic => {
            let legendary = epic::Legendary::locate()
                .context("legendary not found: install `legendary` or Heroic Games Launcher")?;
            anyhow::ensure!(
                legendary.is_logged_in(),
                "not logged in to Epic: run `legendary auth` or log in with Heroic"
            );
            let me = std::env::current_exe().context("locating the arkor binary")?;
            let wrapper = [
                me.display().to_string(),
                "run".into(),
                id.to_string(),
                "--".into(),
                umu_run()?,
            ];
            let (argv, mut env) = legendary.launch_command(&id.id, &wrapper);
            env.extend(proton_env(id, "egs"));
            let err = std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .envs(&env)
                .exec();
            Err(err).with_context(|| format!("failed to exec `{}`", argv[0]))
        }
        Store::Gog => {
            let target = gog::launch_target(&id.id)?;
            let exe = target.exe.display().to_string();
            if target.native {
                let command: Vec<String> = std::iter::once(exe).chain(target.args).collect();
                crate::exec_with(id, &command, &BTreeMap::new(), Some(&target.working_dir))
            } else {
                let command: Vec<String> =
                    [umu_run()?, exe].into_iter().chain(target.args).collect();
                crate::exec_with(
                    id,
                    &command,
                    &proton_env(id, "gog"),
                    Some(&target.working_dir),
                )
            }
        }
        Store::Amazon | Store::Local => {
            anyhow::bail!("launching {} games is not supported yet", id.store.as_str())
        }
    }
}

fn umu_run() -> anyhow::Result<String> {
    find_in_path("umu-run")
        .map(|p| p.display().to_string())
        .context("umu-run not found: install umu-launcher")
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Environment for umu-launcher. Anything here can be overridden from the game's profile
/// (`[env]`), e.g. `PROTONPATH` to pick another Proton build.
pub fn proton_env(id: &GameId, umu_store: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    // Heroic's prefix if it set this game up (keeps saves), otherwise one prefix per game.
    let prefix = heroic::wine_prefix(&id.id).unwrap_or_else(|| default_prefix(id));
    env.insert("WINEPREFIX".into(), prefix.display().to_string());
    env.insert("STORE".into(), umu_store.into());
    // umu requires a GAMEID; this is umu's own default (no per-game protonfixes yet).
    env.insert("GAMEID".into(), "umu-default".into());
    if std::env::var_os("PROTONPATH").is_none() {
        env.insert("PROTONPATH".into(), default_proton());
    }
    env
}

fn default_prefix(id: &GameId) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Games/Arkor/Prefixes").join(id.file_stem())
}

/// Picks a Proton build: an installed one if any (system packages first, e.g. CachyOS's
/// proton-cachyos), otherwise `GE-Proton`, which umu downloads and keeps up to date.
fn default_proton() -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let roots = [
        PathBuf::from("/usr/share/steam/compatibilitytools.d"),
        home.join(".local/share/Steam/compatibilitytools.d"),
        home.join(".steam/root/compatibilitytools.d"),
        home.join(".config/heroic/tools/proton"),
    ];
    let mut candidates = Vec::new();
    for root in roots {
        if let Ok(entries) = std::fs::read_dir(&root) {
            candidates.extend(entries.flatten().map(|e| e.path()));
        }
    }
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
fn natural_key(s: &str) -> Vec<(u64, String)> {
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
        let id = GameId::new(Store::Gog, "no-such-heroic-game-123");
        let env = proton_env(&id, "gog");
        assert_eq!(env["STORE"], "gog");
        assert_eq!(env["GAMEID"], "umu-default");
        assert!(env["WINEPREFIX"].ends_with("Games/Arkor/Prefixes/gog-no-such-heroic-game-123"));
    }
}
