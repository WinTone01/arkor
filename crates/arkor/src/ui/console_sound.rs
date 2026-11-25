//! Plays console mode's sounds. A thread makes them once, mixes what is playing and feeds a
//! small player program (`pacat`, or PipeWire's `pw-cat`) through a pipe kept short, so a
//! sound is heard right when the button is pressed. Nothing runs while it is silent for a
//! while, and nothing at all when no player is installed.

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

pub(super) use super::synth::Cue;
use super::synth::{RATE, make};

/// Frames mixed at a time (5 ms).
const BLOCK: usize = 240;
/// The player is kept this long after the last sound.
const IDLE: Duration = Duration::from_secs(25);
/// Sounds at once at most; the oldest gives way.
const VOICES: usize = 14;

enum Cmd {
    Play {
        cue: Cue,
        gain: f32,
        pan: f32,
    },
    /// Everything that plays dies away.
    Fade(f32),
}

type Samples = Arc<Vec<f32>>;
static MADE: LazyLock<Mutex<HashMap<Cue, Samples>>> = LazyLock::new(Default::default);

/// The sound, made when it is wanted for the first time.
fn samples(cue: Cue) -> Samples {
    let made = || MADE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = made().get(&cue) {
        return s.clone();
    }
    let s = Arc::new(make(cue));
    made().entry(cue).or_insert(s).clone()
}

static PLAYER: LazyLock<Option<Mutex<Sender<Cmd>>>> = LazyLock::new(|| {
    // The headless tests stay silent.
    if cfg!(test) {
        return None;
    }
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("arkor-sounds".into())
        .spawn(move || run(rx))
        .ok()?;
    Some(Mutex::new(tx))
});

fn send(cmd: Cmd) {
    if let Some(tx) = PLAYER.as_ref() {
        let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(cmd);
    }
}

/// Plays `cue`; `volume` 0…1, `pan` -1 (left) … 1 (right).
pub(super) fn play(cue: Cue, volume: f32, pan: f32) {
    if volume > 0. {
        // The ear hears loudness in steps of its own.
        send(Cmd::Play {
            cue,
            gain: volume * volume,
            pan,
        });
    }
}

/// Makes the sounds before the first one is needed.
pub(super) fn warm() {
    if cfg!(test) {
        return;
    }
    static WARM: std::sync::Once = std::sync::Once::new();
    WARM.call_once(|| {
        std::thread::spawn(|| {
            for cue in [Cue::Welcome].into_iter().chain(Cue::ALL) {
                samples(cue);
            }
        });
    });
}

/// Whether `cue` can be played at once.
pub(super) fn ready(cue: Cue) -> bool {
    MADE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&cue)
}

/// What plays dies away in `secs` (the animation was skipped, a game takes over).
pub(super) fn fade(secs: f32) {
    send(Cmd::Fade(secs));
}

struct Voice {
    sound: Samples,
    at: usize,
    gain: (f32, f32),
    /// The loudness now and how much it loses with every frame.
    level: f32,
    loss: f32,
}

/// Adds what plays to `out` (left and right in turn); what is over goes.
fn mix(voices: &mut Vec<Voice>, out: &mut [f32]) {
    out.fill(0.);
    voices.retain_mut(|v| {
        for frame in out.as_chunks_mut::<2>().0 {
            let Some(x) = v.sound.get(v.at..v.at + 2) else {
                return false;
            };
            frame[0] += x[0] * v.gain.0 * v.level;
            frame[1] += x[1] * v.gain.1 * v.level;
            v.at += 2;
            v.level -= v.loss;
            if v.level <= 0. {
                return false;
            }
        }
        true
    });
    // Many at once get closer to the limit instead of breaking.
    for x in out {
        *x = x.tanh();
    }
}

struct Out {
    child: Child,
    pipe: ChildStdin,
}

impl Out {
    fn open() -> Option<Out> {
        let players: [(&str, &[&str]); 2] = [
            (
                "pacat",
                &[
                    "--playback",
                    "--raw",
                    "--format=s16le",
                    "--rate=48000",
                    "--channels=2",
                    "--latency-msec=24",
                    "--process-time-msec=6",
                    "--client-name=Arkor",
                    "--stream-name=Arkor",
                    "--property=media.role=game",
                ],
            ),
            (
                "pw-cat",
                &[
                    "--playback",
                    "--raw",
                    "--format=s16",
                    "--rate=48000",
                    "--channels=2",
                    "--latency=20ms",
                    "--media-role=Game",
                    "-",
                ],
            ),
        ];
        for (name, args) in players {
            let Ok(mut child) = Command::new(name)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            else {
                continue;
            };
            let pipe = child.stdin.take()?;
            // A long pipe would be heard as a delay. One page: 21 ms.
            // SAFETY: a plain call on a descriptor owned here.
            unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETPIPE_SZ, 4096) };
            return Some(Out { child, pipe });
        }
        None
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn run(rx: Receiver<Cmd>) {
    let mut voices: Vec<Voice> = Vec::new();
    let mut out: Option<Out> = None;
    let mut heard = Instant::now();
    let mut failed: Option<Instant> = None;
    let mut block = [0f32; BLOCK * 2];
    let mut bytes = Vec::with_capacity(BLOCK * 4);
    loop {
        let next = if out.is_some() {
            rx.try_recv().map_err(|e| match e {
                std::sync::mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                std::sync::mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            })
        } else {
            rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match next {
            Ok(Cmd::Play { cue, gain, pan }) => {
                let sound = samples(cue);
                heard = Instant::now();
                if voices.len() >= VOICES {
                    voices.remove(0);
                }
                voices.push(Voice {
                    sound,
                    at: 0,
                    gain: (
                        gain * (1. - pan.max(0.) * 0.6),
                        gain * (1. + pan.min(0.) * 0.6),
                    ),
                    level: 1.,
                    loss: 0.,
                });
                if out.is_none() && failed.is_none_or(|t| t.elapsed() > Duration::from_secs(20)) {
                    out = Out::open();
                    failed = out.is_none().then(Instant::now);
                }
                if out.is_none() {
                    voices.clear();
                }
                continue;
            }
            Ok(Cmd::Fade(secs)) => {
                for v in &mut voices {
                    v.loss = 1. / (secs.max(0.01) * RATE as f32);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if !voices.is_empty() {
            heard = Instant::now();
        } else if heard.elapsed() > IDLE {
            out = None;
            continue;
        }
        mix(&mut voices, &mut block);
        bytes.clear();
        for v in block {
            bytes.extend(((v * 32767.) as i16).to_le_bytes());
        }
        // Waits while the player has enough: that is the clock here.
        if let Some(o) = &mut out
            && o.pipe.write_all(&bytes).is_err()
        {
            out = None;
            voices.clear();
            failed = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_mix_fade_and_end() {
        let sound = Arc::new(vec![0.5; 400]);
        let voice = |loss| Voice {
            sound: sound.clone(),
            at: 0,
            gain: (1., 0.5),
            level: 1.,
            loss,
        };
        let mut voices = vec![voice(0.), voice(0.)];
        let mut out = [0f32; 200];
        mix(&mut voices, &mut out);
        assert!((out[0] - 1f32.tanh()).abs() < 1e-6 && (out[1] - 0.5f32.tanh()).abs() < 1e-6);
        assert_eq!(voices.len(), 2);
        mix(&mut voices, &mut out);
        mix(&mut voices, &mut out);
        assert!(voices.is_empty(), "over");
        assert!(out.iter().all(|v| *v == 0.));
        let mut voices = vec![voice(0.02)];
        mix(&mut voices, &mut out);
        assert!(voices.is_empty(), "faded away");
    }
}
