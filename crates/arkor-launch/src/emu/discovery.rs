//! The emulators on this computer: versions Arkor downloaded, Flatpaks, and programs on
//! `PATH` (a distribution package or a manual install). Only the filesystem is read (no
//! `flatpak` calls), so it is quick; still blocking.
//!
//! What Arkor downloaded is recorded in `<emulators>/state.json` (see [`State`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::manifest::{EngineManifest, Source};

/// Where an installation came from. Arkor updates and removes only [`Origin::Managed`]
/// ones (and Flatpaks it installed itself); the others are used as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Managed,
    Flatpak,
    /// On `PATH` (a distribution package or a manual install).
    System,
}

/// One usable install of an emulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    pub engine: String,
    pub origin: Origin,
    /// Known for downloaded versions.
    pub version: Option<String>,
    /// The program to run.
    pub program: PathBuf,
    /// The Flatpak's application id, for [`Origin::Flatpak`].
    pub flatpak: Option<String>,
    /// The downloaded version's folder.
    pub dir: Option<PathBuf>,
}

/// A version Arkor downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Managed {
    pub version: String,
    pub dir: PathBuf,
    pub program: PathBuf,
    #[serde(default)]
    pub size: u64,
}

/// A libretro core Arkor downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreInstall {
    pub path: PathBuf,
    /// The buildbot's build date, `YYYY-MM-DD`.
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub size: u64,
}

/// What Arkor downloaded and installed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Downloaded versions, by emulator id.
    pub managed: BTreeMap<String, Vec<Managed>>,
    pub cores: BTreeMap<String, CoreInstall>,
    /// Flatpaks Arkor installed (so it may remove them again), by emulator id.
    pub flatpaks: BTreeMap<String, String>,
}

fn state_file() -> PathBuf {
    super::dirs::root().join("state.json")
}

static STATE_LOCK: Mutex<()> = Mutex::new(());

pub fn state() -> State {
    super::read_json(&state_file())
}

/// Changes the state on disk.
pub fn update_state<R>(change: impl FnOnce(&mut State) -> R) -> anyhow::Result<R> {
    let _held = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = state();
    let out = change(&mut s);
    super::write_json(&state_file(), &s)?;
    Ok(out)
}

/// Every installation of `manifest`, best first: downloaded versions newest first, then the
/// other sources in manifest order.
pub fn find(manifest: &EngineManifest) -> Vec<Installation> {
    find_with(manifest, &state(), &Probe::system())
}

/// The places discovery looks, so tests can point it elsewhere.
pub(crate) struct Probe {
    pub flatpak_roots: Vec<PathBuf>,
    pub path: Option<std::ffi::OsString>,
}

impl Probe {
    fn system() -> Self {
        let home = ::dirs::home_dir().unwrap_or_default();
        Self {
            flatpak_roots: vec![
                home.join(".local/share/flatpak/app"),
                PathBuf::from("/var/lib/flatpak/app"),
            ],
            path: std::env::var_os("PATH"),
        }
    }
}

pub(crate) fn find_with(
    manifest: &EngineManifest,
    state: &State,
    probe: &Probe,
) -> Vec<Installation> {
    let base = |origin, program: PathBuf| Installation {
        engine: manifest.id.clone(),
        origin,
        version: None,
        program,
        flatpak: None,
        dir: None,
    };
    let mut out = Vec::new();
    let mut managed: Vec<&Managed> = state
        .managed
        .get(&manifest.id)
        .map(|v| v.iter().filter(|m| m.program.exists()).collect())
        .unwrap_or_default();
    managed.sort_by(|a, b| compare_versions(&b.version, &a.version));
    for m in managed {
        out.push(Installation {
            version: Some(m.version.clone()),
            dir: Some(m.dir.clone()),
            ..base(Origin::Managed, m.program.clone())
        });
    }
    for source in &manifest.sources {
        match source {
            Source::Flatpak { id }
                if probe
                    .flatpak_roots
                    .iter()
                    .any(|root| root.join(id).join("current").exists()) =>
            {
                out.push(Installation {
                    flatpak: Some(id.clone()),
                    ..base(Origin::Flatpak, PathBuf::from("flatpak"))
                });
            }
            Source::Path { names } => {
                if let Some(program) = names
                    .iter()
                    .find_map(|n| in_path(n, probe.path.as_deref()))
                    .filter(|p| !is_flatpak_export(p))
                {
                    out.push(base(Origin::System, program));
                }
            }
            _ => {}
        }
    }
    out
}

fn in_path(name: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path?)
        .map(|d| d.join(name))
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Flatpak's exported launchers (`…/flatpak/exports/bin/<id>`) are found as Flatpaks.
fn is_flatpak_export(path: &Path) -> bool {
    path.to_string_lossy().contains("/flatpak/exports/bin/")
}

/// The program in a downloaded folder: the first of `names` found (a name may hold one
/// `*`), in the folder or up to two levels down.
pub fn locate_program(dir: &Path, names: &[String]) -> Option<PathBuf> {
    fn matches(pattern: &str, name: &str) -> bool {
        match pattern.split_once('*') {
            Some((head, tail)) => {
                name.len() >= head.len() + tail.len()
                    && name.starts_with(head)
                    && name.ends_with(tail)
            }
            None => name == pattern,
        }
    }
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..3 {
        let mut next = Vec::new();
        for d in &level {
            let Ok(entries) = std::fs::read_dir(d) else {
                continue;
            };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for pattern in names {
                if let Some(e) = entries
                    .iter()
                    .find(|e| matches(pattern, &e.file_name().to_string_lossy()))
                    .filter(|e| e.path().is_file())
                {
                    return Some(e.path());
                }
            }
            next.extend(
                entries
                    .iter()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    // An unpacked AppImage is not searched for programs of its own.
                    .filter(|e| !e.file_name().to_string_lossy().ends_with(".unpacked"))
                    .map(|e| e.path()),
            );
        }
        level = next;
    }
    None
}

/// Compares versions by their numeric parts (`1.21.0` < `1.22`, `2025-01-02` <
/// `2025-10-01`), then as text.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(s: &str) -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse().unwrap_or(u64::MAX))
            .collect()
    }
    parts(a).cmp(&parts(b)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_downloaded_flatpak_and_system_installs_in_order() {
        let root = super::super::test_root("discovery");
        let m = super::super::engine("duckstation").unwrap();
        let flatpaks = root.join("flatpak");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let program = bin.join("duckstation-qt");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let v1 = root.join("v1/DuckStation-x64.AppImage");
        let v2 = root.join("v2/DuckStation-x64.AppImage");
        for p in [&v1, &v2] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "").unwrap();
        }
        let mut state = State::default();
        state.managed.insert(
            "duckstation".into(),
            vec![
                Managed {
                    version: "0.1-9000".into(),
                    dir: v1.parent().unwrap().into(),
                    program: v1.clone(),
                    size: 0,
                },
                Managed {
                    version: "0.1-10000".into(),
                    dir: v2.parent().unwrap().into(),
                    program: v2.clone(),
                    size: 0,
                },
            ],
        );
        let probe = Probe {
            flatpak_roots: vec![flatpaks.clone()],
            path: Some(bin.clone().into_os_string()),
        };
        let found = find_with(&m, &state, &probe);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].program, v2, "newest download first");
        assert_eq!(found[2].origin, Origin::System);

        // No Flatpak source for DuckStation; PPSSPP has one.
        let ppsspp = super::super::engine("ppsspp").unwrap();
        std::fs::create_dir_all(flatpaks.join("org.ppsspp.PPSSPP/current")).unwrap();
        let found = find_with(&ppsspp, &State::default(), &probe);
        assert_eq!(found[0].origin, Origin::Flatpak);
        assert_eq!(found[0].flatpak.as_deref(), Some("org.ppsspp.PPSSPP"));
    }

    #[test]
    fn locates_programs_by_pattern() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("RetroArch-Linux-x86_64")).unwrap();
        std::fs::write(
            dir.path()
                .join("RetroArch-Linux-x86_64/RetroArch-Linux-x86_64.AppImage"),
            "",
        )
        .unwrap();
        std::fs::write(dir.path().join("PPSSPP-v1.20-anylinux-x86_64.AppImage"), "").unwrap();
        assert!(locate_program(dir.path(), &["PPSSPP-*.AppImage".into()]).is_some());
        assert!(locate_program(dir.path(), &["RetroArch-Linux-x86_64.AppImage".into()]).is_some());
        assert!(locate_program(dir.path(), &["nothing".into()]).is_none());
        assert!(compare_versions("1.21.0", "1.22").is_lt());
        assert!(compare_versions("2025-10-01", "2025-01-02").is_gt());
    }
}
