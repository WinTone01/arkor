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

/// Whose buttons a gamepad has (they are named and marked differently).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Xbox,
    PlayStation,
    Nintendo,
}

impl Kind {
    /// From the maker's USB id, else from the name.
    pub fn of(vendor: Option<u16>, name: &str) -> Kind {
        let n = name.to_lowercase();
        match vendor {
            Some(0x054c) => Kind::PlayStation,
            Some(0x057e) => Kind::Nintendo,
            _ if ["dualsense", "dualshock", "playstation", "ps4", "ps5"]
                .iter()
                .any(|w| n.contains(w)) =>
            {
                Kind::PlayStation
            }
            _ if n.contains("pro controller")
                || n.contains("joy-con")
                || n.contains("nintendo") =>
            {
                Kind::Nintendo
            }
            _ => Kind::Xbox,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::Xbox => "xbox",
            Kind::PlayStation => "playstation",
            Kind::Nintendo => "nintendo",
        }
    }

    pub fn from_key(key: &str) -> Option<Kind> {
        [Kind::Xbox, Kind::PlayStation, Kind::Nintendo]
            .into_iter()
            .find(|k| k.key() == key)
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Xbox => "Xbox",
            Kind::PlayStation => "PlayStation",
            Kind::Nintendo => "Nintendo",
        }
    }

    /// The mark of a button, given by its Xbox name (where it is on the pad).
    pub fn glyph(self, xbox: &'static str) -> &'static str {
        match (self, xbox) {
            (Kind::PlayStation, "A") => "✕",
            (Kind::PlayStation, "B") => "○",
            (Kind::PlayStation, "X") => "□",
            (Kind::PlayStation, "Y") => "△",
            (Kind::PlayStation, "LB") => "L1",
            (Kind::PlayStation, "RB") => "R1",
            (Kind::PlayStation, "LT") => "L2",
            (Kind::PlayStation, "RT") => "R2",
            (Kind::PlayStation, "View") => "Create",
            (Kind::PlayStation, "Menu") => "Options",
            (Kind::PlayStation, "Guide") => "PS",
            // Nintendo's letters sit the other way round.
            (Kind::Nintendo, "A") => "B",
            (Kind::Nintendo, "B") => "A",
            (Kind::Nintendo, "X") => "Y",
            (Kind::Nintendo, "Y") => "X",
            (Kind::Nintendo, "LB") => "L",
            (Kind::Nintendo, "RB") => "R",
            (Kind::Nintendo, "LT") => "ZL",
            (Kind::Nintendo, "RT") => "ZR",
            (Kind::Nintendo, "View") => "−",
            (Kind::Nintendo, "Menu") => "+",
            (Kind::Nintendo, "Guide") => "⌂",
            (_, other) => other,
        }
    }
}

/// A connected gamepad.
#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    pub name: String,
    /// Battery percent, and whether it charges; `None` with a cable (or unknown).
    pub battery: Option<(u8, bool)>,
    pub kind: Kind,
    id: usize,
}

static PADS: Mutex<Vec<Pad>> = Mutex::new(Vec::new());
/// The pad a button was pressed on last.
static LAST: Mutex<Option<usize>> = Mutex::new(None);

/// The gamepad in use (the one pressed last, else the first), if any.
pub fn pad() -> Option<Pad> {
    let pads = pads();
    let last = *LAST.lock().unwrap_or_else(|e| e.into_inner());
    pads.iter()
        .find(|p| Some(p.id) == last)
        .or(pads.first())
        .cloned()
}

/// Every connected gamepad.
pub fn pads() -> Vec<Pad> {
    PADS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What is pressed and where the sticks are, for trying a pad's buttons.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadNow {
    /// Buttons by their Xbox names, d-pad as "Up" etc., sticks' clicks as "LS"/"RS".
    pub buttons: std::collections::BTreeSet<&'static str>,
    pub left: (f32, f32),
    pub right: (f32, f32),
    pub triggers: (f32, f32),
}

static NOW: Mutex<Option<PadNow>> = Mutex::new(None);

/// What the pads do right now.
pub fn now() -> PadNow {
    NOW.lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

fn name_of(b: Button) -> Option<&'static str> {
    Some(match b {
        Button::South => "A",
        Button::East => "B",
        Button::West => "X",
        Button::North => "Y",
        Button::LeftTrigger => "LB",
        Button::RightTrigger => "RB",
        Button::LeftTrigger2 => "LT",
        Button::RightTrigger2 => "RT",
        Button::Select => "View",
        Button::Start => "Menu",
        Button::Mode => "Guide",
        Button::LeftThumb => "LS",
        Button::RightThumb => "RS",
        Button::DPadUp => "Up",
        Button::DPadDown => "Down",
        Button::DPadLeft => "Left",
        Button::DPadRight => "Right",
        _ => return None,
    })
}

fn note(f: impl FnOnce(&mut PadNow)) {
    let mut now = NOW.lock().unwrap_or_else(|e| e.into_inner());
    f(now.get_or_insert_with(PadNow::default));
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
                        EventType::ButtonPressed(b, _) | EventType::ButtonReleased(b, _) => {
                            if let Some(name) = name_of(b) {
                                let down = matches!(event, EventType::ButtonPressed(..));
                                note(|h| {
                                    if down {
                                        h.buttons.insert(name);
                                    } else {
                                        h.buttons.remove(name);
                                    }
                                });
                            }
                            if matches!(event, EventType::ButtonPressed(..)) {
                                *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.into());
                            }
                        }
                        EventType::AxisChanged(axis, value, _) => note(|h| match axis {
                            Axis::LeftStickX => h.left.0 = value,
                            Axis::LeftStickY => h.left.1 = value,
                            Axis::RightStickX => h.right.0 = value,
                            Axis::RightStickY => h.right.1 = value,
                            Axis::LeftZ => h.triggers.0 = value,
                            Axis::RightZ => h.triggers.1 = value,
                            _ => {}
                        }),
                        EventType::ButtonChanged(b, value, _) => note(|h| match b {
                            Button::LeftTrigger2 => h.triggers.0 = value,
                            Button::RightTrigger2 => h.triggers.1 = value,
                            _ => {}
                        }),
                        _ => {}
                    }
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
    let pads = gilrs
        .gamepads()
        .filter(|(_, g)| g.is_connected())
        .map(|(id, g)| Pad {
            name: g.name().to_string(),
            battery: match g.power_info() {
                PowerInfo::Discharging(p) => Some((p, false)),
                PowerInfo::Charging(p) => Some((p, true)),
                PowerInfo::Charged => Some((100, true)),
                PowerInfo::Wired | PowerInfo::Unknown => None,
            },
            kind: Kind::of(g.vendor_id(), g.name()),
            id: id.into(),
        })
        .collect();
    *PADS.lock().unwrap_or_else(|e| e.into_inner()) = pads;
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
    fn pads_are_told_apart() {
        assert_eq!(
            Kind::of(Some(0x054c), "Wireless Controller"),
            Kind::PlayStation
        );
        assert_eq!(Kind::of(None, "Sony DualSense"), Kind::PlayStation);
        assert_eq!(Kind::of(Some(0x057e), "Pro Controller"), Kind::Nintendo);
        assert_eq!(
            Kind::of(Some(0x045e), "Xbox Wireless Controller"),
            Kind::Xbox
        );
        assert_eq!(Kind::PlayStation.glyph("A"), "✕");
        assert_eq!(Kind::Nintendo.glyph("A"), "B");
        assert_eq!(Kind::Xbox.glyph("LB"), "LB");
        assert_eq!(Kind::PlayStation.glyph("Esc"), "Esc");
    }

    #[test]
    fn directions_repeat_and_buttons_do_not() {
        assert!(Nav::Down.repeats() && Nav::PageDown.repeats());
        assert!(!Nav::Accept.repeats() && !Nav::Toggle.repeats());
    }
}
