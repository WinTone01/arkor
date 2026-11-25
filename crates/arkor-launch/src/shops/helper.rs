//! The helper programs that talk to the stores (the ones Heroic uses, too): legendary for
//! Epic, gogdl for GOG, nile for Amazon. A fixed version of each is downloaded from its
//! GitHub releases and checked against a known checksum.

use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Context;

use super::Shop;
use crate::progress::{self, Transfer};
use crate::tools;

struct Helper {
    name: &'static str,
    version: &'static str,
    url: &'static str,
    sha256: &'static str,
}

const fn x86(shop: Shop) -> Helper {
    match shop {
        Shop::Epic => Helper {
            name: "legendary",
            version: "0.21.1",
            url: "https://github.com/legendary-gl/legendary/releases/download/0.21.1/legendary_linux_x64",
            sha256: "dbed33bbe96031e65858233e4badf0a1d401bb6cb0cc995d1237cf3a0c826a45",
        },
        Shop::Gog => Helper {
            name: "gogdl",
            version: "1.3.0",
            url: "https://github.com/Heroic-Games-Launcher/heroic-gogdl/releases/download/v1.3.0/gogdl_linux_x86_64",
            sha256: "cba013d42767c808237c437335ab1d56f58405d07e8f37b3324d264ea5c49655",
        },
        Shop::Amazon => Helper {
            name: "nile",
            version: "1.2.0",
            url: "https://github.com/imLinguin/nile/releases/download/v1.2.0/nile_linux_x86_64",
            sha256: "d73519514df9d52c50c6feec629373cc3a0d9504e087b6c7372a8ca8a95e7bfe",
        },
        // Never used: Steam games go through the Steam client.
        Shop::Steam => Helper {
            name: "steam",
            version: "",
            url: "",
            sha256: "",
        },
    }
}

/// The helper's file name in Arkor's folder, and the version marker beside it.
fn installed(shop: Shop) -> anyhow::Result<(PathBuf, PathBuf)> {
    let h = x86(shop);
    let dir = arkor_core::paths::stores_dir()?.join("bin");
    Ok((dir.join(h.name), dir.join(format!("{}.version", h.name))))
}

fn sha256(path: &Path) -> anyhow::Result<String> {
    let out = Command::new("sha256sum")
        .arg(path)
        .output()
        .context("running sha256sum")?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string())
}

/// The store's helper, downloading Arkor's pinned version when it is missing or another one.
/// Blocking.
pub fn ensure(shop: Shop) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(shop != Shop::Steam, "Steam needs no helper");
    // Tests put their own stand-in here.
    if let Some(dir) = std::env::var_os("ARKOR_STORE_HELPERS") {
        return Ok(PathBuf::from(dir).join(x86(shop).name));
    }
    anyhow::ensure!(
        cfg!(target_arch = "x86_64"),
        "store downloads need an x86-64 machine"
    );
    let h = x86(shop);
    let (bin, marker) = installed(shop)?;
    if bin.is_file() && std::fs::read_to_string(&marker).is_ok_and(|v| v.trim() == h.version) {
        return Ok(bin);
    }
    let fresh = bin.with_extension("new");
    progress::quietly(|| tools::download(h.url, &fresh))
        .with_context(|| format!("downloading {}", h.name))?;
    let got = sha256(&fresh)?;
    if got != h.sha256 {
        let _ = std::fs::remove_file(&fresh);
        anyhow::bail!("{} has an unexpected checksum; not used", h.name);
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&fresh, &bin)?;
    std::fs::write(&marker, h.version)?;
    Ok(bin)
}

/// A command of the store's helper with its config folder set. Nothing is inherited that
/// could point it at another launcher's login.
pub fn command(shop: Shop) -> anyhow::Result<Command> {
    let bin = ensure(shop)?;
    let dir = super::shop_dir(shop)?;
    super::vault::settle(shop);
    let mut cmd = Command::new(bin);
    match shop {
        Shop::Epic => {
            cmd.env("LEGENDARY_CONFIG_PATH", &dir);
        }
        Shop::Gog => {
            cmd.env("GOGDL_CONFIG_PATH", &dir)
                .arg("--auth-config-path")
                .arg(dir.join("auth.json"));
        }
        Shop::Amazon => {
            cmd.env("NILE_CONFIG_PATH", &dir);
        }
        Shop::Steam => {}
    }
    cmd.env("PYTHONUNBUFFERED", "1").stdin(Stdio::null());
    Ok(cmd)
}

/// What a finished helper printed.
pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// The most telling line of what went wrong.
    pub fn error(&self) -> String {
        error_line(&self.stderr)
            .or_else(|| error_line(&self.stdout))
            .unwrap_or_else(|| "the store helper failed".into())
    }

    /// The JSON on stdout (the helpers log to stderr).
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> anyhow::Result<T> {
        // gogdl and nile may print notes before the document.
        let text = self.stdout.trim();
        let start = text.find(['{', '[']).unwrap_or(0);
        serde_json::from_str(&text[start..]).map_err(|_| anyhow::anyhow!("{}", self.error()))
    }
}

/// The last error in a helper's log, without its logger prefix.
fn error_line(text: &str) -> Option<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        // PyInstaller's "unhandled exception" says less than the exception before it.
        .filter(|l| !l.is_empty() && !l.starts_with("[PYI-"))
        .collect();
    let pick = lines
        .iter()
        .rev()
        .find(|l| l.contains("ERROR") || l.contains("CRITICAL") || l.contains("FATAL"))
        .or_else(|| {
            lines
                .iter()
                .rev()
                .find(|l| l.contains("Error:") || l.contains("Exception:"))
        })
        .or(lines.last())?;
    // "[cli] ERROR: text" and "ERROR [CLI]:\t text".
    let text = match pick
        .split_once("ERROR: ")
        .or_else(|| pick.split_once("CRITICAL: "))
    {
        Some((_, rest)) => rest,
        None if pick.starts_with("ERROR [") || pick.starts_with("CRITICAL [") => {
            pick.split_once("]:").map_or(*pick, |(_, rest)| rest)
        }
        None => pick,
    };
    Some(text.trim().to_string()).filter(|t| !t.is_empty())
}

/// Runs a helper to its end (logins, lists, information). Blocking.
pub fn output(mut cmd: Command) -> anyhow::Result<Output> {
    let out = cmd.output().context("running the store helper");
    // The helper may have renewed the login.
    super::vault::settle_all();
    let out = out?;
    Ok(Output {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// As [`output`], ended after `patience` or when `stop` is set. Blocking.
pub fn output_within(
    mut cmd: Command,
    patience: Duration,
    stop: &AtomicBool,
) -> anyhow::Result<Output> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .context("running the store helper")?;
    let pipes: [Option<Box<dyn Read + Send>>; 2] = [
        child.stdout.take().map(|p| Box::new(p) as _),
        child.stderr.take().map(|p| Box::new(p) as _),
    ];
    let readers = pipes.map(|pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    });
    let began = Instant::now();
    let group = -(child.id() as i32);
    let ended = loop {
        if let Some(status) = child.try_wait()? {
            break Ok(status);
        }
        let stopped = stop.load(Ordering::Relaxed);
        if stopped || began.elapsed() > patience {
            // SAFETY: plain syscall on the helper's own process group.
            unsafe { libc::kill(group, libc::SIGKILL) };
            let _ = child.wait();
            break Err(anyhow::anyhow!(if stopped {
                tools::STOPPED
            } else {
                "the store did not answer in time"
            }));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    super::vault::settle_all();
    let [stdout, stderr] = readers.map(|r| r.join().unwrap_or_default());
    Ok(Output {
        ok: ended?.success(),
        stdout,
        stderr,
    })
}

/// How a download stands, read from a helper's log.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct LogState {
    /// Percent done (0..=100).
    pub percent: Option<f64>,
    /// Bytes per second from the network.
    pub speed: Option<f64>,
    /// Bytes this run has to download.
    pub size: Option<u64>,
}

const MIB: f64 = 1024. * 1024.;

/// Reads one log line (all three helpers report alike).
pub fn parse_line(line: &str) -> LogState {
    let number = |after: &str| -> Option<f64> {
        let rest = &line[line.find(after)? + after.len()..];
        let rest = rest.trim_start();
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(rest.len());
        rest[..end].parse().ok()
    };
    LogState {
        percent: number("= Progress:"),
        speed: number("Download\t-").map(|m| m * MIB),
        size: number("Download size:").map(|m| m * MIB).map(|b| b as u64),
    }
}

/// Runs a download of a helper, reporting into this thread's job. `total` is the size known
/// beforehand. Ends it (and everything it started) when the job is stopped. Blocking.
pub fn run(mut cmd: Command, label: &str, total: Option<u64>) -> anyhow::Result<Output> {
    if progress::stop_requested() {
        anyhow::bail!(tools::STOPPED);
    }
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // SAFETY: only an async-signal-safe syscall between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            // The helper must not outlive Arkor: two of them on one game would fight.
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let mut child = cmd.spawn().context("starting the store helper")?;
    let group = -(child.id() as i32);
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    let mut readers = Vec::new();
    let pipes: [(bool, Option<Box<dyn Read + Send>>); 2] = [
        (false, child.stdout.take().map(|p| Box::new(p) as _)),
        (true, child.stderr.take().map(|p| Box::new(p) as _)),
    ];
    for (err, pipe) in pipes {
        let Some(pipe) = pipe else { continue };
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut buf = Vec::new();
            while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                let _ = tx.send((err, String::from_utf8_lossy(&buf).trim_end().to_string()));
                buf.clear();
            }
        }));
    }
    drop(tx);

    let transfer = Transfer::begin(label, total);
    let mut state = LogState::default();
    let (mut stdout, mut stderr) = (Tail::default(), Tail::default());
    let mut stopping: Option<Instant> = None;
    let mut settled = Instant::now();
    let status = loop {
        while let Ok((err, line)) = rx.try_recv() {
            let read = parse_line(&line);
            state.percent = read.percent.or(state.percent);
            state.speed = read.speed.or(state.speed);
            state.size = read.size.or(state.size);
            if err {
                stderr.push(line);
            } else {
                stdout.push(line);
            }
        }
        let total = state.size.filter(|s| *s > 0).or(total).filter(|t| *t > 0);
        let done = total.map_or(0., |t| t as f64 * state.percent.unwrap_or(0.) / 100.);
        let go_on = transfer.report(done as u64, total, state.speed.unwrap_or(0.));
        if let Some(status) = child.try_wait()? {
            break status;
        }
        match stopping {
            None if !go_on => {
                // SAFETY: plain syscall on the helper's own process group.
                unsafe { libc::kill(group, libc::SIGTERM) };
                stopping = Some(Instant::now());
            }
            Some(at) if at.elapsed() > Duration::from_secs(8) => {
                // SAFETY: as above.
                unsafe { libc::kill(group, libc::SIGKILL) };
            }
            _ => {}
        }
        // The login is renewed at the start: not left unsealed for a long download.
        if settled.elapsed() > Duration::from_secs(20) {
            super::vault::settle_all();
            settled = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(150));
    };
    super::vault::settle_all();
    if stopping.is_some() {
        // Workers the helper left behind.
        // SAFETY: as above.
        unsafe { libc::kill(group, libc::SIGKILL) };
        anyhow::bail!(tools::STOPPED);
    }
    for r in readers {
        let _ = r.join();
    }
    while let Ok((err, line)) = rx.try_recv() {
        if err {
            stderr.push(line);
        } else {
            stdout.push(line);
        }
    }
    Ok(Output {
        ok: status.success(),
        stdout: stdout.text(),
        stderr: stderr.text(),
    })
}

/// The last lines of a log.
#[derive(Default)]
struct Tail(std::collections::VecDeque<String>);

impl Tail {
    fn push(&mut self, line: String) {
        // Progress lines would push the interesting ones out.
        if line.contains("= Progress:") || line.contains("\t- ") || line.contains("Downloaded:") {
            return;
        }
        self.0.push_back(line);
        while self.0.len() > 200 {
            self.0.pop_front();
        }
    }

    fn text(self) -> String {
        Vec::from(self.0).join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_progress_of_every_helper() {
        let l = parse_line(
            "[DLManager] INFO: = Progress: 12.50% (10/80), Running for 00:00:05, ETA: 00:00:30",
        );
        assert_eq!(l.percent, Some(12.5));
        let g = parse_line("[PROGRESS] INFO: = Progress: 3.07 300/9771, Running for: 00:00:01");
        assert_eq!(g.percent, Some(3.07));
        let s = parse_line("[DLManager] INFO:  + Download\t- 10.50 MiB/s (raw) / 12 MiB/s");
        assert_eq!(s.speed, Some(10.5 * MIB));
        let n = parse_line("INFO [PROGRESS]:\t + Download\t- 2.00 MiB/s");
        assert_eq!(n.speed, Some(2. * MIB));
        let d = parse_line("[cli] INFO: Download size: 100.00 MiB (Compression savings: 1%)");
        assert_eq!(d.size, Some(100 * 1024 * 1024));
        assert_eq!(parse_line("[cli] INFO: Logging in..."), LogState::default());
    }

    #[test]
    fn picks_the_telling_error() {
        let legendary = "[cli] INFO: Testing\n[cli] ERROR: Login attempt failed, please see log.";
        assert_eq!(
            error_line(legendary).unwrap(),
            "Login attempt failed, please see log."
        );
        assert_eq!(
            error_line("ERROR [CLI]:\t User not logged in").unwrap(),
            "User not logged in"
        );
        let trace = "Traceback (most recent call last):\nValueError: No saved credentials";
        assert_eq!(
            error_line(trace).unwrap(),
            "ValueError: No saved credentials"
        );
        let packed = "KeyError: 'x'\n[PYI-1:ERROR] Failed to execute script 'cli'";
        assert_eq!(error_line(packed).unwrap(), "KeyError: 'x'");
        assert_eq!(error_line("  \n"), None);
    }

    fn sh(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", script]);
        c
    }

    #[test]
    fn a_download_reports_and_can_be_stopped() {
        let job = progress::job("test-helper", "Test");
        let done = run(
            sh("printf ' = Progress: 50.00%% (1/2)\\n' >&2; sleep 0.5; echo fine; exit 0"),
            "x",
            Some(1000),
        )
        .unwrap();
        assert!(done.ok);
        assert_eq!(done.stdout, "fine");
        let e = progress::list()
            .into_iter()
            .find(|e| e.key == "test-helper")
            .unwrap();
        assert_eq!(e.done, 500);

        let failed = run(sh("echo '[cli] ERROR: no space' >&2; exit 1"), "x", None).unwrap();
        assert!(!failed.ok);
        assert_eq!(failed.error(), "no space");

        // Asked to stop: the helper and its children end, and the error says so.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(400));
            progress::stop("test-helper");
        });
        let started = Instant::now();
        let err = match run(sh("sleep 30 & wait"), "x", None) {
            Err(e) => e,
            Ok(_) => panic!("stopped"),
        };
        assert_eq!(err.to_string(), tools::STOPPED);
        assert!(started.elapsed() < Duration::from_secs(5));
        drop(job);
    }
}
