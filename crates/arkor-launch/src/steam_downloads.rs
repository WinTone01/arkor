//! What Steam's client downloads: read from its manifests (what is left), its download
//! folder (how far it is, as it happens) and its content log (whether it runs or is paused).
//! Steam is only looked at here; it has no way to be told to pause or go on.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use arkor_stores::steam_library as lib;

use crate::{downloads, progress, shops, steam_api, steam_control, steam_link};

/// What Steam does with a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    /// Steam is closed.
    #[default]
    Off,
    /// Its turn comes.
    Waiting,
    Paused,
    /// Looks at what is there and makes room.
    Preparing,
    Downloading,
    /// Puts it in its place.
    Installing,
}

/// One app Steam has something to download for, in the order of Steam's queue.
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
    pub step: Step,
    /// Paused by itself: it stays so when Steam starts again.
    pub held: bool,
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
        step: Step::Off,
        held: m.progress.held(),
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

/// What the log tells about one app.
#[derive(Debug, Clone, Default, PartialEq)]
struct Told {
    step: Step,
    /// Downloaded before this run and in all; put together before and in all.
    from: u64,
    total: u64,
    staged: u64,
    to_stage: u64,
    /// Its files were given their full size beforehand: their size says nothing.
    preallocated: bool,
}

/// "download 0/2180800320" in a line.
fn pair(line: &str, what: &str) -> Option<(u64, u64)> {
    let rest = line.split_once(&format!("{what} "))?.1;
    let (a, b) = rest.split([',', ' ']).next()?.split_once('/')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// What Steam does, by the end of its content log: each app, and the order of its queue.
fn told(log: &str) -> (Vec<u32>, HashMap<u32, Told>) {
    let mut queue: Vec<u32> = Vec::new();
    let mut out: HashMap<u32, Told> = HashMap::new();
    for line in log.lines() {
        // Steam started: what was before counts no more.
        if line.contains("] Client version:") {
            queue.clear();
            out.clear();
            continue;
        }
        let Some((_, rest)) = line.split_once("AppID ") else {
            continue;
        };
        let Some((id, what)) = rest.split_once(' ') else {
            continue;
        };
        let Ok(id) = id.parse::<u32>() else { continue };
        if let Some(p) = what.strip_prefix("scheduler update : ") {
            if p.starts_with("Priority First") || p.starts_with("Priority User") {
                queue.retain(|q| *q != id);
                queue.insert(0, id);
            } else if p.starts_with("Priority Paused") {
                queue.retain(|q| *q != id);
                queue.push(id);
            } else if !queue.contains(&id) {
                queue.push(id);
            }
            continue;
        }
        if what.starts_with("scheduler finished : removed") {
            queue.retain(|q| *q != id);
            continue;
        }
        let t = out.entry(id).or_default();
        if let Some(steps) = what.strip_prefix("App update changed : ") {
            if steps.contains("Running Update") && !steps.contains("Stopping") {
                t.step = if steps.contains("Downloading") {
                    Step::Downloading
                } else if steps.contains("Committing") || steps.contains("Staging") {
                    Step::Installing
                } else if t.step == Step::Downloading || t.step == Step::Installing {
                    // Between two of them.
                    t.step
                } else {
                    Step::Preparing
                };
            } else if !matches!(t.step, Step::Paused) {
                t.step = Step::Waiting;
            }
        } else if let Some(state) = what.strip_prefix("state changed : ") {
            if state.contains("(Suspended)") || state.contains("Update Paused") {
                t.step = Step::Paused;
            } else if state.contains("Update Running") {
                if matches!(t.step, Step::Off | Step::Waiting | Step::Paused) {
                    t.step = Step::Preparing;
                }
            } else {
                t.step = Step::Waiting;
            }
        } else if what.starts_with("update started : ") {
            if let Some((a, b)) = pair(what, "download") {
                (t.from, t.total) = (a, b);
            }
            if let Some((a, b)) = pair(what, "stage") {
                (t.staged, t.to_stage) = (a, b);
            }
        } else if what.starts_with("update canceled") {
            t.step = Step::Paused;
        } else if let Some(n) = what.strip_prefix("preallocated ") {
            t.preallocated = !n.starts_with("0 files");
        }
    }
    (queue, out)
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

/// Where a running download stands. Its manifest is only written when Steam stops; what
/// Steam has put together in its download folder grows all the time.
fn live(t: &Told, on_disk: u64) -> u64 {
    let grown = on_disk.saturating_sub(t.staged) as f64;
    let all = t.to_stage.saturating_sub(t.staged) as f64;
    let share = if all > 0. && !t.preallocated {
        (grown / all).min(1.)
    } else {
        0.
    };
    (t.from + (share * t.total.saturating_sub(t.from) as f64) as u64).min(t.total)
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

/// Steam's downloads as Steam itself tells them.
fn from_steam(state: steam_control::State, manifests: &[lib::Manifest]) -> Vec<Item> {
    let mut items = state.items;
    // The queue, then what is paused.
    items.sort_by_key(|i| if i.place < 0 { i32::MAX } else { i.place });
    let now = state.now;
    items
        .into_iter()
        .map(|i| {
            let m = manifests.iter().find(|m| m.appid == i.appid);
            let live = now.as_ref().filter(|n| n.appid == i.appid);
            let (done, total) = match live {
                Some(n) if n.bytes.1 > 0. => n.bytes,
                _ => i.bytes,
            };
            let step = match live.map(|n| n.state.as_str()) {
                Some("Downloading") => Step::Downloading,
                Some("Staging" | "Committing") => Step::Installing,
                Some(_) => Step::Preparing,
                None if i.paused => Step::Paused,
                None if now.as_ref().is_some_and(|n| n.paused) => Step::Paused,
                None => Step::Waiting,
            };
            Item {
                appid: i.appid,
                name: match m {
                    Some(m) if i.name.is_empty() => m.name.clone(),
                    _ => i.name,
                },
                done: done as u64,
                total: total as u64,
                speed: match (live, step) {
                    (Some(n), Step::Downloading) => n.speed,
                    _ => 0.,
                },
                running: live.is_some(),
                update: m.is_some_and(|m| m.progress.state & 4 != 0),
                step,
                held: i.paused,
            }
        })
        .collect()
}

fn look(speeds: &mut Speeds) -> Vec<Item> {
    let on = steam_on();
    if on && let Some(state) = steam_control::state() {
        return from_steam(state, &lib::manifests());
    }
    let (queue, log) = match lib::root() {
        Some(root) if on => told(&log_tail(&root)),
        _ => Default::default(),
    };
    let now = Instant::now();
    let mut out = Vec::new();
    for m in lib::manifests() {
        let p = m.progress;
        let t = log.get(&m.appid).cloned().unwrap_or_default();
        let running = matches!(
            t.step,
            Step::Preparing | Step::Downloading | Step::Installing
        );
        if !p.unfinished() && !running {
            continue;
        }
        // A part of Steam without anything to fetch, that Steam does not care for now.
        let total = if t.total > 0 { t.total } else { p.total };
        if total == 0 && !running && !queue.contains(&m.appid) && p.state & 4 != 0 {
            continue;
        }
        let done = if running && t.total > 0 {
            let folder = m.path.parent().map(|d| d.join("downloading"));
            match (t.step, folder) {
                (Step::Installing, _) => t.total,
                (_, Some(d)) => live(&t, on_disk(&d.join(m.appid.to_string()))),
                _ => t.from,
            }
        } else if running {
            0
        } else {
            p.done.min(total)
        };
        out.push(Item {
            appid: m.appid,
            name: m.name,
            done,
            total,
            speed: if t.step == Step::Downloading {
                speeds.of(m.appid, done, now)
            } else {
                0.
            },
            running,
            update: p.state & 4 != 0,
            step: match t.step {
                _ if !on => Step::Off,
                _ if !running && p.held() => Step::Paused,
                Step::Off => Step::Waiting,
                s => s,
            },
            held: !running && p.held(),
        });
    }
    speeds
        .0
        .retain(|id, _| out.iter().any(|i| i.appid == *id && i.running));
    // What runs, then as Steam's queue has them.
    out.sort_by_key(|i| {
        let place = queue.iter().position(|q| *q == i.appid);
        (!i.running, place.unwrap_or(usize::MAX))
    });
    out
}

/// Keeps `items` and the list of downloads up to date (once per process).
pub(crate) fn start() {
    let _ = std::thread::Builder::new()
        .name("arkor-steam-downloads".into())
        .spawn(|| {
            steam_control::clean_up();
            let mut speeds = Speeds::default();
            let mut before: Vec<Item> = Vec::new();
            let mut acted = (Vec::new(), None);
            loop {
                // A game that was put first and is there now gets its own setting back, when
                // Steam is closed (it would write over it).
                if !steam_on() {
                    shops::steam_first_over();
                }
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
                // A launch's steps are followed closely.
                let acts = (steam_api::actions(), steam_api::moving());
                let busy = !acts.0.is_empty() || acts.1.is_some();
                if acts != acted || steam_api::has_news() {
                    progress::touch();
                }
                acted = acts;
                std::thread::sleep(Duration::from_millis(match (busy, any) {
                    (true, _) => 400,
                    _ if any || steam_control::on() => 1000,
                    _ => 2500,
                }));
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

    /// Lines as Steam writes them.
    const LOG: &str = "\
[2026-11-25 13:52:00] AppID 999 scheduler update : Priority First, not played for 0 seconds, update disabled for 0 seconds
[2026-11-25 14:21:29] Client version: 1794536281
[2026-11-25 14:21:31] AppID 730 scheduler update : Priority Auto Update, not played for 917923 seconds, update disabled for 0 seconds
[2026-11-25 14:21:31] AppID 730 state changed : Update Required,Fully Installed,Update Queued,Update Started,
[2026-11-25 14:21:31] AppID 228980 scheduler update : Priority Auto Update, not played for 1795605691 seconds, update disabled for 0 seconds
[2026-11-25 14:21:31] AppID 228980 state changed : Update Required,Fully Installed,Update Queued,
[2026-11-25 14:21:31] AppID 1305420 scheduler update : Priority Auto Update, not played for 1795605691 seconds, update disabled for 0 seconds
[2026-11-25 14:21:31] AppID 1305420 state changed : Update Required,Update Queued,Update Started,
[2026-11-25 14:21:31] AppID 730 state changed : Update Required,Fully Installed,Update Queued,Update Running,Update Started,
[2026-11-25 14:21:31] AppID 730 App update changed : Running Update,Reconfiguring,
[2026-11-25 14:21:31] AppID 730 preallocated 0 files (0 MB)
[2026-11-25 14:21:31] AppID 730 update started : download 244471968/320765200, store 0/0, reuse 402060357/430371909, delta 0/0, stage 705200172/931611808
[2026-11-25 14:21:31] AppID 730 App update changed : Running Update,Downloading,Staging,
";

    #[test]
    fn reads_what_steam_does_and_in_which_order() {
        let (queue, t) = told(LOG);
        assert_eq!(
            queue,
            [730, 228980, 1305420],
            "what was before is forgotten"
        );
        let cs = &t[&730];
        assert_eq!(cs.step, Step::Downloading);
        assert_eq!((cs.from, cs.total), (244_471_968, 320_765_200));
        assert_eq!((cs.staged, cs.to_stage), (705_200_172, 931_611_808));
        assert_eq!(t[&1305420].step, Step::Waiting);
        // Done, and the next one.
        let more = format!(
            "{LOG}\
[x] AppID 730 App update changed : Running Update,Committing,
[x] AppID 730 App update changed : None
[x] AppID 730 state changed : Fully Installed,
[x] AppID 730 scheduler finished : removed from schedule (result No Error, state 0xc)
[x] AppID 1305420 scheduler update : Priority First, not played for 1 seconds, update disabled for 0 seconds
[x] AppID 1305420 state changed : Update Required,Update Queued,Update Running,Update Started,
[x] AppID 1305420 App update changed : Running Update,
[x] AppID 1305420 update started : download 0/2180800320, store 0/0, reuse 0/0, delta 0/0, stage 0/8295996582
"
        );
        let (queue, t) = told(&more);
        assert_eq!(queue, [1305420, 228980], "moved to the front");
        assert_eq!(t[&1305420].step, Step::Preparing);
        assert_eq!(t[&1305420].total, 2_180_800_320);
        // Paused in Steam.
        let paused = format!(
            "{more}\
[x] AppID 1305420 App update changed : Running Update,Downloading,Staging,
[x] AppID 1305420 update canceled : Disabled (Suspended)
[x] AppID 1305420 App update changed : Running Update,Stopping,
[x] AppID 1305420 App update changed : None
[x] AppID 1305420 state changed : Update Required,Update Queued,Update Started, (Suspended)
[x] AppID 1305420 scheduler finished : staying in schedule (result Suspended, state 0x40a)
"
        );
        let (queue, t) = told(&paused);
        assert_eq!(t[&1305420].step, Step::Paused);
        assert!(queue.contains(&1305420));
    }

    #[test]
    fn steams_own_word_is_taken() {
        let q = |appid, place, paused| steam_control::Queued {
            appid,
            name: format!("G{appid}"),
            active: place == 0,
            paused,
            place,
            bytes: (10., 100.),
        };
        let state = |state: &str, paused| steam_control::State {
            now: Some(steam_control::Now {
                appid: if paused { 0 } else { 2 },
                state: state.into(),
                speed: 5.,
                paused,
                bytes: (50., 100.),
            }),
            items: vec![q(3, -1, true), q(1, 1, false), q(2, 0, false)],
            happening: Default::default(),
        };
        let items = from_steam(state("Downloading", false), &[]);
        assert_eq!(items.iter().map(|i| i.appid).collect::<Vec<_>>(), [2, 1, 3]);
        assert_eq!(
            (items[0].step, items[0].done, items[0].speed),
            (Step::Downloading, 50, 5.)
        );
        assert_eq!((items[1].step, items[1].running), (Step::Waiting, false));
        assert_eq!((items[2].step, items[2].held), (Step::Paused, true));
        assert_eq!(
            from_steam(state("Committing", false), &[])[0].step,
            Step::Installing
        );
        // Downloading is paused as a whole.
        assert!(
            from_steam(state("None", true), &[])
                .iter()
                .all(|i| i.step == Step::Paused)
        );
    }

    #[test]
    fn one_paused_by_itself_is_paused() {
        let log = "\
[x] AppID 1305420 scheduler update : Priority Paused, not played for 1 seconds, update disabled for 0 seconds
[x] AppID 1305420 state changed : Update Required,Update Paused,Update Started,
";
        assert_eq!(told(log).1[&1305420].step, Step::Paused);
    }

    #[test]
    fn a_running_download_is_measured_by_its_folder() {
        // As Steam had it: 879330703 of 8295996582 put together.
        let t = Told {
            step: Step::Downloading,
            total: 2_180_800_320,
            to_stage: 8_295_996_582,
            ..Told::default()
        };
        let done = live(&t, 879_330_703);
        assert!(
            (done as f64 / 2_180_800_320. - 0.106).abs() < 0.001,
            "{done}"
        );
        assert_eq!(live(&t, u64::MAX / 2), t.total, "never more than all of it");
        // It goes on from where it was.
        let half = Told {
            from: 1000,
            total: 2000,
            staged: 400,
            to_stage: 800,
            ..Told::default()
        };
        assert_eq!((live(&half, 400), live(&half, 600)), (1000, 1500));
        assert_eq!(live(&half, 0), 1000);
        let full = Told {
            preallocated: true,
            ..half
        };
        assert_eq!(live(&full, 800), 1000, "a full folder that says nothing");
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
