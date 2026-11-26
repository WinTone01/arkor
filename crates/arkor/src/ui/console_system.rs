//! Console mode's settings of the computer itself: network, Bluetooth, sound, screens and
//! power, so nothing needs the desktop (see `arkor_launch::system`).

use std::time::{Duration, Instant};

use arkor_launch::system::{bluetooth, display, power, sound, wifi};
use gpui::Context;

use super::Library;
use super::console::{Act, Item, Kind, Opt, Overlay, Row, Screen, Value};
use super::console_settings::AppCat;
use super::console_tools::NeedsPassword;
use super::synth::Cue;

/// Seconds a changed screen setting waits to be kept before it goes back.
const KEEP_SECS: u64 = 15;

/// What is known of the computer, read when its screens are open.
#[derive(Default)]
pub(super) struct SysState {
    wifi: Option<Result<wifi::State, String>>,
    bluetooth: Option<Result<bluetooth::State, String>>,
    sound: Option<Result<sound::State, String>>,
    screens: Option<Result<Vec<display::Screen>, String>>,
    power: Option<power::State>,
    /// What is being done, by what it is about.
    busy: Option<String>,
    /// Looking for Bluetooth devices nearby.
    scanning: bool,
    /// A screen's setting that goes back unless it is kept.
    revert: Option<(Change, Instant)>,
    /// Changes chosen where nothing could be started (they start with the next frame).
    queue: Vec<Change>,
    watching: bool,
}

/// A change of the computer's settings.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Change {
    WifiOn(bool),
    BluetoothOn(bool),
    Volume(u8),
    Muted(bool),
    Output(u32),
    Mode(String, String),
    Scale(String, f64),
    Hdr(String, bool),
    Vrr(String, bool),
    Primary(String),
    Profile(String),
}

impl Change {
    fn run(&self) -> anyhow::Result<()> {
        match self {
            Change::WifiOn(on) => wifi::set_on(*on),
            Change::BluetoothOn(on) => bluetooth::set_on(*on),
            Change::Volume(v) => sound::set_volume(*v),
            Change::Muted(m) => sound::set_muted(*m),
            Change::Output(id) => sound::set_output(*id),
            Change::Mode(s, m) => display::set_mode(s, m),
            Change::Scale(s, x) => display::set_scale(s, *x),
            Change::Hdr(s, on) => display::set_hdr(s, *on),
            Change::Vrr(s, on) => display::set_vrr(s, *on),
            Change::Primary(s) => display::set_primary(s),
            Change::Profile(p) => power::set_profile(p),
        }
    }

    fn cat(&self) -> AppCat {
        match self {
            Change::WifiOn(_) => AppCat::Network,
            Change::BluetoothOn(_) => AppCat::Bluetooth,
            Change::Volume(_) | Change::Muted(_) | Change::Output(_) => AppCat::Sound,
            Change::Profile(_) => AppCat::Power,
            _ => AppCat::Display,
        }
    }
}

const SCALES: [u32; 7] = [100, 125, 150, 175, 200, 250, 300];

impl Library {
    // ---- Reading ----

    /// Reads what a category shows, in the background.
    pub(super) fn sys_load(&mut self, cat: AppCat, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let read = bg
                .spawn(async move {
                    let e = |e: anyhow::Error| format!("{e:#}");
                    let mut s = SysState::default();
                    match cat {
                        AppCat::Network => s.wifi = Some(wifi::state(false).map_err(e)),
                        AppCat::Bluetooth => s.bluetooth = Some(bluetooth::state().map_err(e)),
                        AppCat::Sound => s.sound = Some(sound::state().map_err(e)),
                        AppCat::Display => s.screens = Some(display::screens().map_err(e)),
                        AppCat::Power => s.power = Some(power::state()),
                        _ => {}
                    }
                    s
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let s = &mut this.console.sys;
                // Only what was read: the rest stays.
                if read.wifi.is_some() {
                    s.wifi = read.wifi;
                }
                if read.bluetooth.is_some() {
                    s.bluetooth = read.bluetooth;
                }
                if read.sound.is_some() {
                    s.sound = read.sound;
                }
                if read.screens.is_some() {
                    s.screens = read.screens;
                }
                if read.power.is_some() {
                    s.power = read.power;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// While one of these screens is open, it is read again now and then (a device that
    /// connects, a network that comes).
    pub(super) fn sys_opened(&mut self, cat: AppCat, cx: &mut Context<Self>) {
        self.sys_load(cat, cx);
        if cfg!(test) || std::mem::replace(&mut self.console.sys.watching, true) {
            return;
        }
        let timer = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                timer.timer(Duration::from_secs(4)).await;
                let open = this
                    .update(cx, |this, cx| {
                        let cat = match this.console.screen() {
                            Screen::SettingsCat(c) if sys_cat(c) => Some(c),
                            _ => None,
                        };
                        match cat {
                            Some(c) if this.console.on && this.console.sys.busy.is_none() => {
                                this.sys_load(c, cx);
                                true
                            }
                            Some(_) => true,
                            None => {
                                this.console.sys.watching = false;
                                false
                            }
                        }
                    })
                    .unwrap_or(false);
                if !open {
                    return;
                }
            }
        })
        .detach();
    }

    // ---- Changing ----

    /// Starts what was chosen (called with every frame; usually nothing).
    pub(super) fn sys_flush(&mut self, cx: &mut Context<Self>) {
        for change in std::mem::take(&mut self.console.sys.queue) {
            self.sys_change(change, cx);
        }
        // A screen setting nobody kept goes back by itself.
        if let Some((_, at)) = &self.console.sys.revert
            && at.elapsed() > Duration::from_secs(KEEP_SECS)
        {
            self.sys_revert(cx);
        }
    }

    fn sys_change(&mut self, change: Change, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let cat = change.cat();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { change.run() }).await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.status = Some(format!("{e:#}").into());
                }
                this.sys_load(cat, cx);
            });
        })
        .detach();
    }

    /// A screen setting: done at once, and kept only when that is said in time.
    fn sys_try_screen(&mut self, change: Change, back: Change) {
        self.console.sys.queue.push(change);
        self.console.sys.revert = Some((back, Instant::now()));
        let text = format!(
            "{} {KEEP_SECS} {}",
            self.tr(
                "Does the picture look right? Otherwise it goes back in",
                "Görüntü düzgün mü? Değilse eski haline dönecek:"
            ),
            self.tr("seconds.", "saniye.")
        );
        let yes = self.tr("Keep it", "Böyle kalsın").to_string();
        self.console_overlay(Overlay::Ask(text, yes, Act::DisplayKeep));
    }

    pub(super) fn sys_keep(&mut self) {
        self.console.sys.revert = None;
        self.console.overlay = None;
    }

    /// Puts the screen setting back.
    pub(super) fn sys_revert(&mut self, cx: &mut Context<Self>) {
        if let Some((back, _)) = self.console.sys.revert.take() {
            if matches!(
                &self.console.overlay,
                Some(Overlay::Ask(_, _, Act::DisplayKeep))
            ) {
                self.console.overlay = None;
            }
            self.sys_change(back, cx);
            cx.notify();
        }
    }

    /// Something that takes a while, by what it is about.
    fn sys_do(
        &mut self,
        about: String,
        cat: AppCat,
        what: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
        done: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        if cfg!(test) || self.console.sys.busy.is_some() {
            return;
        }
        self.console.sys.busy = Some(about);
        self.console.overlay = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { what() }).await;
            let _ = this.update(cx, |this, cx| {
                this.console.sys.busy = None;
                match result {
                    Ok(()) => {
                        if let Some(text) = done {
                            this.status = Some(text.into());
                        }
                    }
                    Err(e) => {
                        this.cue(Cue::Error);
                        this.status = Some(format!("{e:#}").into());
                    }
                }
                this.sys_load(cat, cx);
            });
        })
        .detach();
    }

    // ---- Settings, by what they are ----

    fn sys_screen(&self, name: &str) -> Option<&display::Screen> {
        match &self.console.sys.screens {
            Some(Ok(list)) => list.iter().find(|s| s.name == name),
            _ => None,
        }
    }

    /// Whether power profiles can be chosen here.
    pub(super) fn sys_has_profiles(&self) -> bool {
        self.console
            .sys
            .power
            .as_ref()
            .is_some_and(|p| !p.profiles.is_empty())
    }

    fn sys_sound(&self) -> Option<&sound::State> {
        self.console
            .sys
            .sound
            .as_ref()
            .and_then(|r| r.as_ref().ok())
    }

    pub(super) fn sys_opt_value(&self, o: &Opt) -> Option<Value> {
        let s = &self.console.sys;
        let ok = |r: &Option<Result<_, String>>| matches!(r, Some(Ok(_)));
        Some(match o {
            Opt::Wifi => Value::Switch(match &s.wifi {
                Some(Ok(w)) => w.on,
                _ => false,
            }),
            Opt::Bluetooth => Value::Switch(match &s.bluetooth {
                Some(Ok(b)) => b.on,
                _ => false,
            }),
            Opt::Muted => Value::Switch(self.sys_sound().is_some_and(|x| x.muted)),
            Opt::Hdr(n) => Value::Switch(self.sys_screen(n).and_then(|x| x.hdr) == Some(true)),
            Opt::Vrr(n) => Value::Switch(self.sys_screen(n).and_then(|x| x.vrr) == Some(true)),
            Opt::Primary(n) => Value::Switch(self.sys_screen(n).is_some_and(|x| x.primary)),
            Opt::Volume | Opt::SoundOutput if !ok(&s.sound) => Value::Text(String::new()),
            Opt::Volume
            | Opt::SoundOutput
            | Opt::DisplayMode(_)
            | Opt::DisplayScale(_)
            | Opt::PowerProfile => {
                let key = self.sys_opt_key(o)?;
                let label = self
                    .sys_opt_choices(o)?
                    .into_iter()
                    .find(|(_, k)| *k == key)
                    .map_or(key, |(l, _)| l);
                // What is chosen from a list shows no arrows for left and right.
                match Self::sys_turns(o) {
                    true => Value::Choice(label),
                    false => Value::Text(label),
                }
            }
            _ => return None,
        })
    }

    pub(super) fn sys_opt_key(&self, o: &Opt) -> Option<String> {
        Some(match o {
            Opt::Volume => {
                let v = self.sys_sound().map_or(0, |s| s.volume);
                // On the steps of the choice.
                ((u32::from(v) + 2) / 5 * 5).min(100).to_string()
            }
            Opt::SoundOutput => self
                .sys_sound()?
                .outputs
                .iter()
                .find(|x| x.default)?
                .id
                .to_string(),
            Opt::DisplayMode(n) => self.sys_screen(n)?.mode.clone(),
            Opt::DisplayScale(n) => {
                let scale = self.sys_screen(n)?.scale;
                ((scale * 100.).round() as u32).to_string()
            }
            Opt::PowerProfile => self.console.sys.power.as_ref()?.current.clone()?,
            _ => return None,
        })
    }

    pub(super) fn sys_opt_choices(&self, o: &Opt) -> Option<Vec<(String, String)>> {
        Some(match o {
            Opt::Volume => (0..=20)
                .map(|n| (self.percent(n as f32 / 20.), (n * 5).to_string()))
                .collect(),
            Opt::SoundOutput => self
                .sys_sound()?
                .outputs
                .iter()
                .map(|x| (x.name.clone(), x.id.to_string()))
                .collect(),
            Opt::DisplayMode(n) => self
                .sys_screen(n)?
                .modes
                .iter()
                .map(|m| (m.label(), m.id.clone()))
                .collect(),
            Opt::DisplayScale(n) => {
                let now = (self.sys_screen(n)?.scale * 100.).round() as u32;
                let mut list: Vec<u32> = SCALES.to_vec();
                if !list.contains(&now) {
                    list.push(now);
                    list.sort();
                }
                list.into_iter()
                    .map(|p| (format!("{p} %"), p.to_string()))
                    .collect()
            }
            Opt::PowerProfile => self
                .console
                .sys
                .power
                .as_ref()?
                .profiles
                .iter()
                .map(|p| (self.profile_name(p), p.clone()))
                .collect(),
            _ => return None,
        })
    }

    fn profile_name(&self, p: &str) -> String {
        match p {
            "performance" => self.tr("Performance", "Performans"),
            "balanced" => self.tr("Balanced", "Dengeli"),
            "power-saver" => self.tr("Power saver", "Güç tasarrufu"),
            other => other,
        }
        .to_string()
    }

    /// A choice was made; `false` when it is no setting of the computer.
    pub(super) fn sys_opt_set(&mut self, o: &Opt, key: &str) -> bool {
        let change = match o {
            Opt::Volume => key.parse().ok().map(Change::Volume),
            Opt::SoundOutput => key.parse().ok().map(Change::Output),
            Opt::PowerProfile => Some(Change::Profile(key.to_string())),
            Opt::DisplayMode(n) => {
                let Some(now) = self.sys_screen(n).map(|s| s.mode.clone()) else {
                    return true;
                };
                if now != key {
                    self.sys_try_screen(
                        Change::Mode(n.clone(), key.to_string()),
                        Change::Mode(n.clone(), now),
                    );
                }
                return true;
            }
            Opt::DisplayScale(n) => {
                let Some(now) = self.sys_screen(n).map(|s| s.scale) else {
                    return true;
                };
                if let Ok(p) = key.parse::<u32>()
                    && (f64::from(p) / 100. - now).abs() > 0.001
                {
                    self.sys_try_screen(
                        Change::Scale(n.clone(), f64::from(p) / 100.),
                        Change::Scale(n.clone(), now),
                    );
                }
                return true;
            }
            _ => return false,
        };
        // Seen at once; read back after.
        if let (Opt::Volume, Some(Change::Volume(v))) = (o, &change)
            && let Some(Ok(s)) = &mut self.console.sys.sound
        {
            s.volume = *v;
        }
        self.console.sys.queue.extend(change);
        true
    }

    /// A switch was turned; `false` when it is no setting of the computer.
    pub(super) fn sys_opt_toggle(&mut self, o: &Opt) -> bool {
        let on = |v: Option<Value>| matches!(v, Some(Value::Switch(true)));
        let now = on(self.sys_opt_value(o));
        let change = match o {
            Opt::Wifi => Change::WifiOn(!now),
            Opt::Bluetooth => Change::BluetoothOn(!now),
            Opt::Muted => Change::Muted(!now),
            Opt::Hdr(n) => {
                self.sys_try_screen(Change::Hdr(n.clone(), !now), Change::Hdr(n.clone(), now));
                return true;
            }
            Opt::Vrr(n) => Change::Vrr(n.clone(), !now),
            // The first screen cannot be made "not first": another one is chosen instead.
            Opt::Primary(n) if !now => Change::Primary(n.clone()),
            Opt::Primary(_) => return true,
            _ => return false,
        };
        // Seen at once; read back after.
        let s = &mut self.console.sys;
        match (&change, &mut s.wifi, &mut s.bluetooth, &mut s.sound) {
            (Change::WifiOn(v), Some(Ok(w)), _, _) => w.on = *v,
            (Change::BluetoothOn(v), _, Some(Ok(b)), _) => b.on = *v,
            (Change::Muted(v), _, _, Some(Ok(x))) => x.muted = *v,
            _ => {}
        }
        s.queue.push(change);
        true
    }

    /// Whether left and right may turn it (a screen's size would change at every step).
    pub(super) fn sys_turns(o: &Opt) -> bool {
        !matches!(
            o,
            Opt::DisplayMode(_) | Opt::DisplayScale(_) | Opt::SoundOutput
        )
    }

    // ---- The screens ----

    /// What the settings' list says about a category.
    pub(super) fn sys_summary(&self, c: AppCat) -> Value {
        let s = &self.console.sys;
        let text = match c {
            AppCat::Network => match &s.wifi {
                Some(Ok(w)) => match w.networks.iter().find(|n| n.connected) {
                    Some(n) => n.ssid.clone(),
                    None if w.wired => self.tr("Cable", "Kablolu").into(),
                    None => self.tr("Not connected", "Bağlı değil").into(),
                },
                _ => String::new(),
            },
            AppCat::Sound => match self.sys_sound() {
                Some(x) if x.muted => self.tr("Muted", "Sessiz").into(),
                Some(x) => self.percent(f32::from(x.volume) / 100.),
                None => String::new(),
            },
            AppCat::Power => s
                .power
                .as_ref()
                .and_then(|p| p.current.as_deref())
                .map(|p| self.profile_name(p))
                .unwrap_or_default(),
            _ => String::new(),
        };
        Value::Text(text)
    }

    fn sys_line(&self, act: Act, icon: &'static str, label: &str, value: Value) -> Row {
        Row::new(Kind::Line, vec![Item::line(act, icon, label, value)])
    }

    fn sys_opt(&self, o: Opt, icon: &'static str, label: &str) -> Row {
        let value = self.sys_opt_value(&o).unwrap_or(Value::None);
        self.sys_line(Act::Opt(o), icon, label, value)
    }

    /// A note in place of the settings: still reading, or what went wrong.
    fn sys_note(&self, error: Option<&String>, what: &str) -> Vec<Row> {
        let row = match error {
            Some(e) => {
                let mut row = self.sys_line(
                    Act::Nothing,
                    "circle-alert",
                    &format!("{} {what}", self.tr("Cannot read:", "Okunamadı:")),
                    Value::None,
                );
                row.items[0].sub = e.clone();
                row
            }
            None => self.sys_line(
                Act::Nothing,
                "loader-circle",
                self.tr("Looking…", "Bakılıyor…"),
                Value::None,
            ),
        };
        vec![row]
    }

    pub(super) fn sys_rows(&self, c: AppCat) -> Vec<Row> {
        match c {
            AppCat::Network => self.wifi_rows(),
            AppCat::Bluetooth => self.bluetooth_rows(),
            AppCat::Sound => self.sound_rows(),
            AppCat::Display => self.display_rows(),
            AppCat::Power => self.power_rows(),
            _ => Vec::new(),
        }
    }

    fn wifi_rows(&self) -> Vec<Row> {
        let w = match &self.console.sys.wifi {
            Some(Ok(w)) => w,
            Some(Err(e)) => return self.sys_note(Some(e), self.tr("the network", "ağ")),
            None => return self.sys_note(None, ""),
        };
        let mut rows = Vec::new();
        if w.wired {
            rows.push(self.sys_line(
                Act::Nothing,
                "cable",
                self.tr("Cable", "Kablolu bağlantı"),
                Value::Text(self.tr("Connected", "Bağlı").into()),
            ));
        }
        if !w.card {
            rows.push(self.sys_line(
                Act::Nothing,
                "wifi-off",
                self.tr(
                    "This computer has no wireless card",
                    "Bu bilgisayarda kablosuz ağ kartı yok",
                ),
                Value::None,
            ));
            return rows;
        }
        rows.push(self.sys_opt(Opt::Wifi, "wifi", "Wi-Fi"));
        if !w.on {
            return rows;
        }
        let busy = self.console.sys.busy.clone();
        rows.push(self.sys_line(
            Act::WifiScan,
            "refresh-cw",
            self.tr("Look for networks again", "Ağları yeniden ara"),
            Value::Text(match busy.as_deref() {
                Some("scan") => self.tr("Looking…", "Bakılıyor…").into(),
                _ => String::new(),
            }),
        ));
        for (n, net) in w.networks.iter().enumerate() {
            let icon = match net.signal {
                70.. => "wifi",
                45.. => "wifi-high",
                20.. => "wifi-low",
                _ => "wifi-zero",
            };
            let value = if busy.as_deref() == Some(net.ssid.as_str()) {
                self.tr("Connecting…", "Bağlanıyor…").to_string()
            } else if net.connected {
                self.tr("Connected", "Bağlı").to_string()
            } else if net.secured && !net.known {
                format!("🔒 {}", self.percent(f32::from(net.signal) / 100.))
            } else {
                self.percent(f32::from(net.signal) / 100.)
            };
            let mut row = self.sys_line(
                Act::WifiNet(net.ssid.clone()),
                icon,
                &net.ssid,
                Value::Text(value),
            );
            if n == 0 {
                row.title = Some(self.tr("Networks", "Ağlar").into());
            }
            rows.push(row);
        }
        rows
    }

    fn bluetooth_rows(&self) -> Vec<Row> {
        let b = match &self.console.sys.bluetooth {
            Some(Ok(b)) => b,
            Some(Err(e)) => return self.sys_note(Some(e), "Bluetooth"),
            None => return self.sys_note(None, ""),
        };
        if !b.adapter {
            return vec![self.sys_line(
                Act::Nothing,
                "bluetooth-off",
                self.tr(
                    "This computer has no Bluetooth",
                    "Bu bilgisayarda Bluetooth yok",
                ),
                Value::None,
            )];
        }
        let mut rows = vec![self.sys_opt(Opt::Bluetooth, "bluetooth", "Bluetooth")];
        if !b.on {
            return rows;
        }
        let busy = self.console.sys.busy.clone();
        let mut search = self.sys_line(
            Act::BtScan,
            "bluetooth-searching",
            self.tr("Look for devices nearby", "Yakındaki cihazları ara"),
            Value::Text(if self.console.sys.scanning {
                self.tr("Looking…", "Bakılıyor…").into()
            } else {
                String::new()
            }),
        );
        search.items[0].sub = self
            .tr(
                "Put the gamepad or headset into pairing mode first.",
                "Önce kolu ya da kulaklığı eşleşme moduna al.",
            )
            .into();
        rows.push(search);
        let mut first_paired = true;
        let mut first_new = true;
        for d in &b.devices {
            let icon = match d.icon.as_str() {
                i if i.contains("gaming") => "gamepad-2",
                i if i.contains("audio") || i.contains("head") => "headphones",
                i if i.contains("keyboard") => "keyboard",
                i if i.contains("mouse") => "mouse",
                _ => "bluetooth",
            };
            let value = if busy.as_deref() == Some(d.mac.as_str()) {
                self.tr("Working…", "Yapılıyor…").to_string()
            } else if d.connected {
                match d.battery {
                    Some(p) => format!(
                        "{} · {}",
                        self.tr("Connected", "Bağlı"),
                        self.percent(f32::from(p) / 100.)
                    ),
                    None => self.tr("Connected", "Bağlı").to_string(),
                }
            } else if d.paired {
                self.tr("Not connected", "Bağlı değil").to_string()
            } else {
                self.tr("Pair", "Eşleştir").to_string()
            };
            let act = if d.paired {
                Act::BtDevice(d.mac.clone())
            } else {
                Act::BtPair(d.mac.clone())
            };
            let name = if d.name.is_empty() {
                d.mac.clone()
            } else {
                d.name.clone()
            };
            let mut row = self.sys_line(act, icon, &name, Value::Text(value));
            if d.paired && std::mem::take(&mut first_paired) {
                row.title = Some(self.tr("My devices", "Cihazlarım").into());
            }
            if !d.paired && std::mem::take(&mut first_new) {
                row.title = Some(self.tr("Nearby", "Yakındakiler").into());
            }
            rows.push(row);
        }
        rows
    }

    fn sound_rows(&self) -> Vec<Row> {
        match &self.console.sys.sound {
            Some(Ok(_)) => {}
            Some(Err(e)) => return self.sys_note(Some(e), self.tr("the sound", "ses")),
            None => return self.sys_note(None, ""),
        }
        let mut volume = self.sys_opt(Opt::Volume, "volume-2", self.tr("Volume", "Ses düzeyi"));
        volume.items[0].sub = self
            .tr("Left and right change it.", "Sol ve sağ ile değişir.")
            .into();
        vec![
            volume,
            self.sys_opt(Opt::Muted, "volume-x", self.tr("Mute", "Sessiz")),
            self.sys_opt(
                Opt::SoundOutput,
                "speaker",
                self.tr("Sound comes out of", "Sesin çıktığı yer"),
            ),
        ]
    }

    fn display_rows(&self) -> Vec<Row> {
        let list = match &self.console.sys.screens {
            Some(Ok(l)) => l,
            Some(Err(e)) => return self.sys_note(Some(e), self.tr("the screens", "ekranlar")),
            None => return self.sys_note(None, ""),
        };
        let mut rows = Vec::new();
        for s in list.iter().filter(|s| s.enabled) {
            let n = &s.name;
            let mut mode = self.sys_opt(
                Opt::DisplayMode(n.clone()),
                "monitor",
                self.tr("Size and refresh rate", "Çözünürlük ve yenileme hızı"),
            );
            mode.title = Some(n.clone());
            rows.push(mode);
            rows.push(self.sys_opt(
                Opt::DisplayScale(n.clone()),
                "zoom-in",
                self.tr("Scale", "Ölçek"),
            ));
            if s.hdr.is_some() {
                rows.push(self.sys_opt(Opt::Hdr(n.clone()), "sun", "HDR"));
            }
            if s.vrr.is_some() {
                rows.push(self.sys_opt(
                    Opt::Vrr(n.clone()),
                    "activity",
                    self.tr("Variable refresh rate", "Değişken yenileme hızı"),
                ));
            }
            if list.iter().filter(|s| s.enabled).count() > 1 {
                rows.push(self.sys_opt(
                    Opt::Primary(n.clone()),
                    "star",
                    self.tr(
                        "First screen (games go here)",
                        "Ana ekran (oyunlar burada açılır)",
                    ),
                ));
            }
        }
        rows
    }

    fn power_rows(&self) -> Vec<Row> {
        let Some(p) = &self.console.sys.power else {
            return self.sys_note(None, "");
        };
        let mut rows = Vec::new();
        if !p.profiles.is_empty() {
            let mut row = self.sys_opt(
                Opt::PowerProfile,
                "gauge",
                self.tr("Power profile", "Güç profili"),
            );
            row.items[0].sub = self
                .tr(
                    "Performance: more speed, more heat and noise.",
                    "Performans: daha hızlı, daha sıcak ve gürültülü.",
                )
                .into();
            rows.push(row);
        }
        if let Some((percent, charging)) = p.battery {
            let icon = match (charging, percent) {
                (true, _) => "battery-charging",
                (_, 70..) => "battery-full",
                (_, 30..) => "battery-medium",
                _ => "battery-low",
            };
            rows.push(self.sys_line(
                Act::Nothing,
                icon,
                self.tr("Battery", "Pil"),
                Value::Text(self.percent(f32::from(percent) / 100.)),
            ));
        }
        if rows.is_empty() {
            rows.push(self.sys_line(
                Act::Nothing,
                "gauge",
                self.tr(
                    "Power profiles are not available (install power-profiles-daemon).",
                    "Güç profilleri yok (power-profiles-daemon kurulmalı).",
                ),
                Value::None,
            ));
        }
        rows
    }

    // ---- What their lines do ----

    fn wifi_net(&self, ssid: &str) -> Option<wifi::Network> {
        match &self.console.sys.wifi {
            Some(Ok(w)) => w.networks.iter().find(|n| n.ssid == ssid).cloned(),
            _ => None,
        }
    }

    pub(super) fn sys_act(&mut self, act: Act, cx: &mut Context<Self>) {
        match act {
            Act::WifiScan => self.sys_do(
                "scan".into(),
                AppCat::Network,
                || wifi::state(true).map(|_| ()),
                None,
                cx,
            ),
            Act::WifiNet(ssid) => {
                let Some(net) = self.wifi_net(&ssid) else {
                    return;
                };
                if net.connected || net.known {
                    let mut options = Vec::new();
                    if net.connected {
                        options.push((
                            self.tr("Disconnect", "Bağlantıyı kes").to_string(),
                            Act::WifiDisconnect(ssid.clone()),
                            false,
                        ));
                    } else {
                        options.push((
                            self.tr("Connect", "Bağlan").to_string(),
                            Act::WifiConnect(ssid.clone()),
                            false,
                        ));
                    }
                    options.push((
                        self.tr("Forget its password", "Şifresini unut").to_string(),
                        Act::WifiForget(ssid.clone()),
                        false,
                    ));
                    options.push((self.tr("Close", "Kapat").to_string(), Act::Close, false));
                    self.cue(Cue::Open);
                    self.console_overlay(Overlay::Pick(ssid, options));
                } else if net.secured {
                    self.console_password(NeedsPassword::Wifi(ssid));
                } else {
                    self.sys_act(Act::WifiConnect(ssid), cx);
                }
            }
            Act::WifiConnect(ssid) => {
                let Some(net) = self.wifi_net(&ssid) else {
                    return;
                };
                let done = self.tr("Connected.", "Bağlandı.");
                self.sys_do(
                    ssid,
                    AppCat::Network,
                    move || wifi::connect(&net, None),
                    Some(done),
                    cx,
                );
            }
            Act::WifiDisconnect(ssid) => {
                let Some(net) = self.wifi_net(&ssid) else {
                    return;
                };
                self.sys_do(
                    ssid,
                    AppCat::Network,
                    move || wifi::disconnect(&net),
                    None,
                    cx,
                );
            }
            Act::WifiForget(ssid) => {
                let Some(net) = self.wifi_net(&ssid) else {
                    return;
                };
                self.sys_do(ssid, AppCat::Network, move || wifi::forget(&net), None, cx);
            }
            Act::BtScan => self.bluetooth_scan(cx),
            Act::BtDevice(mac) => {
                let Some(d) = self.bt_device(&mac) else {
                    return;
                };
                let mut options = Vec::new();
                if d.connected {
                    options.push((
                        self.tr("Disconnect", "Bağlantıyı kes").to_string(),
                        Act::BtDisconnect(mac.clone()),
                        false,
                    ));
                } else {
                    options.push((
                        self.tr("Connect", "Bağlan").to_string(),
                        Act::BtConnect(mac.clone()),
                        false,
                    ));
                }
                options.push((
                    self.tr("Forget it", "Unut").to_string(),
                    Act::BtForget(mac.clone()),
                    false,
                ));
                options.push((self.tr("Close", "Kapat").to_string(), Act::Close, false));
                self.cue(Cue::Open);
                let title = if d.name.is_empty() { mac } else { d.name };
                self.console_overlay(Overlay::Pick(title, options));
            }
            Act::BtPair(mac) => {
                let done = self.tr("Paired and connected.", "Eşleşti ve bağlandı.");
                self.sys_do(
                    mac.clone(),
                    AppCat::Bluetooth,
                    move || bluetooth::pair(&mac),
                    Some(done),
                    cx,
                );
            }
            Act::BtConnect(mac) => {
                self.sys_do(
                    mac.clone(),
                    AppCat::Bluetooth,
                    move || bluetooth::connect(&mac),
                    None,
                    cx,
                );
            }
            Act::BtDisconnect(mac) => {
                self.sys_do(
                    mac.clone(),
                    AppCat::Bluetooth,
                    move || bluetooth::disconnect(&mac),
                    None,
                    cx,
                );
            }
            Act::BtForget(mac) => {
                self.sys_do(
                    mac.clone(),
                    AppCat::Bluetooth,
                    move || bluetooth::remove(&mac),
                    None,
                    cx,
                );
            }
            Act::DisplayKeep => self.sys_keep(),
            _ => {}
        }
        cx.notify();
    }

    fn bt_device(&self, mac: &str) -> Option<bluetooth::Device> {
        match &self.console.sys.bluetooth {
            Some(Ok(b)) => b.devices.iter().find(|d| d.mac == mac).cloned(),
            _ => None,
        }
    }

    /// Looks for devices for a while; they show as they come.
    fn bluetooth_scan(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || std::mem::replace(&mut self.console.sys.scanning, true) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async { bluetooth::scan(20) }).await;
            let _ = this.update(cx, |this, cx| {
                this.console.sys.scanning = false;
                if let Err(e) = result {
                    this.status = Some(format!("{e:#}").into());
                }
                this.sys_load(AppCat::Bluetooth, cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// The Wi-Fi password was typed.
    pub(super) fn wifi_with_password(
        &mut self,
        ssid: String,
        password: String,
        cx: &mut Context<Self>,
    ) {
        let Some(net) = self.wifi_net(&ssid) else {
            return;
        };
        let done = self.tr("Connected.", "Bağlandı.");
        self.sys_do(
            ssid,
            AppCat::Network,
            move || wifi::connect(&net, Some(&password)),
            Some(done),
            cx,
        );
    }
}

/// The categories about the computer.
pub(super) fn sys_cat(c: AppCat) -> bool {
    matches!(
        c,
        AppCat::Network | AppCat::Bluetooth | AppCat::Sound | AppCat::Display | AppCat::Power
    )
}

#[cfg(test)]
impl SysState {
    /// What a test computer has.
    pub(super) fn fake() -> SysState {
        SysState {
            wifi: Some(Ok(wifi::State {
                card: true,
                on: true,
                wired: false,
                networks: vec![
                    wifi::Network {
                        ssid: "Home".into(),
                        signal: 80,
                        secured: true,
                        connected: false,
                        known: false,
                    },
                    wifi::Network {
                        ssid: "Cafe".into(),
                        signal: 30,
                        secured: false,
                        connected: false,
                        known: false,
                    },
                ],
            })),
            sound: Some(Ok(sound::State {
                volume: 40,
                muted: false,
                outputs: vec![
                    sound::Output {
                        id: 52,
                        name: "TV".into(),
                        default: false,
                    },
                    sound::Output {
                        id: 53,
                        name: "Headset".into(),
                        default: true,
                    },
                ],
            })),
            screens: Some(Ok(vec![display::Screen {
                name: "DP-1".into(),
                enabled: true,
                primary: true,
                modes: vec![
                    display::Mode {
                        id: "26".into(),
                        rate: 180.,
                        size: display::Size {
                            width: 2560,
                            height: 1440,
                        },
                    },
                    display::Mode {
                        id: "25".into(),
                        rate: 60.,
                        size: display::Size {
                            width: 2560,
                            height: 1440,
                        },
                    },
                ],
                mode: "26".into(),
                scale: 1.,
                hdr: Some(false),
                vrr: None,
            }])),
            ..SysState::default()
        }
    }

    pub(super) fn waits_to_be_kept(&self) -> bool {
        self.revert.is_some()
    }

    pub(super) fn queued(&self) -> &[Change] {
        &self.queue
    }
}
