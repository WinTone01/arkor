//! Gamepads: their buttons and sticks as navigation, with repetition while held.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gilrs::{Axis, Button, EventType, GamepadId, Gilrs, PowerInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    /// A: choose.
    Accept,
    /// B: back.
    Back,
    /// X: the second action of what is chosen.
    Extra,
    /// Y: search.
    Search,
    /// LB / RB: the sections.
    PrevTab,
    NextTab,
    /// LT / RT: a page up or down.
    PageUp,
    PageDown,
    /// Start: the menu.
    Menu,
    /// The middle button: console mode.
    Toggle,
}

impl Nav {
    /// Held, it goes on.
    fn repeats(self) -> bool {
        matches!(
            self,
            Nav::Up | Nav::Down | Nav::Left | Nav::Right | Nav::PageUp | Nav::PageDown
        )
    }
}

const FIRST: Duration = Duration::from_millis(380);
const NEXT: Duration = Duration::from_millis(95);
/// A stick counts as pushed beyond this, and as let go below the second value.
const PUSHED: f32 = 0.6;
const RELEASED: f32 = 0.4;

/// What the gamepad in use is.
#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    pub name: String,
    /// Battery percent, and whether it charges; `None` with a cable (or unknown).
    pub battery: Option<(u8, bool)>,
}

static PAD: Mutex<Option<Pad>> = Mutex::new(None);

/// The connected gamepad, if any.
pub fn pad() -> Option<Pad> {
    PAD.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn button(b: Button) -> Option<Nav> {
    Some(match b {
        Button::DPadUp => Nav::Up,
        Button::DPadDown => Nav::Down,
        Button::DPadLeft => Nav::Left,
        Button::DPadRight => Nav::Right,
        Button::South => Nav::Accept,
        Button::East => Nav::Back,
        Button::West => Nav::Extra,
        Button::North => Nav::Search,
        Button::LeftTrigger => Nav::PrevTab,
        Button::RightTrigger => Nav::NextTab,
        Button::LeftTrigger2 => Nav::PageUp,
        Button::RightTrigger2 => Nav::PageDown,
        Button::Start => Nav::Menu,
        Button::Mode => Nav::Toggle,
        _ => return None,
    })
}

/// Where a stick axis points, given where it pointed before.
fn push(value: f32, before: i8) -> i8 {
    let limit = if before == 0 { PUSHED } else { RELEASED };
    if value > limit {
        1
    } else if value < -limit {
        -1
    } else {
        0
    }
}

/// What is held, and when it repeats next.
#[derive(Default)]
struct Held {
    keys: HashMap<(GamepadId, Nav), Instant>,
}

impl Held {
    fn press(&mut self, pad: GamepadId, nav: Nav, now: Instant) {
        if nav.repeats() {
            // Only what was pressed last repeats.
            self.keys.clear();
            self.keys.insert((pad, nav), now + FIRST);
        }
    }

    fn release(&mut self, pad: GamepadId, nav: Nav) {
        self.keys.remove(&(pad, nav));
    }

    /// What is due at `now`.
    fn due(&mut self, now: Instant) -> Vec<Nav> {
        let mut out = Vec::new();
        for ((_, nav), at) in &mut self.keys {
            if now >= *at {
                out.push(*nav);
                *at = now + NEXT;
            }
        }
        out
    }

    fn wait(&self, now: Instant) -> Duration {
        self.keys
            .values()
            .map(|at| at.saturating_duration_since(now))
            .min()
            .unwrap_or(Duration::from_millis(1000))
            .clamp(Duration::from_millis(5), Duration::from_millis(1000))
    }
}

/// Reads gamepads on a thread; `None` when they cannot be read.
pub(super) fn start() -> Option<async_channel::Receiver<Nav>> {
    let (tx, rx) = async_channel::unbounded();
    std::thread::Builder::new()
        .name("arkor-gamepad".into())
        .spawn(move || {
            let Ok(mut gilrs) = Gilrs::new() else {
                return;
            };
            let mut held = Held::default();
            let mut sticks: HashMap<(GamepadId, Axis), i8> = HashMap::new();
            let mut looked: Option<Instant> = None;
            loop {
                let now = Instant::now();
                if looked.is_none_or(|t| now - t > Duration::from_secs(20)) {
                    looked = Some(now);
                    describe(&gilrs);
                }
                let event = gilrs.next_event_blocking(Some(held.wait(now)));
                let now = Instant::now();
                let mut out = Vec::new();
                if let Some(gilrs::Event { id, event, .. }) = event {
                    match event {
                        EventType::ButtonPressed(b, _) => {
                            if let Some(nav) = button(b) {
                                held.press(id, nav, now);
                                out.push(nav);
                            }
                        }
                        EventType::ButtonReleased(b, _) => {
                            if let Some(nav) = button(b) {
                                held.release(id, nav);
                            }
                        }
                        EventType::AxisChanged(
                            axis @ (Axis::LeftStickX | Axis::LeftStickY),
                            value,
                            _,
                        ) => {
                            let before = sticks.entry((id, axis)).or_default();
                            let dir = push(value, *before);
                            if dir != *before {
                                let towards = |d: i8| match (axis, d) {
                                    (Axis::LeftStickX, 1) => Some(Nav::Right),
                                    (Axis::LeftStickX, -1) => Some(Nav::Left),
                                    (_, 1) => Some(Nav::Up),
                                    (_, -1) => Some(Nav::Down),
                                    _ => None,
                                };
                                if let Some(nav) = towards(*before) {
                                    held.release(id, nav);
                                }
                                if let Some(nav) = towards(dir) {
                                    held.press(id, nav, now);
                                    out.push(nav);
                                }
                                *before = dir;
                            }
                        }
                        EventType::Connected => describe(&gilrs),
                        EventType::Disconnected => {
                            held.keys.retain(|(pad, _), _| *pad != id);
                            sticks.retain(|(pad, _), _| *pad != id);
                            describe(&gilrs);
                        }
                        _ => {}
                    }
                }
                out.extend(held.due(now));
                for nav in out {
                    if tx.send_blocking(nav).is_err() {
                        return;
                    }
                }
            }
        })
        .ok()?;
    Some(rx)
}

fn describe(gilrs: &Gilrs) {
    let pad = gilrs
        .gamepads()
        .find(|(_, g)| g.is_connected())
        .map(|(_, g)| Pad {
            name: g.name().to_string(),
            battery: match g.power_info() {
                PowerInfo::Discharging(p) => Some((p, false)),
                PowerInfo::Charging(p) => Some((p, true)),
                PowerInfo::Charged => Some((100, true)),
                PowerInfo::Wired | PowerInfo::Unknown => None,
            },
        });
    *PAD.lock().unwrap_or_else(|e| e.into_inner()) = pad;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stick_has_to_come_back_before_it_counts_again() {
        assert_eq!(push(0.5, 0), 0);
        assert_eq!(push(0.7, 0), 1);
        assert_eq!(push(0.5, 1), 1, "wobbling does not let go");
        assert_eq!(push(0.3, 1), 0);
        assert_eq!(push(-0.7, 1), -1);
    }

    #[test]
    fn directions_repeat_and_buttons_do_not() {
        assert!(Nav::Down.repeats() && Nav::PageDown.repeats());
        assert!(!Nav::Accept.repeats() && !Nav::Toggle.repeats());
    }
}
