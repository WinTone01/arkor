//! What needs the administrator password, asked by Arkor itself (in console mode there is
//! no desktop password window to answer): installing packages and mounting disks. The
//! password goes to `sudo -S` through a pipe; it is never written anywhere. AUR packages are
//! left out on purpose: their build scripts want reading first, and a password handed to the
//! build could be read by it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Context;
use serde::Deserialize;

/// Runs `cmd` with sudo. The password is checked first, on its own: a wrong one is tried
/// once only, so it cannot lock the account.
fn sudo(password: &str, cmd: &[&str]) -> anyhow::Result<std::process::Output> {
    check_password(password)?;
    let mut child = Command::new("sudo")
        .args(["-S", "-p", "", "--"])
        .args(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running sudo")?;
    if let Some(mut stdin) = child.stdin.take() {
        let line = format!("{password}\n");
        let _ = stdin.write_all(line.as_bytes());
    }
    Ok(child.wait_with_output()?)
}

/// Whether `password` is the user's (`sudo -v`), without running anything.
pub fn check_password(password: &str) -> anyhow::Result<()> {
    let mut child = Command::new("sudo")
        .args(["-S", "-k", "-v", "-p", ""])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("running sudo")?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(format!("{password}\n").as_bytes());
    }
    let out = child.wait_with_output()?;
    anyhow::ensure!(out.status.success(), "wrong password");
    Ok(())
}

/// Installs repository packages (Arch's names) with the distribution's package manager.
/// Blocking.
pub fn install_packages(packages: &[&str], password: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!packages.is_empty(), "nothing to install");
    let m = crate::pkg::manager().context("no package manager Arkor knows was found")?;
    let (names, missing) = crate::pkg::translate(packages);
    anyhow::ensure!(
        !names.is_empty(),
        "not packaged for this distribution: {}",
        missing.join(", ")
    );
    let cmd = m.install_command(&names);
    let cmd: Vec<&str> = cmd.iter().map(String::as_str).collect();
    let out = sudo(password, &cmd)?;
    anyhow::ensure!(
        out.status.success(),
        "{} failed: {}",
        m.program(),
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .filter(|l| l.contains("error"))
            .collect::<Vec<_>>()
            .join("; ")
    );
    Ok(())
}

/// A partition that can be mounted.
#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    /// `/dev/nvme0n1p3`.
    pub path: String,
    pub label: String,
    pub uuid: String,
    pub fstype: String,
    pub size: String,
}

impl Disk {
    /// The folder a desktop mounts it at, below /run/media/<user>: its label, else its UUID
    /// (never its device name: that changes from one start to the next).
    fn folder(&self) -> String {
        let name = [&self.label, &self.uuid]
            .into_iter()
            .find(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| self.path.rsplit('/').next().unwrap_or("disk").to_string());
        let name: String = name
            .chars()
            .map(|c| if c == '/' || c.is_control() { '_' } else { c })
            .collect();
        if name.trim_matches('.').is_empty() {
            "disk".into()
        } else {
            name
        }
    }

    /// Where it is mounted, or would be.
    fn point(&self) -> anyhow::Result<PathBuf> {
        let user = std::env::var("USER").context("no user")?;
        Ok(Path::new("/run/media").join(user).join(self.folder()))
    }
}

#[derive(Deserialize)]
struct Lsblk {
    blockdevices: Vec<Dev>,
}

#[derive(Deserialize)]
struct Dev {
    path: Option<String>,
    label: Option<String>,
    uuid: Option<String>,
    fstype: Option<String>,
    size: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    parttype: Option<String>,
    #[serde(default)]
    mountpoints: Vec<Option<String>>,
    #[serde(default)]
    children: Vec<Dev>,
}

/// A disk games can be put on: mounted (and where), or waiting to be.
#[derive(Debug, Clone, PartialEq)]
pub struct Drive {
    pub disk: Disk,
    pub mount: Option<String>,
    /// All of its room and what is free, in bytes, when it is mounted.
    pub space: Option<(u64, u64)>,
}

/// All the room of the file system at `path`, and what is free.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    // SAFETY: the path ends with a zero and the answer is written to a struct of its own.
    let s = unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return None;
        }
        s
    };
    Some((s.f_blocks * s.f_frsize, s.f_bavail * s.f_frsize))
}

/// The disk one of `paths` is on when it is not where that path expects it: not mounted, or
/// mounted somewhere else.
pub fn missing_disk(paths: &[PathBuf], drives: &[Drive]) -> Option<Drive> {
    paths.iter().find_map(|p| {
        let name = disk_folder(p)?;
        let root = p.ancestors().find(|a| a.ends_with(&name))?;
        if drives
            .iter()
            .any(|d| d.mount.as_deref().is_some_and(|m| Path::new(m) == root))
        {
            return None;
        }
        drives
            .iter()
            .find(|d| !name.is_empty() && (d.disk.label == name || d.disk.uuid == name))
            .cloned()
    })
}

/// `Games` for `/run/media/<user>/Games/…`.
pub fn disk_folder(path: &Path) -> Option<String> {
    let mut parts = path.strip_prefix("/run/media").ok()?.components();
    parts.next()?;
    Some(parts.next()?.as_os_str().to_string_lossy().into_owned())
}

/// The partitions with a file system, the system's own ones (root, boot, recovery) and the
/// encrypted ones left out. Blocking.
pub fn drives() -> Vec<Drive> {
    let out = Command::new("lsblk")
        .args([
            "-J",
            "-o",
            "PATH,LABEL,UUID,FSTYPE,SIZE,TYPE,PARTTYPE,MOUNTPOINTS",
        ])
        .output();
    let mut found = out.map(|o| parse_drives(&o.stdout)).unwrap_or_default();
    for d in &mut found {
        d.space = d.mount.as_deref().and_then(|m| space(Path::new(m)));
    }
    found
}

fn parse_drives(json: &[u8]) -> Vec<Drive> {
    /// Containers and other things that cannot be mounted as they are.
    const NOT_FILES: [&str; 5] = [
        "swap",
        "crypto_LUKS",
        "LVM2_member",
        "linux_raid_member",
        "iso9660",
    ];
    /// Partition types of EFI systems and of Windows' recovery.
    const SYSTEM: [&str; 2] = [
        "c12a7328-f81f-11d2-ba4b-00a0c93ec93b",
        "de94bba4-06d1-4d40-a16a-bfd50179d6ac",
    ];
    /// Mounted below one of these, a partition is part of the system.
    const OWN: [&str; 9] = [
        "/boot", "/efi", "/home", "/nix", "/root", "/snap", "/srv", "/usr", "/var",
    ];
    fn walk(devs: &[Dev], out: &mut Vec<Drive>) {
        for d in devs {
            let fs = d.fstype.clone().unwrap_or_default();
            let mounts: Vec<&String> = d.mountpoints.iter().flatten().collect();
            let system = mounts.iter().any(|m| {
                *m == "/"
                    || *m == "[SWAP]"
                    || OWN.iter().any(|o| std::path::Path::new(m).starts_with(o))
            }) || d
                .parttype
                .as_deref()
                .is_some_and(|t| SYSTEM.contains(&t.to_lowercase().as_str()));
            if !fs.is_empty()
                && !NOT_FILES.contains(&fs.as_str())
                && matches!(d.kind.as_deref(), Some("part" | "disk" | "lvm" | "crypt"))
                && !system
                && let Some(path) = &d.path
            {
                out.push(Drive {
                    disk: Disk {
                        path: path.clone(),
                        label: d.label.clone().unwrap_or_default(),
                        uuid: d.uuid.clone().unwrap_or_default(),
                        fstype: fs,
                        size: d.size.clone().unwrap_or_default(),
                    },
                    mount: mounts.first().map(|m| m.to_string()),
                    space: None,
                });
            }
            walk(&d.children, out);
        }
    }
    let mut found = Vec::new();
    if let Ok(tree) = serde_json::from_slice::<Lsblk>(json) {
        walk(&tree.blockdevices, &mut found);
    }
    found
}

/// Mounts a disk the way the desktop would (udisks, under /run/media/<user>), asking for
/// nothing; `NeedsPassword` when that is not allowed without one. Blocking.
pub fn mount(disk: &Disk) -> Result<String, MountError> {
    let out = Command::new("udisksctl")
        .args(["mount", "--no-user-interaction", "-b", &disk.path])
        .output()
        .map_err(|e| MountError::Failed(e.to_string()))?;
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        // "Mounted /dev/sdb1 at /run/media/user/Games"
        return Ok(text
            .split(" at ")
            .nth(1)
            .map(|p| p.trim().trim_end_matches('.').to_string())
            .unwrap_or_default());
    }
    if text.contains("NotAuthorized") || text.contains("not authorized") {
        return Err(MountError::NeedsPassword);
    }
    Err(MountError::Failed(text.trim().to_string()))
}

#[derive(Debug, PartialEq)]
pub enum MountError {
    NeedsPassword,
    Failed(String),
}

/// Mounts a disk, letting the desktop ask for what it needs. Blocking.
pub fn mount_asking(disk: &Disk) -> anyhow::Result<()> {
    let out = Command::new("udisksctl")
        .args(["mount", "-b", &disk.path])
        .stdin(Stdio::null())
        .output()
        .context("running udisksctl")?;
    anyhow::ensure!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(())
}

/// Mounts a disk with the password where the desktop would, owned by the user. Blocking.
pub fn mount_with_password(disk: &Disk, password: &str) -> anyhow::Result<String> {
    let point = disk.point()?;
    let taken = std::fs::read_to_string("/proc/self/mounts").is_ok_and(|m| {
        m.lines()
            .filter_map(|l| l.split(' ').nth(1))
            .any(|p| Path::new(&unescape(p)) == point)
    });
    anyhow::ensure!(!taken, "{} is in use", point.display());
    let point = point.display().to_string();
    let out = sudo(password, &["mkdir", "-p", "--", &point])?;
    anyhow::ensure!(out.status.success(), "could not make {point}");
    // As the desktop does: no programs with other people's rights, no devices.
    let mut options = "nosuid,nodev".to_string();
    // File systems without owners get the user as owner.
    if ["vfat", "exfat", "ntfs", "ntfs3"].contains(&disk.fstype.as_str()) {
        // SAFETY: these calls cannot fail.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        options += &format!(",uid={uid},gid={gid}");
    }
    let out = sudo(
        password,
        &["mount", "-o", &options, "--", &disk.path, &point],
    )?;
    if !out.status.success() {
        let _ = sudo(password, &["rmdir", "--", &point]);
        anyhow::bail!(
            "mount failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(point)
}

/// `/proc/self/mounts` writes spaces as `\040`.
fn unescape(s: &str) -> String {
    s.replace("\\040", " ").replace("\\134", "\\")
}

/// Takes a disk out of use without a password, when that is allowed. Blocking.
pub fn unmount(disk: &Disk) -> Result<(), MountError> {
    let out = Command::new("udisksctl")
        .args(["unmount", "--no-user-interaction", "-b", &disk.path])
        .output()
        .map_err(|e| MountError::Failed(e.to_string()))?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    if text.contains("NotAuthorized") || text.contains("not authorized") {
        return Err(MountError::NeedsPassword);
    }
    Err(MountError::Failed(text.trim().to_string()))
}

/// Takes the disk mounted at `point` out of use with the password. Blocking.
pub fn unmount_with_password(point: &str, password: &str) -> anyhow::Result<()> {
    let out = sudo(password, &["umount", "--", point])?;
    anyhow::ensure!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    // The empty folder that was made for it.
    if point.starts_with("/run/media/") {
        let _ = sudo(password, &["rmdir", "--", point]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drives_leave_the_system_out() {
        let json = br#"{"blockdevices":[
          {"path":"/dev/sda","label":null,"fstype":null,"size":"931,5G","type":"disk","parttype":null,"mountpoints":[null],
           "children":[{"path":"/dev/sda1","label":null,"fstype":"ntfs","size":"931,5G","type":"part","parttype":null,"mountpoints":[null]}]},
          {"path":"/dev/nvme1n1","label":null,"fstype":null,"size":"465G","type":"disk","parttype":null,"mountpoints":[null],
           "children":[
             {"path":"/dev/nvme1n1p1","label":null,"fstype":"vfat","size":"4G","type":"part","parttype":null,"mountpoints":["/boot"]},
             {"path":"/dev/nvme1n1p2","label":null,"fstype":"btrfs","size":"461G","type":"part","parttype":null,"mountpoints":["/var/tmp","/home","/"]},
             {"path":"/dev/nvme1n1p3","label":null,"fstype":"vfat","size":"1G","type":"part","parttype":"C12A7328-F81F-11D2-BA4B-00A0C93EC93B","mountpoints":[null]}]},
          {"path":"/dev/zram0","label":"zram0","fstype":"swap","size":"15G","type":"disk","parttype":null,"mountpoints":["[SWAP]"]},
          {"path":"/dev/sdb1","label":"Games","fstype":"ext4","size":"1T","type":"part","parttype":null,"mountpoints":["/data/games"]}
        ]}"#;
        let d = parse_drives(json);
        assert_eq!(
            d.iter()
                .map(|d| (d.disk.path.as_str(), d.mount.as_deref()))
                .collect::<Vec<_>>(),
            [("/dev/sda1", None), ("/dev/sdb1", Some("/data/games"))]
        );
    }

    #[test]
    fn a_disk_is_found_by_the_path_of_a_game() {
        let drive = |path: &str, label: &str, uuid: &str, mount: Option<&str>| Drive {
            disk: Disk {
                path: path.into(),
                label: label.into(),
                uuid: uuid.into(),
                fstype: "btrfs".into(),
                size: "1T".into(),
            },
            mount: mount.map(Into::into),
            space: None,
        };
        let drives = [
            drive("/dev/sda1", "", "4cb5-67", Some("/run/media/me/nvme1n1p1")),
            drive("/dev/sdb1", "My Games", "77", None),
            drive("/dev/sdc1", "Here", "88", Some("/run/media/me/Here")),
        ];
        let find = |p: &str| {
            missing_disk(&[PathBuf::from("/home/me/x"), PathBuf::from(p)], &drives)
                .map(|d| d.disk.path)
        };
        // Mounted under another name, not mounted, and where it should be.
        assert_eq!(
            find("/run/media/me/4cb5-67/a/b.exe").as_deref(),
            Some("/dev/sda1")
        );
        assert_eq!(
            find("/run/media/me/My Games/b.exe").as_deref(),
            Some("/dev/sdb1")
        );
        assert_eq!(find("/run/media/me/Here/b.exe"), None);
        assert_eq!(find("/run/media/me/other/b.exe"), None);
        assert_eq!(find("/mnt/b.exe"), None);
        // The desktop's folder: the label, else the UUID, never the device's name.
        assert_eq!(drives[0].disk.folder(), "4cb5-67");
        assert_eq!(drives[1].disk.folder(), "My Games");
        assert_eq!(
            unescape("/run/media/me/My\\040Games"),
            "/run/media/me/My Games"
        );
    }
}
