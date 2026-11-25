//! What needs the administrator password, asked by Arkor itself (in console mode there is
//! no desktop password window to answer): installing packages and mounting disks. The
//! password goes to `sudo -S` through a pipe; it is never written anywhere. AUR packages are
//! left out on purpose: their build scripts want reading first, and a password handed to the
//! build could be read by it.

use std::io::Write;
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

/// Installs repository packages with pacman. Blocking.
pub fn install_packages(packages: &[&str], password: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!packages.is_empty(), "nothing to install");
    let mut cmd = vec!["pacman", "-S", "--needed", "--noconfirm", "--"];
    cmd.extend(packages);
    let out = sudo(password, &cmd)?;
    anyhow::ensure!(
        out.status.success(),
        "pacman failed: {}",
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
    pub fstype: String,
    pub size: String,
}

#[derive(Deserialize)]
struct Lsblk {
    blockdevices: Vec<Dev>,
}

#[derive(Deserialize)]
struct Dev {
    path: Option<String>,
    label: Option<String>,
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
}

/// The partitions with a file system, the system's own ones (root, boot, recovery) and the
/// encrypted ones left out. Blocking.
pub fn drives() -> Vec<Drive> {
    let out = Command::new("lsblk")
        .args([
            "-J",
            "-o",
            "PATH,LABEL,FSTYPE,SIZE,TYPE,PARTTYPE,MOUNTPOINTS",
        ])
        .output();
    out.map(|o| parse_drives(&o.stdout)).unwrap_or_default()
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
                        fstype: fs,
                        size: d.size.clone().unwrap_or_default(),
                    },
                    mount: mounts.first().map(|m| m.to_string()),
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

/// Mounts a disk with the password, under /run/media/<user>/<label>, owned by the user.
/// Blocking.
pub fn mount_with_password(disk: &Disk, password: &str) -> anyhow::Result<String> {
    let user = std::env::var("USER").context("no user")?;
    let name: String = if disk.label.is_empty() {
        disk.path.rsplit('/').next().unwrap_or("disk").to_string()
    } else {
        disk.label.clone()
    }
    .chars()
    .map(|c| {
        if c.is_alphanumeric() || "-_ ".contains(c) {
            c
        } else {
            '_'
        }
    })
    .collect();
    let point = format!("/run/media/{user}/{name}");
    let out = sudo(password, &["mkdir", "-p", &point])?;
    anyhow::ensure!(out.status.success(), "could not make {point}");
    // File systems without owners get the user as owner.
    let ids = format!(
        "uid={},gid={}",
        // SAFETY: these calls cannot fail.
        unsafe { libc::getuid() },
        unsafe { libc::getgid() }
    );
    let mut cmd = vec!["mount"];
    if ["vfat", "exfat", "ntfs", "ntfs3"].contains(&disk.fstype.as_str()) {
        cmd.extend(["-o", ids.as_str()]);
    }
    cmd.extend([disk.path.as_str(), point.as_str()]);
    let out = sudo(password, &cmd)?;
    anyhow::ensure!(
        out.status.success(),
        "mount failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(point)
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
}
