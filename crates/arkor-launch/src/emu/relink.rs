//! Emulators that convert a game into a program of this computer before it plays
//! (`EngineKind::Relinker`: AnyPS5, experimental), ported from Irori's `relink`.
//!
//! The converted program lives in Arkor's emulators folder with links to the game's own
//! files beside it (`app0/`), so the user's game folder is never written. A game is
//! converted again only when its program, its modules or the converter change.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use super::discovery::Installation;
use super::manifest::EngineManifest;

/// Folders beside a game's program that hold its own modules.
const MODULE_DIRS: [&str; 3] = ["sce_module", "sce_modules", "prx"];
/// The converter's system libraries, beside its program.
const LIBS: &str = "libs";
const STAMP: &str = "arkor-relink.json";

/// Where a game's converted program lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub dir: PathBuf,
    pub program: PathBuf,
}

fn root() -> PathBuf {
    super::dirs::root().join("relinked")
}

/// The game's folder: its program's (`eboot.bin`).
fn game_folder(game: &Path) -> PathBuf {
    if game.is_dir() {
        game.to_path_buf()
    } else {
        game.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

/// The converted program of `game` for `engine`.
pub fn output(engine: &str, game: &Path) -> Output {
    let folder = game_folder(game);
    let key = format!(
        "{:08x}",
        crc32fast::hash(folder.to_string_lossy().as_bytes())
    );
    let dir = root().join(engine).join(key);
    let program = dir.join("app.elf");
    Output { dir, program }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    path: PathBuf,
    size: u64,
    mtime: u64,
}

impl FileStamp {
    fn of(path: &Path) -> anyhow::Result<Self> {
        let meta =
            std::fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        Ok(Self {
            path: path.to_path_buf(),
            size: meta.len(),
            mtime,
        })
    }
}

/// What a conversion was made from; a change means converting again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    engine: FileStamp,
    program: FileStamp,
    modules: Vec<(String, FileStamp)>,
}

fn stamp_of(installation: &Installation, game: &Path) -> anyhow::Result<Stamp> {
    let folder = game_folder(game);
    let mut modules = Vec::new();
    for dir in MODULE_DIRS.iter().map(|d| folder.join(d)) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                let name = entry
                    .path()
                    .strip_prefix(&folder)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                modules.push((name, FileStamp::of(&entry.path())?));
            }
        }
    }
    modules.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Stamp {
        engine: FileStamp::of(&installation.program)?,
        program: FileStamp::of(game)?,
        modules,
    })
}

/// Whether `game` was converted with `installation` and has not changed since.
pub fn is_current(installation: &Installation, game: &Path, out: &Output) -> bool {
    let saved: Option<Stamp> = std::fs::read_to_string(out.dir.join(STAMP))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    out.program.is_file()
        && saved.is_some_and(|s| stamp_of(installation, game).is_ok_and(|n| n == s))
}

/// The processor features of `wanted` this computer lacks (all of them off x86-64).
pub fn missing_cpu_features(wanted: &[String]) -> Vec<String> {
    wanted
        .iter()
        .filter(|f| !has_cpu_feature(f))
        .cloned()
        .collect()
}

#[cfg(target_arch = "x86_64")]
fn has_cpu_feature(name: &str) -> bool {
    match name {
        "avx" => std::arch::is_x86_feature_detected!("avx"),
        "avx2" => std::arch::is_x86_feature_detected!("avx2"),
        "fma" => std::arch::is_x86_feature_detected!("fma"),
        "f16c" => std::arch::is_x86_feature_detected!("f16c"),
        "bmi1" => std::arch::is_x86_feature_detected!("bmi1"),
        "bmi2" => std::arch::is_x86_feature_detected!("bmi2"),
        "lzcnt" => std::arch::is_x86_feature_detected!("lzcnt"),
        "movbe" => std::arch::is_x86_feature_detected!("movbe"),
        "popcnt" => std::arch::is_x86_feature_detected!("popcnt"),
        "sse4.2" => std::arch::is_x86_feature_detected!("sse4.2"),
        _ => true,
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn has_cpu_feature(_name: &str) -> bool {
    false
}

/// Refuses a game this computer cannot run before converting it: a program still
/// encrypted, or a processor without what the converter's games use.
pub fn check(manifest: &EngineManifest, game: &Path) -> anyhow::Result<()> {
    let missing = missing_cpu_features(&manifest.cpu_features);
    if !missing.is_empty() {
        bail!(
            "{} games need a processor with {}, which this one lacks",
            manifest.name,
            missing.join(", ")
        );
    }
    let mut magic = [0u8; 4];
    std::fs::File::open(game)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut magic))
        .with_context(|| format!("Could not read {}", game.display()))?;
    if &magic != b"\x7fELF" {
        bail!(
            "{} is not a decrypted program, so {} cannot convert it. Use a dump whose eboot.bin is a plain ELF file.",
            game.display(),
            manifest.name
        );
    }
    Ok(())
}

/// Converts `game` with `installation` unless the last conversion is still current; the
/// converted program. Blocking (a conversion takes a while).
pub fn prepare(
    manifest: &EngineManifest,
    installation: &Installation,
    game: &Path,
) -> anyhow::Result<Output> {
    check(manifest, game)?;
    let out = output(&manifest.id, game);
    if is_current(installation, game, &out) {
        return Ok(out);
    }
    let stamp = stamp_of(installation, game)?;
    if out.dir.exists() {
        arkor_core::fs_guard::remove_dir_in(&out.dir, &root())?;
    }
    std::fs::create_dir_all(&out.dir)?;
    let engine_dir = installation
        .dir
        .clone()
        .or_else(|| installation.program.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let libs = super::discovery::locate_dir(&engine_dir, LIBS).unwrap_or(engine_dir.join(LIBS));
    if !libs.is_dir() {
        bail!(
            "{}'s system libraries are missing from {}; install it again",
            manifest.name,
            engine_dir.display()
        );
    }
    let mut cmd = std::process::Command::new(&installation.program);
    cmd.arg("--rpath").arg(&libs);
    // PlayStation 5 games use AMD-only instructions; an Intel processor needs them converted.
    if intel() {
        cmd.arg("--to-intel");
    }
    let folder = game_folder(game);
    if !MODULE_DIRS.iter().any(|d| folder.join(d).is_dir()) {
        // The converter needs a module folder; a game without modules gets an empty one.
        let parent = out.dir.join("no-modules");
        std::fs::create_dir_all(parent.join(MODULE_DIRS[0]))?;
        cmd.arg("--sce-module-path").arg(parent);
    }
    eprintln!("arkor: {} converts the game (once)", manifest.name);
    let result = cmd
        .arg(game)
        .arg(&out.program)
        .current_dir(&out.dir)
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("Could not start {}", manifest.name))?;
    if !result.status.success() {
        let detail = String::from_utf8_lossy(&result.stderr)
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no details")
            .trim()
            .to_string();
        bail!("{} could not convert this game: {detail}", manifest.name);
    }
    link_app0(&folder, &out.dir.join("app0"))?;
    super::write_json(&out.dir.join(STAMP), &stamp)?;
    Ok(out)
}

/// Whether this computer's processor is Intel's.
fn intel() -> bool {
    std::fs::read_to_string("/proc/cpuinfo").is_ok_and(|t| {
        t.lines()
            .find(|l| l.starts_with("vendor_id"))
            .is_some_and(|l| l.contains("GenuineIntel"))
    })
}

/// Links every entry of the game folder into `app0`, except what the converter wrote.
fn link_app0(game: &Path, app0: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(app0)?;
    for entry in std::fs::read_dir(game).with_context(|| format!("reading {}", game.display()))? {
        let entry = entry?;
        let to = app0.join(entry.file_name());
        if std::fs::symlink_metadata(&to).is_ok() {
            continue;
        }
        std::os::unix::fs::symlink(entry.path(), &to)
            .with_context(|| format!("Could not link {}", to.display()))?;
    }
    Ok(())
}

/// Whether Linux keeps `/dev/udmabuf` from this user: AnyPS5 shares game memory with the
/// graphics card through it (drivers other than NVIDIA's), so games run badly without it.
pub fn udmabuf_denied() -> bool {
    let path = Path::new("/dev/udmabuf");
    path.exists()
        && std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn converts_once_links_the_game_and_again_after_changes() {
        let root = super::super::test_root("relink");
        let engine = root.join("engine");
        std::fs::create_dir_all(engine.join(LIBS)).unwrap();
        let calls = root.join("calls.txt");
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nfor last; do :; done\nprintf x > \"$last\"\n",
            calls.display()
        );
        let program = engine.join("relinker-v0.1");
        std::fs::write(&program, script).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let installation = Installation {
            engine: "anyps5".into(),
            origin: super::super::Origin::Managed,
            version: Some("0.1".into()),
            program,
            flatpak: None,
            dir: Some(engine),
        };
        let m = super::super::engine("anyps5").unwrap();
        let folder = root.join("PPSA00000");
        std::fs::create_dir_all(folder.join("sce_sys")).unwrap();
        let eboot = folder.join("eboot.bin");
        std::fs::write(&eboot, b"SCE\0encrypted").unwrap();
        assert!(
            format!("{:#}", prepare(&m, &installation, &eboot).unwrap_err())
                .contains("not a decrypted program")
                || !missing_cpu_features(&m.cpu_features).is_empty()
        );
        if !missing_cpu_features(&m.cpu_features).is_empty() {
            return;
        }
        std::fs::write(&eboot, b"\x7fELF....").unwrap();
        let out = prepare(&m, &installation, &eboot).unwrap();
        assert!(out.program.is_file());
        assert!(
            out.dir.join("app0/sce_sys").exists(),
            "the game is linked in"
        );
        prepare(&m, &installation, &eboot).unwrap();
        let runs = std::fs::read_to_string(&calls).unwrap().lines().count();
        assert_eq!(runs, 1, "converted once");
        std::fs::write(&eboot, b"\x7fELF.....changed").unwrap();
        prepare(&m, &installation, &eboot).unwrap();
        assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 2);
    }
}
