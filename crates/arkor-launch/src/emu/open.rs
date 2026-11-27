//! Starting an emulator on its own, without a game: to set up its controls and graphics, or
//! browse its menus. It starts the installation games use with the settings Arkor keeps
//! for them (RetroArch's file), so what the user changes there is what games get. The
//! launch template is left out: it names a game.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::profile::Emulators;

use super::discovery::{self, Origin};
use super::launch::Command;
use super::manifest::EngineKind;

/// Whether `id` has a window of its own to open.
pub fn can_open(id: &str) -> bool {
    super::engine(id).is_some_and(|m| m.kind != EngineKind::Relinker)
}

/// The command that starts emulator `id` without a game. Blocking (writes RetroArch's
/// settings).
pub fn command(id: &str, settings: &Emulators) -> anyhow::Result<Command> {
    let m = super::engine(id).with_context(|| format!("unknown emulator {id}"))?;
    anyhow::ensure!(
        m.kind != EngineKind::Relinker,
        "{} only converts games; it has no window to open",
        m.name
    );
    let installs = discovery::find(&m);
    let installation = super::launch::pick(&installs, &m, settings)
        .with_context(|| format!("{} is not installed", m.name))?;
    let home = super::dirs::home(&m.id);
    std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;
    let engine_dir = installation
        .dir
        .clone()
        .or_else(|| installation.program.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let mut args = Vec::new();
    let mut shared: Vec<PathBuf> = Vec::new();
    if m.kind == EngineKind::Retroarch {
        let values = super::launch::option_values(&m, settings);
        let config = home.join("retroarch.cfg");
        super::retroarch::write_config(&config, &super::retroarch::managed_settings(&values))?;
        args.extend(["--config".to_string(), config.display().to_string()]);
        shared.extend([
            home.clone(),
            super::dirs::system(),
            super::retroarch::cores_dir(),
        ]);
    }
    let argv = match installation.origin {
        Origin::Flatpak => {
            let app = installation
                .flatpak
                .clone()
                .context("the Flatpak has no application id")?;
            let mut argv = vec!["flatpak".to_string(), "run".to_string()];
            argv.extend(
                shared
                    .iter()
                    .map(|d| format!("--filesystem={}", d.display())),
            );
            argv.push(app);
            argv.extend(args);
            argv
        }
        _ => std::iter::once(installation.program.display().to_string())
            .chain(args)
            .collect(),
    };
    Ok(Command {
        argv,
        env: Default::default(),
        cwd: if engine_dir.is_dir() {
            engine_dir
        } else {
            home
        },
        emulator: m.name.clone(),
    })
}

/// Starts emulator `id` on its own, in a process group of its own (closing Arkor leaves
/// it open). Blocking only for the start.
pub fn start(id: &str, settings: &Emulators) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let c = command(id, settings)?;
    std::process::Command::new(&c.argv[0])
        .args(&c.argv[1..])
        .envs(&c.env)
        .current_dir(&c.cwd)
        .stdin(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .with_context(|| format!("starting {}", c.emulator))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_without_a_game_or_says_it_is_missing() {
        let _ = super::super::test_root("open");
        let e = command("duckstation", &Emulators::default()).unwrap_err();
        assert!(e.to_string().contains("not installed"), "{e:#}");
        assert!(can_open("retroarch"));
        assert!(command("nothing", &Emulators::default()).is_err());
    }
}
