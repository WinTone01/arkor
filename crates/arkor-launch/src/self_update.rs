//! Arkor's own updates from its GitHub releases: check for a newer version, download it,
//! put it in place of the running program and start it again.
//!
//! A release carries `arkor-<version>-x86_64-linux.tar.gz` with `bin/arkor`, the desktop
//! entry and the icons. Installs in a user folder (the install script puts Arkor in
//! `~/.local`) update themselves; a copy installed by the package manager in `/usr` is
//! left to it.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

const API: &str = "https://api.github.com/repos/WinTone01/arkor/releases/latest";

/// The running version.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// Version without the leading `v`, e.g. `0.2.0`.
    pub version: String,
    pub url: String,
    pub size: u64,
    /// Release notes (Markdown).
    pub notes: String,
}

/// `a` is newer than `b` (dotted numbers; a suffix like `-beta` counts as older).
pub fn is_newer(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> (Vec<u64>, bool) {
        let v = v.trim_start_matches('v');
        let (nums, pre) = match v.split_once('-') {
            Some((n, _)) => (n, true),
            None => (v, false),
        };
        (
            nums.split('.').map(|p| p.parse().unwrap_or(0)).collect(),
            pre,
        )
    };
    let (mut na, pa) = parse(a);
    let (mut nb, pb) = parse(b);
    let len = na.len().max(nb.len());
    na.resize(len, 0);
    nb.resize(len, 0);
    match na.cmp(&nb) {
        std::cmp::Ordering::Equal => pb && !pa,
        o => o == std::cmp::Ordering::Greater,
    }
}

/// The newest release, when it is newer than the running version. Blocking.
pub fn check() -> anyhow::Result<Option<Release>> {
    #[derive(Deserialize)]
    struct Gh {
        tag_name: String,
        #[serde(default)]
        body: String,
        assets: Vec<Asset>,
    }
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        size: u64,
        browser_download_url: String,
    }
    let gh: Gh =
        serde_json::from_str(&crate::tools::http_get(API)?).context("unexpected GitHub release")?;
    let version = gh.tag_name.trim_start_matches('v').to_string();
    if !is_newer(&version, current_version()) {
        return Ok(None);
    }
    let asset = gh
        .assets
        .into_iter()
        .find(|a| a.name.ends_with("x86_64-linux.tar.gz"))
        .context("the release has no Linux build")?;
    Ok(Some(Release {
        version,
        url: asset.browser_download_url,
        size: asset.size,
        notes: gh.body,
    }))
}

/// Where the running program lives, if Arkor may replace it there (a user-writable
/// folder; not the package manager's `/usr`).
pub fn updatable_exe() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("where Arkor runs from")?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    anyhow::ensure!(
        !exe.starts_with("/usr"),
        "Arkor was installed by the package manager; update it there (e.g. run the install command again)"
    );
    let dir = exe.parent().context("program folder")?;
    let probe = dir.join(".arkor-write-test");
    std::fs::write(&probe, b"").with_context(|| format!("{} is not writable", dir.display()))?;
    let _ = std::fs::remove_file(probe);
    Ok(exe)
}

/// Downloads `release` and puts it in place of the running program (and refreshes the
/// desktop entry and icons when Arkor is installed under `~/.local`). Returns the program
/// to start. The running process keeps working until it exits. Blocking.
pub fn install(release: &Release) -> anyhow::Result<PathBuf> {
    let exe = updatable_exe()?;
    let work = dirs::cache_dir()
        .context("no cache dir")?
        .join("arkor/update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let archive = work.join("arkor.tar.gz");
    crate::tools::download(&release.url, &archive)?;
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&work)
        .status()
        .context("running tar")?;
    anyhow::ensure!(status.success(), "could not unpack the update");
    let root = find_root(&work).context("the update has no bin/arkor")?;
    replace_file(&root.join("bin/arkor"), &exe, 0o755)?;
    // Desktop entry and icons next to a ~/.local install.
    if let Some(prefix) = exe.parent().and_then(Path::parent)
        && prefix.join("share").is_dir()
        && dirs::home_dir().is_some_and(|h| prefix.starts_with(h))
    {
        let _ = copy_tree(&root.join("share"), &prefix.join("share"));
        // The packaged entry runs `arkor` from PATH; a ~/.local install points at itself.
        let entry = prefix.join("share/applications/arkor.desktop");
        if let Ok(text) = std::fs::read_to_string(&entry) {
            let _ = std::fs::write(&entry, desktop_exec(&text, &exe));
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(exe)
}

/// The desktop entry with `Exec=` pointing at `exe`.
fn desktop_exec(text: &str, exe: &Path) -> String {
    text.lines()
        .map(|l| {
            if l.starts_with("Exec=") {
                format!("Exec={}", exe.display())
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// The folder holding `bin/arkor` inside the unpacked archive.
fn find_root(dir: &Path) -> Option<PathBuf> {
    if dir.join("bin/arkor").is_file() {
        return Some(dir.to_path_buf());
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .find_map(|e| find_root(&e.path()))
}

/// Writes `src` over `dest` atomically (a new file renamed into place, so the running
/// program, which still has the old file open, is not disturbed).
fn replace_file(src: &Path, dest: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let tmp = dest.with_extension("new");
    std::fs::copy(src, &tmp).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&tmp, dest).with_context(|| format!("replacing {}", dest.display()))?;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            std::fs::create_dir_all(&dst)?;
            copy_tree(&src, &dst)?;
        } else {
            replace_file(&src, &dst, 0o644)?;
        }
    }
    Ok(())
}

/// Starts the (updated) program again with the same arguments. The caller then quits.
pub fn restart(exe: &Path) -> anyhow::Result<()> {
    std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_points_at_the_install() {
        let text = "[Desktop Entry]\nName=Arkor\nExec=arkor\nIcon=arkor\n";
        let out = desktop_exec(text, Path::new("/home/u/.local/bin/arkor"));
        assert!(out.contains("Exec=/home/u/.local/bin/arkor\n") && out.contains("Icon=arkor"));
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.2.0", "0.2.0-beta"), "a release beats its beta");
        assert!(!is_newer("0.2.0-beta", "0.2.0"));
    }

    #[test]
    fn replaces_a_file_and_finds_the_archive_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("arkor-0.2.0");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/arkor"), "new").unwrap();
        assert_eq!(find_root(tmp.path()), Some(root.clone()));
        let dest = tmp.path().join("installed-arkor");
        std::fs::write(&dest, "old").unwrap();
        replace_file(&root.join("bin/arkor"), &dest, 0o755).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}
