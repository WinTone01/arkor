//! Bringing a window to the front from outside it. On Wayland a window may not take the
//! front by itself while another one (a game) has it, so the desktop's own way is used:
//! KWin's scripts on KDE ([`super::kwin`]), `hyprctl` on Hyprland, `swaymsg` on Sway and
//! `niri msg` on niri. Elsewhere (GNOME, …) there is none: Arkor only asks for itself, which
//! the desktop may answer with a notice instead.

use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::Value;

use super::kwin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Desk {
    Kwin,
    Hyprland,
    Sway,
    Niri,
}

fn has(program: &str) -> bool {
    arkor_launch::system::has(program)
}

fn desk() -> Option<Desk> {
    static DESK: OnceLock<Option<Desk>> = OnceLock::new();
    *DESK.get_or_init(|| {
        let env = |k: &str| std::env::var_os(k).is_some();
        if kwin::available() {
            Some(Desk::Kwin)
        } else if env("HYPRLAND_INSTANCE_SIGNATURE") && has("hyprctl") {
            Some(Desk::Hyprland)
        } else if env("SWAYSOCK") && has("swaymsg") {
            Some(Desk::Sway)
        } else if env("NIRI_SOCKET") && has("niri") {
            Some(Desk::Niri)
        } else {
            None
        }
    })
}

/// Whether windows can be brought to the front here.
pub(super) fn available() -> bool {
    !cfg!(test) && desk().is_some()
}

/// Whether Steam's own windows can be kept away from console mode (KDE only).
pub(super) fn guards_steam() -> bool {
    !cfg!(test) && desk() == Some(Desk::Kwin)
}

/// Steam's windows are put away from now on. Blocking.
pub(super) fn guard() {
    if desk() == Some(Desk::Kwin) {
        kwin::guard();
    }
}

/// Steam's windows are left alone again. Blocking.
pub(super) fn unguard() {
    if desk() == Some(Desk::Kwin) {
        kwin::unguard();
    }
}

/// A program's output, when it succeeded.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn hypr_focus(what: &str) -> bool {
    // hyprctl answers "ok" when it did it.
    run("hyprctl", &["dispatch", "focuswindow", what]).is_some_and(|o| o.trim() == "ok")
}

fn sway_focus(criteria: &str) -> bool {
    run("swaymsg", &[&format!("{criteria} focus")]).is_some()
}

fn niri_windows() -> Vec<Value> {
    run("niri", &["msg", "--json", "windows"])
        .and_then(|o| serde_json::from_str::<Vec<Value>>(&o).ok())
        .unwrap_or_default()
}

fn niri_focus(id: u64) -> bool {
    run(
        "niri",
        &["msg", "action", "focus-window", "--id", &id.to_string()],
    )
    .is_some()
}

/// The first of niri's windows that `pick` takes.
fn niri_pick(windows: &[Value], pick: impl Fn(&Value) -> bool) -> Option<u64> {
    windows
        .iter()
        .find(|w| pick(w))
        .and_then(|w| w["id"].as_u64())
}

/// A window of Arkor's own, or a launcher's, not a game.
fn not_a_game(class: &str) -> bool {
    matches!(
        class.to_lowercase().as_str(),
        "arkor" | "arkor-login" | "arkor-splash" | "steam" | "steamwebhelper"
    )
}

/// Of Hyprland's windows (`hyprctl -j clients`): the game's, one of `pids`' (a full-screen one
/// first), else the one in front most lately that is not Arkor's or Steam's.
fn hypr_game(clients: &str, pids: &[u32]) -> Option<String> {
    let list: Vec<Value> = serde_json::from_str(clients).ok()?;
    let class = |w: &Value| w["class"].as_str().unwrap_or_default().to_string();
    let full = |w: &Value| match &w["fullscreen"] {
        Value::Bool(b) => *b,
        v => v.as_u64().is_some_and(|n| n > 0),
    };
    let ours = list
        .iter()
        .filter(|w| {
            w["pid"]
                .as_u64()
                .is_some_and(|p| pids.iter().any(|q| u64::from(*q) == p))
                && !not_a_game(&class(w))
        })
        .max_by_key(|w| full(w));
    let last = || {
        list.iter()
            .filter(|w| !not_a_game(&class(w)) && w["mapped"] != false)
            .filter_map(|w| Some((w["focusHistoryID"].as_i64()?, w)))
            .min_by_key(|(n, _)| *n)
            .map(|(_, w)| w)
    };
    ours.or_else(last)?["address"].as_str().map(str::to_string)
}

/// Brings Arkor's window to the front. Blocking.
pub(super) fn show_arkor() {
    match desk() {
        Some(Desk::Kwin) => kwin::show_arkor(),
        Some(Desk::Hyprland) => {
            hypr_focus("class:^(arkor)$");
        }
        Some(Desk::Sway) => {
            sway_focus(r#"[app_id="^arkor$"]"#);
        }
        Some(Desk::Niri) => {
            if let Some(id) = niri_pick(&niri_windows(), |w| w["app_id"] == "arkor") {
                niri_focus(id);
            }
        }
        None => {}
    }
}

/// Brings Steam's windows to the front. Blocking.
pub(super) fn show_steam() {
    match desk() {
        Some(Desk::Kwin) => kwin::show_steam(),
        Some(Desk::Hyprland) => {
            hypr_focus("class:^(steam)$");
        }
        Some(Desk::Sway) => {
            sway_focus(r#"[class="^steam$"]"#);
        }
        Some(Desk::Niri) => {
            if let Some(id) = niri_pick(&niri_windows(), |w| w["app_id"] == "steam") {
                niri_focus(id);
            }
        }
        None => {}
    }
}

/// Brings a running game's window (one of `pids`') to the front; `false` when it could not.
/// Blocking.
pub(super) fn show_game(pids: &[u32]) -> bool {
    match desk() {
        Some(Desk::Kwin) => kwin::show_game(pids),
        Some(Desk::Hyprland) => run("hyprctl", &["-j", "clients"])
            .and_then(|c| hypr_game(&c, pids))
            .is_some_and(|a| hypr_focus(&format!("address:{a}"))),
        Some(Desk::Sway) => pids.iter().any(|p| sway_focus(&format!("[pid={p}]"))),
        Some(Desk::Niri) => {
            let windows = niri_windows();
            niri_pick(&windows, |w| {
                w["pid"]
                    .as_u64()
                    .is_some_and(|p| pids.iter().any(|q| u64::from(*q) == p))
            })
            .is_some_and(niri_focus)
        }
        None => false,
    }
}

/// Brings the window of process `pid` to the front (one that opened behind a game). Blocking.
pub(super) fn show_process(pid: u32) {
    match desk() {
        Some(Desk::Kwin) => kwin::show_process(pid),
        Some(Desk::Hyprland) => {
            hypr_focus(&format!("pid:{pid}"));
        }
        Some(Desk::Sway) => {
            sway_focus(&format!("[pid={pid}]"));
        }
        Some(Desk::Niri) => {
            if let Some(id) = niri_pick(&niri_windows(), |w| w["pid"] == pid) {
                niri_focus(id);
            }
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_game_among_hyprlands_windows() {
        let clients = r#"[
            {"address":"0xa","pid":10,"class":"arkor","fullscreen":2,"focusHistoryID":0,"mapped":true},
            {"address":"0xb","pid":20,"class":"steam","fullscreen":0,"focusHistoryID":1,"mapped":true},
            {"address":"0xc","pid":30,"class":"game.exe","fullscreen":0,"focusHistoryID":3,"mapped":true},
            {"address":"0xd","pid":30,"class":"game.exe","fullscreen":2,"focusHistoryID":2,"mapped":true},
            {"address":"0xe","pid":40,"class":"firefox","fullscreen":false,"focusHistoryID":4,"mapped":true}
        ]"#;
        assert_eq!(
            hypr_game(clients, &[30]).as_deref(),
            Some("0xd"),
            "full-screen first"
        );
        // Started by a launcher Arkor does not know of: the one in front last.
        assert_eq!(hypr_game(clients, &[99]).as_deref(), Some("0xd"));
        assert_eq!(
            hypr_game(clients, &[10]).as_deref(),
            Some("0xd"),
            "never Arkor"
        );
        assert_eq!(hypr_game("not json", &[30]), None);
    }

    #[test]
    fn picks_niris_windows() {
        let windows: Vec<Value> = serde_json::from_str(
            r#"[{"id":1,"app_id":"arkor","pid":10},{"id":7,"app_id":"steam","pid":20}]"#,
        )
        .unwrap();
        assert_eq!(niri_pick(&windows, |w| w["app_id"] == "steam"), Some(7));
        assert_eq!(niri_pick(&windows, |w| w["pid"] == 10u32), Some(1));
        assert_eq!(niri_pick(&windows, |w| w["pid"] == 99u32), None);
    }
}
