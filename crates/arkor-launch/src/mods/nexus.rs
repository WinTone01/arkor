//! Nexus Mods through Amethyst: its OAuth login (Amethyst keeps the tokens in the system's
//! keyring; Arkor never sees them), the account's name and Premium state, downloads from
//! the site's "Mod Manager Download" button (`nxm://` links, which free accounts get too),
//! and update checks.
//!
//! An `nxm://` link reaches Arkor as `arkor nxm <link>`: it is left in a private inbox
//! (`$XDG_RUNTIME_DIR/arkor-nxm/`, this user only) for the running Arkor to take, so a
//! second Arkor never opens. A link carries a one-time key: it is never logged, and its
//! file goes as soon as it is read.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};

use super::Link;
use super::rpc::{Client, RpcError};

/// The signed-in account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub name: String,
    pub premium: bool,
}

/// Whether a Nexus login is saved (no network).
pub fn signed_in(c: &mut Client) -> anyhow::Result<bool> {
    let v = c.call("nexus.auth.get", json!({}))?;
    Ok(v["credentials"]["authenticated"].as_bool().unwrap_or(false))
}

/// The account, checked with Nexus; `None` when nobody is signed in.
pub fn account(c: &mut Client) -> anyhow::Result<Option<Account>> {
    if !signed_in(c)? {
        return Ok(None);
    }
    match c.job("nexus.user.get.start", json!({}), |_| {}) {
        Ok(v) => Ok(Some(Account {
            name: v["name"].as_str().unwrap_or_default().to_string(),
            premium: v["premium"].as_bool().unwrap_or(false),
        })),
        Err(e) => match e.downcast_ref::<RpcError>() {
            Some(r) if r.name == "nexus_not_authenticated" || r.name == "nexus_auth_expired" => {
                Ok(None)
            }
            _ => Err(e),
        },
    }
}

/// A login in the browser: its page, or how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    /// Open this page; the login waits for the user there.
    Open(String),
    Waiting,
    Done,
    Failed(String),
}

fn login_of(v: &Value) -> Login {
    let l = &v["login"];
    match l["state"].as_str().unwrap_or_default() {
        "succeeded" => Login::Done,
        "failed" | "cancelled" => Login::Failed(
            l["error"]
                .as_str()
                .or_else(|| l["message"].as_str())
                .unwrap_or("the login did not finish")
                .to_string(),
        ),
        _ => match l["authorize_url"].as_str() {
            Some(url) if !url.is_empty() => Login::Open(url.to_string()),
            _ => Login::Waiting,
        },
    }
}

/// Starts a login; the page to open in the browser. Nexus sends the browser back to
/// Amethyst's own address on this computer (`localhost:7890`) when it is done.
pub fn login_start(c: &mut Client) -> anyhow::Result<String> {
    let mut v = c.call("nexus.auth.login.start", json!({"open_browser": false}))?;
    for _ in 0..50 {
        match login_of(&v) {
            Login::Open(url) => return Ok(url),
            Login::Failed(why) => anyhow::bail!(why),
            Login::Done => anyhow::bail!("already signed in"),
            Login::Waiting => {}
        }
        std::thread::sleep(Duration::from_millis(100));
        v = c.call("nexus.auth.get", json!({}))?;
    }
    anyhow::bail!("Nexus Mods did not answer")
}

/// How the login in the browser stands.
pub fn login_state(c: &mut Client) -> anyhow::Result<Login> {
    Ok(login_of(&c.call("nexus.auth.get", json!({}))?))
}

pub fn login_cancel(c: &mut Client) -> anyhow::Result<()> {
    c.call("nexus.auth.login.cancel", json!({}))?;
    Ok(())
}

pub fn logout(c: &mut Client) -> anyhow::Result<()> {
    c.job("nexus.auth.logout.start", json!({"confirm": true}), |_| {})?;
    Ok(())
}

/// What an `nxm://` link names (its key stays in the link).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NxmLink {
    /// The site's name for the game (`skyrimspecialedition`).
    pub domain: String,
    pub mod_id: u64,
    pub file_id: u64,
}

/// `nxm://<domain>/mods/<mod>/files/<file>?key=…&expires=…`
pub fn parse_nxm(link: &str) -> Option<NxmLink> {
    let rest = link.strip_prefix("nxm://")?;
    let path = rest.split(['?', '#']).next()?;
    let mut parts = path.split('/');
    let domain = parts.next()?.to_ascii_lowercase();
    let (Some("mods"), Some(m), Some("files"), Some(f)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let plain = !domain.is_empty()
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then_some(())?;
    Some(NxmLink {
        domain,
        mod_id: m.parse().ok()?,
        file_id: f.parse().ok()?,
    })
}

/// Downloads the file an `nxm://` link names into Amethyst's cache for the linked game;
/// the archive. Does not install it.
pub fn download(c: &mut Client, link: &Link, nxm: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(parse_nxm(nxm).is_some(), "not a Nexus Mods download link");
    let v = c.job(
        "nexus.download.start",
        json!({"game": link.game, "nxm_url": nxm}),
        |_| {},
    )?;
    v["archive"]["path"]
        .as_str()
        .map(PathBuf::from)
        .context("Amethyst did not say where the download went")
}

/// Asks Nexus (and Thunderstore) which of the game's mods have newer versions; how many.
/// The list then marks them.
pub fn check_updates(c: &mut Client, link: &Link) -> anyhow::Result<usize> {
    let v = c.job(
        "mods.check_updates.start",
        json!({"game": link.game, "profile": link.profile}),
        |_| {},
    )?;
    let count = |k: &str| v[k].as_array().map_or(0, Vec::len);
    Ok(count("updates") + count("thunderstore_updates"))
}

/// The game's mod pages on Nexus Mods.
pub fn mods_page(domain: &str) -> String {
    format!("https://www.nexusmods.com/{domain}/mods")
}

// ---- links handed over by `arkor nxm <link>` ----

fn inbox() -> anyhow::Result<PathBuf> {
    let base = dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .context("no runtime folder")?;
    Ok(base.join("arkor-nxm"))
}

/// Leaves a link for the running Arkor (or the next one to start).
pub fn hand_over(link: &str) -> anyhow::Result<()> {
    anyhow::ensure!(parse_nxm(link).is_some(), "not a Nexus Mods download link");
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let dir = inbox()?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{stamp}.part"));
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(link.as_bytes())?;
    }
    std::fs::rename(&tmp, dir.join(format!("{stamp}.nxm")))?;
    Ok(())
}

/// Takes the links left so far, oldest first; their files go.
pub fn take_handed() -> Vec<String> {
    let Ok(dir) = inbox() else { return Vec::new() };
    take_from(&dir)
}

fn take_from(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "nxm"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok();
            let _ = std::fs::remove_file(&p);
            text.map(|t| t.trim().to_string())
                .filter(|t| parse_nxm(t).is_some())
        })
        .collect()
}

/// The desktop entry that hands `nxm://` links to Arkor.
const HANDLER: &str = "arkor-nxm.desktop";

fn handler_file() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("applications")
        .join(HANDLER))
}

/// Whether the browser hands `nxm://` links to Arkor.
pub fn is_handler() -> bool {
    std::process::Command::new("xdg-mime")
        .args(["query", "default", "x-scheme-handler/nxm"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == HANDLER)
}

/// Makes Arkor the program for `nxm://` links (the user's own choice; another manager's
/// can be chosen back in the system settings).
pub fn become_handler() -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("finding Arkor's program")?;
    let path = handler_file()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let quoted = format!("\"{}\"", exe.display().to_string().replace('"', "\\\""));
    std::fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nName=Arkor (Nexus Mods)\nExec={quoted} nxm %u\nIcon=arkor\nNoDisplay=true\nTerminal=false\nMimeType=x-scheme-handler/nxm;\n"
        ),
    )?;
    let _ = std::process::Command::new("update-desktop-database")
        .arg(path.parent().unwrap_or(Path::new(".")))
        .output();
    let status = std::process::Command::new("xdg-mime")
        .args(["default", HANDLER, "x-scheme-handler/nxm"])
        .status()
        .context("running xdg-mime")?;
    anyhow::ensure!(status.success(), "xdg-mime could not set it");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_nxm_link() {
        let l = parse_nxm(
            "nxm://skyrimspecialedition/mods/3863/files/1000172397?key=abc&expires=1700000000&user_id=1",
        )
        .unwrap();
        assert_eq!(
            l,
            NxmLink {
                domain: "skyrimspecialedition".into(),
                mod_id: 3863,
                file_id: 1000172397
            }
        );
        assert!(parse_nxm("nxm://valheim/collections/abc/revisions/3").is_none());
        assert!(parse_nxm("https://www.nexusmods.com/valheim/mods/1").is_none());
        assert!(
            parse_nxm("nxm://../mods/1/files/2").is_none(),
            "a plain domain only"
        );
        assert!(parse_nxm("nxm://valheim/mods/x/files/2").is_none());
    }

    #[test]
    fn reads_the_login_state() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert_eq!(
            login_of(&v(
                r#"{"login":{"state":"waiting_for_user","authorize_url":"https://n/x"}}"#
            )),
            Login::Open("https://n/x".into())
        );
        assert_eq!(
            login_of(&v(r#"{"login":{"state":"starting","authorize_url":null}}"#)),
            Login::Waiting
        );
        assert_eq!(
            login_of(&v(r#"{"login":{"state":"succeeded"}}"#)),
            Login::Done
        );
        assert_eq!(
            login_of(&v(r#"{"login":{"state":"failed","error":"denied"}}"#)),
            Login::Failed("denied".into())
        );
    }

    #[test]
    fn links_wait_in_the_inbox_until_taken() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("arkor-nxm");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("2.nxm"), "nxm://valheim/mods/2/files/20?key=b").unwrap();
        std::fs::write(dir.join("1.nxm"), "nxm://valheim/mods/1/files/10?key=a\n").unwrap();
        std::fs::write(dir.join("3.nxm"), "not a link").unwrap();
        std::fs::write(dir.join(".4.part"), "nxm://valheim/mods/4/files/40").unwrap();
        let got = take_from(&dir);
        assert_eq!(
            got,
            [
                "nxm://valheim/mods/1/files/10?key=a",
                "nxm://valheim/mods/2/files/20?key=b"
            ],
            "oldest first; a broken one and an unfinished one are not links"
        );
        assert!(take_from(&dir).is_empty(), "taken once");
        assert!(
            dir.join(".4.part").exists(),
            "one still being written stays"
        );
    }
}
