//! Screens through KDE's `kscreen-doctor`: their size, refresh rate, scale, HDR and which one
//! comes first.

use serde::Deserialize;
use serde_json::Value;

use super::run;

#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    pub name: String,
    pub enabled: bool,
    /// The first screen (where panels and full-screen games go).
    pub primary: bool,
    pub modes: Vec<Mode>,
    pub mode: String,
    pub scale: f64,
    /// `None`: the screen cannot.
    pub hdr: Option<bool>,
    pub vrr: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Mode {
    pub id: String,
    #[serde(rename = "refreshRate")]
    pub rate: f64,
    pub size: Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Mode {
    pub fn label(&self) -> String {
        format!(
            "{}×{} · {} Hz",
            self.size.width,
            self.size.height,
            self.rate.round() as u32
        )
    }
}

pub(crate) fn parse(json: &str) -> anyhow::Result<Vec<Screen>> {
    let v: Value = serde_json::from_str(json)?;
    let outputs = v["outputs"].as_array().cloned().unwrap_or_default();
    let mut screens: Vec<Screen> = outputs
        .iter()
        .filter(|o| o["connected"] == true)
        .filter_map(|o| {
            let mut modes: Vec<Mode> = serde_json::from_value(o["modes"].clone()).ok()?;
            // One per size and rate (the same mode comes more than once), largest first.
            modes.sort_by(|a, b| {
                let key = |m: &Mode| (m.size.width * m.size.height, (m.rate * 100.).round() as u64);
                key(b).cmp(&key(a))
            });
            modes.dedup_by(|a, b| a.label() == b.label());
            Some(Screen {
                name: o["name"].as_str()?.to_string(),
                enabled: o["enabled"] == true,
                primary: o["priority"].as_u64() == Some(1),
                mode: o["currentModeId"].as_str().unwrap_or_default().to_string(),
                scale: o["scale"].as_f64().unwrap_or(1.),
                hdr: o.get("hdr").and_then(Value::as_bool),
                vrr: o.get("vrrPolicy").and_then(Value::as_u64).map(|p| p != 0),
                modes,
            })
        })
        .collect();
    screens.sort_by_key(|s| !s.primary);
    Ok(screens)
}

/// The screens that are connected, the first one first. Blocking.
pub fn screens() -> anyhow::Result<Vec<Screen>> {
    parse(&run("kscreen-doctor", &["-j"], None, 10)?)
}

/// Screen names go to the program as part of an argument: plain ones only.
fn plain(name: &str) -> anyhow::Result<&str> {
    anyhow::ensure!(
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)),
        "not a screen"
    );
    Ok(name)
}

fn set(what: String) -> anyhow::Result<()> {
    run("kscreen-doctor", &[&what], None, 15)?;
    Ok(())
}

pub fn set_mode(screen: &str, mode: &str) -> anyhow::Result<()> {
    let mode = plain(mode)?;
    set(format!("output.{}.mode.{mode}", plain(screen)?))
}

pub fn set_scale(screen: &str, scale: f64) -> anyhow::Result<()> {
    anyhow::ensure!((0.5..=3.).contains(&scale), "not a scale");
    set(format!("output.{}.scale.{scale}", plain(screen)?))
}

pub fn set_hdr(screen: &str, on: bool) -> anyhow::Result<()> {
    let s = plain(screen)?;
    set(format!(
        "output.{s}.hdr.{}",
        if on { "enable" } else { "disable" }
    ))
}

pub fn set_vrr(screen: &str, on: bool) -> anyhow::Result<()> {
    let s = plain(screen)?;
    set(format!(
        "output.{s}.vrrpolicy.{}",
        if on { "automatic" } else { "never" }
    ))
}

pub fn set_primary(screen: &str) -> anyhow::Result<()> {
    set(format!("output.{}.priority.1", plain(screen)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_screens() {
        let json = r#"{"outputs":[
          {"name":"HDMI-A-2","connected":true,"enabled":true,"priority":2,"currentModeId":"1","scale":1,
           "modes":[{"id":"1","name":"1920x1080@60","refreshRate":60,"size":{"width":1920,"height":1080}},
                    {"id":"2","name":"1920x1080@60","refreshRate":59.94,"size":{"width":1920,"height":1080}},
                    {"id":"3","name":"1280x720@60","refreshRate":60,"size":{"width":1280,"height":720}}]},
          {"name":"DP-1","connected":true,"enabled":true,"priority":1,"currentModeId":"26","scale":1.25,"hdr":false,"vrrPolicy":0,
           "modes":[{"id":"25","refreshRate":60,"size":{"width":2560,"height":1440}},
                    {"id":"26","refreshRate":179.96,"size":{"width":2560,"height":1440}}]},
          {"name":"DP-2","connected":false,"enabled":false,"modes":[]}]}"#;
        let s = parse(json).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "DP-1");
        assert!(s[0].primary && s[0].hdr == Some(false) && s[0].vrr == Some(false));
        assert_eq!(s[0].modes[0].label(), "2560×1440 · 180 Hz");
        assert_eq!(s[1].modes.len(), 2, "59.94 and 60 Hz are one");
        assert_eq!(s[1].hdr, None);
        assert!(set_mode("DP-1; reboot", "1").is_err());
        assert!(set_scale("DP-1", 9.).is_err());
    }
}
