//! The desktop's accent colour, for "follow the desktop": a pywal or wallust palette when
//! there is one (they colour the whole desktop after the wallpaper), else KDE's accent, else
//! GNOME's. Read again now and then, so a change on the desktop shows in Arkor.

use std::path::PathBuf;

fn hex(text: &str) -> Option<u32> {
    let t = text.trim().trim_start_matches('#');
    (t.len() == 6)
        .then(|| u32::from_str_radix(t, 16).ok())
        .flatten()
}

/// pywal's and wallust's `colors.json` (`colors.color4`, the accent most themes take).
fn wal() -> Option<u32> {
    let cache = dirs::cache_dir()?;
    [
        cache.join("wal/colors.json"),
        cache.join("wallust/colors.json"),
    ]
    .into_iter()
    .find_map(|p: PathBuf| {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
        hex(v["colors"]["color4"].as_str()?)
    })
}

/// A kdeglobals colour: `r,g,b` or `#rrggbb`.
fn kde_colour(v: &str) -> Option<u32> {
    let rgb: Vec<u32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    if let [r, g, b] = rgb[..] {
        return Some((r << 16) | (g << 8) | b);
    }
    hex(v)
}

/// `key` in `[section]` of kdeglobals' text.
fn kde_value<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == section;
            continue;
        }
        if inside && let Some(v) = line.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
            return Some(v);
        }
    }
    None
}

/// KDE's accent (`AccentColor` in kdeglobals), else its colour scheme's selection colour.
fn kde() -> Option<u32> {
    let text = std::fs::read_to_string(dirs::config_dir()?.join("kdeglobals")).ok()?;
    kde_value(&text, "[General]", "AccentColor")
        .or_else(|| kde_value(&text, "[Colors:Selection]", "BackgroundNormal"))
        .and_then(kde_colour)
}

/// GNOME's accent (`org.gnome.desktop.interface accent-color`, a name since GNOME 47).
fn gnome() -> Option<u32> {
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "accent-color"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&out.stdout)
        .trim()
        .trim_matches('\'')
        .to_string();
    Some(match name.as_str() {
        "blue" => 0x3584e4,
        "teal" => 0x2190a4,
        "green" => 0x3a944a,
        "yellow" => 0xc88800,
        "orange" => 0xed5b00,
        "red" => 0xe62d42,
        "pink" => 0xd56199,
        "purple" => 0x9141ac,
        "slate" => 0x6f8396,
        _ => return None,
    })
}

/// The desktop's accent colour, if it has one. Blocking (briefly).
pub fn read() -> Option<u32> {
    wal().or_else(kde).or_else(|| {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        desktop
            .to_lowercase()
            .contains("gnome")
            .then(gnome)
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_colours() {
        assert_eq!(super::hex("#3daee9"), Some(0x3daee9));
        assert_eq!(super::hex("nope"), None);
        let kde =
            "[Colors:Selection]\nBackgroundNormal=61,174,233\n[General]\nColorScheme=BreezeDark\n";
        let v = super::kde_value(kde, "[General]", "AccentColor")
            .or_else(|| super::kde_value(kde, "[Colors:Selection]", "BackgroundNormal"));
        assert_eq!(v.and_then(super::kde_colour), Some(0x3daee9));
    }
}
