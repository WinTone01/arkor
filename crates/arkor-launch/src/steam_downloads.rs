//! What Steam's client downloads: read from its manifests (what is left), its download
//! folder (how far it is, as it happens) and its content log (whether it runs or is paused).
//! Steam is only looked at here; it has no way to be told to pause or go on.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use arkor_stores::steam_library as lib;

use crate::{downloads, progress, shops, steam_link};

/// One app Steam has something to download for.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub appid: u32,
    pub name: String,
    pub done: u64,
    pub total: u64,
    /// Bytes per second.
    pub speed: f64,
    /// Steam works on it now (else it waits, is paused, or Steam is closed).
    pub running: bool,
    /// It is installed and only gets newer.
    pub update: bool,
}

impl Item {
    pub fn left(&self) -> u64 {
        self.total.saturating_sub(self.done)
    }
}

static ITEMS: Mutex<Vec<Item>> = Mutex::new(Vec::new());

/// What was found when it was last looked (every few seconds).
pub fn items() -> Vec<Item> {
    ITEMS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Forgets what is no longer in Steam (something was removed just now).
pub fn look_again() {
    let there: Vec<u32> = lib::manifests()
        .into_iter()
        .filter(|m| m.progress.unfinished())
        .map(|m| m.appid)
        .collect();
    ITEMS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|i| there.contains(&i.appid));
    progress::touch();
}

/// Looks now whether `appid` has to be updated before it can be played. Reads its manifest.
pub fn pending(appid: u32) -> Option<Item> {
    let m = lib::manifests().into_iter().find(|m| m.appid == appid)?;
    m.progress.needs_update().then_some(Item {
        appid,
        name: m.name,
        done: m.progress.done,
        total: m.progress.total,
        speed: 0.,
        running: false,
        update: true,
    })
}

/// Steam's client runs (its own pid file, no search).
pub fn steam_on() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let pid = std::fs::read_to_string(home.join(".steam/steam.pid")).unwrap_or_default();
    let pid = pid.trim();
    !pid.is_empty()
        && std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "steam")
}

/// Which apps Steam works on, by the end of its content log.
fn working(log: &str) -> HashMap<u32, bool> {
    let mut out = HashMap::new();
    for line in log.lines() {
        let Some((_, rest)) = line.split_once("AppID ") else {
            continue;
        };
        let Some((id, what)) = rest.split_once(' ') else {
            continue;
        };
        let Ok(id) = id.parse::<u32>() else { continue };
        if let Some(steps) = what.strip_prefix("App update changed : ") {
            out.insert(
                id,
                steps.contains("Running Update") && !steps.contains("Stopping"),
            );
        } else if let Some(state) = what.strip_prefix("state changed : ") {
            // "…, (Suspended)" or "Fully Installed,": nothing goes on.
            if !state.contains("Update Running") {
                out.insert(id, false);
            }
        }
    }
    out
}

/// The end of Steam's content log.
fn log_tail(root: &Path) -> String {
    let mut text = Vec::new();
    if let Ok(mut f) = std::fs::File::open(root.join("logs/content_log.txt")) {
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let _ = f.seek(SeekFrom::Start(len.saturating_sub(96 * 1024)));
        let _ = f.read_to_end(&mut text);
    }
    String::from_utf8_lossy(&text).into_owned()
}

/// Bytes on disk below `dir`.
fn on_disk(dir: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let mut sum = 0;
    let mut todo = vec![dir.to_path_buf()];
    let mut seen = 0;
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                todo.push(e.path());
            } else {
                sum += m.blocks() * 512;
            }
            seen += 1;
            if seen > 60_000 {
                return sum;
            }
        }
    }
    sum
}

/// Where a download stands: its manifest is only written now and then, what Steam has put
/// together in its download folder grows all the time.
fn done(p: &lib::Progress, staged: u64) -> u64 {
    if p.total == 0 {
        return 0;
    }
    let share = if p.to_stage > 0 {
        staged as f64 / p.to_stage as f64
    } else {
        0.
    };
    p.done
        .max((share.min(1.) * p.total as f64) as u64)
        .min(p.total)
}

/// How fast each of them went.
#[derive(Default)]
struct Speeds(HashMap<u32, (Instant, u64, f64)>);

impl Speeds {
    fn of(&mut self, appid: u32, done: u64, now: Instant) -> f64 {
        let Some((at, before, speed)) = self.0.get(&appid).copied() else {
            self.0.insert(appid, (now, done, 0.));
            return 0.;
        };
        let secs = (now - at).as_secs_f64();
        if secs < 1. {
            return speed;
        }
        let raw = done.saturating_sub(before) as f64 / secs;
        let smooth = if speed == 0. {
            raw
        } else {
            speed * 0.6 + raw * 0.4
        };
        self.0.insert(appid, (now, done, smooth));
        smooth
    }
}

fn look(speeds: &mut Speeds) -> Vec<Item> {
    let on = steam_on();
    let log = match lib::root() {
        Some(root) if on => working(&log_tail(&root)),
        _ => HashMap::new(),
    };
    let now = Instant::now();
    let mut out = Vec::new();
    for m in lib::manifests() {
        let p = m.progress;
        // Up to date, or only a small part of Steam itself without anything to fetch.
        if !p.unfinished() || (p.total == 0 && (m.runtime || p.state & 4 != 0)) {
            continue;
        }
        let running = log.get(&m.appid).copied().unwrap_or(false);
        let folder: Option<PathBuf> = m
            .path
            .parent()
            .map(|d| d.join("downloading").join(m.appid.to_string()));
        let staged = match &folder {
            Some(d) if running => on_disk(d),
            _ => p.staged,
        };
        let done = done(&p, staged);
        out.push(Item {
            appid: m.appid,
            name: m.name,
            done,
            total: p.total,
            speed: if running {
                speeds.of(m.appid, done, now)
            } else {
                0.
            },
            running,
            update: p.needs_update(),
        });
    }
    speeds
        .0
        .retain(|id, _| out.iter().any(|i| i.appid == *id && i.running));
    out
}

/// Keeps `items` and the list of downloads up to date (once per process).
pub(crate) fn start() {
    let _ = std::thread::Builder::new()
        .name("arkor-steam-downloads".into())
        .spawn(|| {
            let mut speeds = Speeds::default();
            let mut before: Vec<Item> = Vec::new();
            loop {
                let now = look(&mut speeds);
                let running: Vec<progress::External> = now
                    .iter()
                    .filter(|i| i.running)
                    .map(|i| progress::External {
                        key: shops::key(shops::Shop::Steam, &i.appid.to_string()),
                        label: i.name.clone(),
                        done: i.done,
                        total: (i.total > 0).then_some(i.total),
                        speed: i.speed,
                    })
                    .collect();
                let any = !running.is_empty();
                progress::set_external(running);
                // What was there and is complete now.
                for old in before.iter().filter(|o| o.total > 0) {
                    if !now.iter().any(|n| n.appid == old.appid)
                        && lib::progress(old.appid).is_some_and(|p| p.installed())
                    {
                        downloads::note_done(
                            &shops::key(shops::Shop::Steam, &old.appid.to_string()),
                            &old.name,
                            "Steam",
                        );
                    }
                }
                if now != before {
                    *ITEMS.lock().unwrap_or_else(|e| e.into_inner()) = now.clone();
                    progress::touch();
                }
                before = now;
                std::thread::sleep(Duration::from_secs(if any { 2 } else { 4 }));
            }
        });
}

/// Lets Steam go on with its updates: it does so by itself when it starts. `false` when it
/// runs already (a paused download can only be continued in Steam).
pub fn let_steam_update() -> anyhow::Result<bool> {
    if steam_on() {
        return Ok(false);
    }
    steam_link::start_silent()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_steam_works_on() {
        let log = "\
[2026-11-25 13:52:00] AppID 730 state changed : Update Required,Fully Installed,Update Queued,Update Running,Update Started,
[2026-11-25 13:52:00] AppID 730 App update changed : Running Update,Downloading,Staging,
[2026-11-25 13:52:01] AppID 10 App update changed : Running Update,Downloading,Staging,
[2026-11-25 13:52:02] Current download rate: 50.121 Mbps
[2026-11-25 13:52:11] AppID 730 App update changed : Running Update,Downloading,Staging,Stopping,
[2026-11-25 13:52:11] AppID 730 App update changed : None
[2026-11-25 13:52:11] AppID 730 state changed : Update Required,Fully Installed,Update Queued,Update Started, (Suspended)
[2026-11-25 13:52:12] AppID x App update changed : Running Update,
[2026-11-25 13:52:13] AppID 20 App update changed : Running Update,Committing,
[2026-11-25 13:52:14] AppID 20 state changed : Fully Installed,
";
        let w = working(log);
        assert_eq!(w.get(&730), Some(&false), "paused");
        assert_eq!(w.get(&10), Some(&true));
        assert_eq!(w.get(&20), Some(&false), "finished");
        assert_eq!(w.len(), 3);
    }

    #[test]
    fn the_folder_tells_more_than_the_manifest() {
        let p = lib::Progress {
            state: 1030,
            done: 100,
            total: 1000,
            staged: 0,
            to_stage: 4000,
        };
        assert_eq!(done(&p, 0), 100);
        assert_eq!(done(&p, 2000), 500);
        assert_eq!(done(&p, 9000), 1000, "never more than all of it");
        assert!(p.needs_update() && p.unfinished() && !p.installed());
        let installing = lib::Progress { state: 1026, ..p };
        assert!(!installing.needs_update() && installing.unfinished());
        let fine = lib::Progress {
            state: 4,
            done: 1000,
            ..p
        };
        assert!(!fine.unfinished() && fine.installed());
    }

    #[test]
    fn speed_is_smooth() {
        let mut s = Speeds::default();
        let t = Instant::now();
        let sec = Duration::from_secs(1);
        assert_eq!(s.of(1, 0, t), 0.);
        assert_eq!(s.of(1, 100, t + sec), 100.);
        let v = s.of(1, 300, t + 2 * sec);
        assert!(v > 100. && v < 200., "{v}");
    }
}
