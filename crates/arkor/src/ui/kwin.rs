//! Keeps console mode in front on KDE Plasma: Steam's own windows (its library, a file check,
//! news) are minimised and Arkor comes back to the front. Game windows Steam starts have a
//! class of their own (`steam_app_…`) and are left alone. Uses KWin's scripting over D-Bus.

use std::process::{Command, Stdio};

/// Stays loaded while it is wanted and only acts when a window appears or comes to the front,
/// so it costs nothing in between. It does nothing unless Arkor fills a screen: should Arkor
/// end without unloading it, Steam is left alone.
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
function away(w) {
    if (!isSteam(w)) { return; }
    var a = arkor();
    if (!a) { return; }
    if (!w.minimized) { w.minimized = true; }
    if (workspace.activeWindow !== undefined) { workspace.activeWindow = a; }
    else { workspace.activeClient = a; }
}
(workspace.windowAdded || workspace.clientAdded).connect(away);
(workspace.windowActivated || workspace.clientActivated).connect(away);
var list = all();
for (var i = 0; i < list.length; i++) { if (isSteam(list[i]) && !list[i].minimized) { away(list[i]); } }
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
