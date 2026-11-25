//! The pointer moved with the gamepad, for what the page's script cannot reach. The left stick
//! moves it (any direction, faster the further it is pushed), the right one scrolls, the
//! d-pad moves it too; A is its button, down as long as it is held. Everything it does in the
//! page is what the user's hand does on the gamepad: nothing is moved or clicked by itself.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gilrs::{Axis, Button, EventType, Gilrs};
use gtk::gdk::ffi;
use gtk::glib;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;

/// How the sticks and the d-pad are held now.
#[derive(Debug, Clone, Copy)]
struct Held {
    left: (f64, f64),
    scroll: f64,
    up: bool,
    down: bool,
    to_left: bool,
    to_right: bool,
}

static HELD: Mutex<Held> = Mutex::new(Held {
    left: (0., 0.),
    scroll: 0.,
    up: false,
    down: false,
    to_left: false,
    to_right: false,
});
/// The gamepad answers to this window itself.
static DIRECT: AtomicBool = AtomicBool::new(false);

/// Whether the gamepad is read here (and not only through Arkor).
pub fn direct() -> bool {
    DIRECT.load(Ordering::Relaxed)
}

fn held<T>(f: impl FnOnce(&mut Held) -> T) -> T {
    f(&mut HELD.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Reads the gamepad on a thread; `press` hears of A, until it answers `false`.
pub fn read_gamepad(press: impl Fn(bool) -> bool + Send + 'static) {
    std::thread::spawn(move || {
        let Ok(mut gilrs) = Gilrs::new() else { return };
        loop {
            let Some(event) = gilrs.next_event_blocking(Some(Duration::from_secs(5))) else {
                continue;
            };
            let (button, down) = match event.event {
                EventType::ButtonPressed(b, _) => (b, true),
                EventType::ButtonReleased(b, _) => (b, false),
                EventType::AxisChanged(axis, value, _) => {
                    let v = f64::from(value);
                    held(|h| match axis {
                        Axis::LeftStickX => h.left.0 = v,
                        Axis::LeftStickY => h.left.1 = -v,
                        Axis::RightStickY => h.scroll = -v,
                        _ => {}
                    });
                    DIRECT.store(true, Ordering::Relaxed);
                    continue;
                }
                // Nothing stays held by a gamepad that is gone.
                EventType::Disconnected => {
                    held(|h| {
                        *h = Held {
                            left: (0., 0.),
                            scroll: 0.,
                            up: false,
                            down: false,
                            to_left: false,
                            to_right: false,
                        }
                    });
                    if !press(false) {
                        return;
                    }
                    continue;
                }
                _ => continue,
            };
            DIRECT.store(true, Ordering::Relaxed);
            held(|h| match button {
                Button::DPadUp => h.up = down,
                Button::DPadDown => h.down = down,
                Button::DPadLeft => h.to_left = down,
                Button::DPadRight => h.to_right = down,
                _ => {}
            });
            if button == Button::South && !press(down) {
                return;
            }
        }
    });
}

/// Debug builds: `stick 0.7 -0.3` holds the left stick there, as a gamepad would.
pub fn pretend(line: &str) {
    let v: Vec<f64> = line
        .split(' ')
        .skip(1)
        .filter_map(|n| n.parse().ok())
        .collect();
    if let [x, y] = v[..] {
        DIRECT.store(true, Ordering::Relaxed);
        held(|h| h.left = (x, y));
    }
}

/// Where the pointer goes (pixels a second) for how far the stick is pushed: slow near the
/// middle to aim, fast at the edge, the same in every direction.
fn velocity((x, y): (f64, f64), full: f64) -> (f64, f64) {
    const DEAD: f64 = 0.12;
    let far = x.hypot(y);
    if far <= DEAD {
        return (0., 0.);
    }
    let k = ((far.min(1.) - DEAD) / (1. - DEAD)).powf(1.8) * full / far;
    (x * k, y * k)
}

struct State {
    at: (f64, f64),
    out: bool,
    /// A is held down in the page.
    down: bool,
    /// The last frame, and how long the d-pad has been held.
    frame: Option<i64>,
    pad_held: f64,
    /// Steps from Arkor: the last one, when, and how many in a row.
    last: Option<(String, Instant, u32)>,
}

#[derive(Clone)]
pub struct Pointer {
    page: gtk::Widget,
    state: Rc<RefCell<State>>,
}

impl Pointer {
    pub fn new(page: gtk::Widget) -> Self {
        let state = Rc::new(RefCell::new(State {
            at: (0., 0.),
            out: false,
            down: false,
            frame: None,
            pad_held: 0.,
            last: None,
        }));
        // Drawn by the window itself over the page: no waiting for the page's script.
        let s = state.clone();
        page.connect_local("draw", true, move |args| {
            let state = s.borrow();
            if state.out
                && let Ok(cr) = args[1].get::<gtk::cairo::Context>()
            {
                draw(&cr, state.at, state.down);
            }
            Some(false.to_value())
        });
        Self { page, state }
    }

    pub fn out(&self) -> bool {
        self.state.borrow().out
    }

    pub fn show(&self, x: f64, y: f64) {
        {
            let mut s = self.state.borrow_mut();
            if std::mem::replace(&mut s.out, true) {
                return;
            }
            (s.at, s.frame, s.pad_held, s.last) = ((x, y), None, 0., None);
        }
        self.page.queue_draw();
        send(&self.page, What::Move, (x, y), false);
        let me = self.clone();
        // With every refresh of the screen, as long as it is out.
        self.page.add_tick_callback(move |_, clock| {
            if !me.out() {
                return glib::ControlFlow::Break;
            }
            me.tick(clock.frame_time());
            glib::ControlFlow::Continue
        });
    }

    pub fn hide(&self) {
        self.press(false);
        self.state.borrow_mut().out = false;
        self.page.queue_draw();
    }

    /// One frame: moves and scrolls by how the sticks are held.
    fn tick(&self, now: i64) {
        let h = held(|h| *h);
        let dt = {
            let mut s = self.state.borrow_mut();
            let dt = s.frame.map_or(0., |t| (now - t) as f64 / 1e6).min(0.05);
            s.frame = Some(now);
            dt
        };
        let full = f64::from(self.page.allocated_height()) * 1.15;
        let (mut vx, mut vy) = velocity(h.left, full);
        let one = |plus: bool, minus: bool| f64::from(i8::from(plus) - i8::from(minus));
        let (px, py) = (one(h.to_right, h.to_left), one(h.down, h.up));
        let mut s = self.state.borrow_mut();
        if (px, py) == (0., 0.) {
            s.pad_held = 0.;
        } else {
            s.pad_held += dt;
            let speed = full * (0.08 + 0.5 * s.pad_held.min(1.));
            let k = speed / px.hypot(py);
            vx += px * k;
            vy += py * k;
        }
        drop(s);
        if (vx, vy) != (0., 0.) {
            self.move_by(vx * dt, vy * dt);
        }
        if h.scroll.abs() > 0.15 {
            let at = self.state.borrow().at;
            send(&self.page, What::Scroll(h.scroll * dt * 14.), at, false);
        }
    }

    fn move_by(&self, dx: f64, dy: f64) {
        let (w, h) = (
            f64::from(self.page.allocated_width()),
            f64::from(self.page.allocated_height()),
        );
        let (from, to, down) = {
            let mut s = self.state.borrow_mut();
            let from = s.at;
            s.at = (
                (from.0 + dx).clamp(0., (w - 1.).max(0.)),
                (from.1 + dy).clamp(0., (h - 1.).max(0.)),
            );
            (from, s.at, s.down)
        };
        if from != to {
            for (x, y) in [from, to] {
                self.page
                    .queue_draw_area(x as i32 - 12, y as i32 - 12, 48, 52);
            }
            send(&self.page, What::Move, to, down);
        }
    }

    /// A went down or up.
    pub fn press(&self, down: bool) {
        let at = {
            let mut s = self.state.borrow_mut();
            if !s.out || s.down == down {
                return;
            }
            s.down = down;
            s.at
        };
        self.page.queue_draw();
        send(
            &self.page,
            if down { What::Press } else { What::Release },
            at,
            !down,
        );
    }

    /// A step from Arkor, when the gamepad cannot be read here.
    pub fn step(&self, nav: &str) {
        let (dx, dy) = match nav {
            "up" => (0., -1.),
            "down" => (0., 1.),
            "left" => (-1., 0.),
            "right" => (1., 0.),
            "accept" => {
                self.press(true);
                let me = self.clone();
                glib::timeout_add_local_once(Duration::from_millis(60), move || me.press(false));
                return;
            }
            _ => return,
        };
        let now = Instant::now();
        let run = match &self.state.borrow().last {
            Some((n, t, run)) if n == nav && now - *t < Duration::from_millis(200) => run + 1,
            _ => 0,
        };
        self.state.borrow_mut().last = Some((nav.to_string(), now, run));
        let step = (8. + 7. * f64::from(run)).min(64.);
        self.move_by(dx * step, dy * step);
    }
}

/// An arrow with its tip at `at`.
fn draw(cr: &gtk::cairo::Context, (x, y): (f64, f64), down: bool) {
    let _ = cr.save();
    cr.translate(x, y);
    if down {
        cr.scale(0.9, 0.9);
    }
    cr.move_to(0., 0.);
    for (px, py) in [
        (0., 25.),
        (6., 19.5),
        (10., 28.5),
        (14., 26.7),
        (10.2, 18.),
        (18., 17.5),
    ] {
        cr.line_to(px, py);
    }
    cr.close_path();
    cr.set_source_rgb(1., 1., 1.);
    let _ = cr.fill_preserve();
    cr.set_source_rgb(0.43, 0.16, 0.85);
    cr.set_line_width(1.8);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    let _ = cr.stroke();
    let _ = cr.restore();
}

enum What {
    Move,
    Press,
    Release,
    Scroll(f64),
}

/// A real pointer event in the page, as from a mouse.
fn send(page: &gtk::Widget, what: What, (x, y): (f64, f64), down: bool) {
    let (Some(window), Some(pointer)) = (
        page.window(),
        page.display().default_seat().and_then(|s| s.pointer()),
    ) else {
        return;
    };
    let (_, ox, oy) = window.origin();
    let root = (f64::from(ox) + x, f64::from(oy) + y);
    let state = if down { ffi::GDK_BUTTON1_MASK } else { 0 };
    let time = (glib::monotonic_time() / 1000) as u32;
    let kind = match what {
        What::Move => ffi::GDK_MOTION_NOTIFY,
        What::Press => ffi::GDK_BUTTON_PRESS,
        What::Release => ffi::GDK_BUTTON_RELEASE,
        What::Scroll(_) => ffi::GDK_SCROLL,
    };
    // SAFETY: the event is made by GDK for this kind and only the fields of that kind are
    // written; the window gets the reference the event gives back when it is freed.
    unsafe {
        let event = ffi::gdk_event_new(kind);
        let win: *mut ffi::GdkWindow = window.to_glib_full();
        match what {
            What::Move => {
                let e = &mut *(event as *mut ffi::GdkEventMotion);
                (e.window, e.x, e.y, e.state, e.time) = (win, x, y, state, time);
                (e.x_root, e.y_root) = root;
            }
            What::Press | What::Release => {
                let e = &mut *(event as *mut ffi::GdkEventButton);
                (e.window, e.x, e.y, e.state, e.time) = (win, x, y, state, time);
                (e.x_root, e.y_root, e.button) = (root.0, root.1, 1);
            }
            What::Scroll(dy) => {
                let e = &mut *(event as *mut ffi::GdkEventScroll);
                (e.window, e.x, e.y, e.state, e.time) = (win, x, y, state, time);
                (e.x_root, e.y_root) = root;
                (e.direction, e.delta_x, e.delta_y) = (ffi::GDK_SCROLL_SMOOTH, 0., dy);
            }
        }
        let device: *mut ffi::GdkDevice = pointer.to_glib_none().0;
        ffi::gdk_event_set_device(event, device);
        ffi::gdk_event_set_source_device(event, device);
        gtk::ffi::gtk_widget_event(page.to_glib_none().0, event);
        ffi::gdk_event_free(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stick_moves_it_the_same_in_every_direction() {
        assert_eq!(velocity((0.05, 0.05), 1000.), (0., 0.), "a resting stick");
        let (x, y) = velocity((0.6, -0.6), 1000.);
        assert!(x > 0. && (x + y).abs() < 1e-9, "diagonal");
        let straight = velocity((0.6_f64.hypot(0.6), 0.), 1000.).0;
        assert!((x.hypot(y) - straight).abs() < 1e-6);
        assert!((velocity((1., 0.), 1000.).0 - 1000.).abs() < 1e-6);
        assert!(velocity((0.3, 0.), 1000.).0 < 120., "slow to aim");
        // A stick's corner is further out than its edge: not faster.
        let (x, y) = velocity((1., 1.), 1000.);
        assert!((x.hypot(y) - 1000.).abs() < 1e-6);
    }
}
