//! The computer itself, for console mode: its network, Bluetooth, sound, screens and power,
//! through the programs the desktop uses for them (NetworkManager's `nmcli`, BlueZ's
//! `bluetoothctl`, PipeWire's `wpctl`, KDE's `kscreen-doctor`, `powerprofilesctl`).
//! What they print is read here; nothing needs a password for the user at the computer.

pub mod bluetooth;
pub mod display;
pub mod power;
pub mod sound;
pub mod wifi;

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;

/// Runs a program to its end, or ends it after `secs`; its output. `input` is written to it.
pub(crate) fn run(
    program: &str,
    args: &[&str],
    input: Option<&str>,
    secs: u64,
) -> anyhow::Result<String> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("{program} is not installed"))?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(text.as_bytes());
    }
    let mut out = child.stdout.take().context("no output")?;
    let mut err = child.stderr.take().context("no output")?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let began = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if began.elapsed() > Duration::from_secs(secs) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    let text = reader.join().unwrap_or_default();
    let errors = errors.join().unwrap_or_default();
    match status {
        Some(s) if s.success() => Ok(text),
        Some(_) => {
            let why = errors
                .lines()
                .chain(text.lines())
                .map(str::trim)
                .rfind(|l| !l.is_empty())
                .unwrap_or("it did not work")
                .to_string();
            anyhow::bail!("{why}")
        }
        // Programs that keep going (a scan) end by the clock: what they said counts.
        None => Ok(text),
    }
}

/// Whether a program is there.
pub fn has(program: &str) -> bool {
    crate::start::find_in_path(program).is_some()
}

#[cfg(test)]
mod real {
    /// Reads this computer's state (changes nothing); prints no names of networks or devices.
    #[test]
    #[ignore = "reads this computer"]
    fn reads_this_computer() {
        let w = super::wifi::state(false).unwrap();
        println!(
            "wifi card {} on {} wired {} networks {}",
            w.card,
            w.on,
            w.wired,
            w.networks.len()
        );
        let b = super::bluetooth::state().unwrap();
        println!(
            "bt adapter {} on {} devices {}",
            b.adapter,
            b.on,
            b.devices.len()
        );
        let s = super::sound::state().unwrap();
        println!(
            "sound {}% muted {} outputs {} default {:?}",
            s.volume,
            s.muted,
            s.outputs.len(),
            s.outputs.iter().position(|o| o.default)
        );
        for d in super::display::screens().unwrap() {
            println!(
                "screen {} primary {} mode {:?} scale {} hdr {:?} vrr {:?} modes {}",
                d.name,
                d.primary,
                d.modes.iter().find(|m| m.id == d.mode).map(|m| m.label()),
                d.scale,
                d.hdr,
                d.vrr,
                d.modes.len()
            );
        }
        println!("{:?}", super::power::state());
    }

    /// Sets what is set already (changes nothing): the commands are understood.
    #[test]
    #[ignore = "talks to this computer's sound and power"]
    fn same_values_again() {
        let s = super::sound::state().unwrap();
        super::sound::set_volume(s.volume).unwrap();
        super::sound::set_muted(s.muted).unwrap();
        let out = s.outputs.iter().find(|o| o.default).unwrap();
        super::sound::set_output(out.id).unwrap();
        assert_eq!(super::sound::state().unwrap(), s);
        let p = super::power::state();
        super::power::set_profile(p.current.as_deref().unwrap()).unwrap();
        assert_eq!(super::power::state().current, p.current);
        println!("ok");
    }
}
