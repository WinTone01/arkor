//! System libraries a downloaded emulator needs and this computer lacks.
//!
//! AppImages bring most of their libraries, not all: RetroArch's expects JACK's and ALSA's
//! from the system, which a desktop usually has but not always (Debian's, without JACK).
//! Before a game starts, the emulator's programs are asked (`ldd`, with the libraries the
//! AppImage brings on the path) what they cannot find; the libraries Arkor knows a package
//! for are installed with the distribution's package manager (see `crate::pkg`).

use std::path::{Path, PathBuf};

use super::discovery::{Installation, Origin};

/// Libraries by their file name → the Arch package that has them (see `crate::pkg` for the
/// other distributions' names).
const PACKAGES: &[(&str, &str)] = &[
    ("libjack.so.0", "pipewire-jack"),
    ("libasound.so.2", "alsa-lib"),
    ("libfuse.so.2", "fuse2"),
    ("libSDL2-2.0.so.0", "sdl2"),
    ("libopenal.so.1", "openal"),
];

/// The libraries `installation`'s programs cannot find. Only for downloads (a package or a
/// Flatpak brings what it needs). Blocking (runs `ldd`, a few tens of milliseconds).
pub fn missing(installation: &Installation) -> Vec<String> {
    if installation.origin != Origin::Managed {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for (program, paths) in programs(&installation.program) {
        for lib in ldd_missing(&program, &paths) {
            if !out.contains(&lib) {
                out.push(lib);
            }
        }
    }
    out
}

/// The Arch packages of `libs` Arkor knows, and the libraries it knows none for.
pub fn packages(libs: &[String]) -> (Vec<&'static str>, Vec<String>) {
    let mut known = Vec::new();
    let mut unknown = Vec::new();
    for lib in libs {
        match PACKAGES.iter().find(|(l, _)| l == lib) {
            Some((_, p)) if !known.contains(p) => known.push(*p),
            Some(_) => {}
            None => unknown.push(lib.clone()),
        }
    }
    (known, unknown)
}

fn is_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && head == *b"\x7fELF"
}

/// The programs to ask: the program itself when it is one, or the programs of the AppImage
/// Arkor unpacked for it, with the folders of the libraries it brings.
fn programs(program: &Path) -> Vec<(PathBuf, Vec<PathBuf>)> {
    if is_elf(program) {
        return vec![(program.to_path_buf(), Vec::new())];
    }
    let Some(name) = program.file_name() else {
        return Vec::new();
    };
    let unpacked = program.with_file_name(format!("{}.unpacked", name.to_string_lossy()));
    if !unpacked.is_dir() {
        return Vec::new();
    }
    let lib_dirs: Vec<PathBuf> = [
        "usr/lib",
        "usr/lib64",
        "usr/lib/x86_64-linux-gnu",
        "lib",
        "lib64",
        "lib/x86_64-linux-gnu",
        "shared/lib",
    ]
    .iter()
    .map(|d| unpacked.join(d))
    .filter(|d| d.is_dir())
    .collect();
    std::fs::read_dir(unpacked.join("usr/bin"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_elf(p))
        .map(|p| (p, lib_dirs.clone()))
        .collect()
}

fn ldd_missing(program: &Path, lib_dirs: &[PathBuf]) -> Vec<String> {
    let mut cmd = std::process::Command::new("ldd");
    cmd.arg(program)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if !lib_dirs.is_empty() {
        cmd.env(
            "LD_LIBRARY_PATH",
            std::env::join_paths(lib_dirs).unwrap_or_default(),
        );
    }
    let Ok(out) = cmd.output() else {
        return Vec::new();
    };
    parse_ldd(&String::from_utf8_lossy(&out.stdout))
}

/// The `=> not found` libraries of an `ldd` listing.
fn parse_ldd(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some((lib, rest)) = line.trim().split_once("=>")
            && rest.trim() == "not found"
        {
            let lib = lib.trim().to_string();
            if !out.contains(&lib) {
                out.push(lib);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ldd_and_names_packages() {
        let text = "\tlinux-vdso.so.1 (0x00007ffd)\n\tlibjack.so.0 => not found\n\tlibc.so.6 => /usr/lib/libc.so.6 (0x7f)\n\tlibjack.so.0 => not found\n\tlibweird.so.3 => not found\n";
        let libs = parse_ldd(text);
        assert_eq!(libs, ["libjack.so.0", "libweird.so.3"]);
        let (known, unknown) = packages(&libs);
        assert_eq!(known, ["pipewire-jack"]);
        assert_eq!(unknown, ["libweird.so.3"]);
    }

    #[test]
    fn a_program_of_this_computer_misses_nothing() {
        let me = std::env::current_exe().unwrap();
        let inst = Installation {
            engine: "x".into(),
            origin: Origin::Managed,
            version: None,
            program: me,
            flatpak: None,
            dir: None,
        };
        assert!(missing(&inst).is_empty());
    }
}
