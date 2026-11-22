//! EA app under Wine installs each update into a versioned folder
//! (`EA Desktop/13.x.y/EA Desktop/`), leaving the launcher path Arkor stored stale. Like
//! Faugus, Arkor copies the newest version into the fixed `EA Desktop/EA Desktop/` folder
//! before launching, so `EALauncher.exe` keeps working.

use std::path::Path;

pub const EA_LAUNCHER: &str =
    "drive_c/Program Files/Electronic Arts/EA Desktop/EA Desktop/EALauncher.exe";

/// Applies the fix if `exe` is the EA app inside `prefix`; returns whether anything changed.
pub fn apply(prefix: &Path, exe: &Path) -> anyhow::Result<bool> {
    if !exe.to_string_lossy().contains("Electronic Arts/EA Desktop") {
        return Ok(false);
    }
    let base = prefix.join("drive_c/Program Files/Electronic Arts/EA Desktop");
    let Ok(entries) = std::fs::read_dir(&base) else {
        return Ok(false);
    };
    let mut versions: Vec<(Vec<u64>, std::path::PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "EA Desktop" || !name.starts_with(|c: char| c.is_ascii_digit()) {
                return None;
            }
            let nums = name
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|n| n.parse().ok())
                .collect();
            Some((nums, e.path()))
        })
        .collect();
    if versions.is_empty() {
        return Ok(false);
    }
    versions.sort();
    let newest = versions.last().expect("not empty").1.join("EA Desktop");
    if !newest.is_dir() {
        return Ok(false);
    }
    copy_dir(&newest, &base.join("EA Desktop"))?;
    for (_, dir) in versions {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(true)
}

fn copy_dir(from: &Path, to: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_version_moves_into_fixed_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp
            .path()
            .join("drive_c/Program Files/Electronic Arts/EA Desktop");
        for (v, body) in [("13.100.0.1", "old"), ("13.200.1.5", "new")] {
            let d = base.join(v).join("EA Desktop");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("EALauncher.exe"), body).unwrap();
        }
        let exe = tmp.path().join(EA_LAUNCHER);
        assert!(apply(tmp.path(), &exe).unwrap());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert!(!base.join("13.100.0.1").exists() && !base.join("13.200.1.5").exists());
        assert!(!apply(tmp.path(), &exe).unwrap(), "nothing left to fix");
        assert!(!apply(tmp.path(), Path::new("/x/game.exe")).unwrap());
    }
}
