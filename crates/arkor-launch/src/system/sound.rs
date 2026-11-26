//! Sound through PipeWire: how loud, muted, and where it comes out.

use super::run;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// 0..=150 percent.
    pub volume: u8,
    pub muted: bool,
    pub outputs: Vec<Output>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub id: u32,
    pub name: String,
    pub default: bool,
}

/// The outputs (sinks) of `wpctl status`.
pub(crate) fn parse_outputs(status: &str) -> Vec<Output> {
    let mut out = Vec::new();
    let mut in_audio = false;
    let mut in_sinks = false;
    for line in status.lines() {
        let plain = line.trim_start_matches(|c: char| " │├└─".contains(c));
        if !line.starts_with([' ', '│', '├', '└']) {
            in_audio = line.trim() == "Audio";
            in_sinks = false;
            continue;
        }
        if plain.ends_with(':') {
            in_sinks = in_audio && plain.trim() == "Sinks:";
            continue;
        }
        if !in_sinks {
            continue;
        }
        let plain = plain.trim();
        let (default, plain) = match plain.strip_prefix('*') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, plain),
        };
        let Some((id, rest)) = plain.split_once(". ") else {
            continue;
        };
        let Ok(id) = id.trim().parse() else { continue };
        let name = rest
            .split(" [vol:")
            .next()
            .unwrap_or(rest)
            .trim()
            .to_string();
        out.push(Output { id, name, default });
    }
    out
}

/// `Volume: 0.60 [MUTED]`.
pub(crate) fn parse_volume(text: &str) -> Option<(u8, bool)> {
    let rest = text.trim().strip_prefix("Volume:")?.trim();
    let number = rest.split_whitespace().next()?;
    let v: f32 = number.parse().ok()?;
    Some((
        (v * 100.).round().clamp(0., 150.) as u8,
        rest.contains("[MUTED]"),
    ))
}

const SINK: &str = "@DEFAULT_AUDIO_SINK@";

/// How things are now. Blocking.
pub fn state() -> anyhow::Result<State> {
    let (volume, muted) =
        parse_volume(&run("wpctl", &["get-volume", SINK], None, 5)?).unwrap_or((0, false));
    let outputs = parse_outputs(&run("wpctl", &["status"], None, 5)?);
    Ok(State {
        volume,
        muted,
        outputs,
    })
}

/// 0..=100 percent (louder than that distorts).
pub fn set_volume(percent: u8) -> anyhow::Result<()> {
    let v = format!("{:.2}", f32::from(percent.min(100)) / 100.);
    run("wpctl", &["set-volume", "-l", "1.0", SINK, &v], None, 5)?;
    Ok(())
}

pub fn set_muted(muted: bool) -> anyhow::Result<()> {
    run(
        "wpctl",
        &["set-mute", SINK, if muted { "1" } else { "0" }],
        None,
        5,
    )?;
    Ok(())
}

pub fn set_output(id: u32) -> anyhow::Result<()> {
    run("wpctl", &["set-default", &id.to_string()], None, 5)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "PipeWire 'pipewire-0' [1.4.9, me@pc, cookie:1]
 └─ Clients:
        33. pipewire                            [1.4.9, me@pc, pid:1]

Audio
 ├─ Devices:
 │      43. GA106 High Definition Audio Controller [alsa]
 │      44. USB Audio Device                    [alsa]
 │  
 ├─ Sinks:
 │      52. GA106 High Definition Audio Controller Digital Stereo (HDMI) [GL27R180Q] [vol: 0.40]
 │  *   53. USB Audio Device Analog Stereo      [vol: 0.60 MUTED]
 │  
 ├─ Sources:
 │  *   54. USB Audio Device Mono               [vol: 1.00]
 │  
 └─ Streams:
        93. Zen

Video
 ├─ Devices:
 ├─ Sinks:
 │      70. Not audio
";

    #[test]
    fn reads_outputs_and_volume() {
        let o = parse_outputs(STATUS);
        assert_eq!(o.len(), 2);
        assert_eq!(
            o[0].name,
            "GA106 High Definition Audio Controller Digital Stereo (HDMI) [GL27R180Q]"
        );
        assert!(!o[0].default && o[1].default && o[1].id == 53);
        assert_eq!(parse_volume("Volume: 0.60\n"), Some((60, false)));
        assert_eq!(parse_volume("Volume: 0.35 [MUTED]"), Some((35, true)));
        assert_eq!(parse_volume("nonsense"), None);
    }
}
