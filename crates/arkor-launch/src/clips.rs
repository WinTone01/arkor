//! Clips of games, with GPU Screen Recorder (`gpu-screen-recorder`): while a game started by
//! `arkor run` runs, its last moments are kept in memory (replay mode); saving one writes
//! them to `~/Videos/Arkor/<game>/`. Only the screen is captured (the monitor through KMS),
//! so nothing asks for permission and it works the same under gamescope.
//!
//! The recorder of the running game is named in `$XDG_RUNTIME_DIR/arkor-clips` (its process
//! and folder), so the quick menu, `arkor clip` or a keyboard shortcut can save from another
//! process with a signal (SIGUSR1).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use arkor_core::profile::Clips;

const PROGRAM: &str = "gpu-screen-recorder";

/// Whether GPU Screen Recorder is installed.
pub fn available() -> bool {
    crate::start::find_in_path(PROGRAM).is_some()
}

fn state_file() -> Option<PathBuf> {
    dirs::runtime_dir().map(|d| d.join("arkor-clips"))
}

/// Where a game's clips go.
pub fn folder(game: &str) -> Option<PathBuf> {
    let safe: String = game
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim();
    let videos = dirs::video_dir().or_else(|| dirs::home_dir().map(|h| h.join("Videos")))?;
    Some(
        videos
            .join("Arkor")
            .join(if safe.is_empty() { "Game" } else { safe }),
    )
}

/// The screen to capture: the first monitor GPU Screen Recorder lists (`DP-1|2560x1440`).
fn monitor() -> Option<String> {
    let out = Command::new(PROGRAM)
        .arg("--list-capture-options")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    first_monitor(&String::from_utf8_lossy(&out.stdout))
}

fn first_monitor(list: &str) -> Option<String> {
    list.lines()
        .filter_map(|l| l.split_once('|'))
        .find(|(_, size)| {
            size.contains('x') && size.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .map(|(name, _)| name.to_string())
}

/// The recorder of the running game: what is written to the state file.
fn state_text(pid: u32, dir: &Path) -> String {
    format!("{pid}\n{}\n", dir.display())
}

fn parse_state(text: &str) -> Option<(u32, PathBuf)> {
    let mut lines = text.lines();
    let pid = lines.next()?.trim().parse().ok()?;
    let dir = PathBuf::from(lines.next()?);
    Some((pid, dir))
}

/// The recorder that is running now, and where its clips go.
fn current() -> Option<(u32, PathBuf)> {
    let (pid, dir) = parse_state(&std::fs::read_to_string(state_file()?).ok()?)?;
    let alive = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .is_ok_and(|c| PROGRAM.starts_with(c.trim()));
    alive.then_some((pid, dir))
}

/// Whether a game's last moments are being kept now.
pub fn recording() -> bool {
    current().is_some()
}

/// GPU Screen Recorder keeping a game's last moments; dropping it stops it (nothing saved).
#[derive(Debug)]
pub struct Recorder {
    child: Child,
}

impl Recorder {
    /// Starts keeping the last moments of `game`, when clips are on and nothing else is
    /// being recorded. Problems go to `clips.log` in Arkor's state folder.
    pub fn start(game: &str, clips: &Clips) -> Option<Recorder> {
        if !clips.enabled || !available() || recording() {
            return None;
        }
        let dir = folder(game)?;
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("arkor: no clips: {}: {e}", dir.display());
            return None;
        }
        let Some(screen) = monitor() else {
            eprintln!("arkor: no clips: GPU Screen Recorder found no screen to capture");
            return None;
        };
        let mut cmd = Command::new(PROGRAM);
        cmd.args(["-w", &screen, "-f", "60", "-c", "mp4", "-cursor", "no"])
            .args(["-r", &clips.seconds().to_string(), "-replay-storage", "ram"])
            .args(["-v", "no", "-o"])
            .arg(&dir);
        if !clips.no_sound {
            cmd.args(["-a", "default_output"]);
        }
        let log = dirs::state_dir().map(|d| d.join("arkor")).and_then(|d| {
            std::fs::create_dir_all(&d).ok()?;
            std::fs::File::create(d.join("clips.log")).ok()
        });
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log.map_or(Stdio::null(), Stdio::from));
        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("arkor: no clips: {PROGRAM}: {e}");
                return None;
            }
        };
        if let Some(state) = state_file() {
            let _ = std::fs::write(state, state_text(child.id(), &dir));
        }
        Some(Recorder { child })
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // SIGINT ends replay mode without saving.
        // SAFETY: a plain signal to our own child.
        unsafe { libc::kill(self.child.id() as i32, libc::SIGINT) };
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(state) = state_file()
            && std::fs::read_to_string(&state)
                .ok()
                .and_then(|t| parse_state(&t))
                .is_some_and(|(pid, _)| pid == self.child.id())
        {
            let _ = std::fs::remove_file(state);
        }
    }
}

/// The newest video in `dir`, and when it was written.
fn newest(dir: &Path) -> Option<(PathBuf, SystemTime)> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "mp4"))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .max_by_key(|(_, t)| *t)
}

/// Saves the running game's last moments; the clip's file, once it is written. Blocking
/// (a few seconds).
pub fn save() -> anyhow::Result<PathBuf> {
    let (pid, dir) = current().context("no game is being recorded")?;
    let before = newest(&dir).map(|(p, _)| p);
    // SAFETY: a plain signal; the process was checked to be GPU Screen Recorder.
    let sent = unsafe { libc::kill(pid as i32, libc::SIGUSR1) };
    anyhow::ensure!(sent == 0, "GPU Screen Recorder did not answer");
    let until = Instant::now() + Duration::from_secs(30);
    let mut seen: Option<(PathBuf, u64)> = None;
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(400));
        let Some((file, _)) = newest(&dir).filter(|(p, _)| Some(p) != before.as_ref()) else {
            continue;
        };
        let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        // Written once its size stops changing.
        if size > 0 && seen.as_ref().is_some_and(|(p, s)| *p == file && *s == size) {
            return Ok(file);
        }
        seen = Some((file, size));
    }
    anyhow::bail!("the clip was not written")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_first_monitor() {
        let list = "DP-1|2560x1440\nHDMI-A-2|1920x1080\nregion\nportal\n";
        assert_eq!(first_monitor(list).as_deref(), Some("DP-1"));
        assert_eq!(first_monitor("region\nportal\n"), None);
        assert_eq!(
            first_monitor("screen|Screen\nDP-2|1920x1080"),
            Some("DP-2".into())
        );
    }

    #[test]
    fn reads_back_its_state() {
        let text = state_text(42, Path::new("/home/u/Videos/Arkor/Far Cry 5"));
        assert_eq!(
            parse_state(&text),
            Some((42, PathBuf::from("/home/u/Videos/Arkor/Far Cry 5")))
        );
        assert_eq!(parse_state("x\n/a"), None);
    }

    #[test]
    fn a_folder_per_game() {
        let f = folder("R.E.P.O.: Deluxe").unwrap();
        assert!(f.ends_with("Arkor/R_E_P_O__ Deluxe"), "{f:?}");
    }
}
