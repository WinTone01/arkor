//! `arkor launch <store:id>`: start a game the way its source needs, with the profile applied.
//!
//! - Steam: hand off to Steam (`steam://rungameid`); the profile applies through the
//!   `arkor run ... %command%` launch option.
//! - Epic: `legendary launch` (login/ownership), with `arkor run <id> -- umu-run` as wrapper.
//! - Everything else (GOG, Heroic sideloads, Faugus, imported prefixes): an [`ExeTarget`]
//!   run through `arkor run`, with umu-run for Windows builds.
//!
//! Windows games run through umu-launcher with a Proton build (see [`proton_env`]).

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{GameId, Store};
use arkor_stores::target::ExeTarget;
use arkor_stores::{epic, heroic};

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
            env.extend(proton_env(
                id,
                Some("egs"),
                heroic::wine_prefix(&id.id),
                chosen_proton(id, heroic::proton(&id.id)),
            ));
            let err = std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .envs(&env)
                .exec();
            Err(err).with_context(|| format!("failed to exec `{}`", argv[0]))
        }
        Store::Gog | Store::Heroic | Store::Faugus | Store::Lutris | Store::Local => {
            let target = arkor_stores::launch_target(id)?;
            // umu's STORE selects store-specific protonfixes; only GOG is a store umu knows.
            let umu_store = (id.store == Store::Gog).then_some("gog");
            run_target(id, target, umu_store)
        }
    }
}

fn run_target(
    id: &GameId,
    target: ExeTarget,
    umu_store: Option<&str>,
) -> anyhow::Result<Infallible> {
    if target.native {
        let command: Vec<String> = std::iter::once(target.exe.display().to_string())
            .chain(target.args)
            .collect();
        return crate::exec_with(id, &command, &BTreeMap::new(), Some(&target.working_dir));
    }
    anyhow::ensure!(
        target.needs_start() || target.exe.is_file(),
        "{} not found (is its drive mounted?)",
        target.exe.display()
    );
    let mut command = vec![umu_run()?];
    if target.needs_start() {
        // Shortcuts open through Wine's `start`, which resolves .lnk/.url inside the prefix.
        command.extend(["start.exe".into(), "/unix".into()]);
    }
    command.push(target.exe.display().to_string());
    command.extend(target.args.iter().cloned());
    let proton = chosen_proton(id, target.proton.clone());
    let env = proton_env(id, umu_store, target.prefix.clone(), proton);
    crate::exec_with(id, &command, &env, Some(&target.working_dir))
}

/// The game's Proton: the profile's choice if set and installed, else the source launcher's.
fn chosen_proton(id: &GameId, from_source: Option<PathBuf>) -> Option<PathBuf> {
    // An unreadable profile must not lose the source launcher's Proton.
    let wanted = arkor_core::Profile::load(id).ok().and_then(|p| p.proton);
    match wanted {
        Some(name) => match crate::tools::resolve_proton(&name) {
            Some(path) => Some(path),
            None => {
                eprintln!("arkor: Proton `{name}` is not installed, using the default");
                from_source
            }
        },
        None => from_source,
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
///
/// `prefix` / `proton` are what the game's source launcher uses (kept so saves and the
/// tested Proton stay the same); otherwise one prefix per game and Arkor's default Proton.
pub fn proton_env(
    id: &GameId,
    umu_store: Option<&str>,
    prefix: Option<PathBuf>,
    proton: Option<PathBuf>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    let prefix = prefix.unwrap_or_else(|| default_prefix(id));
    env.insert("WINEPREFIX".into(), prefix.display().to_string());
    if let Some(store) = umu_store {
        env.insert("STORE".into(), store.into());
    }
    // umu requires a GAMEID; this is umu's own default (no per-game protonfixes yet).
    env.insert("GAMEID".into(), "umu-default".into());
    if std::env::var_os("PROTONPATH").is_none() {
        let proton = proton
            .map(|p| p.display().to_string())
            .unwrap_or_else(default_proton);
        env.insert("PROTONPATH".into(), proton);
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
        let id = GameId::new(Store::Gog, "no-such-heroic-game-123");
        let env = proton_env(&id, Some("gog"), None, None);
        assert_eq!(env["STORE"], "gog");
        assert_eq!(env["GAMEID"], "umu-default");
        assert!(env["WINEPREFIX"].ends_with("Games/Arkor/Prefixes/gog-no-such-heroic-game-123"));

        let local = GameId::new(Store::Local, "x");
        let env = proton_env(&local, None, Some("/pfx".into()), Some("/tools/GE".into()));
        assert_eq!(env["WINEPREFIX"], "/pfx");
        assert!(!env.contains_key("STORE"));
        if std::env::var_os("PROTONPATH").is_none() {
            assert_eq!(env["PROTONPATH"], "/tools/GE");
        }
    }
}
