//! Backups of Arkor's own data (settings, library, profiles, play stats, covers) and,
//! optionally, prefixes inside the home folder, as one `.tar.gz` with home-relative paths.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackupSettings {
    /// Where backups go. `None` = `~/Arkor-backups`.
    pub dest: Option<PathBuf>,
    /// Automatic backup every N days (0 = off), checked when Arkor starts.
    pub every_days: u32,
    /// How many automatic backups to keep.
    pub keep: u32,
    /// Unix time of the last automatic backup.
    pub last: Option<u64>,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            dest: None,
            every_days: 0,
            keep: 5,
            last: None,
        }
    }
}

impl BackupSettings {
    pub fn dest_dir(&self) -> PathBuf {
        self.dest
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Arkor-backups"))
    }

    pub fn due(&self, now: u64) -> bool {
        self.every_days > 0
            && self
                .last
                .is_none_or(|last| now.saturating_sub(last) >= u64::from(self.every_days) * 86_400)
    }
}

/// Arkor's own data paths that exist.
fn data_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(c) = crate::paths::config_dir() {
        out.push(c);
    }
    if let Ok(s) = crate::stats::Stats::path() {
        out.push(s);
    }
    if let Some(d) = dirs::data_dir() {
        out.push(d.join("arkor/covers"));
    }
    out.into_iter().filter(|p| p.exists()).collect()
}

/// Paths relative to home; anything outside home is reported back instead.
fn relative_to_home(paths: &[PathBuf]) -> anyhow::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let home = dirs::home_dir().context("no home dir")?;
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for p in paths {
        match p.strip_prefix(&home) {
            Ok(rel) if !rel.as_os_str().is_empty() => inside.push(rel.to_path_buf()),
            _ => outside.push(p.clone()),
        }
    }
    Ok((inside, outside))
}

/// Writes a backup into `dest` and returns its path plus the prefixes it had to skip
/// (outside the home folder). Blocking.
pub fn create(
    dest: &Path,
    prefixes: &[PathBuf],
    tag: &str,
) -> anyhow::Result<(PathBuf, Vec<PathBuf>)> {
    let home = dirs::home_dir().context("no home dir")?;
    let mut paths = data_paths();
    paths.extend(prefixes.iter().filter(|p| p.exists()).cloned());
    let (rel, skipped) = relative_to_home(&paths)?;
    anyhow::ensure!(!rel.is_empty(), "nothing to back up yet");
    std::fs::create_dir_all(dest)?;
    let stamp = chrono_like_stamp();
    let file = dest.join(format!(
        "arkor-{tag}{}{stamp}.tar.gz",
        if tag.is_empty() { "" } else { "-" }
    ));
    let status = std::process::Command::new("tar")
        .arg("-C")
        .arg(&home)
        .arg("-czf")
        .arg(&file)
        .args(&rel)
        .status()
        .context("running tar")?;
    anyhow::ensure!(status.success(), "tar failed writing {}", file.display());
    Ok((file, skipped))
}

/// Restores a backup made by `create` over the current data. Blocking.
pub fn restore(file: &Path) -> anyhow::Result<()> {
    let home = dirs::home_dir().context("no home dir")?;
    // Refuse archives with absolute or parent paths: everything must land inside home.
    let list = std::process::Command::new("tar")
        .arg("-tzf")
        .arg(file)
        .output()
        .context("running tar")?;
    anyhow::ensure!(
        list.status.success(),
        "not a readable backup: {}",
        file.display()
    );
    let names = String::from_utf8_lossy(&list.stdout);
    anyhow::ensure!(
        names
            .lines()
            .all(|n| !n.starts_with('/') && !n.split('/').any(|c| c == "..")),
        "the archive contains paths outside the home folder"
    );
    let status = std::process::Command::new("tar")
        .arg("-C")
        .arg(&home)
        .arg("-xzf")
        .arg(file)
        .status()?;
    anyhow::ensure!(status.success(), "could not restore {}", file.display());
    Ok(())
}

/// Removes the oldest automatic backups beyond `keep`.
pub fn prune(dest: &Path, keep: u32) {
    let Ok(entries) = std::fs::read_dir(dest) else {
        return;
    };
    let mut autos: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("arkor-auto-"))
        })
        .collect();
    autos.sort();
    while autos.len() > keep.max(1) as usize {
        let _ = std::fs::remove_file(autos.remove(0));
    }
}

/// `20261122-0432` from the local clock (via `date`, to avoid a time-zone dependency).
fn chrono_like_stamp() -> String {
    std::process::Command::new("date")
        .arg("+%Y%m%d-%H%M%S")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::stats::now().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_and_prune() {
        let s = BackupSettings {
            every_days: 7,
            last: Some(1_000),
            ..Default::default()
        };
        assert!(!s.due(1_000 + 6 * 86_400));
        assert!(s.due(1_000 + 7 * 86_400));
        assert!(!BackupSettings::default().due(u64::MAX));
        let tmp = tempfile::tempdir().unwrap();
        for n in [
            "arkor-auto-1.tar.gz",
            "arkor-auto-2.tar.gz",
            "arkor-auto-3.tar.gz",
            "arkor-manual.tar.gz",
        ] {
            std::fs::write(tmp.path().join(n), "").unwrap();
        }
        prune(tmp.path(), 2);
        let mut left: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "arkor-auto-2.tar.gz",
                "arkor-auto-3.tar.gz",
                "arkor-manual.tar.gz"
            ]
        );
    }
}
