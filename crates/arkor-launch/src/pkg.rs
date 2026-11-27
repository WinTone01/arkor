//! The distribution's package manager: pacman (Arch and its family), apt (Debian, Ubuntu,
//! Mint, Pop!_OS…), dnf (Fedora, Nobara…) or zypper (openSUSE). Arkor names packages as Arch
//! does; [`Manager::name`] gives this distribution's name for one, or none when it does not
//! package it (those come from elsewhere: Arkor downloads them, or the user does).

use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Pacman,
    Apt,
    Dnf,
    Zypper,
}

/// This computer's package manager, if it is one Arkor knows.
pub fn manager() -> Option<Manager> {
    static FOUND: OnceLock<Option<Manager>> = OnceLock::new();
    *FOUND.get_or_init(|| {
        let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        let has = |p: &str| crate::start::find_in_path(p).is_some();
        from_release(&release)
            .filter(|m| has(m.program()))
            .or_else(|| {
                [Manager::Pacman, Manager::Apt, Manager::Dnf, Manager::Zypper]
                    .into_iter()
                    .find(|m| has(m.program()))
            })
    })
}

/// The family `/etc/os-release` names (its `ID`, then `ID_LIKE`).
fn from_release(text: &str) -> Option<Manager> {
    let value = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').to_lowercase())
            .unwrap_or_default()
    };
    let ids = format!("{} {}", value("ID"), value("ID_LIKE"));
    ids.split_whitespace().find_map(|id| match id {
        "arch" | "archlinux" | "cachyos" | "manjaro" | "endeavouros" | "garuda" | "steamos" => {
            Some(Manager::Pacman)
        }
        "debian" | "ubuntu" => Some(Manager::Apt),
        "fedora" | "rhel" | "centos" => Some(Manager::Dnf),
        "opensuse" | "suse" | "opensuse-tumbleweed" | "opensuse-leap" => Some(Manager::Zypper),
        _ => None,
    })
}

/// Arch's name → (Debian/Ubuntu, Fedora, openSUSE); `None`: not packaged there.
/// Names not listed are the same everywhere.
const NAMES: &[(&str, [Option<&str>; 3])] = &[
    ("umu-launcher", [None, Some("umu-launcher"), None]),
    (
        "vkbasalt",
        [Some("vkbasalt"), Some("vkBasalt"), Some("vkbasalt")],
    ),
    ("obs-vkcapture", [None, Some("obs-vkcapture"), None]),
    ("lsfg-vk", [None, None, None]),
    ("cachyos-settings", [None, None, None]),
    ("gpu-screen-recorder", [None, None, None]),
    (
        "7zip",
        [Some("p7zip-full"), Some("p7zip-plugins"), Some("7zip")],
    ),
    (
        "libarchive",
        [Some("libarchive-tools"), Some("bsdtar"), Some("bsdtar")],
    ),
    (
        "webkit2gtk-4.1",
        [
            Some("libwebkit2gtk-4.1-0"),
            Some("webkit2gtk4.1"),
            Some("libwebkit2gtk-4_1-0"),
        ],
    ),
    ("lib32-vulkan-radeon", [None, None, None]),
    ("lib32-vulkan-intel", [None, None, None]),
    ("lib32-nvidia-utils", [None, None, None]),
    (
        "vulkan-radeon",
        [
            Some("mesa-vulkan-drivers"),
            Some("mesa-vulkan-drivers"),
            Some("libvulkan_radeon"),
        ],
    ),
    (
        "vulkan-intel",
        [
            Some("mesa-vulkan-drivers"),
            Some("mesa-vulkan-drivers"),
            Some("libvulkan_intel"),
        ],
    ),
    ("nvidia-utils", [None, None, None]),
    // Libraries emulators' downloads expect from the system (see `emu::libs`).
    (
        "pipewire-jack",
        [
            Some("libjack-jackd2-0"),
            Some("pipewire-jack-audio-connection-kit-libs"),
            Some("libjack0"),
        ],
    ),
    (
        "alsa-lib",
        [Some("libasound2"), Some("alsa-lib"), Some("libasound2")],
    ),
    (
        "fuse2",
        [Some("libfuse2"), Some("fuse-libs"), Some("libfuse2")],
    ),
    (
        "sdl2",
        [Some("libsdl2-2.0-0"), Some("SDL2"), Some("libSDL2-2_0-0")],
    ),
    (
        "openal",
        [Some("libopenal1"), Some("openal-soft"), Some("libopenal1")],
    ),
];

impl Manager {
    pub fn program(self) -> &'static str {
        match self {
            Manager::Pacman => "pacman",
            Manager::Apt => "apt-get",
            Manager::Dnf => "dnf",
            Manager::Zypper => "zypper",
        }
    }

    /// This distribution's name for an Arch package, `None` when it has none.
    pub fn name(self, arch: &str) -> Option<String> {
        let column = match self {
            Manager::Pacman => return Some(arch.to_string()),
            Manager::Apt => 0,
            Manager::Dnf => 1,
            Manager::Zypper => 2,
        };
        match NAMES.iter().find(|(a, _)| *a == arch) {
            Some((_, names)) => names[column].map(str::to_string),
            None => Some(arch.to_string()),
        }
    }

    /// The command that installs `packages` (already this distribution's names), without
    /// asking; run it as root.
    pub fn install_command(self, packages: &[String]) -> Vec<String> {
        let head: &[&str] = match self {
            Manager::Pacman => &["pacman", "-S", "--needed", "--noconfirm", "--"],
            Manager::Apt => &["apt-get", "install", "-y"],
            Manager::Dnf => &["dnf", "install", "-y"],
            Manager::Zypper => &["zypper", "--non-interactive", "install"],
        };
        head.iter()
            .map(|s| s.to_string())
            .chain(packages.iter().cloned())
            .collect()
    }

    /// The installed version of a package (this distribution's name).
    pub fn version(self, package: &str) -> Option<String> {
        let out = match self {
            Manager::Pacman => run(&["pacman", "-Q", "--", package])?,
            Manager::Apt => run(&[
                "dpkg-query",
                "-W",
                "-f=${db:Status-Status} ${Version}",
                "--",
                package,
            ])?,
            Manager::Dnf | Manager::Zypper => {
                run(&["rpm", "-q", "--qf", "%{VERSION}", "--", package])?
            }
        };
        let version = match self {
            Manager::Pacman => out.split_whitespace().nth(1)?.to_string(),
            Manager::Apt => out.strip_prefix("installed ")?.trim().to_string(),
            _ => out.trim().to_string(),
        };
        (!version.is_empty()).then_some(version)
    }

    /// Whether the distribution's repositories have a package (this distribution's name).
    pub fn in_repos(self, package: &str) -> bool {
        match self {
            Manager::Pacman => run(&["pacman", "-Si", "--", package]).is_some(),
            Manager::Apt => run(&["apt-cache", "policy", "--", package])
                .is_some_and(|o| o.contains("Candidate:") && !o.contains("Candidate: (none)")),
            Manager::Dnf => {
                // dnf5 takes no `--`; the names come from Arkor's own list.
                run(&["dnf", "-q", "info", package]).is_some()
            }
            Manager::Zypper => run(&[
                "zypper",
                "--non-interactive",
                "-q",
                "search",
                "-x",
                "--",
                package,
            ])
            .is_some(),
        }
    }
}

/// A command's output when it succeeded.
fn run(cmd: &[&str]) -> Option<String> {
    let out = Command::new(cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// This distribution's names for Arch packages, and those it does not package.
pub fn translate(arch: &[&str]) -> (Vec<String>, Vec<String>) {
    let Some(m) = manager() else {
        return (Vec::new(), arch.iter().map(|s| s.to_string()).collect());
    };
    let mut have = Vec::new();
    let mut not = Vec::new();
    for p in arch {
        match m.name(p) {
            Some(n) if !have.contains(&n) => have.push(n),
            Some(_) => {}
            None => not.push(p.to_string()),
        }
    }
    (have, not)
}

/// The line to type in a terminal that installs these (Arch names): `sudo apt-get install …`.
pub fn install_line(arch: &[&str]) -> Option<String> {
    let m = manager()?;
    let (names, _) = translate(arch);
    (!names.is_empty()).then(|| format!("sudo {}", m.install_command(&names).join(" ")))
}

/// After an install said it worked: the packages (this distribution's names) that are still
/// not installed (dnf, for one, succeeds without installing a package it cannot find).
pub fn not_installed(m: Manager, names: &[String]) -> anyhow::Result<()> {
    let missing: Vec<&str> = names
        .iter()
        .filter(|n| m.version(n).is_none())
        .map(String::as_str)
        .collect();
    anyhow::ensure!(missing.is_empty(), "not installed: {}", missing.join(", "));
    Ok(())
}

/// The installed version of a package, by its Arch name.
pub fn version(arch: &str) -> Option<String> {
    let m = manager()?;
    m.version(&m.name(arch)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_family() {
        let r = |t: &str| from_release(t);
        assert_eq!(r("ID=cachyos\nID_LIKE=arch\n"), Some(Manager::Pacman));
        assert_eq!(
            r("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"),
            Some(Manager::Apt)
        );
        assert_eq!(r("ID=pop\nID_LIKE=\"ubuntu debian\""), Some(Manager::Apt));
        assert_eq!(
            r("ID=nobara\nID_LIKE=\"rhel centos fedora\""),
            Some(Manager::Dnf)
        );
        assert_eq!(
            r("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\""),
            Some(Manager::Zypper)
        );
        assert_eq!(r("ID=nixos"), None);
    }

    #[test]
    fn names_packages_as_the_distribution_does() {
        assert_eq!(
            Manager::Apt.name("libarchive").as_deref(),
            Some("libarchive-tools")
        );
        assert_eq!(Manager::Dnf.name("vkbasalt").as_deref(), Some("vkBasalt"));
        assert_eq!(Manager::Apt.name("umu-launcher"), None);
        assert_eq!(
            Manager::Zypper.name("gamescope").as_deref(),
            Some("gamescope")
        );
        assert_eq!(Manager::Pacman.name("lsfg-vk").as_deref(), Some("lsfg-vk"));
        assert_eq!(
            Manager::Apt.install_command(&["mangohud".into()]),
            ["apt-get", "install", "-y", "mangohud"]
        );
    }
}
