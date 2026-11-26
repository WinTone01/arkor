//! Wireless networks through NetworkManager.

use super::run;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// There is a wireless card.
    pub card: bool,
    /// Its radio is on.
    pub on: bool,
    /// A cable connects this computer.
    pub wired: bool,
    pub networks: Vec<Network>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub ssid: String,
    /// 0..=100.
    pub signal: u8,
    pub secured: bool,
    pub connected: bool,
    /// Connected to before: it needs no password again.
    pub known: bool,
}

/// `nmcli -t` fields: separated by `:`, with `\:` inside a value.
fn fields(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.last_mut().expect("one").push(n);
                }
            }
            ':' => out.push(String::new()),
            c => out.last_mut().expect("one").push(c),
        }
    }
    out
}

pub(crate) fn parse(devices: &str, radio: &str, list: &str, known: &str) -> State {
    let mut s = State {
        on: radio.trim() == "enabled",
        ..State::default()
    };
    for f in devices.lines().map(fields) {
        match (f.get(1).map(String::as_str), f.get(2).map(String::as_str)) {
            (Some("wifi"), _) => s.card = true,
            (Some("ethernet"), Some("connected")) => s.wired = true,
            _ => {}
        }
    }
    let known: Vec<String> = known
        .lines()
        .map(fields)
        .filter(|f| f.get(1).is_some_and(|t| t.ends_with("wireless")))
        .map(|f| f[0].clone())
        .collect();
    for f in list.lines().map(fields) {
        let [in_use, ssid, signal, security] = &f[..] else {
            continue;
        };
        // Hidden networks have no name to show.
        if ssid.is_empty() {
            continue;
        }
        let net = Network {
            ssid: ssid.clone(),
            signal: signal.parse().unwrap_or(0),
            secured: !security.is_empty() && security != "--",
            connected: in_use == "*",
            known: known.contains(ssid),
        };
        match s.networks.iter_mut().find(|n| n.ssid == net.ssid) {
            // One name, several access points: the strongest one.
            Some(n) => {
                n.connected |= net.connected;
                n.signal = n.signal.max(net.signal);
            }
            None => s.networks.push(net),
        }
    }
    s.networks
        .sort_by_key(|n| (!n.connected, !n.known, std::cmp::Reverse(n.signal)));
    s
}

/// How things are now. `rescan`: look for networks again (a few seconds). Blocking.
pub fn state(rescan: bool) -> anyhow::Result<State> {
    let devices = run(
        "nmcli",
        &["-t", "-f", "DEVICE,TYPE,STATE", "device"],
        None,
        5,
    )?;
    let radio = run("nmcli", &["radio", "wifi"], None, 5).unwrap_or_default();
    let known = run(
        "nmcli",
        &["-t", "-f", "NAME,TYPE", "connection", "show"],
        None,
        5,
    )
    .unwrap_or_default();
    let rescan = if rescan { "yes" } else { "auto" };
    let list = run(
        "nmcli",
        &[
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            rescan,
        ],
        None,
        15,
    )
    .unwrap_or_default();
    Ok(parse(&devices, &radio, &list, &known))
}

pub fn set_on(on: bool) -> anyhow::Result<()> {
    run(
        "nmcli",
        &["radio", "wifi", if on { "on" } else { "off" }],
        None,
        10,
    )?;
    Ok(())
}

/// Connects; a password only for networks never used before.
pub fn connect(net: &Network, password: Option<&str>) -> anyhow::Result<()> {
    if net.known {
        run("nmcli", &["connection", "up", "id", &net.ssid], None, 45)?;
        return Ok(());
    }
    match password {
        // Given on its input, not as an argument: those are visible to every program.
        Some(pw) => run(
            "nmcli",
            &["--ask", "device", "wifi", "connect", &net.ssid],
            Some(&format!("{pw}\n")),
            45,
        )?,
        None => run("nmcli", &["device", "wifi", "connect", &net.ssid], None, 45)?,
    };
    Ok(())
}

pub fn disconnect(net: &Network) -> anyhow::Result<()> {
    run("nmcli", &["connection", "down", "id", &net.ssid], None, 20)?;
    Ok(())
}

/// Forgets the network's password.
pub fn forget(net: &Network) -> anyhow::Result<()> {
    run(
        "nmcli",
        &["connection", "delete", "id", &net.ssid],
        None,
        20,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_networks() {
        let devices = "wlan0:wifi:connected\nenp3s0:ethernet:unavailable\nlo:loopback:connected (externally)\n";
        let list =
            "*:Home\\:5G:78:WPA2\n :Home\\:5G:60:WPA2\n :Cafe:40:\n :Office:90:WPA2 802.1X\n :::\n";
        let known = "Home\\:5G:802-11-wireless\nOffice:802-11-wireless\nWired:802-3-ethernet\n";
        let s = parse(devices, "enabled\n", list, known);
        assert!(s.card && s.on && !s.wired);
        let names: Vec<&str> = s.networks.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["Home:5G", "Office", "Cafe"]);
        assert!(s.networks[0].connected && s.networks[0].known && s.networks[0].secured);
        assert_eq!(s.networks[0].signal, 78);
        assert!(!s.networks[2].secured && !s.networks[2].known);
        let wired = parse("enp3s0:ethernet:connected\n", "enabled", "", "");
        assert!(!wired.card && wired.wired);
    }
}
