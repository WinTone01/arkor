//! Gamepads in console mode: the marks of their buttons, which are connected and how full
//! their batteries are, trying their buttons, and a word when a battery runs low.

use std::time::{Duration, Instant};

use super::Library;
use super::console::{Act, Item, Kind as RowKind, Opt, Row, Screen, Value};
use super::console_settings::AppCat;
use super::gamepad::{self, Kind};

#[derive(Default)]
pub(super) struct PadsState {
    /// B was pressed once while trying the buttons.
    back_at: Option<Instant>,
}

impl Library {
    /// Whose marks the buttons get: the setting, else the pad in use.
    pub(super) fn pad_kind(&self) -> Kind {
        self.global
            .pad_glyphs
            .as_deref()
            .and_then(Kind::from_key)
            .or_else(|| gamepad::pad().map(|p| p.kind))
            .unwrap_or_default()
    }

    /// With every frame: the battery word, and the test screen moving.
    pub(super) fn pad_watch(&mut self) {
        if self.console.screen() == Screen::PadTest {
            self.console.unsettled = true;
        }
    }

    /// B twice within a second leaves the test.
    pub(super) fn pad_test_leave(&mut self) -> bool {
        let p = &mut self.console.pads;
        match p.back_at.take() {
            Some(t) if t.elapsed() < Duration::from_millis(1200) => true,
            _ => {
                p.back_at = Some(Instant::now());
                false
            }
        }
    }

    pub(super) fn pads_summary(&self) -> Value {
        let pads = gamepad::pads();
        Value::Text(match pads.len() {
            0 => self.tr("None", "Yok").to_string(),
            1 => pads[0].kind.name().to_string(),
            n => format!("{n}"),
        })
    }

    pub(super) fn pads_rows(&self) -> Vec<Row> {
        let line = |act: Act, icon: &'static str, label: &str, value: Value| {
            Row::new(RowKind::Line, vec![Item::line(act, icon, label, value)])
        };
        let mut rows = Vec::new();
        let pads = gamepad::pads();
        let using = gamepad::pad();
        for (n, p) in pads.iter().enumerate() {
            let battery = match p.battery {
                Some((b, true)) => format!(
                    "{} · {}",
                    self.percent(f32::from(b) / 100.),
                    self.tr("charging", "şarj oluyor")
                ),
                Some((b, false)) => self.percent(f32::from(b) / 100.),
                None => self.tr("cable", "kablolu").to_string(),
            };
            let icon = match p.battery {
                Some((_, true)) => "battery-charging",
                Some((70.., _)) => "battery-full",
                Some((30.., _)) => "battery-medium",
                Some(_) => "battery-low",
                None => "cable",
            };
            let mut row = line(
                Act::Nothing,
                icon,
                &p.name,
                Value::Text(format!("{} · {battery}", p.kind.name())),
            );
            if using.as_ref() == Some(p) && pads.len() > 1 {
                row.items[0].sub = self.tr("In use", "Kullanılan").into();
            }
            if n == 0 {
                row.title = Some(self.tr("Connected", "Bağlı").into());
            }
            rows.push(row);
        }
        if pads.is_empty() {
            rows.push(line(
                Act::Nothing,
                "gamepad-2",
                self.tr("No gamepad is connected", "Bağlı kol yok"),
                Value::None,
            ));
        }
        let glyphs = self.opt_value(&Opt::PadGlyphs);
        let mut marks = line(
            Act::Opt(Opt::PadGlyphs),
            "shapes",
            self.tr("Button marks", "Tuş simgeleri"),
            glyphs,
        );
        marks.title = Some(self.tr("Buttons", "Tuşlar").into());
        rows.push(marks);
        rows.push(line(
            Act::Screen(Screen::PadTest),
            "gamepad",
            self.tr("Try the buttons", "Tuşları dene"),
            Value::None,
        ));
        rows.push(line(
            Act::Screen(Screen::SettingsCat(AppCat::Bluetooth)),
            "bluetooth",
            self.tr("Connect a wireless gamepad", "Kablosuz kol bağla"),
            Value::None,
        ));
        rows
    }

    pub(super) fn pads_opt_key(&self, o: &Opt) -> Option<String> {
        (*o == Opt::PadGlyphs).then(|| self.global.pad_glyphs.clone().unwrap_or_default())
    }

    pub(super) fn pads_opt_choices(&self, o: &Opt) -> Option<Vec<(String, String)>> {
        if *o != Opt::PadGlyphs {
            return None;
        }
        let mut out = vec![(
            self.tr("As the gamepad is", "Kola göre").to_string(),
            String::new(),
        )];
        for k in [Kind::Xbox, Kind::PlayStation, Kind::Nintendo] {
            out.push((k.name().to_string(), k.key().to_string()));
        }
        Some(out)
    }

    pub(super) fn pads_opt_set(&mut self, o: &Opt, key: &str) -> bool {
        if *o != Opt::PadGlyphs {
            return false;
        }
        self.global.pad_glyphs = Some(key.to_string()).filter(|k| !k.is_empty());
        let r = self.global.save();
        self.report(r);
        true
    }

    /// Trying the buttons: each lights up while it is held.
    pub(super) fn pad_test_rows(&self) -> Vec<Row> {
        let now = gamepad::now();
        let kind = self.pad_kind();
        let pill = |name: &'static str| {
            let label = match name {
                "Up" => self.tr("Up", "Yukarı"),
                "Down" => self.tr("Down", "Aşağı"),
                "Left" => self.tr("Left", "Sol"),
                "Right" => self.tr("Right", "Sağ"),
                other => kind.glyph(other),
            };
            let mut item = Item::pill(Act::Nothing, "circle", label, now.buttons.contains(name));
            if let super::console::Look::Pill { icon, .. } = &mut item.look {
                *icon = match name {
                    "Up" => "arrow-up",
                    "Down" => "arrow-down",
                    "Left" => "arrow-left",
                    "Right" => "arrow-right",
                    _ if now.buttons.contains(name) => "circle-dot",
                    _ => "circle",
                };
            }
            item
        };
        let row = |names: &[&'static str]| {
            Row::new(RowKind::Pills, names.iter().map(|n| pill(n)).collect())
        };
        let stick = |(x, y): (f32, f32)| format!("{:+.2}  {:+.2}", x, y);
        let line = |icon: &'static str, label: &str, value: String| {
            Row::new(
                RowKind::Line,
                vec![Item::line(Act::Nothing, icon, label, Value::Text(value))],
            )
        };
        let mut head = line(
            "info",
            &format!(
                "{} {} {}",
                self.tr(
                    "Press any button. To leave, press",
                    "Herhangi bir tuşa bas. Çıkmak için"
                ),
                kind.glyph("B"),
                self.tr("twice.", "tuşuna iki kez bas.")
            ),
            String::new(),
        );
        head.title = Some(gamepad::pad().map(|p| p.name).unwrap_or_default());
        vec![
            head,
            row(&["LT", "LB", "View", "Guide", "Menu", "RB", "RT"]),
            row(&["Up", "Down", "Left", "Right", "LS", "RS"]),
            row(&["A", "B", "X", "Y"]),
            line("move", self.tr("Left stick", "Sol çubuk"), stick(now.left)),
            line(
                "move",
                self.tr("Right stick", "Sağ çubuk"),
                stick(now.right),
            ),
            line(
                "chevrons-down",
                self.tr("Triggers", "Tetikler"),
                format!("{:.2}  {:.2}", now.triggers.0, now.triggers.1),
            ),
        ]
    }
}
