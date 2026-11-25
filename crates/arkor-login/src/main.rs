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

use std::io::Write;
use std::time::{Duration, Instant};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

/// A common desktop browser: some login pages refuse what they do not know.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

/// Epic shows the code as a page of JSON at this address.
const EPIC_DONE: &str = "https://www.epicgames.com/id/api/redirect";

enum Done {
    Code(String),
    /// A button of the gamepad, from Arkor.
    Nav(String),
    /// The pointer is wanted, here.
    Pointer(f64, f64),
}

/// The pointer moved with the gamepad.
struct Pointer {
    x: f64,
    y: f64,
    /// The last direction, when, and how often in a row: held, it gets faster.
    last: Option<(String, Instant, u32)>,
}

impl Pointer {
    /// Moves one step; `false` when `nav` is not a direction.
    fn step(&mut self, nav: &str, now: Instant, (w, h): (f64, f64)) -> bool {
        let (dx, dy) = match nav {
            "up" => (0., -1.),
            "down" => (0., 1.),
            "left" => (-1., 0.),
            "right" => (1., 0.),
            _ => return false,
        };
        let run = match &self.last {
            Some((n, t, run)) if n == nav && now - *t < Duration::from_millis(200) => run + 1,
            _ => 0,
        };
        self.last = Some((nav.to_string(), now, run));
        let step = (8. + 7. * f64::from(run)).min(64.);
        self.x = (self.x + dx * step).clamp(0., w - 1.);
        self.y = (self.y + dy * step).clamp(0., h - 1.);
        true
    }
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

/// Real pointer events in the page, as from a mouse.
#[cfg(target_os = "linux")]
mod mouse {
    use gtk::gdk::ffi;
    use gtk::glib::translate::ToGlibPtr;
    use gtk::prelude::*;

    fn send(widget: &gtk::Widget, kind: ffi::GdkEventType, x: f64, y: f64, pressed: bool) {
        let (Some(window), Some(pointer)) = (
            widget.window(),
            widget.display().default_seat().and_then(|s| s.pointer()),
        ) else {
            return;
        };
        let (_, ox, oy) = window.origin();
        let state = if pressed { ffi::GDK_BUTTON1_MASK } else { 0 };
        // SAFETY: the event is made by GDK for this kind, only the fields of that kind are
        // written, the window gets the reference the event gives back when it is freed.
        unsafe {
            let event = ffi::gdk_event_new(kind);
            let win: *mut ffi::GdkWindow = window.to_glib_full();
            if kind == ffi::GDK_MOTION_NOTIFY {
                let e = &mut *(event as *mut ffi::GdkEventMotion);
                (e.window, e.x, e.y, e.state) = (win, x, y, state);
                (e.x_root, e.y_root) = (f64::from(ox) + x, f64::from(oy) + y);
                e.time = (gtk::glib::monotonic_time() / 1000) as u32;
            } else {
                let e = &mut *(event as *mut ffi::GdkEventButton);
                (e.window, e.x, e.y, e.state, e.button) = (win, x, y, state, 1);
                (e.x_root, e.y_root) = (f64::from(ox) + x, f64::from(oy) + y);
                e.time = (gtk::glib::monotonic_time() / 1000) as u32;
            }
            let device: *mut ffi::GdkDevice = pointer.to_glib_none().0;
            ffi::gdk_event_set_device(event, device);
            gtk::ffi::gtk_widget_event(widget.to_glib_none().0, event);
            ffi::gdk_event_free(event);
        }
    }

    pub fn moved(widget: &gtk::Widget, x: f64, y: f64) {
        send(widget, ffi::GDK_MOTION_NOTIFY, x, y, false);
    }

    pub fn click(widget: &gtk::Widget, x: f64, y: f64) {
        moved(widget, x, y);
        send(widget, ffi::GDK_BUTTON_PRESS, x, y, false);
        send(widget, ffi::GDK_BUTTON_RELEASE, x, y, true);
    }
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
        .with_user_agent(USER_AGENT)
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
    let mut pointer: Option<Pointer> = None;
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Done::Nav(nav)) => match nav.as_str() {
                // The menu button leaves the login.
                "menu" => std::process::exit(1),
                n if pointer.is_some() => {
                    use wry::WebViewExtUnix;
                    let Some(p) = pointer.as_mut() else { return };
                    let page = gtk::prelude::Cast::upcast::<gtk::Widget>(webview.webview());
                    let size = {
                        use gtk::prelude::WidgetExt;
                        (
                            f64::from(page.allocated_width()),
                            f64::from(page.allocated_height()),
                        )
                    };
                    let draw = |x: &str, y: &str| {
                        let _ = webview.evaluate_script(&format!(
                            "window.__arkor && window.__arkor.pointer({x}, {y})"
                        ));
                    };
                    if p.step(n, Instant::now(), size) {
                        mouse::moved(&page, p.x, p.y);
                        draw(&p.x.to_string(), &p.y.to_string());
                    } else if n == "accept" {
                        mouse::click(&page, p.x, p.y);
                        // After a click the page may be another one: drawn again.
                        draw(&p.x.to_string(), &p.y.to_string());
                    } else if matches!(n, "back" | "extra") {
                        pointer = None;
                        draw("null", "null");
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
                pointer = Some(Pointer { x, y, last: None });
                let _ = webview.evaluate_script(&format!(
                    "window.__arkor && window.__arkor.pointer({x}, {y})"
                ));
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
    fn the_pointer_speeds_up_while_held_and_stays_inside() {
        let mut p = Pointer {
            x: 50.,
            y: 50.,
            last: None,
        };
        let t = Instant::now();
        let ms = Duration::from_millis;
        assert!(p.step("right", t, (100., 100.)));
        assert_eq!(p.x, 58.);
        assert!(p.step("right", t + ms(95), (100., 100.)));
        assert_eq!(p.x, 73.);
        // Let go, then pressed again: a small step.
        assert!(p.step("right", t + ms(600), (100., 100.)));
        assert_eq!(p.x, 81.);
        for n in 0..9 {
            p.step("up", t + ms(700 + n * 90), (100., 100.));
        }
        assert_eq!(p.y, 0.);
        assert!(!p.step("accept", t, (100., 100.)));
    }

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
