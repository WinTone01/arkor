//! Everything Arkor downloads, as one queue.
//!
//! Each downloadable thing has a key: a layer id (`mako`, `afmf`, `vknemu`, `vksumi`),
//! a mod (`optiscaler`, `dlss-enabler`, `reshade`, `streamline`, a bundle key), a "Latest"
//! Proton (`latest:<name>`), a component (`umu`, `runtime`, `anticheat`, `amethyst`) or a store game
//! (`game:<store>:<id>`, `repair:<store>:<id>`). `run` installs
//! or updates one; the queue runs them one after another (store games on a worker of their
//! own, so small downloads do not wait behind them), survives restarts
//! (`~/.local/state/arkor/queue.toml`) and can pause (the partial file is kept and
//! continued), resume, retry and cancel. `needs` lists what a game's settings use but is
//! missing, so it can be fetched as soon as it is switched on, and again before launch.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use arkor_core::Profile;
use serde::{Deserialize, Serialize};

use crate::components::{self, Latest};
use crate::{bundles, dlss_enabler, layers, optiscaler, progress, reshade, shops, streamline};

/// A display name for `key`.
pub fn label(key: &str) -> String {
    if let Some(title) = shops::label(key) {
        return title;
    }
    if let Some(name) = key.strip_prefix("latest:") {
        return name.to_string();
    }
    if let Some(rest) = key.strip_prefix("emu:") {
        let (id, version) = rest.split_once('@').unwrap_or((rest, ""));
        let name = crate::emu::engine(id).map_or(id.to_string(), |m| m.name.clone());
        return if version.is_empty() {
            name
        } else {
            format!("{name} {version}")
        };
    }
    if let Some(core) = key.strip_prefix("core:") {
        let name = crate::emu::engine("retroarch")
            .and_then(|m| m.core(core).map(|c| c.name.clone()))
            .unwrap_or_else(|| core.to_string());
        return format!("RetroArch: {name}");
    }
    if let Some(t) = layers::TOOLS.iter().find(|t| t.id == key) {
        return t.name.to_string();
    }
    if let Some(b) = bundles::Bundle::ALL.iter().find(|b| b.key() == key) {
        return b.name().to_string();
    }
    match key {
        "optiscaler" => "OptiScaler",
        "amethyst" => "Amethyst Mod Manager",
        "dlss-enabler" => "DLSS Enabler",
        "reshade" => "ReShade",
        "streamline" => "Streamline SDK",
        "umu" => "umu-launcher",
        "runtime" => "Steam Linux Runtime",
        "anticheat" => "EasyAntiCheat & BattlEye",
        other => other,
    }
    .to_string()
}

/// Installs or updates `key` to its newest release; returns a short report. Blocking.
pub fn run(key: &str) -> anyhow::Result<String> {
    if shops::parse_key(key).is_some() {
        return shops::run(key);
    }
    if let Some(name) = key.strip_prefix("latest:") {
        let l = Latest::from_value(name).with_context(|| format!("unknown Proton {name}"))?;
        components::ensure_latest(l, true)?;
        return Ok(format!(
            "{name} {}",
            l.installed_version().unwrap_or_default()
        ));
    }
    if let Some((id, version)) = key.strip_prefix("emu:").and_then(|k| k.split_once('@')) {
        return crate::emu::install::install_version(id, version);
    }
    if let Some(id) = key.strip_prefix("emu:") {
        return crate::emu::install::install(id);
    }
    if let Some(core) = key.strip_prefix("core:") {
        let date = crate::emu::retroarch::install_core(core)?;
        return Ok(format!("{} ({date})", label(key)));
    }
    if let Some(tool) = layers::TOOLS.iter().find(|t| t.id == key) {
        return layers::install(tool).map(|v| format!("{} {v}", tool.name));
    }
    if let Some(b) = bundles::Bundle::ALL.iter().find(|b| b.key() == key) {
        let (v, dir) = bundles::download(*b)?;
        prune(bundles::downloaded(*b), &dir)?;
        return Ok(format!("{} {v}", b.name()));
    }
    let (name, version, kept, all) = match key {
        "optiscaler" => {
            let r = optiscaler::latest_release()?;
            let dir = optiscaler::download(&r)?;
            ("OptiScaler", r.tag, dir, optiscaler::downloaded())
        }
        "dlss-enabler" => {
            let builds = dlss_enabler::builds()?;
            let b = builds
                .iter()
                .find(|b| b.stable)
                .or(builds.first())
                .context("no DLSS Enabler build found")?;
            let dir = dlss_enabler::download(b)?;
            (
                "DLSS Enabler",
                b.version.clone(),
                dir,
                dlss_enabler::downloaded(),
            )
        }
        "reshade" => {
            let v = reshade::latest_version()?;
            let dir = reshade::download(&v)?;
            ("ReShade", v, dir, reshade::downloaded())
        }
        "streamline" => {
            let v = streamline::latest_version()?;
            let dir = streamline::download(&v)?;
            ("Streamline SDK", v, dir, streamline::downloaded())
        }
        "amethyst" => {
            return Ok(format!(
                "Amethyst Mod Manager {}",
                crate::mods::amethyst::ensure(true)?
            ));
        }
        "umu" => {
            components::ensure_umu(true)?;
            return Ok(format!(
                "umu-launcher {}",
                components::umu_version().unwrap_or_default()
            ));
        }
        "runtime" => {
            components::ensure_runtime(true)?;
            return Ok(format!(
                "Steam Linux Runtime {}",
                components::runtime_version().unwrap_or_default()
            ));
        }
        "anticheat" => {
            components::ensure_anticheat(true)?;
            return Ok(format!(
                "EasyAntiCheat & BattlEye {}",
                components::anticheat_version().unwrap_or_default()
            ));
        }
        other => anyhow::bail!("nothing to download for {other}"),
    };
    prune(all, &kept)?;
    Ok(format!("{name} {version}"))
}

/// Removes older downloaded copies; never `kept` (compared by folder, so a copy named
/// differently from its version is safe).
fn prune(all: Vec<(String, PathBuf)>, kept: &Path) -> anyhow::Result<()> {
    let kept = std::fs::canonicalize(kept).unwrap_or(kept.to_path_buf());
    let root = dirs::data_dir().context("no XDG data dir")?.join("arkor");
    for (_, dir) in all {
        let same = std::fs::canonicalize(&dir).unwrap_or(dir.clone());
        if same != kept
            && !kept.starts_with(&same)
            && !same.starts_with(&kept)
            && same.starts_with(&root)
        {
            arkor_core::fs_guard::remove_dir_in(&dir, &root)?;
        }
    }
    Ok(())
}

/// What `profile` switches on but is not installed: (key, label).
pub fn needs(profile: &Profile) -> Vec<(String, String)> {
    let l = &profile.layers;
    let mut out = Vec::new();
    let mut need = |on: bool, key: &str, enable: &str| {
        if on && !layers::present(enable) {
            out.push((key.to_string(), label(key)));
        }
    };
    need(l.mako.enabled || l.mako.scaling, "mako", "ENABLE_MAKO");
    need(l.afmf.enabled, "afmf", "AFMF_ENABLE");
    need(l.vknemu.enabled, "vknemu", "VKNEMU");
    need(l.vksumi, "vksumi", "ENABLE_VKSUMI");
    if let Some(latest) = profile.proton.as_deref().and_then(Latest::from_value)
        && latest.installed_version().is_none()
    {
        let key = format!("latest:{}", latest.dir_name());
        out.push((key.clone(), label(&key)));
    }
    out
}

/// Installs what `profile` needs right away (before a game starts). Problems are printed;
/// the game still starts. Blocking.
pub fn ensure_needs(profile: &Profile) {
    for (key, name) in needs(profile) {
        eprintln!("arkor: installing {name}, which this game uses");
        if let Err(e) = run(&key) {
            eprintln!("arkor: {name} not installed: {e:#}");
        }
    }
}

/// A newer version of something already downloaded.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub key: String,
    pub label: String,
    pub installed: String,
    pub latest: String,
}

/// Looks for newer versions of everything installed that follows the newest release.
/// Failed lookups are skipped. Blocking (one request per item).
pub fn check_updates() -> Vec<Update> {
    let mut out = Vec::new();
    let mut add = |key: String,
                   installed: Option<String>,
                   latest: anyhow::Result<String>,
                   same: &dyn Fn(&str, &str) -> bool| {
        if let (Some(installed), Ok(latest)) = (installed, latest)
            && !same(&installed, &latest)
        {
            out.push(Update {
                label: label(&key),
                key,
                installed,
                latest,
            });
        }
    };
    let exact = |a: &str, b: &str| a.trim_start_matches('v') == b.trim_start_matches('v');
    for l in Latest::ALL {
        if let Some(v) = l.installed_version() {
            add(
                format!("latest:{}", l.dir_name()),
                Some(v),
                components::latest_tag(l),
                &components::same_version,
            );
        }
    }
    add(
        "umu".into(),
        components::umu_version(),
        components::umu_latest(),
        &exact,
    );
    add(
        "amethyst".into(),
        crate::mods::amethyst::installed_version(),
        crate::mods::amethyst::latest_version(),
        &exact,
    );
    add(
        "anticheat".into(),
        components::anticheat_version(),
        components::anticheat_latest(),
        &exact,
    );
    for t in &layers::TOOLS {
        if layers::arkor_installed(t.id) {
            add(
                t.id.into(),
                layers::installed_version(t.id),
                layers::latest_version(t),
                &exact,
            );
        }
    }
    let newest = |list: Vec<(String, PathBuf)>| list.into_iter().next().map(|(v, _)| v);
    if let Some(v) = newest(optiscaler::downloaded()) {
        add(
            "optiscaler".into(),
            Some(v),
            optiscaler::latest_release().map(|r| r.tag),
            &exact,
        );
    }
    if let Some(v) = newest(reshade::downloaded()) {
        add("reshade".into(), Some(v), reshade::latest_version(), &exact);
    }
    if let Some(v) = newest(streamline::downloaded()) {
        add(
            "streamline".into(),
            Some(v),
            streamline::latest_version(),
            &exact,
        );
    }
    if let Some(v) = newest(dlss_enabler::downloaded()) {
        let latest = dlss_enabler::builds().and_then(|b| {
            b.iter()
                .find(|b| b.stable)
                .or(b.first())
                .map(|b| b.version.clone())
                .context("no build")
        });
        add("dlss-enabler".into(), Some(v), latest, &|a, b| {
            a == dlss_enabler::folder_name(b) || exact(a, b)
        });
    }
    for b in bundles::Bundle::ALL {
        if let Some(v) = newest(bundles::downloaded(b)) {
            add(
                b.key().into(),
                Some(v),
                bundles::latest(b).map(|(v, _)| v),
                &exact,
            );
        }
    }
    for (id, installed, latest) in crate::emu::install::updates() {
        let key = format!("emu:{id}");
        out.push(Update {
            label: label(&key),
            key,
            installed,
            latest,
        });
    }
    for core in crate::emu::retroarch::outdated_cores() {
        let key = format!("core:{core}");
        out.push(Update {
            label: label(&key),
            key,
            installed: String::new(),
            latest: String::new(),
        });
    }
    out.extend(shops::updates().into_iter().map(|(g, _)| Update {
        key: shops::key(g.shop, &g.id),
        label: g.title,
        // Stores name builds by numbers that say nothing.
        installed: String::new(),
        latest: String::new(),
    }));
    out
}

// ---- The queue ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Queued,
    Paused,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub key: String,
    pub label: String,
    pub state: State,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    item: Vec<Item>,
}

static QUEUE: Mutex<Vec<Item>> = Mutex::new(Vec::new());
static WORKER: std::sync::Once = std::sync::Once::new();

fn queue_file() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    Some(dirs::state_dir()?.join("arkor/queue.toml"))
}

fn with<R>(f: impl FnOnce(&mut Vec<Item>) -> R) -> R {
    let mut q = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    let r = f(&mut q);
    if let Some(path) = queue_file() {
        let _ = path.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(
            &path,
            toml::to_string(&Saved { item: q.clone() }).unwrap_or_default(),
        );
    }
    drop(q);
    progress::touch();
    r
}

/// What is waiting, paused or failed (the running one is in `progress::list`).
pub fn items() -> Vec<Item> {
    QUEUE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Whether `key` is waiting or running.
pub fn pending(key: &str) -> bool {
    progress::running(key)
        || items()
            .iter()
            .any(|i| i.key == key && i.state == State::Queued)
}

/// Keys the user wanted to go (on) in this session, not only loaded from the saved queue.
static ASKED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether the user asked for `key` in this session; asked once, answered once.
pub(crate) fn asked_to_go_on(key: &str) -> bool {
    let mut asked = ASKED.lock().unwrap_or_else(|e| e.into_inner());
    let before = asked.len();
    asked.retain(|k| k != key);
    asked.len() != before
}

/// Moves a waiting `key` one place up or down among those waiting.
pub fn shift(key: &str, up: bool) {
    with(|q| shift_in(q, key, up, progress::running));
}

fn shift_in(q: &mut [Item], key: &str, up: bool, running: impl Fn(&str) -> bool) {
    let waiting: Vec<usize> = (0..q.len()).filter(|i| !running(&q[*i].key)).collect();
    let Some(at) = waiting.iter().position(|i| q[*i].key == key) else {
        return;
    };
    let to = if up { at.checked_sub(1) } else { Some(at + 1) };
    if let Some(to) = to.and_then(|t| waiting.get(t)) {
        q.swap(waiting[at], *to);
    }
}

/// Adds `key` to the queue (a paused or failed one starts again). Already there: no-op.
pub fn enqueue(key: &str) {
    ASKED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(key.to_string());
    with(|q| match q.iter_mut().find(|i| i.key == key) {
        Some(i) => i.state = State::Queued,
        None => q.push(Item {
            key: key.to_string(),
            label: label(key),
            state: State::Queued,
        }),
    });
}

/// Stops `key` and keeps it for later (its partial download is kept).
pub fn pause(key: &str) {
    with(|q| {
        if let Some(i) = q.iter_mut().find(|i| i.key == key) {
            i.state = State::Paused;
        }
    });
    progress::stop(key);
}

pub fn resume(key: &str) {
    enqueue(key);
}

/// Removes `key` from the queue, stopping it when it runs.
pub fn cancel(key: &str) {
    with(|q| q.retain(|i| i.key != key));
    if progress::running(key) {
        // The worker cleans up when the job has ended.
        progress::stop(key);
    } else if shops::parse_key(key).is_some() {
        let key = key.to_string();
        std::thread::spawn(move || shops::cancelled(&key));
    }
}

/// The end of one queue item.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub key: String,
    /// What it was, e.g. "OptiScaler".
    pub label: String,
    pub ok: bool,
    pub report: String,
    /// When it ended (Unix seconds).
    pub at: u64,
}

static HISTORY: Mutex<Vec<Finished>> = Mutex::new(Vec::new());

/// Queue items that ended in this session, newest first (failures included).
pub fn history() -> Vec<Finished> {
    HISTORY.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Another program's download ended well.
pub(crate) fn note_done(key: &str, label: &str, report: &str) {
    let mut history = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    history.retain(|f| f.key != key);
    history.insert(
        0,
        Finished {
            key: key.to_string(),
            label: label.to_string(),
            ok: true,
            report: report.to_string(),
            at: arkor_core::stats::now(),
        },
    );
    drop(history);
    progress::touch();
}

/// Forgets the finished list.
pub fn clear_history() {
    HISTORY.lock().unwrap_or_else(|e| e.into_inner()).clear();
    progress::touch();
}

/// Moves `key` to the front of the queue, so it is next.
pub fn move_to_front(key: &str) {
    with(|q| {
        if let Some(pos) = q.iter().position(|i| i.key == key) {
            let item = q.remove(pos);
            q.insert(0, item);
        }
    });
}

/// How many queue items have ended, and the last one (for the UI to notice). Paused or
/// cancelled items count, without a report.
pub fn finished() -> (u64, Option<Finished>) {
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone();
    (FINISHED.load(Ordering::Relaxed), last)
}

static FINISHED: AtomicU64 = AtomicU64::new(0);
static LAST: Mutex<Option<Finished>> = Mutex::new(None);

/// Loads the saved queue and starts the worker (once per process).
pub fn start() {
    WORKER.call_once(|| {
        if let Some(saved) = queue_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| toml::from_str::<Saved>(&t).ok())
        {
            with(|q| *q = saved.item);
        }
        crate::steam_downloads::start();
        // Store games take hours: the small downloads must not wait behind them.
        for games in [false, true] {
            let _ = std::thread::Builder::new()
                .name("arkor-downloads".into())
                .spawn(move || worker(games));
        }
    });
}

fn worker(games: bool) {
    loop {
        let next = QUEUE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|i| {
                i.state == State::Queued
                    && shops::parse_key(&i.key).is_some() == games
                    && !progress::running(&i.key)
            })
            .cloned();
        let Some(item) = next else {
            std::thread::sleep(std::time::Duration::from_millis(400));
            continue;
        };
        let result = {
            let _job = progress::job(&item.key, &item.label);
            run(&item.key)
        };
        let mut cancelled = false;
        let report = with(|q| {
            let pos = q.iter().position(|i| i.key == item.key);
            match (&result, pos) {
                (Ok(msg), Some(pos)) => {
                    q.remove(pos);
                    Some((true, msg.clone()))
                }
                (Ok(msg), None) => Some((true, msg.clone())),
                // Paused or cancelled meanwhile: nothing to report.
                (Err(_), Some(pos)) if q[pos].state == State::Paused => None,
                (Err(_), None) => {
                    cancelled = true;
                    None
                }
                (Err(e), Some(pos)) => {
                    let msg = format!("{e:#}");
                    q[pos].state = State::Failed(msg.clone());
                    Some((false, format!("{}: {msg}", item.label)))
                }
            }
        });
        if cancelled {
            shops::cancelled(&item.key);
        }
        let finished = report.map(|(ok, report)| Finished {
            key: item.key.clone(),
            label: item.label.clone(),
            ok,
            report,
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        if let Some(f) = &finished {
            let mut h = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
            h.retain(|old| old.key != f.key);
            h.insert(0, f.clone());
            h.truncate(30);
        }
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = finished;
        FINISHED.fetch_add(1, Ordering::Relaxed);
        progress::touch();
    }
}

/// Retries a failed item.
pub fn retry(key: &str) {
    enqueue(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_downloads_change_places() {
        let mut q: Vec<Item> = ["run", "a", "b", "c"]
            .map(|k| Item {
                key: k.into(),
                label: k.into(),
                state: State::Queued,
            })
            .into();
        let running = |k: &str| k == "run";
        let keys = |q: &[Item]| q.iter().map(|i| i.key.clone()).collect::<Vec<_>>();
        shift_in(&mut q, "c", true, running);
        assert_eq!(keys(&q), ["run", "a", "c", "b"]);
        shift_in(&mut q, "a", true, running);
        assert_eq!(
            keys(&q),
            ["run", "a", "c", "b"],
            "not over the one that runs"
        );
        shift_in(&mut q, "a", false, running);
        shift_in(&mut q, "b", false, running);
        shift_in(&mut q, "run", false, running);
        assert_eq!(keys(&q), ["run", "c", "a", "b"]);
    }

    #[test]
    fn labels_and_needs() {
        assert_eq!(label("latest:Proton-GE Latest"), "Proton-GE Latest");
        assert_eq!(label("mako"), "MAKO");
        assert_eq!(label("optiscaler"), "OptiScaler");
        let p = Profile::default();
        assert!(needs(&p).is_empty(), "nothing switched on, nothing needed");
    }

    /// Network: lists what has a newer version in the current data folder.
    #[test]
    #[ignore]
    fn checks_for_updates() {
        for u in check_updates() {
            println!("{} {} -> {}", u.label, u.installed, u.latest);
        }
    }

    #[test]
    fn queue_states() {
        enqueue("test-item");
        assert!(
            items()
                .iter()
                .any(|i| i.key == "test-item" && i.state == State::Queued)
        );
        enqueue("test-item");
        assert_eq!(
            items().iter().filter(|i| i.key == "test-item").count(),
            1,
            "no duplicates"
        );
        pause("test-item");
        assert!(
            items()
                .iter()
                .any(|i| i.key == "test-item" && i.state == State::Paused)
        );
        assert!(!pending("test-item"));
        resume("test-item");
        assert!(pending("test-item"));
        cancel("test-item");
        assert!(!items().iter().any(|i| i.key == "test-item"));
    }
}
