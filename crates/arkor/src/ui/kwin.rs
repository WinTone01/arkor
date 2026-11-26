//! Keeps console mode in front on KDE Plasma: Steam's own windows (its library, a file check,
//! news) are minimised and Arkor comes back to the front. Game windows Steam starts have a
//! class of their own (`steam_app_…`) and are left alone. Uses KWin's scripting over D-Bus.

use std::process::{Command, Stdio};

/// Stays loaded while it is wanted and only acts when a window appears, comes to the front or
/// is no longer minimised, so it costs nothing in between. It does nothing unless Arkor fills
/// a screen: should Arkor end without unloading it, Steam is left alone.
///
/// What was in front comes back: Arkor, or the game (the pad's middle button makes Steam come
/// forward by itself). Steam asks for the front more than once, so it is looked at again a
/// little later.
const SCRIPT: &str = r#"
function all() { return workspace.windowList ? workspace.windowList() : workspace.clientList(); }
function kind(w) { return w ? String(w.resourceClass).toLowerCase() : ""; }
function isSteam(w) { var k = kind(w); return k === "steam" || k === "steamwebhelper"; }
function arkor() {
    var list = all();
    for (var i = 0; i < list.length; i++) {
        if (kind(list[i]) === "arkor" && list[i].fullScreen) { return list[i]; }
    }
    return null;
}
var last = null;
function back(a) {
    var list = all();
    for (var i = 0; i < list.length; i++) {
        if (last !== null && list[i] === last && !last.minimized) { return last; }
    }
    return a;
}
function away(w) {
    if (!isSteam(w)) { return; }
    var a = arkor();
    if (!a) { return; }
    if (!w.minimized) { w.minimized = true; }
    var to = back(a);
    if (workspace.activeWindow !== undefined) { workspace.activeWindow = to; }
    else { workspace.activeClient = to; }
}
function open(w) {
    var list = all();
    for (var i = 0; i < list.length; i++) {
        if (list[i] === w) { return !w.minimized; }
    }
    return false;
}
function later(w, ms) {
    var t = new QTimer();
    t.singleShot = true;
    t.interval = ms;
    t.timeout.connect(function () { if (open(w)) { away(w); } t.stop(); });
    t.start();
}
function seen(w) {
    if (!isSteam(w)) { return; }
    away(w);
    later(w, 60);
    later(w, 400);
}
function watch(w) {
    if (!isSteam(w)) { return; }
    if (w.minimizedChanged) {
        w.minimizedChanged.connect(function () { if (!w.minimized) { seen(w); } });
    }
}
function added(w) { watch(w); seen(w); }
function activated(w) {
    if (!w) { return; }
    if (isSteam(w)) { seen(w); }
    else if (w.normalWindow && (w.fullScreen || kind(w) === "arkor")) { last = w; }
}
(workspace.windowAdded || workspace.clientAdded).connect(added);
(workspace.windowActivated || workspace.clientActivated).connect(activated);
activated(workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient);
var list = all();
for (var i = 0; i < list.length; i++) {
    watch(list[i]);
    if (isSteam(list[i]) && !list[i].minimized) { away(list[i]); }
}
"#;

const NAME: &str = "arkor-steam-back";

/// Whether KWin can be scripted here.
pub(super) fn available() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_uppercase().contains("KDE"))
        && which("gdbus")
}

fn which(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

fn gdbus(args: &[&str]) -> Option<String> {
    let out = Command::new("gdbus")
        .args(["call", "--session", "--dest", "org.kde.KWin"])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Brings Steam's windows to the front, once.
const SHOW: &str = r#"
var list = workspace.windowList ? workspace.windowList() : workspace.clientList();
for (var i = 0; i < list.length; i++) {
    var k = String(list[i].resourceClass).toLowerCase();
    if (k === "steam" || k === "steamwebhelper") {
        list[i].minimized = false;
        if (workspace.activeWindow !== undefined) { workspace.activeWindow = list[i]; }
        else { workspace.activeClient = list[i]; }
    }
}
"#;

/// Loads and runs a script under a name (instead of what had that name).
fn run_script(name: &str, code: &str) {
    let Some(dir) = dirs::runtime_dir().or_else(dirs::cache_dir) else {
        return;
    };
    let path = dir.join(format!("{name}.js"));
    if std::fs::write(&path, code).is_err() {
        return;
    }
    unload(name);
    let Some(reply) = gdbus(&[
        "--object-path",
        "/Scripting",
        "--method",
        "org.kde.kwin.Scripting.loadScript",
        &path.to_string_lossy(),
        name,
    ]) else {
        return;
    };
    // "(3,)"
    let id: String = reply.chars().filter(|c| c.is_ascii_digit()).collect();
    if id.is_empty() {
        return;
    }
    for object in [format!("/Scripting/Script{id}"), format!("/{id}")] {
        if gdbus(&[
            "--object-path",
            &object,
            "--method",
            "org.kde.kwin.Script.run",
        ])
        .is_some()
        {
            break;
        }
    }
}

fn unload(name: &str) {
    let _ = gdbus(&[
        "--object-path",
        "/Scripting",
        "--method",
        "org.kde.kwin.Scripting.unloadScript",
        name,
    ]);
}

/// Steam's windows are put away from now on. Blocking (a few D-Bus calls).
pub(super) fn guard() {
    run_script(NAME, SCRIPT);
}

/// Steam's windows are left alone again. Blocking.
pub(super) fn unguard() {
    unload(NAME);
}

/// Steam's windows come to the front. Blocking.
pub(super) fn show_steam() {
    if !available() {
        return;
    }
    unguard();
    run_script("arkor-steam-show", SHOW);
    unload("arkor-steam-show");
}

/// Brings a running game's window to the front: one of `pids`', else the window that was in
/// front before Arkor (not Steam's, not the desktop's). Blocking.
pub(super) fn show_game(pids: &[u32]) -> bool {
    if !available() {
        return false;
    }
    let list = pids
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let code = format!(
        r#"
var pids = [{list}];
var all = workspace.windowList ? workspace.windowList() : workspace.clientList();
function kind(w) {{ return String(w.resourceClass).toLowerCase(); }}
function usable(w) {{
    var k = kind(w);
    return w.normalWindow && k !== "arkor" && k !== "steam" && k !== "steamwebhelper"
        && k !== "plasmashell";
}}
var best = null;
for (var i = 0; i < all.length; i++) {{
    var w = all[i];
    if (pids.indexOf(w.pid) >= 0 && usable(w) && (best === null || w.fullScreen)) {{ best = w; }}
}}
if (best === null && workspace.stackingOrder) {{
    var order = workspace.stackingOrder;
    for (var j = order.length - 1; j >= 0; j--) {{
        if (usable(order[j])) {{ best = order[j]; break; }}
    }}
}}
if (best !== null) {{
    best.minimized = false;
    if (workspace.activeWindow !== undefined) {{ workspace.activeWindow = best; }}
    else {{ workspace.activeClient = best; }}
}}
"#
    );
    run_script("arkor-show-game", &code);
    unload("arkor-show-game");
    true
}

/// Brings Arkor's window to the front (a request of its own may be refused while a game is
/// in front). Blocking.
pub(super) fn show_arkor() {
    if !available() {
        return;
    }
    run_script(
        "arkor-show-self",
        r#"
var all = workspace.windowList ? workspace.windowList() : workspace.clientList();
for (var i = 0; i < all.length; i++) {
    if (String(all[i].resourceClass).toLowerCase() === "arkor" && all[i].normalWindow) {
        all[i].minimized = false;
        if (workspace.activeWindow !== undefined) { workspace.activeWindow = all[i]; }
        else { workspace.activeClient = all[i]; }
        break;
    }
}
"#,
    );
    unload("arkor-show-self");
}
