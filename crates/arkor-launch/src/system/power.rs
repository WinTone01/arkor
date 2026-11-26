//! Power: the profile of the processor and the fans, and the battery of a laptop.

use super::run;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// `performance`, `balanced`, `power-saver`: those this computer has.
    pub profiles: Vec<String>,
    pub current: Option<String>,
    /// Percent, and whether it charges.
    pub battery: Option<(u8, bool)>,
}

/// The profiles of `powerprofilesctl list`, and the chosen one.
pub(crate) fn parse(list: &str) -> (Vec<String>, Option<String>) {
    let mut profiles = Vec::new();
    let mut current = None;
    for line in list.lines() {
        // Their names start the line; their details are indented further.
        let chosen = line.starts_with('*');
        let name = line.trim_start_matches(['*', ' ']);
        if line.starts_with("    ") || !name.ends_with(':') || name.contains(' ') {
            continue;
        }
        let name = name.trim_end_matches(':').to_string();
        if chosen {
            current = Some(name.clone());
        }
        profiles.push(name);
    }
    (profiles, current)
}

fn battery() -> Option<(u8, bool)> {
    std::fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .find_map(|e| {
            let read = |f: &str| std::fs::read_to_string(e.path().join(f)).ok();
            (read("type")?.trim() == "Battery").then_some(())?;
            let percent = read("capacity")?.trim().parse().ok()?;
            let charging = read("status").is_some_and(|s| s.trim() == "Charging");
            Some((percent, charging))
        })
}

/// How things are now. Blocking.
pub fn state() -> State {
    let (profiles, current) =
        parse(&run("powerprofilesctl", &["list"], None, 5).unwrap_or_default());
    State {
        profiles,
        current,
        battery: battery(),
    }
}

pub fn set_profile(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "not a profile"
    );
    run("powerprofilesctl", &["set", name], None, 10)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_profiles() {
        let list = "  performance:\n    CpuDriver:\tintel_pstate\n    Degraded:   no\n\n* balanced:\n    CpuDriver:\tintel_pstate\n\n  power-saver:\n    CpuDriver:\tintel_pstate\n";
        let (p, c) = super::parse(list);
        assert_eq!(p, ["performance", "balanced", "power-saver"]);
        assert_eq!(c.as_deref(), Some("balanced"));
        assert!(super::set_profile("x; rm").is_err());
    }
}
