//! Turning a game file into the command that plays it: pick the emulator and its
//! installation, link the system's BIOS files where the emulator looks, write RetroArch's
//! settings, and expand the manifest's launch template.
//!
//! The result is a plain command that `arkor run` starts like any Linux game, so MangoHud,
//! gamescope, play time, clips and the quick menu all apply.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::profile::Emulators;
use serde_json::Value;

use super::bios::{self, BiosIndex};
use super::discovery::{self, Installation, Origin};
use super::manifest::{Arg, EngineKind, EngineManifest};
use super::{EngineRef, systems};

/// What a game needs is not installed; the keys download it (see `crate::downloads`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotInstalled {
    pub name: String,
    pub keys: Vec<String>,
}

impl std::fmt::Display for NotInstalled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} is not installed", self.name)
    }
}

impl std::error::Error for NotInstalled {}

/// A system's games need BIOS files the user has not provided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiosMissing {
    pub system: String,
    pub name: String,
    /// The file names that would do.
    pub files: Vec<String>,
    pub folder: PathBuf,
}

impl std::fmt::Display for BiosMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} games need a startup file from your console (BIOS): put {} in {}",
            self.name,
            self.files.join(" or "),
            self.folder.display()
        )
    }
}

impl std::error::Error for BiosMissing {}

/// The command that plays a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
    /// The emulator, for messages.
    pub emulator: String,
}

/// The command that plays `game` (its file or folder) of `system` with the game's own
/// emulator choice `own`, else the system's default. Blocking: links files, writes
/// settings.
pub fn command(
    game: &Path,
    system: &str,
    own: Option<&str>,
    settings: &Emulators,
) -> anyhow::Result<Command> {
    let sys =
        systems::get(system).with_context(|| format!("Arkor does not know the system {system}"))?;
    let choice = super::choice_for_game(system, own, settings)
        .with_context(|| format!("no emulator plays {}", sys.name))?;
    let m = super::engine(&choice.engine).context("unknown emulator")?;
    let installs = discovery::find(&m);
    let missing = super::missing_keys(&choice);
    if !missing.is_empty() {
        return Err(NotInstalled {
            name: choice.label(),
            keys: missing,
        }
        .into());
    }
    let installation = installs.first().context("no installation")?;
    let bios_root = super::bios_dir(settings);
    let index = if bios_root.is_dir() {
        bios::scan(&bios_root, &bios::cached_index()).unwrap_or_default()
    } else {
        BiosIndex::default()
    };
    if bios::status(sys, &index).state == bios::SystemState::Missing {
        return Err(BiosMissing {
            system: sys.id.clone(),
            name: sys.name.clone(),
            files: sys.bios.files.iter().map(|f| f.name.clone()).collect(),
            folder: bios_root,
        }
        .into());
    }
    let mut values = m.option_values(&BTreeMap::new());
    if settings.windowed.contains(&m.id) && values.contains_key("fullscreen") {
        values.insert("fullscreen".into(), Value::Bool(false));
    }
    build(&Input {
        manifest: &m,
        installation,
        game,
        system: sys,
        choice: &choice,
        values: &values,
        bios: &index,
    })
}

pub(crate) struct Input<'a> {
    pub manifest: &'a EngineManifest,
    pub installation: &'a Installation,
    pub game: &'a Path,
    pub system: &'a systems::System,
    pub choice: &'a EngineRef,
    pub values: &'a BTreeMap<String, Value>,
    pub bios: &'a BiosIndex,
}

pub(crate) fn build(input: &Input) -> anyhow::Result<Command> {
    let Input {
        manifest,
        installation,
        game,
        system,
        choice,
        values,
        bios,
    } = *input;
    let rom_dir = if game.is_dir() {
        game.to_path_buf()
    } else {
        game.parent().map(Path::to_path_buf).unwrap_or_default()
    };
    let home = super::dirs::home(&manifest.id);
    let engine_dir = installation
        .dir
        .clone()
        .or_else(|| installation.program.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let vars = Vars {
        rom: game.display().to_string(),
        rom_dir: rom_dir.display().to_string(),
        rom_dir_name: rom_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        home: ::dirs::home_dir().unwrap_or_default().display().to_string(),
        engine_home: home.display().to_string(),
        engine_dir: engine_dir.display().to_string(),
        xdg_data: ::dirs::data_dir().unwrap_or_default().display().to_string(),
    };
    // Folders a Flatpak must see.
    let mut shared: Vec<PathBuf> = vec![rom_dir.clone()];
    let mut args: Vec<String> = Vec::new();
    match manifest.kind {
        EngineKind::Retroarch => {
            let core = choice
                .core
                .as_deref()
                .or_else(|| {
                    manifest
                        .cores_for(&system.id)
                        .first()
                        .map(|c| c.id.as_str())
                })
                .context("RetroArch needs a core for this system")?;
            let core_path = super::retroarch::installed_core(core).ok_or_else(|| NotInstalled {
                name: choice.label(),
                keys: vec![format!("core:{core}")],
            })?;
            bios::link_into(system, bios, &super::dirs::system())?;
            let config = home.join("retroarch.cfg");
            super::retroarch::write_config(&config, &super::retroarch::managed_settings(values))?;
            args.extend([
                "--config".into(),
                config.display().to_string(),
                "-L".into(),
                core_path.display().to_string(),
            ]);
            shared.extend([
                home.clone(),
                super::dirs::system(),
                super::retroarch::cores_dir(),
            ]);
            if let Ok(real) = std::fs::canonicalize(&bios.root) {
                shared.push(real);
            }
        }
        _ => {
            if let Some(template) = &manifest.bios_dir.linux {
                let dir = PathBuf::from(vars.expand(template));
                if !dir.as_os_str().is_empty()
                    && (installation.origin == Origin::Managed
                        || !template.contains("{engine_dir}"))
                {
                    bios::link_into(system, bios, &dir)?;
                    shared.push(bios.root.clone());
                }
            }
        }
    }
    expand_args(&manifest.launch.args, values, &vars, &mut args);
    let has_rom = manifest
        .launch
        .args
        .iter()
        .any(|a| matches!(a, Arg::Plain(s) if s.contains("{rom")));
    match manifest.kind {
        EngineKind::Dosbox => {
            let conf = game
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("conf"));
            if conf {
                args.extend(["--conf".into(), game.display().to_string()]);
            } else {
                args.push(game.display().to_string());
                if flag(values, "exit_on_quit") && !game.is_dir() {
                    args.push("-exit".into());
                }
            }
        }
        EngineKind::Scummvm => {
            args.push(format!("--path={}", rom_dir.display()));
            args.push("--auto-detect".into());
        }
        EngineKind::Mame => {
            let machine = game
                .file_stem()
                .context("the game file has no name")?
                .to_string_lossy()
                .into_owned();
            args.extend(["-rompath".into(), rom_dir.display().to_string(), machine]);
        }
        EngineKind::Retroarch | EngineKind::Standalone => {
            if !has_rom {
                args.push(game.display().to_string());
            }
        }
        EngineKind::Relinker => anyhow::bail!("{} is not supported by Arkor", manifest.name),
    }
    let mut env = BTreeMap::new();
    let argv = match installation.origin {
        Origin::Flatpak => {
            let id = installation
                .flatpak
                .clone()
                .context("the Flatpak has no application id")?;
            let mut argv = vec!["flatpak".to_string(), "run".to_string()];
            shared.sort();
            shared.dedup();
            for dir in shared.iter().filter(|d| !d.as_os_str().is_empty()) {
                argv.push(format!("--filesystem={}", dir.display()));
            }
            argv.push(id);
            argv.extend(args);
            argv
        }
        _ => {
            let program = &installation.program;
            // An AppImage Arkor could not unpack: its own runtime unpacks it each time
            // (where it can), so FUSE is not needed.
            if program
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("appimage"))
                && !std::fs::read(program).is_ok_and(|b| b.starts_with(b"#!"))
            {
                env.insert("APPIMAGE_EXTRACT_AND_RUN".into(), "1".into());
            }
            std::iter::once(program.display().to_string())
                .chain(args)
                .collect()
        }
    };
    for (k, v) in &manifest.launch.env {
        env.insert(k.clone(), vars.expand(v));
    }
    std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;
    Ok(Command {
        argv,
        env,
        cwd: rom_dir,
        emulator: choice.label(),
    })
}

fn flag(values: &BTreeMap<String, Value>, id: &str) -> bool {
    values.get(id).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) struct Vars {
    pub rom: String,
    pub rom_dir: String,
    pub rom_dir_name: String,
    pub home: String,
    pub engine_home: String,
    pub engine_dir: String,
    pub xdg_data: String,
}

impl Vars {
    pub(crate) fn expand(&self, s: &str) -> String {
        s.replace("{rom_dir_name}", &self.rom_dir_name)
            .replace("{rom_dir}", &self.rom_dir)
            .replace("{rom}", &self.rom)
            .replace("{engine_home}", &self.engine_home)
            .replace("{engine_dir}", &self.engine_dir)
            .replace("{xdg_data}", &self.xdg_data)
            .replace("{home}", &self.home)
    }
}

fn expand_args(
    template: &[Arg],
    values: &BTreeMap<String, Value>,
    vars: &Vars,
    out: &mut Vec<String>,
) {
    for arg in template {
        match arg {
            Arg::Plain(s) => {
                let mut text = vars.expand(s);
                for (id, value) in values {
                    let key = format!("{{opt.{id}}}");
                    if text.contains(&key) {
                        let v = match value {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        text = text.replace(&key, &v);
                    }
                }
                out.push(text);
            }
            Arg::When { when, args } if flag(values, when) => expand_args(args, values, vars, out),
            Arg::Unless { unless, args }
                if values.contains_key(unless) && !flag(values, unless) =>
            {
                expand_args(args, values, vars, out)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installation(engine: &str, origin: Origin, program: &Path) -> Installation {
        Installation {
            engine: engine.into(),
            origin,
            version: None,
            program: program.to_path_buf(),
            flatpak: (origin == Origin::Flatpak).then(|| format!("org.example.{engine}")),
            dir: None,
        }
    }

    fn run(
        engine: &str,
        origin: Origin,
        rom: &Path,
        system: &str,
        values: Option<BTreeMap<String, Value>>,
    ) -> Command {
        let m = super::super::engine(engine).unwrap();
        let program = rom.parent().unwrap().join("emu-bin");
        let inst = installation(engine, origin, &program);
        let values = values.unwrap_or_else(|| m.option_values(&BTreeMap::new()));
        build(&Input {
            manifest: &m,
            installation: &inst,
            game: rom,
            system: systems::get(system).unwrap(),
            choice: &EngineRef {
                engine: engine.into(),
                core: None,
            },
            values: &values,
            bios: &BiosIndex::default(),
        })
        .unwrap()
    }

    #[test]
    fn standalone_command_follows_the_template() {
        let root = super::super::test_root("launch-standalone");
        let rom = root.join("psx/Game (USA).cue");
        std::fs::create_dir_all(rom.parent().unwrap()).unwrap();
        std::fs::write(&rom, b"FILE").unwrap();
        let c = run("duckstation", Origin::System, &rom, "psx", None);
        assert_eq!(
            c.argv[1..],
            ["-batch", "-fullscreen", "--", rom.to_str().unwrap()]
        );
        assert_eq!(c.cwd, rom.parent().unwrap());

        let m = super::super::engine("duckstation").unwrap();
        let mut off = m.option_values(&BTreeMap::new());
        off.insert("fullscreen".into(), Value::Bool(false));
        let c = run("duckstation", Origin::System, &rom, "psx", Some(off));
        assert!(!c.argv.iter().any(|a| a == "-fullscreen"));
    }

    #[test]
    fn flatpaks_see_the_game_folder() {
        let root = super::super::test_root("launch-flatpak");
        let rom = root.join("psp/game.iso");
        std::fs::create_dir_all(rom.parent().unwrap()).unwrap();
        std::fs::write(&rom, b"x").unwrap();
        let c = run("ppsspp", Origin::Flatpak, &rom, "psp", None);
        assert_eq!(c.argv[..2], ["flatpak", "run"]);
        assert!(
            c.argv
                .contains(&format!("--filesystem={}", rom.parent().unwrap().display()))
        );
        assert!(c.argv.contains(&"org.example.ppsspp".to_string()));
        assert_eq!(c.argv.last().unwrap(), rom.to_str().unwrap());
    }

    #[test]
    fn mame_runs_the_machine_by_name() {
        let root = super::super::test_root("launch-mame");
        let rom = root.join("arcade/pacman.zip");
        std::fs::create_dir_all(rom.parent().unwrap()).unwrap();
        std::fs::write(&rom, b"PK").unwrap();
        let c = run("mame", Origin::System, &rom, "arcade", None);
        let n = c.argv.len();
        assert_eq!(
            c.argv[n - 3..],
            [
                "-rompath",
                rom.parent().unwrap().to_str().unwrap(),
                "pacman"
            ]
        );
    }

    #[test]
    fn missing_emulators_say_what_to_download() {
        let root = super::super::test_root("launch-missing");
        let rom = root.join("snes/Game.sfc");
        std::fs::create_dir_all(rom.parent().unwrap()).unwrap();
        std::fs::write(&rom, b"x").unwrap();
        let mut settings = Emulators::default();
        settings
            .defaults
            .insert("snes".into(), "retroarch:snes9x".into());
        let e = command(&rom, "snes", None, &settings).unwrap_err();
        let missing = e.downcast_ref::<NotInstalled>().unwrap();
        assert!(
            missing.keys.contains(&"core:snes9x".to_string()),
            "{missing:?}"
        );
    }

    #[test]
    fn psx_games_need_a_bios() {
        let root = super::super::test_root("launch-bios");
        let rom = root.join("psx/Game.chd");
        std::fs::create_dir_all(rom.parent().unwrap()).unwrap();
        std::fs::write(&rom, b"x").unwrap();
        // DuckStation "installed" on PATH for the check.
        let m = super::super::engine("duckstation").unwrap();
        let sys = systems::get("psx").unwrap();
        assert_eq!(
            bios::status(sys, &BiosIndex::default()).state,
            bios::SystemState::Missing
        );
        assert!(m.systems.contains(&"psx".to_string()));
    }
}
