//! `arkor-login <epic|gog|amazon> <url>`: the store's login page in a window of its own (as
//! Heroic does it). When the store hands over its login code, it is printed on standard
//! output and the window closes; nothing has to be copied by hand.
//!
//! Exit codes: 0 with the code printed, 1 when the window was closed, 2 when no browser
//! window could be opened (Arkor then falls back to the user's browser).
//!
//! With `--console` (Arkor's console mode) it fills the screen, and the page gets a gamepad
//! way to move between its fields and an on-screen keyboard: Arkor sends the gamepad's
//! buttons as lines on standard input (`up`, `accept`, `back`, …). A pointer moved with the
//! gamepad reaches what the page's script cannot (a "not a robot" box in a frame of its own):
//! each click is one press of the user's button, nothing is clicked by itself.
//!
//! `arkor-login browse <url>` is a mod browser instead (Nexus Mods, Thunderstore): its
//! logins are kept (cookies in `~/.local/share/arkor/browser`) and it stays open. What the
//! sites hand to a mod manager is printed, one line each, for Arkor to install:
//! `nxm <link>` (Nexus' "Mod Manager Download"), `ts <namespace> <name> <version>`
//! (Thunderstore's "Install with Mod Manager") and `file <path>` (a file downloaded by hand,
//! saved in `~/Downloads/Arkor`).
//!
//! It is a program of its own because Arkor's window toolkit cannot hold a browser.

mod pointer;

use std::io::Write;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

/// Epic shows the code as a page of JSON at this address.
const EPIC_DONE: &str = "https://www.epicgames.com/id/api/redirect";

enum Done {
    Code(String),
    /// A line for Arkor; the browser stays open.
    Line(String),
    /// A page that asked for a window of its own: shown here instead.
    Open(String),
    /// A page is drawn: the gamepad's pointer goes on it.
    Ready,
    /// A button of the gamepad, from Arkor.
    Nav(String),
    /// The page's on-screen keyboard opened (`true`) or closed: it has the gamepad then.
    Keyboard(bool),
    /// A was pressed or let go (read from the gamepad directly).
    Press(bool),
}

/// The page's gamepad script, with its words in the user's language.
fn pad_script(turkish: bool) -> String {
    let labels = if turkish {
        r#"{"choose":"Seç","back":"Geri","close":"Kapat","space":"Boşluk","done":"Tamam","pointer":"İmleç","click":"Tıkla","leave":"İmleçten çık"}"#
    } else {
        r#"{"choose":"Choose","back":"Back","close":"Close","space":"Space","done":"Done","pointer":"Pointer","click":"Click","leave":"Leave the pointer"}"#
    };
    include_str!("pad.js").replace("__LABELS__", labels)
}

/// Where the mod browser may start.
fn browsable(url: &str) -> bool {
    [
        "https://www.nexusmods.com/",
        "https://users.nexusmods.com/",
        "https://next.nexusmods.com/",
        "https://thunderstore.io/",
    ]
    .iter()
    .any(|p| url.starts_with(p))
}

/// What the mod browser tells Arkor about a page it was sent to; `None`: an ordinary page.
/// `Some(None)`: not for a browser (another program's link), so it is not followed.
fn handoff(url: &str) -> Option<Option<String>> {
    if url.starts_with("nxm://") {
        return Some(Some(format!("nxm {url}")));
    }
    // ror2mm://v1/install/thunderstore.io/<namespace>/<name>/<version>/
    if let Some(rest) = url.strip_prefix("ror2mm://v1/install/") {
        let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
        // Thunderstore's names: letters, digits and `_`, versions with dots; never `..`.
        let plain = |s: &&str| {
            s.starts_with(|c: char| c.is_ascii_alphanumeric())
                && !s.contains("..")
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        };
        return Some(match parts.as_slice() {
            [_, ns, name, version] if [ns, name, version].iter().all(|p| plain(p)) => {
                Some(format!("ts {ns} {name} {version}"))
            }
            _ => None,
        });
    }
    let web = ["https://", "http://", "about:", "blob:", "data:"];
    (!web.iter().any(|w| url.starts_with(w))).then_some(None)
}

/// Where a file downloaded by hand in the mod browser goes.
fn downloads_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    let base = std::env::var_os("XDG_DOWNLOAD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(&home).join("Downloads"));
    Some(base.join("Arkor"))
}

/// Whether `url` is where the store hands over the code (GOG and Amazon put it in the
/// address).
fn carries_code(store: &str, url: &str) -> bool {
    match store {
        "gog" => url.starts_with("https://embed.gog.com/on_login_success") && url.contains("code="),
        "amazon" => url.contains("openid.oa2.authorization_code="),
        _ => false,
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let console = args.iter().any(|a| a == "--console");
    let turkish = args.iter().any(|a| a == "--tr");
    args.retain(|a| !a.starts_with("--"));
    let (Some(store), Some(url)) = (args.first().cloned(), args.get(1).cloned()) else {
        eprintln!("usage: arkor-login <epic|gog|amazon> <url>");
        std::process::exit(2);
    };
    // Debug builds also open a page of this computer, to try things out.
    let local = cfg!(debug_assertions) && url.starts_with("http://127.0.0.1:");
    if !url.starts_with("https://") && !local {
        std::process::exit(2);
    }
    if store == "browse" && !browsable(&url) && !local {
        std::process::exit(2);
    }
    let title = match store.as_str() {
        "epic" => "Epic Games",
        "gog" => "GOG",
        "amazon" => "Amazon Games",
        "browse" => "Mods",
        _ => std::process::exit(2),
    };
    let result = std::panic::catch_unwind(|| run(&store, &url, title, console, turkish));
    // A missing WebKitGTK shows up as a failure (or panic) here.
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("arkor-login: {e}");
            std::process::exit(2);
        }
        Err(_) => std::process::exit(2),
    }
}

fn run(
    store: &str,
    url: &str,
    title: &str,
    console: bool,
    turkish: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let browse = store == "browse";
    let event_loop = EventLoopBuilder::<Done>::with_user_event().build();
    let mut builder = WindowBuilder::new()
        .with_title(format!("Arkor · {title}"))
        .with_inner_size(if browse {
            LogicalSize::new(1280.0, 860.0)
        } else {
            LogicalSize::new(520.0, 760.0)
        })
        // Shown at once: a window that starts hidden is never drawn by WebKit on some machines
        // (it stayed grey on NVIDIA). The page's dark background covers the wait instead.
        .with_always_on_top(!browse);
    if console {
        builder = builder.with_fullscreen(Some(tao::window::Fullscreen::Borderless(None)));
    }
    let window = builder.build(&event_loop)?;
    if console {
        let presses = event_loop.create_proxy();
        pointer::read_gamepad(move |down| presses.send_event(Done::Press(down)).is_ok());
        // Arkor's gamepad buttons, one per line.
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if proxy
                    .send_event(Done::Nav(line.trim().to_string()))
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    let proxy = event_loop.create_proxy();
    let nav_proxy = event_loop.create_proxy();
    let store_nav = store.to_string();
    // Epic: once on the page with the code, the page's text is handed to us.
    let script = format!(
        r#"(function() {{
            if (!location.href.startsWith("{EPIC_DONE}")) return;
            const send = () => window.ipc.postMessage(document.body ? document.body.innerText : "");
            if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", send);
            else send();
        }})();"#
    );
    // The mod browser keeps its logins; a store's login window starts afresh every time.
    let mut context = wry::WebContext::new(std::env::var_os("HOME").filter(|_| browse).map(|h| {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(&h).join(".local/share"))
            .join("arkor/browser")
    }));
    let builder = if browse {
        WebViewBuilder::new_with_web_context(&mut context)
    } else {
        WebViewBuilder::new()
    };
    let ready_proxy = event_loop.create_proxy();
    let line_proxy = event_loop.create_proxy();
    let open_proxy = event_loop.create_proxy();
    let builder = builder
        // Dark until the page draws itself (no white flash).
        .with_background_color((12, 11, 18, 255))
        .with_on_page_load_handler(move |event, _| {
            // Every page gets the pointer drawn.
            if matches!(event, wry::PageLoadEvent::Finished) {
                let _ = ready_proxy.send_event(Done::Ready);
            }
        })
        .with_url(url)
        .with_initialization_script(&script)
        .with_initialization_script(if console {
            pad_script(turkish)
        } else {
            String::new()
        })
        .with_ipc_handler(move |req| {
            let text = req.body().to_string();
            match text.as_str() {
                "arkor:kbd:1" => {
                    let _ = proxy.send_event(Done::Keyboard(true));
                }
                "arkor:kbd:0" => {
                    let _ = proxy.send_event(Done::Keyboard(false));
                }
                _ if text.contains("authorizationCode") => {
                    let _ = proxy.send_event(Done::Code(text));
                }
                _ => {}
            }
        })
        .with_navigation_handler(move |to: String| {
            if store_nav == "browse" {
                return match handoff(&to) {
                    None => true,
                    Some(line) => {
                        if let Some(line) = line {
                            let _ = nav_proxy.send_event(Done::Line(line));
                        }
                        false
                    }
                };
            }
            if carries_code(&store_nav, &to) {
                let _ = nav_proxy.send_event(Done::Code(to));
                return false;
            }
            true
        });
    let builder = if browse {
        builder
            // Links that open a window of their own open here (one window for the gamepad).
            .with_new_window_req_handler(move |to, _| {
                let _ = open_proxy.send_event(Done::Open(to));
                wry::NewWindowResponse::Deny
            })
            .with_download_started_handler(|url, path| {
                let Some(dir) = downloads_dir() else {
                    return false;
                };
                let _ = std::fs::create_dir_all(&dir);
                let name = path
                    .file_name()
                    .map(|n| n.to_owned())
                    .or_else(|| {
                        url.split(['?', '#'])
                            .next()
                            .and_then(|u| u.rsplit('/').next())
                            .filter(|n| !n.is_empty())
                            .map(Into::into)
                    })
                    .unwrap_or_else(|| "mod.zip".into());
                *path = dir.join(name);
                true
            })
            .with_download_completed_handler(move |_, path, ok| {
                if let (true, Some(path)) = (ok, path) {
                    let _ = line_proxy.send_event(Done::Line(format!("file {}", path.display())));
                }
            })
    } else {
        let _ = (line_proxy, open_proxy);
        builder
    };

    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or("no window box")?;
        builder.build_gtk(vbox)?
    };
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window)?;
    let webview = std::rc::Rc::new(webview);

    // The gamepad's pointer, drawn by the page's script (see `pad.js`).
    let drawn = webview.clone();
    let pointer = pointer::Pointer::new(
        gtk::prelude::Cast::upcast(wry::WebViewExtUnix::webview(&*webview)),
        move |(x, y), down, on| {
            let _ = drawn.evaluate_script(&format!(
                "window.__arkor && window.__arkor.cursor({x:.1},{y:.1},{down},{on})"
            ));
        },
    );
    // The page's keyboard has the gamepad.
    let mut keyboard = false;
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Done::Press(down)) => {
                if !keyboard {
                    pointer.press(down);
                }
            }
            Event::UserEvent(Done::Nav(nav)) => match nav.as_str() {
                // The menu button leaves the window.
                "menu" => std::process::exit(1),
                n if cfg!(debug_assertions) && n.contains(' ') => pointer::pretend(n),
                // The keyboard's keys.
                n if keyboard && n.chars().all(|c| c.is_ascii_lowercase()) => {
                    let _ = webview
                        .evaluate_script(&format!("window.__arkor && window.__arkor.nav({n:?})"));
                }
                "back" => {
                    let _ = webview.evaluate_script("history.back()");
                }
                n if pointer.out() => {
                    // The gamepad is read directly; without that, Arkor's steps do.
                    if !pointer::direct() {
                        pointer.step(n);
                    }
                }
                _ => {}
            },
            Event::UserEvent(Done::Keyboard(open)) => {
                keyboard = open;
                if !console {
                } else if open {
                    pointer.hide();
                } else {
                    let at = pointer.at();
                    pointer.show(at.0, at.1);
                }
            }
            Event::UserEvent(Done::Ready) => {
                // Console mode: the pointer is there from the start, in the middle.
                if console && !keyboard {
                    if pointer.out() {
                        pointer.repaint();
                    } else {
                        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
                        pointer.show(size.width / 2., size.height / 2.);
                    }
                    let _ =
                        webview.evaluate_script("window.__arkor && window.__arkor.pointer(true)");
                }
            }
            Event::UserEvent(Done::Line(line)) => {
                let mut out = std::io::stdout();
                let _ = writeln!(out, "{line}");
                let _ = out.flush();
            }
            Event::UserEvent(Done::Open(to)) => {
                if to.starts_with("https://") {
                    let _ = webview.load_url(&to);
                }
            }
            Event::UserEvent(Done::Code(code)) => {
                let mut out = std::io::stdout();
                let _ = writeln!(out, "{code}");
                let _ = out.flush();
                std::process::exit(0);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => std::process::exit(1),
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_mod_manager_links_to_arkor() {
        assert_eq!(
            handoff("nxm://repo/mods/1/files/81?key=k&expires=1"),
            Some(Some(
                "nxm nxm://repo/mods/1/files/81?key=k&expires=1".into()
            ))
        );
        assert_eq!(
            handoff("ror2mm://v1/install/thunderstore.io/BepInEx/BepInExPack/5.4.2100/"),
            Some(Some("ts BepInEx BepInExPack 5.4.2100".into()))
        );
        assert_eq!(
            handoff("ror2mm://v1/install/x/../a/b/"),
            Some(None),
            "only plain names"
        );
        assert_eq!(
            handoff("steam://run/1"),
            Some(None),
            "other programs' links stay here"
        );
        assert_eq!(handoff("https://www.nexusmods.com/repo/mods/1"), None);
        assert!(browsable("https://thunderstore.io/c/repo/"));
        assert!(!browsable("https://example.com/"));
    }

    #[test]
    fn knows_where_the_code_is() {
        assert!(carries_code(
            "gog",
            "https://embed.gog.com/on_login_success?origin=client&code=abc"
        ));
        assert!(!carries_code("gog", "https://login.gog.com/"));
        assert!(carries_code(
            "amazon",
            "https://www.amazon.com/?openid.oa2.authorization_code=AN1&x=1"
        ));
        assert!(!carries_code("epic", "https://www.epicgames.com/id/login"));
    }
}
