//! Links that start games or open Arkor's pages, from a browser, a desktop shortcut or
//! another program:
//!
//! - `arkor://launch/<id>` starts a library game (`local:<id>` or the bare id);
//! - `arkor://open/<page>` opens Arkor on a page (as `arkor open <page>`);
//! - `heroic://launch/<runner>/<appName>` and `heroic://launch?appName=…&runner=…`, the
//!   links Heroic Games Launcher makes, start the library game of that store id.
//!
//! Arkor's own scheme is registered when the window opens; `heroic://` only when Heroic is
//! not installed (its links are Heroic's to take).

use std::path::PathBuf;

use anyhow::Context;
use arkor_core::GameId;
use arkor_core::library::{GameKind, Library};

/// What a link asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Launch(GameId),
    Open(String),
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The library game Heroic's `runner` and `app_name` mean.
fn heroic_game(library: &Library, runner: &str, app_name: &str) -> Option<GameId> {
    library
        .games
        .iter()
        .find(|g| {
            let origin = g.origin.as_deref().unwrap_or_default();
            match &g.kind {
                GameKind::Epic { app_name: a } => a == app_name,
                GameKind::Amazon { id } => id == app_name,
                _ => {
                    origin.ends_with(&format!(":{app_name}"))
                        && (runner.is_empty()
                            || origin.starts_with(runner)
                            || origin.starts_with("heroic")
                            || origin.starts_with("gog"))
                }
            }
        })
        .map(|g| g.game_id())
}

/// Reads a link.
pub fn parse(link: &str, library: &Library) -> anyhow::Result<Link> {
    let link = link.trim();
    if let Some(rest) = link.strip_prefix("arkor://") {
        let (what, arg) = rest.split_once('/').unwrap_or((rest, ""));
        let arg = decode(arg.trim_end_matches('/'));
        return match what {
            "launch" | "run" | "play" => {
                let id = if arg.contains(':') {
                    arg.parse()?
                } else {
                    GameId::new(arkor_core::Store::Local, &arg)
                };
                anyhow::ensure!(
                    library.get(&id.id).is_some(),
                    "`{arg}` is not in Arkor's library"
                );
                Ok(Link::Launch(id))
            }
            "open" => Ok(Link::Open(arg)),
            other => anyhow::bail!("Arkor has no `{other}` link"),
        };
    }
    if let Some(rest) = link.strip_prefix("heroic://") {
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let mut parts = path.trim_end_matches('/').split('/');
        anyhow::ensure!(parts.next() == Some("launch"), "not a Heroic launch link");
        let (mut runner, mut app) = (
            parts.next().map(decode).unwrap_or_default(),
            parts.next().map(decode).unwrap_or_default(),
        );
        if app.is_empty() && !runner.is_empty() && query.is_empty() {
            std::mem::swap(&mut runner, &mut app);
        }
        for pair in query.split('&') {
            match pair.split_once('=') {
                Some(("appName", v)) => app = decode(v),
                Some(("runner", v)) => runner = decode(v),
                _ => {}
            }
        }
        anyhow::ensure!(!app.is_empty(), "the Heroic link names no game");
        let id = heroic_game(library, &runner, &app)
            .with_context(|| format!("no game {app} in Arkor's library"))?;
        return Ok(Link::Launch(id));
    }
    anyhow::bail!("not a link Arkor takes: {link}")
}

const HANDLER: &str = "arkor-url.desktop";

fn handler_file() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("applications").join(HANDLER))
}

fn default_for(scheme: &str) -> String {
    std::process::Command::new("xdg-mime")
        .args(["query", "default", &format!("x-scheme-handler/{scheme}")])
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Registers Arkor for `arkor://` links (and `heroic://` without Heroic). Quick when it is
/// already done. Blocking.
pub fn register() {
    let (Ok(exe), Some(path)) = (std::env::current_exe(), handler_file()) else {
        return;
    };
    let heroic = default_for("heroic");
    let take_heroic = heroic.is_empty() || heroic == HANDLER;
    let schemes = if take_heroic {
        "x-scheme-handler/arkor;x-scheme-handler/heroic;"
    } else {
        "x-scheme-handler/arkor;"
    };
    let quoted = format!("\"{}\"", exe.display().to_string().replace('"', "\\\""));
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Arkor\nExec={quoted} url %u\nIcon=arkor\nNoDisplay=true\nTerminal=false\nMimeType={schemes}\n"
    );
    if std::fs::read_to_string(&path).ok().as_deref() == Some(entry.as_str())
        && default_for("arkor") == HANDLER
    {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(&path, entry).is_err() {
        return;
    }
    let _ = std::process::Command::new("update-desktop-database")
        .arg(path.parent().unwrap_or(std::path::Path::new(".")))
        .output();
    let mut args = vec!["default", HANDLER, "x-scheme-handler/arkor"];
    if take_heroic {
        args.push("x-scheme-handler/heroic");
    }
    let _ = std::process::Command::new("xdg-mime").args(&args).output();
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkor_core::library::{LibraryGame, NewGame};

    #[test]
    fn reads_arkor_and_heroic_links() {
        let mut lib = Library::default();
        let mut epic = NewGame::windows("Fall Guys", "/g".into());
        epic.kind = GameKind::Epic {
            app_name: "0a2d9f6403244d12969e11da6713137b".into(),
        };
        lib.add(epic);
        let mut gog = NewGame::windows("Cyberpunk 2077", "/g/c.exe".into());
        gog.origin = Some("gog:1423049311".into());
        lib.add(gog);
        let _: Option<&LibraryGame> = lib.get("fall-guys");
        assert_eq!(
            parse("arkor://launch/fall-guys", &lib).unwrap(),
            Link::Launch(GameId::new(arkor_core::Store::Local, "fall-guys"))
        );
        assert_eq!(
            parse("arkor://open/console%23library", &lib).unwrap(),
            Link::Open("console#library".into())
        );
        assert!(parse("arkor://launch/nothing", &lib).is_err());
        assert_eq!(
            parse(
                "heroic://launch/legendary/0a2d9f6403244d12969e11da6713137b",
                &lib
            )
            .unwrap(),
            Link::Launch(GameId::new(arkor_core::Store::Local, "fall-guys"))
        );
        assert_eq!(
            parse("heroic://launch?appName=1423049311&runner=gog", &lib).unwrap(),
            Link::Launch(GameId::new(arkor_core::Store::Local, "cyberpunk-2077"))
        );
        assert!(parse("https://example.com", &lib).is_err());
    }
}
