//! Emulators: the games of consoles and older computers, from ROM files, disc images or
//! game folders the user has.
//!
//! The systems catalog, the emulator manifests (`assets/emu`), the BIOS index, the disc
//! readers, ROM names and ROM folder scanning come from Irori
//! (<https://github.com/irori-app/irori>, GPL-3.0-or-later), trimmed to Linux.
//!
//! This works the same on every distribution: an emulator Arkor downloads goes into a folder
//! of its own (AppImages are unpacked there, so no FUSE is needed); one Arkor has no build of
//! is installed from Flathub for this user; one already installed (a distribution package, a
//! Flatpak) is used as it is.
//!
//! - [`systems`]: what each system is, its files and BIOS;
//! - [`manifest`]: what each emulator is, where it comes from, how it starts a game;
//! - [`discovery`]: the emulators on this computer;
//! - [`install`]: downloading and removing them; [`retroarch`]: RetroArch's cores;
//! - [`bios`]: the user's BIOS files; [`scan`], [`rom_sources`]: finding games;
//! - [`launch`]: the command that plays a game.

pub mod bios;
pub mod discovery;
pub mod games;
pub mod host;
pub mod install;
pub mod iso9660;
pub mod launch;
pub mod libs;
pub mod manifest;
pub mod names;
pub mod open;
pub mod retroarch;
pub mod rom_sources;
pub mod scan;
pub mod sfo;
pub mod systems;
pub mod zip;

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

pub use discovery::{Installation, Origin};
pub use manifest::{EngineKind, EngineManifest};
pub use systems::System;

/// The emulators Arkor knows, by id.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("retroarch", include_str!("../../assets/emu/retroarch.json")),
    (
        "duckstation",
        include_str!("../../assets/emu/duckstation.json"),
    ),
    ("ppsspp", include_str!("../../assets/emu/ppsspp.json")),
    ("dolphin", include_str!("../../assets/emu/dolphin.json")),
    ("scummvm", include_str!("../../assets/emu/scummvm.json")),
    (
        "dosbox-staging",
        include_str!("../../assets/emu/dosbox-staging.json"),
    ),
    ("uzdoom", include_str!("../../assets/emu/uzdoom.json")),
    ("gzdoom", include_str!("../../assets/emu/gzdoom.json")),
    ("pcsx2", include_str!("../../assets/emu/pcsx2.json")),
    ("melonds", include_str!("../../assets/emu/melonds.json")),
    ("mgba", include_str!("../../assets/emu/mgba.json")),
    ("azahar", include_str!("../../assets/emu/azahar.json")),
    ("mame", include_str!("../../assets/emu/mame.json")),
    ("gopher64", include_str!("../../assets/emu/gopher64.json")),
    (
        "mupen64plus",
        include_str!("../../assets/emu/mupen64plus.json"),
    ),
    ("m64py", include_str!("../../assets/emu/m64py.json")),
    ("desmume", include_str!("../../assets/emu/desmume.json")),
    ("epsxe", include_str!("../../assets/emu/epsxe.json")),
    ("cemu", include_str!("../../assets/emu/cemu.json")),
    ("rpcs3", include_str!("../../assets/emu/rpcs3.json")),
    ("xemu", include_str!("../../assets/emu/xemu.json")),
    ("shadps4", include_str!("../../assets/emu/shadps4.json")),
    ("flycast", include_str!("../../assets/emu/flycast.json")),
    ("vita3k", include_str!("../../assets/emu/vita3k.json")),
    ("ryujinx", include_str!("../../assets/emu/ryujinx.json")),
];

/// An emulator choice as stored in settings: `duckstation`, or `retroarch:<core>` for a
/// libretro core.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EngineRef {
    pub engine: String,
    pub core: Option<String>,
}

impl EngineRef {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        Some(match s.split_once(':') {
            Some((engine, core)) if !core.is_empty() => Self {
                engine: engine.to_string(),
                core: Some(core.to_string()),
            },
            _ => Self {
                engine: s.trim_end_matches(':').to_string(),
                core: None,
            },
        })
    }

    /// "RetroArch (Snes9x)", "DuckStation".
    pub fn label(&self) -> String {
        let Some(m) = engine(&self.engine) else {
            return self.to_string();
        };
        match self.core.as_deref().and_then(|c| m.core(c)) {
            Some(core) => format!("{} ({})", m.name, core.name),
            None => m.name.clone(),
        }
    }
}

impl std::fmt::Display for EngineRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.core {
            Some(core) => write!(f, "{}:{core}", self.engine),
            None => f.write_str(&self.engine),
        }
    }
}

/// Where Arkor keeps emulators and what they need.
pub mod dirs {
    use std::path::PathBuf;

    /// `~/.local/share/arkor/emulators`.
    pub fn root() -> PathBuf {
        #[cfg(test)]
        {
            super::test_base().join("data")
        }
        #[cfg(not(test))]
        {
            ::dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("arkor/emulators")
        }
    }

    /// Downloaded versions: `<root>/<id>/<version>/`.
    pub fn engine(id: &str) -> PathBuf {
        root().join(id)
    }

    /// The folder Arkor gives an emulator for its settings and saves (RetroArch's).
    pub fn home(id: &str) -> PathBuf {
        root().join("home").join(id)
    }

    /// BIOS files linked for RetroArch (its `system_directory`).
    pub fn system() -> PathBuf {
        root().join("system")
    }

    /// Arkor's BIOS folder, where the user puts their files (unless they chose another).
    pub fn bios() -> PathBuf {
        root().join("bios")
    }

    /// Playlists of games on several discs.
    pub fn playlists() -> PathBuf {
        root().join("playlists")
    }

    /// Release lists, partial downloads and the BIOS index.
    pub fn cache() -> PathBuf {
        #[cfg(test)]
        {
            super::test_base().join("cache")
        }
        #[cfg(not(test))]
        {
            ::dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("arkor/emulators")
        }
    }
}

static REGISTRY: LazyLock<Arc<Vec<Arc<EngineManifest>>>> = LazyLock::new(|| {
    Arc::new(
        BUILT_IN
            .iter()
            .filter_map(|(id, json)| match EngineManifest::parse(json) {
                Ok(m) => Some(Arc::new(m)),
                Err(e) => {
                    eprintln!("arkor: emulator {id}: {e:#}");
                    None
                }
            })
            .collect(),
    )
});

/// Every emulator Arkor knows.
pub fn engines() -> Arc<Vec<Arc<EngineManifest>>> {
    REGISTRY.clone()
}

pub fn engine(id: &str) -> Option<Arc<EngineManifest>> {
    REGISTRY.iter().find(|m| m.id == id).cloned()
}

/// Emulators that play `system`, as choices: RetroArch counts once per core. The system's
/// own recommendation order comes first.
pub fn choices_for(system: &str) -> Vec<EngineRef> {
    let mut out: Vec<EngineRef> = systems::get(system)
        .map(|s| {
            s.engines
                .iter()
                .filter_map(|e| EngineRef::parse(e))
                .collect()
        })
        .unwrap_or_default();
    for m in engines().iter() {
        if !m.systems.iter().any(|s| s == system) {
            continue;
        }
        let refs: Vec<EngineRef> = if m.kind == EngineKind::Retroarch {
            m.cores_for(system)
                .into_iter()
                .map(|c| EngineRef {
                    engine: m.id.clone(),
                    core: Some(c.id.clone()),
                })
                .collect()
        } else {
            vec![EngineRef {
                engine: m.id.clone(),
                core: None,
            }]
        };
        for r in refs {
            if !out.contains(&r) {
                out.push(r);
            }
        }
    }
    out.retain(|r| engine(&r.engine).is_some());
    out
}

/// The emulator a game of `system` plays with when it names none: `chosen` (the user's
/// default for the system) while Arkor knows it, else the first recommended choice that
/// `plays` accepts, else the first recommended one Arkor can get here; experimental
/// emulators only when nothing else is offered.
pub fn default_choice(
    system: &str,
    chosen: Option<&str>,
    plays: impl Fn(&EngineRef) -> bool,
) -> Option<EngineRef> {
    if let Some(chosen) = chosen
        .and_then(EngineRef::parse)
        .filter(|c| engine(&c.engine).is_some())
    {
        return Some(chosen);
    }
    let choices = choices_for(system);
    let experimental = |c: &EngineRef| engine(&c.engine).is_some_and(|m| m.experimental);
    let gettable = |c: &EngineRef| engine(&c.engine).is_some_and(|m| install::can_get(&m));
    choices
        .iter()
        .find(|c| plays(c) && !experimental(c))
        .or_else(|| choices.iter().find(|c| !experimental(c) && gettable(c)))
        .or_else(|| choices.iter().find(|c| !experimental(c)))
        .or_else(|| choices.iter().find(|c| plays(c)))
        .or_else(|| choices.first())
        .cloned()
}

/// Whether `choice` can play right now: the emulator is installed and, for RetroArch, the
/// core too. Blocking (reads the disk).
pub fn plays_now(choice: &EngineRef) -> bool {
    let Some(m) = engine(&choice.engine) else {
        return false;
    };
    !discovery::find(&m).is_empty()
        && choice
            .core
            .as_deref()
            .is_none_or(|c| retroarch::installed_core(c).is_some())
}

/// Whether Arkor has an emulator for `system` (not PlayStation 5: only a converter plays
/// those).
pub fn playable(system: &str) -> bool {
    !choices_for(system).is_empty()
}

/// The emulator a game plays with: its own choice, else the system's default.
pub fn choice_for_game(
    system: &str,
    own: Option<&str>,
    settings: &arkor_core::profile::Emulators,
) -> Option<EngineRef> {
    own.and_then(EngineRef::parse)
        .filter(|c| engine(&c.engine).is_some())
        .or_else(|| {
            default_choice(
                system,
                settings.defaults.get(system).map(String::as_str),
                plays_now,
            )
        })
}

/// The download keys (see `crate::downloads`) of what `choice` needs and lacks: the
/// emulator (`emu:<id>`) and its core (`core:<id>`). Blocking.
pub fn missing_keys(choice: &EngineRef) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(m) = engine(&choice.engine)
        && discovery::find(&m).is_empty()
    {
        out.push(format!("emu:{}", m.id));
    }
    if let Some(core) = &choice.core
        && retroarch::installed_core(core).is_none()
    {
        out.push(format!("core:{core}"));
    }
    out
}

/// The BIOS folder in use.
pub fn bios_dir(settings: &arkor_core::profile::Emulators) -> PathBuf {
    settings.bios_dir.clone().unwrap_or_else(dirs::bios)
}

pub(crate) fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Where this crate's tests keep emulator data: one temporary folder for the run.
#[cfg(test)]
fn test_base() -> &'static Path {
    static BASE: LazyLock<tempfile::TempDir> =
        LazyLock::new(|| tempfile::tempdir().expect("temp dir"));
    BASE.path()
}

/// A folder of its own for one test.
#[cfg(test)]
pub(crate) fn test_root(name: &str) -> PathBuf {
    let dir = test_base().join("tests").join(name);
    std::fs::create_dir_all(&dir).expect("test dir");
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_refs_parse_and_print() {
        let r = EngineRef::parse("retroarch:snes9x").unwrap();
        assert_eq!(r.engine, "retroarch");
        assert_eq!(r.core.as_deref(), Some("snes9x"));
        assert_eq!(r.to_string(), "retroarch:snes9x");
        assert_eq!(r.label(), "RetroArch (Snes9x)");
        assert_eq!(EngineRef::parse("duckstation").unwrap().core, None);
        assert!(EngineRef::parse(" ").is_none());
    }

    #[test]
    fn every_system_engine_exists() {
        for system in systems::all() {
            // PlayStation 5 games need AnyPS5's converter, which Arkor does not offer.
            if system.id == "ps5" {
                assert!(choices_for("ps5").is_empty());
                continue;
            }
            for e in &system.engines {
                let r = EngineRef::parse(e).unwrap();
                let m = engine(&r.engine).unwrap_or_else(|| panic!("{}: no {e}", system.id));
                assert!(
                    m.systems.contains(&system.id),
                    "{e} does not list {}",
                    system.id
                );
                if let Some(core) = &r.core {
                    let c = m.core(core).unwrap_or_else(|| panic!("no core {core}"));
                    assert!(
                        c.systems.contains(&system.id),
                        "{core} does not play {}",
                        system.id
                    );
                }
            }
        }
    }

    #[test]
    fn choices_put_the_recommended_engine_first() {
        let psx = choices_for("psx");
        assert_eq!(psx[0].to_string(), "duckstation");
        assert!(psx.iter().any(|r| r.to_string() == "retroarch:swanstation"));
        let gb = choices_for("gb");
        assert_eq!(gb[0].to_string(), "retroarch:gambatte");
        assert_eq!(gb[2].to_string(), "mgba");
    }

    #[test]
    fn a_default_that_is_gone_falls_back_to_the_recommendation() {
        let pick = |chosen| default_choice("psx", chosen, |_| true).map(|c| c.to_string());
        assert_eq!(pick(Some("epsxe")).as_deref(), Some("epsxe"));
        assert_eq!(pick(Some("custom-removed")).as_deref(), Some("duckstation"));
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex_encode(&[0, 0xab, 0x10]), "00ab10");
        assert_eq!(hex_decode("00ab10"), Some(vec![0, 0xab, 0x10]));
        assert_eq!(hex_decode("0g"), None);
    }
}
