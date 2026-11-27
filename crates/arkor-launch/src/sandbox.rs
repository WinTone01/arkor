//! Running a game it is better not to trust in a sandbox (bubblewrap): no network, and no
//! home folder. Inside, `$HOME` is a private folder of the game
//! (`~/.local/share/arkor/sandbox/<game>`, kept between runs, so its saves and caches stay);
//! only what the launch needs from the real home shows through: the folders named on the
//! command line and in the launch's variables (the prefix, the game, Proton, Arkor's own
//! files), umu's runtime, and the display and sound sockets. The session bus is cut off.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::GameId;

/// The private home folder of a game's sandbox.
pub fn home_of(id: &GameId) -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no data folder")?
        .join("arkor/sandbox")
        .join(id.file_stem()))
}

/// Folders of the real home a launch needs: those of the absolute paths in `argv` and
/// `env` (a file gives its folder), and umu's. Never the home folder itself.
fn needed(argv: &[String], env: &BTreeMap<String, String>, home: &Path) -> Vec<PathBuf> {
    let skip_vars = [
        "PATH",
        "LD_LIBRARY_PATH",
        "XDG_DATA_DIRS",
        "XDG_CONFIG_DIRS",
        "HOME",
    ];
    let mut candidates: Vec<PathBuf> = argv
        .iter()
        .filter(|a| a.starts_with('/'))
        .map(PathBuf::from)
        .collect();
    for (k, v) in env {
        if skip_vars.contains(&k.as_str()) {
            continue;
        }
        candidates.extend(
            v.split(':')
                .filter(|p| p.starts_with('/'))
                .map(PathBuf::from),
        );
    }
    candidates.push(home.join(".local/share/umu"));
    candidates.push(home.join(".local/share/Steam/compatibilitytools.d"));
    let mut out: Vec<PathBuf> = Vec::new();
    for c in candidates {
        let dir = if c.is_dir() {
            c
        } else if c.exists() {
            match c.parent() {
                Some(p) => p.to_path_buf(),
                None => continue,
            }
        } else {
            continue;
        };
        if !dir.starts_with(home) || dir == home {
            continue;
        }
        // Not when an outer folder is already there.
        if out.iter().any(|o| dir.starts_with(o)) {
            continue;
        }
        out.retain(|o| !o.starts_with(&dir));
        out.push(dir);
    }
    out.sort();
    out
}

/// `argv` started in the game's sandbox.
pub fn wrap(
    id: &GameId,
    argv: Vec<String>,
    env: &BTreeMap<String, String>,
    network: bool,
) -> anyhow::Result<Vec<String>> {
    let bwrap = crate::start::find_in_path("bwrap")
        .context("the sandbox needs bubblewrap (the bwrap program); install it")?;
    let home = dirs::home_dir().context("no home folder")?;
    let private = home_of(id)?;
    std::fs::create_dir_all(&private)?;
    let s = |p: &Path| p.display().to_string();
    let mut a: Vec<String> = vec![s(&bwrap), "--die-with-parent".into()];
    if !network {
        a.push("--unshare-net".into());
    }
    a.extend(["--dev-bind", "/", "/"].map(String::from));
    // A private /tmp, with the X11 sockets still there.
    a.extend(
        [
            "--tmpfs",
            "/tmp",
            "--ro-bind-try",
            "/tmp/.X11-unix",
            "/tmp/.X11-unix",
        ]
        .map(String::from),
    );
    a.extend(["--bind".into(), s(&private), s(&home)]);
    for dir in needed(&argv, env, &home) {
        a.extend(["--bind".into(), s(&dir), s(&dir)]);
    }
    // The session bus lets a program start others outside; it is cut off.
    if let Some(run) = dirs::runtime_dir() {
        let bus = run.join("bus");
        if bus.exists() {
            a.extend(["--ro-bind".into(), "/dev/null".into(), s(&bus)]);
        }
    }
    a.extend(["--unsetenv", "DBUS_SESSION_BUS_ADDRESS"].map(String::from));
    if !network {
        // umu would look for runtime updates first and wait for the network.
        a.extend(["--setenv", "UMU_RUNTIME_UPDATE", "0"].map(String::from));
    }
    a.push("--".into());
    a.extend(argv);
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_only_the_folders_the_launch_names() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let prefix = home.join("Games/Arkor/Prefixes/game");
        let game = home.join("Games/Game");
        let proton = home.join(".local/share/Steam/compatibilitytools.d/GE-Proton");
        for d in [&prefix, &game, &proton] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(game.join("game.exe"), "").unwrap();
        let argv = vec![
            "/usr/bin/umu-run".to_string(),
            game.join("game.exe").display().to_string(),
        ];
        let env: BTreeMap<String, String> = [
            ("WINEPREFIX".to_string(), prefix.display().to_string()),
            ("PROTONPATH".to_string(), proton.display().to_string()),
            ("HOME".to_string(), home.display().to_string()),
            ("PATH".to_string(), home.join("bin").display().to_string()),
        ]
        .into();
        let dirs = needed(&argv, &env, home);
        assert!(dirs.contains(&prefix) && dirs.contains(&game));
        assert!(
            dirs.contains(&home.join(".local/share/Steam/compatibilitytools.d")),
            "Proton's folder is inside compatibilitytools.d, shown once"
        );
        assert!(!dirs.contains(&home.to_path_buf()), "never the whole home");
        assert!(
            !dirs.iter().any(|d| d.ends_with("bin")),
            "PATH is not followed"
        );
    }
}
