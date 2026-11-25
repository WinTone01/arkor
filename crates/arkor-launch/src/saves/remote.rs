//! The stores' clouds, through their helpers. Nothing here touches the game's own folder:
//! saves are fetched into, and sent from, a folder of Arkor's.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;

use super::places::Place;
use super::tree::mark;
use crate::shops::{Shop, helper};
use crate::tools;

/// A store's cloud for one game.
pub trait Remote {
    /// Something that is the same as long as the cloud's saves are. `None`: cannot be told.
    fn mark(&self, place: &Place) -> anyhow::Result<Option<String>>;
    /// The date in a mark (unix time), for stores where the latest date counts.
    fn date(&self, _mark: &str) -> Option<u64> {
        None
    }
    /// Puts the cloud's saves into the empty folder `into` (it stays empty when there are none).
    fn fetch(&self, place: &Place, into: &Path) -> anyhow::Result<()>;
    /// Makes the cloud hold what is in `from`.
    fn push(&self, place: &Place, from: &Path) -> anyhow::Result<()>;
}

/// Long enough for large saves on a slow line; a store that does not answer ends sooner.
const PATIENCE: Duration = Duration::from_secs(180);

pub struct Epic<'a> {
    pub app: &'a str,
    /// A folder that stays empty.
    pub empty: &'a Path,
    pub stop: &'a AtomicBool,
}

impl Epic<'_> {
    fn sync(&self, at: &Path, args: &[&str]) -> anyhow::Result<helper::Output> {
        let mut cmd = helper::command(Shop::Epic)?;
        cmd.args(["-y", "sync-saves"])
            .args(args)
            .arg("--save-path")
            .arg(at)
            .arg(self.app);
        let out = helper::output_within(cmd, PATIENCE, self.stop)?;
        anyhow::ensure!(out.ok, "{}", out.error());
        // It ends well in these cases, too.
        for bad in [
            "Login failed",
            "Unable to lock install data",
            "No chunks were available",
            "may be corrupted",
            "may be incomplete",
            "Manifest is empty",
            "Download failed",
        ] {
            anyhow::ensure!(!out.stderr.contains(bad), "{}", out.error());
        }
        Ok(out)
    }
}

/// The date legendary tells of the cloud's saves.
pub(super) fn epic_date(log: &str) -> Option<&str> {
    let (_, rest) = log.rsplit_once("Cloud save date: ")?;
    let date = rest.lines().next()?.trim();
    (date != "N/A").then_some(date)
}

/// `2026-11-25 20:01:02` (UTC) as unix time.
pub(super) fn utc(date: &str) -> Option<u64> {
    let mut n = date.split([' ', '-', ':']).map(|p| p.parse::<i32>().ok());
    let mut next = || n.next().flatten();
    // SAFETY: a zeroed tm with its fields set; timegm only reads and normalises it.
    let t = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_year = next()? - 1900;
        tm.tm_mon = next()? - 1;
        tm.tm_mday = next()?;
        tm.tm_hour = next()?;
        tm.tm_min = next()?;
        tm.tm_sec = next()?;
        libc::timegm(&mut tm)
    };
    u64::try_from(t).ok()
}

impl Remote for Epic<'_> {
    fn date(&self, mark: &str) -> Option<u64> {
        utc(mark)
    }

    fn mark(&self, _: &Place) -> anyhow::Result<Option<String>> {
        // Asked with an empty folder, the cloud's saves are always the newer ones, and their
        // date is told.
        let log = self
            .sync(self.empty, &["--skip-upload", "--skip-download"])?
            .stderr;
        if log.contains("No cloud or local savegame found") {
            return Ok(Some(String::new()));
        }
        Ok(epic_date(&log).map(str::to_string))
    }

    fn fetch(&self, _: &Place, into: &Path) -> anyhow::Result<()> {
        let out = self.sync(into, &["--force-download", "--skip-upload"])?;
        let nothing = out.stderr.contains("No cloud or local savegame found");
        anyhow::ensure!(
            nothing
                || out
                    .stderr
                    .contains("Successfully completed savegame download"),
            "{}",
            out.error()
        );
        Ok(())
    }

    fn push(&self, _: &Place, from: &Path) -> anyhow::Result<()> {
        // What is in the folder was chosen already; legendary's own filter misreads lists
        // with blanks in them.
        let out = self.sync(
            from,
            &["--force-upload", "--skip-download", "--disable-filters"],
        )?;
        anyhow::ensure!(
            out.stderr.contains("Finished uploading savegame"),
            "{}",
            out.error()
        );
        Ok(())
    }
}

pub struct Gog<'a> {
    pub id: &'a str,
    pub client: &'a str,
    /// The client's key, when the game's description was kept.
    pub secret: Option<&'a str>,
    pub stop: &'a AtomicBool,
}

#[derive(Deserialize)]
struct Credentials {
    access_token: String,
    user_id: String,
}

#[derive(Deserialize)]
struct CloudFile {
    name: String,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    last_modified: String,
}

/// What GOG gives a file that was deleted.
const GONE: &str = "aadd86936a80ee8a369579c3926f1b3c";

/// The mark of a place in GOG's list of a game's files.
pub(super) fn gog_mark(list: &str, place: &str) -> anyhow::Result<String> {
    let files: Vec<CloudFile> = serde_json::from_str(list).context("unexpected list of saves")?;
    let mut lines: Vec<String> = files
        .iter()
        .filter(|f| f.name.starts_with(&format!("{place}/")) && f.hash != GONE)
        .map(|f| format!("{}\n{}\n{}\n", f.name, f.hash, f.last_modified))
        .collect();
    lines.sort();
    Ok(if lines.is_empty() {
        String::new()
    } else {
        mark(&lines.concat())
    })
}

impl Gog<'_> {
    fn sync(&self, place: &Place, at: &Path, how: &str) -> anyhow::Result<()> {
        let mut cmd = helper::command(Shop::Gog)?;
        cmd.arg("save-sync").arg(at).arg(self.id).args([
            "--ts",
            "0",
            "--os",
            "windows",
            "--name",
            &place.name,
            how,
        ]);
        let out = helper::output_within(cmd, PATIENCE, self.stop)?;
        anyhow::ensure!(out.ok, "{}", out.error());
        for bad in ["Downloading file failed", "error uploading a file"] {
            anyhow::ensure!(!out.stderr.contains(bad), "{}", out.error());
        }
        Ok(())
    }

    fn credentials(&self, secret: &str) -> anyhow::Result<Credentials> {
        let mut cmd = helper::command(Shop::Gog)?;
        cmd.args(["auth", "--client-id", self.client])
            .args(["--client-secret", secret]);
        helper::output_within(cmd, Duration::from_secs(30), self.stop)?
            .json()
            .map_err(|_| anyhow::anyhow!("the GOG login ended: log in again"))
    }
}

impl Remote for Gog<'_> {
    fn mark(&self, place: &Place) -> anyhow::Result<Option<String>> {
        let Some(secret) = self.secret else {
            return Ok(None);
        };
        let c = self.credentials(secret)?;
        anyhow::ensure!(
            !c.user_id.is_empty() && c.user_id.chars().all(|c| c.is_ascii_digit()),
            "unexpected GOG account"
        );
        let url = format!(
            "https://cloudstorage.gog.com/v1/{}/{}",
            c.user_id, self.client
        );
        match tools::http_get_json(&url, &c.access_token)? {
            Some(list) => Ok(Some(gog_mark(&list, &place.name)?)),
            None => Ok(Some(String::new())),
        }
    }

    fn fetch(&self, place: &Place, into: &Path) -> anyhow::Result<()> {
        self.sync(place, into, "--force-download")
    }

    fn push(&self, place: &Place, from: &Path) -> anyhow::Result<()> {
        self.sync(place, from, "--force-upload")
    }
}

/// What GOG knows about a game's saves (public).
pub fn gog_config(client: &str) -> anyhow::Result<Value> {
    let text = tools::http_get_quick(&format!(
        "https://remote-config.gog.com/components/galaxy_client/clients/{client}?component_version=2.0.45"
    ))?;
    serde_json::from_str(&text).context("unexpected answer from GOG")
}
