//! Bluetooth through BlueZ: gamepads and headsets.

use super::run;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// There is a Bluetooth adapter.
    pub adapter: bool,
    pub on: bool,
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    pub mac: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    /// What it is, as BlueZ says (`input-gaming`, `audio-headset`, …).
    pub icon: String,
    pub battery: Option<u8>,
}

/// `Device AA:BB:CC:DD:EE:FF Name` lines.
fn device_lines(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("Device ")?;
            let (mac, name) = rest.split_once(' ').unwrap_or((rest, ""));
            valid_mac(mac).then(|| (mac.to_string(), name.trim().to_string()))
        })
        .collect()
}

/// Only an address goes to the program as an argument.
fn valid_mac(mac: &str) -> bool {
    mac.len() == 17
        && mac
            .split(':')
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn info_value<'a>(info: &'a str, key: &str) -> Option<&'a str> {
    info.lines()
        .find_map(|l| l.trim().strip_prefix(key)?.strip_prefix(':'))
        .map(str::trim)
}

pub(crate) fn parse_info(mut d: Device, info: &str) -> Device {
    d.paired |= info_value(info, "Paired") == Some("yes");
    d.connected = info_value(info, "Connected") == Some("yes");
    d.icon = info_value(info, "Icon").unwrap_or_default().to_string();
    // "Battery Percentage: 0x55 (85)".
    d.battery = info_value(info, "Battery Percentage")
        .and_then(|v| v.split_once('(')?.1.strip_suffix(')')?.parse().ok());
    if (d.name.is_empty() || valid_mac(&d.name.replace('-', ":")))
        && let Some(n) = info_value(info, "Name").or_else(|| info_value(info, "Alias"))
    {
        d.name = n.to_string();
    }
    d
}

/// How things are now. Blocking.
pub fn state() -> anyhow::Result<State> {
    let show = run("bluetoothctl", &["show"], None, 5).unwrap_or_default();
    if !show.contains("Controller ") {
        return Ok(State::default());
    }
    let paired: Vec<String> =
        device_lines(&run("bluetoothctl", &["devices", "Paired"], None, 5).unwrap_or_default())
            .into_iter()
            .map(|(mac, _)| mac)
            .collect();
    let mut devices: Vec<Device> = device_lines(&run("bluetoothctl", &["devices"], None, 5)?)
        .into_iter()
        .map(|(mac, name)| {
            let info = run("bluetoothctl", &["info", &mac], None, 5).unwrap_or_default();
            parse_info(
                Device {
                    paired: paired.contains(&mac),
                    mac,
                    name,
                    ..Device::default()
                },
                &info,
            )
        })
        .collect();
    devices.sort_by_key(|d| (!d.connected, !d.paired, d.name.to_lowercase()));
    Ok(State {
        adapter: true,
        on: info_value(&show, "Powered") == Some("yes"),
        devices,
    })
}

pub fn set_on(on: bool) -> anyhow::Result<()> {
    run(
        "bluetoothctl",
        &["power", if on { "on" } else { "off" }],
        None,
        10,
    )?;
    Ok(())
}

/// Looks for devices nearby for a while. Blocking.
pub fn scan(secs: u64) -> anyhow::Result<()> {
    let t = secs.to_string();
    run(
        "bluetoothctl",
        &["--timeout", &t, "scan", "on"],
        None,
        secs + 5,
    )?;
    Ok(())
}

fn checked(mac: &str) -> anyhow::Result<&str> {
    anyhow::ensure!(valid_mac(mac), "not a Bluetooth address");
    Ok(mac)
}

/// Pairs a device that asks for no code (gamepads, headsets), trusts and connects it.
pub fn pair(mac: &str) -> anyhow::Result<()> {
    let mac = checked(mac)?;
    let out = run(
        "bluetoothctl",
        &["--agent", "NoInputNoOutput", "--timeout", "30", "pair", mac],
        None,
        40,
    )?;
    anyhow::ensure!(
        !out.contains("Failed to pair") && !out.contains("not available"),
        "{}",
        out.lines()
            .rfind(|l| !l.trim().is_empty())
            .unwrap_or("it did not pair")
    );
    run("bluetoothctl", &["trust", mac], None, 10)?;
    connect(mac)
}

pub fn connect(mac: &str) -> anyhow::Result<()> {
    let out = run(
        "bluetoothctl",
        &["--timeout", "20", "connect", checked(mac)?],
        None,
        30,
    )?;
    anyhow::ensure!(
        !out.contains("Failed to connect"),
        "it did not connect (is it on?)"
    );
    Ok(())
}

pub fn disconnect(mac: &str) -> anyhow::Result<()> {
    run("bluetoothctl", &["disconnect", checked(mac)?], None, 15)?;
    Ok(())
}

/// Forgets a paired device.
pub fn remove(mac: &str) -> anyhow::Result<()> {
    run("bluetoothctl", &["remove", checked(mac)?], None, 15)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_devices() {
        let list = "Device 10:7C:61:27:65:D7 DualSense Wireless Controller\nDevice AA:BB:CC:DD:EE:FF AA-BB-CC-DD-EE-FF\nnoise\nDevice nonsense here\n";
        let d = device_lines(list);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].1, "DualSense Wireless Controller");
        let info = "Device 10:7C:61:27:65:D7 (public)\n\tName: DualSense Wireless Controller\n\tIcon: input-gaming\n\tPaired: yes\n\tConnected: yes\n\tBattery Percentage: 0x55 (85)\n";
        let dev = parse_info(
            Device {
                mac: d[0].0.clone(),
                name: d[0].1.clone(),
                ..Device::default()
            },
            info,
        );
        assert!(dev.paired && dev.connected);
        assert_eq!((dev.icon.as_str(), dev.battery), ("input-gaming", Some(85)));
        // A device known only by its address gets its name from what it says.
        let dev = parse_info(
            Device {
                mac: d[1].0.clone(),
                name: d[1].1.clone(),
                ..Device::default()
            },
            "\tAlias: Headset X\n\tConnected: no\n",
        );
        assert_eq!(dev.name, "Headset X");
        assert!(!valid_mac("10:7C:61:27:65") && !valid_mac("--help ; x"));
        assert!(pair("-rf").is_err());
    }
}
