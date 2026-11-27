//! Arkor's copy of Amethyst Mod Manager (github.com/ChrisDKN/Amethyst-Mod-Manager, GPL-3.0):
//! its AppImage, fetched on demand into `~/.local/share/arkor/amethyst/` and checked against
//! the SHA-1 its release publishes. Arkor runs it as a headless service ([`super::rpc`]);
//! the same AppImage opens Amethyst's own window for what Arkor does not show.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::Deserialize;

use super::rpc::Client;
use crate::tools;

const RELEASES: &str = "https://api.github.com/repos/ChrisDKN/Amethyst-Mod-Manager/releases/latest";

/// Where Arkor keeps it.
pub fn dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("arkor/amethyst"))
}

pub fn appimage() -> anyhow::Result<PathBuf> {
    Ok(dir()?.join("Amethyst.AppImage"))
}

/// Its version, when Arkor has it.
pub fn installed_version() -> Option<String> {
    let dir = dir().ok()?;
    appimage()
        .ok()
        .filter(|p| p.is_file())
        .and_then(|_| std::fs::read_to_string(dir.join("version.txt")).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// The x86_64 AppImage of a release and its `.zsync` (which carries the file's SHA-1).
fn pick(release: &Release) -> Option<(&Asset, &Asset)> {
    let image = release
        .assets
        .iter()
        .find(|a| a.name.ends_with("-x86_64.AppImage"))?;
    let zsync = release
        .assets
        .iter()
        .find(|a| a.name == format!("{}.zsync", image.name))?;
    Some((image, zsync))
}

/// The newest release's tag.
pub fn latest_version() -> anyhow::Result<String> {
    let r: Release = serde_json::from_str(&tools::http_get_quick(RELEASES)?)
        .context("unexpected Amethyst release list")?;
    Ok(r.tag_name)
}

/// `SHA-1:` from a zsync file's header (text lines up to the first empty one).
fn zsync_sha1(head: &[u8]) -> Option<String> {
    let end = head
        .windows(2)
        .position(|w| w == b"\n\n")
        .unwrap_or(head.len());
    String::from_utf8_lossy(&head[..end])
        .lines()
        .find_map(|l| l.strip_prefix("SHA-1:"))
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| v.len() == 40 && v.chars().all(|c| c.is_ascii_hexdigit()))
}

fn sha1_of(path: &std::path::Path) -> anyhow::Result<String> {
    let out = Command::new("sha1sum")
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .context("running sha1sum")?;
    anyhow::ensure!(out.status.success(), "sha1sum failed");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .context("sha1sum printed nothing")
}

/// Downloads it when missing or (with `update`) when a newer release is out; its version.
/// Blocking.
pub fn ensure(update: bool) -> anyhow::Result<String> {
    if let Some(v) = installed_version()
        && !update
    {
        return Ok(v);
    }
    let release: Release = serde_json::from_str(&tools::http_get_quick(RELEASES)?)
        .context("unexpected Amethyst release list")?;
    if installed_version().as_deref() == Some(release.tag_name.as_str()) {
        return Ok(release.tag_name);
    }
    let (image, zsync) = pick(&release).context("this Amethyst release has no x86_64 AppImage")?;
    let dir = dir()?;
    std::fs::create_dir_all(&dir)?;
    let sum_file = dir.join("Amethyst.AppImage.zsync");
    tools::download(&zsync.browser_download_url, &sum_file)?;
    let head = std::fs::read(&sum_file).unwrap_or_default();
    let _ = std::fs::remove_file(&sum_file);
    let want = zsync_sha1(&head[..head.len().min(4096)])
        .context("Amethyst's release gives no checksum")?;
    let fresh = dir.join("Amethyst.AppImage.new");
    tools::download(&image.browser_download_url, &fresh)?;
    let got = sha1_of(&fresh)?;
    if got != want {
        let _ = std::fs::remove_file(&fresh);
        anyhow::bail!("the downloaded Amethyst is damaged (checksum differs); try again");
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755))?;
    // Not while it runs: the service is ended first, so the next use starts the new one.
    release_now();
    std::fs::rename(&fresh, appimage()?)?;
    std::fs::write(dir.join("version.txt"), &release.tag_name)?;
    Ok(release.tag_name)
}

/// Removes Arkor's copy (not what it set up: the mods and their folders stay).
pub fn remove() -> anyhow::Result<()> {
    release_now();
    let dir = dir()?;
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// Where its messages go.
fn log_file() -> Option<std::fs::File> {
    let dir = dirs::state_dir()?.join("arkor");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::File::create(dir.join("amethyst.log")).ok()
}

fn command(extract: bool) -> anyhow::Result<Command> {
    let image = appimage()?;
    anyhow::ensure!(image.is_file(), "Amethyst is not installed");
    let mut cmd = Command::new(image);
    cmd.args(["headless", "--stdio"]);
    // Without FUSE (some containers) the AppImage runs from a copy it unpacks.
    if extract {
        cmd.env("APPIMAGE_EXTRACT_AND_RUN", "1");
    }
    cmd.stderr(log_file().map_or(Stdio::null(), Stdio::from));
    Ok(cmd)
}

/// Starts the service.
pub fn start() -> anyhow::Result<Client> {
    match Client::spawn(command(false)?) {
        Ok(c) => Ok(c),
        Err(first) => Client::spawn(command(true)?).map_err(|_| first),
    }
}

/// One service for all of Arkor, started on first use and ended after a quiet minute.
static SHARED: Mutex<Option<(Client, Instant)>> = Mutex::new(None);
const IDLE: Duration = Duration::from_secs(60);

/// Runs `f` with the shared service. Blocking; one caller at a time.
pub fn with<R>(f: impl FnOnce(&mut Client) -> anyhow::Result<R>) -> anyhow::Result<R> {
    let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if !shared.as_mut().is_some_and(|(c, _)| c.alive()) {
        *shared = Some((start()?, Instant::now()));
    }
    let (client, _) = shared.as_mut().expect("started above");
    let out = f(client);
    if let Some((_, used)) = shared.as_mut() {
        *used = Instant::now();
    }
    drop(shared);
    std::thread::spawn(|| {
        std::thread::sleep(IDLE + Duration::from_secs(1));
        let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        if shared
            .as_ref()
            .is_some_and(|(_, used)| used.elapsed() >= IDLE)
        {
            *shared = None;
        }
    });
    out
}

/// Ends the shared service now (before Amethyst is replaced, or when Arkor quits).
pub fn release_now() {
    let taken = SHARED.lock().unwrap_or_else(|e| e.into_inner()).take();
    drop(taken);
}

/// Opens Amethyst's own window (for what Arkor does not show: LOOT, collections…).
pub fn open_window() -> anyhow::Result<()> {
    let image = appimage()?;
    anyhow::ensure!(image.is_file(), "Amethyst is not installed");
    // Its window and Arkor's service must not change the same game at once.
    release_now();
    Command::new(image)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting Amethyst")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_checksum_of_a_zsync_header() {
        let head = b"zsync: 0.6.2\nFilename: A.AppImage\nLength: 3\nSHA-1: 9806BD72A97680002CDD8138B77F5A8668CA654C\n\n\x00\x01binary";
        assert_eq!(
            zsync_sha1(head).as_deref(),
            Some("9806bd72a97680002cdd8138b77f5a8668ca654c")
        );
        assert_eq!(
            zsync_sha1(b"Filename: A\n\nSHA-1: 00\n"),
            None,
            "only the header"
        );
        assert_eq!(zsync_sha1(b"SHA-1: nothex\n\n"), None);
    }

    #[test]
    fn picks_the_x86_64_appimage_and_its_zsync() {
        let asset = |n: &str| Asset {
            name: n.into(),
            browser_download_url: format!("https://x/{n}"),
        };
        let r = Release {
            tag_name: "v2.6.4".into(),
            assets: vec![
                asset("AmethystModManager-2.6.4-aarch64.AppImage"),
                asset("AmethystModManager-2.6.4-aarch64.AppImage.zsync"),
                asset("AmethystModManager-2.6.4-x86_64.AppImage"),
                asset("AmethystModManager-2.6.4-x86_64.AppImage.zsync"),
                asset("AmethystModManager.flatpak"),
            ],
        };
        let (image, zsync) = pick(&r).unwrap();
        assert_eq!(image.name, "AmethystModManager-2.6.4-x86_64.AppImage");
        assert_eq!(zsync.name, "AmethystModManager-2.6.4-x86_64.AppImage.zsync");
        let none = Release {
            tag_name: "v1".into(),
            assets: vec![asset("AmethystModManager.flatpak")],
        };
        assert!(pick(&none).is_none());
    }
}
