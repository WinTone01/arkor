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
    /// A button of the gamepad, from Arkor.
    Nav(String),
    /// The pointer is wanted, here.
    Pointer(f64, f64),
    /// A was pressed or let go (read from the gamepad directly).
    Press(bool),
}

/// What the page's script answered: `{"x":…,"y":…}`.
fn point(answer: &str) -> Option<(f64, f64)> {
    let number = |key: &str| -> Option<f64> {
        let rest = answer.split_once(&format!("\"{key}\":"))?.1;
        let end = rest.find([',', '}'])?;
        rest[..end].trim().parse().ok()
    };
    let (x, y) = (number("x")?, number("y")?);
    (x.is_finite() && y.is_finite()).then_some((x, y))
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
    let title = match store.as_str() {
        "epic" => "Epic Games",
        "gog" => "GOG",
        "amazon" => "Amazon Games",
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
    let event_loop = EventLoopBuilder::<Done>::with_user_event().build();
    let mut builder = WindowBuilder::new()
        .with_title(format!("Arkor · {title}"))
        .with_inner_size(LogicalSize::new(520.0, 760.0))
        .with_always_on_top(true);
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
    let builder = WebViewBuilder::new()
        .with_url(url)
        .with_initialization_script(&script)
        .with_initialization_script(if console {
            pad_script(turkish)
        } else {
            String::new()
        })
        .with_ipc_handler(move |req| {
            let text = req.body().to_string();
            if text.contains("authorizationCode") {
                let _ = proxy.send_event(Done::Code(text));
            }
        })
        .with_navigation_handler(move |to: String| {
            if carries_code(&store_nav, &to) {
                let _ = nav_proxy.send_event(Done::Code(to));
                return false;
            }
            true
        });

    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or("no window box")?;
        builder.build_gtk(vbox)?
    };
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window)?;

    let pointer_proxy = event_loop.create_proxy();
    let pointer = pointer::Pointer::new(gtk::prelude::Cast::upcast(wry::WebViewExtUnix::webview(
        &webview,
    )));
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        // The page shows the pointer's hints while it is out.
        let hints = |on: bool| {
            let _ =
                webview.evaluate_script(&format!("window.__arkor && window.__arkor.pointer({on})"));
        };
        match event {
            Event::UserEvent(Done::Press(down)) => pointer.press(down),
            Event::UserEvent(Done::Nav(nav)) => match nav.as_str() {
                // The menu button leaves the login.
                "menu" => std::process::exit(1),
                "back" | "extra" if pointer.out() => {
                    pointer.hide();
                    hints(false);
                }
                n if cfg!(debug_assertions) && n.contains(' ') => pointer::pretend(n),
                n if pointer.out() => {
                    // The gamepad is read directly; without that, Arkor's steps do.
                    if !pointer::direct() {
                        pointer.step(n);
                    }
                }
                n if n.chars().all(|c| c.is_ascii_lowercase()) => {
                    let proxy = pointer_proxy.clone();
                    let _ = webview.evaluate_script_with_callback(
                        &format!("window.__arkor && window.__arkor.nav({n:?})"),
                        move |answer| {
                            if let Some((x, y)) = point(&answer) {
                                let _ = proxy.send_event(Done::Pointer(x, y));
                            }
                        },
                    );
                }
                _ => {}
            },
            Event::UserEvent(Done::Pointer(x, y)) => {
                pointer.show(x, y);
                hints(true);
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
    fn reads_where_the_pointer_starts() {
        assert_eq!(point(r#"{"x":12.5,"y":40}"#), Some((12.5, 40.)));
        assert_eq!(point("null"), None);
        assert_eq!(point(""), None);
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
