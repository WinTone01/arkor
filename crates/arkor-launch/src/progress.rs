//! What is being downloaded right now, for progress bars in the UI.
//!
//! A job (`job`) is one thing the user asked for, e.g. "OptiScaler": it stays listed from
//! the release lookup to the unpacking. `tools::download` reports bytes into the job of its
//! thread; downloads outside a job (Proton builds, umu, anti-cheat) get an entry of their
//! own for as long as the transfer runs. Everything is in-process state.

use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Looking up the release, before bytes flow.
    Preparing,
    Downloading,
    /// Unpacking or installing after the transfer.
    Finishing,
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// Stable id the UI asks about (a feature id like "optiscaler", or a file name).
    pub key: String,
    /// What to show, e.g. "OptiScaler" or "GE-Proton10-25.tar.gz".
    pub label: String,
    pub phase: Phase,
    pub done: u64,
    pub total: Option<u64>,
    /// Bytes per second, averaged since the transfer began.
    pub speed: f64,
    started: Option<Instant>,
    /// Asked to stop (pause or cancel); the transfer ends at its next check.
    stop: bool,
}

impl Entry {
    /// Share done in 0..=1, when the size is known.
    pub fn fraction(&self) -> Option<f32> {
        self.total
            .filter(|t| *t > 0)
            .map(|t| (self.done as f32 / t as f32).clamp(0., 1.))
    }

    /// Seconds left at the current speed, when it can be told.
    pub fn eta_secs(&self) -> Option<u64> {
        let total = self.total?;
        (self.speed > 1.).then(|| (total.saturating_sub(self.done) as f64 / self.speed) as u64)
    }
}

static ENTRIES: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
/// Bumped on every change, so the UI redraws only when something moved.
static GENERATION: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static CURRENT: RefCell<Option<String>> = const { RefCell::new(None) };
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` with its downloads left out of the list (small files such as previews).
pub fn quietly<R>(f: impl FnOnce() -> R) -> R {
    let before = QUIET.with(|q| q.replace(true));
    let r = f();
    QUIET.with(|q| q.set(before));
    r
}

fn with<R>(f: impl FnOnce(&mut Vec<Entry>) -> R) -> R {
    let mut guard = ENTRIES.lock().unwrap_or_else(|e| e.into_inner());
    let r = f(&mut guard);
    GENERATION.fetch_add(1, Ordering::Relaxed);
    r
}

/// Everything in progress, oldest first.
pub fn list() -> Vec<Entry> {
    ENTRIES.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Marks a change elsewhere (e.g. the download queue) so the UI redraws.
pub fn touch() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Asks the running job `key` to stop (its download ends; a partial file is kept, so the
/// next download of the same file continues where it stopped).
pub fn stop(key: &str) {
    with(|v| {
        if let Some(e) = v.iter_mut().find(|e| e.key == key) {
            e.stop = true;
        }
    });
}

/// Whether the job `key` is running.
pub fn running(key: &str) -> bool {
    ENTRIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|e| e.key == key)
}

/// Whether this thread's job was asked to stop.
pub(crate) fn stop_requested() -> bool {
    let Some(key) = CURRENT.with(|c| c.borrow().clone()) else {
        return false;
    };
    ENTRIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|e| e.key == key && e.stop)
}

/// Listed while alive; removes its entry when dropped.
pub struct Job {
    key: String,
    previous: Option<String>,
}

impl Drop for Job {
    fn drop(&mut self) {
        let key = std::mem::take(&mut self.key);
        with(|v| v.retain(|e| e.key != key));
        CURRENT.with(|c| *c.borrow_mut() = self.previous.take());
    }
}

/// Starts job `key` on this thread: downloads made by this thread report into it.
pub fn job(key: &str, label: &str) -> Job {
    with(|v| {
        v.retain(|e| e.key != key);
        v.push(Entry {
            key: key.to_string(),
            label: label.to_string(),
            phase: Phase::Preparing,
            done: 0,
            total: None,
            speed: 0.,
            started: None,
            stop: false,
        });
    });
    let previous = CURRENT.with(|c| c.borrow_mut().replace(key.to_string()));
    Job {
        key: key.to_string(),
        previous,
    }
}

/// One transfer: reports into the thread's job, or into an entry of its own.
pub(crate) struct Transfer {
    key: String,
    own: bool,
    started: Instant,
}

impl Transfer {
    pub(crate) fn begin(label: &str, total: Option<u64>) -> Transfer {
        let current = CURRENT.with(|c| c.borrow().clone());
        let started = Instant::now();
        if QUIET.with(|q| q.get()) {
            // Not listed: updates and the drop find no entry.
            return Transfer {
                key: String::new(),
                own: false,
                started,
            };
        }
        match current {
            Some(key) => {
                with(|v| {
                    if let Some(e) = v.iter_mut().find(|e| e.key == key) {
                        e.phase = Phase::Downloading;
                        e.done = 0;
                        e.total = total;
                        e.speed = 0.;
                        e.started = Some(started);
                    }
                });
                Transfer {
                    key,
                    own: false,
                    started,
                }
            }
            None => {
                let key = format!("download:{label}");
                with(|v| {
                    v.push(Entry {
                        key: key.clone(),
                        label: label.to_string(),
                        phase: Phase::Downloading,
                        done: 0,
                        total,
                        speed: 0.,
                        started: Some(started),
                        stop: false,
                    })
                });
                Transfer {
                    key,
                    own: true,
                    started,
                }
            }
        }
    }

    /// Reports the state as another program measured it; `false` when the job was asked to
    /// stop. `speed` in bytes per second.
    pub(crate) fn report(&self, done: u64, total: Option<u64>, speed: f64) -> bool {
        with(|v| match v.iter_mut().find(|e| e.key == self.key) {
            Some(e) => {
                e.done = done;
                e.total = total.or(e.total);
                e.speed = speed;
                !e.stop
            }
            None => true,
        })
    }

    /// Reports `done` bytes; `false` when the job was asked to stop.
    pub(crate) fn update(&self, done: u64) -> bool {
        let secs = self.started.elapsed().as_secs_f64().max(0.001);
        with(|v| match v.iter_mut().find(|e| e.key == self.key) {
            Some(e) => {
                e.done = done;
                e.speed = done as f64 / secs;
                !e.stop
            }
            None => true,
        })
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        let key = self.key.clone();
        if self.own {
            with(|v| v.retain(|e| e.key != key));
        } else {
            with(|v| {
                if let Some(e) = v.iter_mut().find(|e| e.key == key) {
                    e.phase = Phase::Finishing;
                }
            });
        }
    }
}

/// "1.4 GB", "820 MB", "12 KB".
pub fn human_bytes(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.2} GB", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1} MB", n / 1e6)
    } else {
        format!("{:.0} KB", n / 1e3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_collects_its_transfer_and_ends_with_it() {
        {
            let _job = job("test-tool", "Test tool");
            assert!(running("test-tool"));
            {
                let t = Transfer::begin("file.zip", Some(1000));
                t.update(250);
                let e = list().into_iter().find(|e| e.key == "test-tool").unwrap();
                assert_eq!(
                    (e.phase, e.done, e.fraction()),
                    (Phase::Downloading, 250, Some(0.25))
                );
            }
            let e = list().into_iter().find(|e| e.key == "test-tool").unwrap();
            assert_eq!(e.phase, Phase::Finishing, "still listed while unpacking");
            assert!(!list().iter().any(|e| e.key == "download:file.zip"));
        }
        assert!(!running("test-tool"));
        // Outside a job a transfer lists itself.
        let t = Transfer::begin("proton.tar.gz", None);
        assert!(running("download:proton.tar.gz"));
        drop(t);
        assert!(!running("download:proton.tar.gz"));
    }
}
