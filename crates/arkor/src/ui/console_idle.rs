//! Console mode when nobody plays: the screen darkens after a while, the computer may go
//! to sleep after a longer one, and while the pad is in use (or something downloads) the
//! desktop is kept from locking or sleeping, since it does not see a gamepad as use. Words
//! about ended downloads and low batteries reach the player in a game, too.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use gpui::{Context, Div, FontWeight, Window, div, prelude::*, px};

use super::Library;
use super::console::Opt;
use super::console::Value;
use super::style::pal;
use super::synth::Cue;

/// Minutes of no input after which the desktop may lock or sleep again.
const KEEP_AWAKE: u64 = 10;
/// Seconds each picture stays while resting.
const PICTURE_SECS: u64 = 25;

/// When Arkor started (the resting screen's movement follows it).
static START: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

/// Below this a pad's battery is low.
const LOW: u8 = 15;

pub(super) const DIM_CHOICES: [u32; 5] = [0, 2, 5, 10, 30];
pub(super) const SLEEP_CHOICES: [u32; 5] = [0, 15, 30, 60, 120];

#[derive(Default)]
pub(super) struct IdleState {
    /// When something was pressed or moved last.
    last: Option<Instant>,
    pub(super) dimmed: bool,
    dimmed_at: Option<Instant>,
    /// The picture shown, its game and since when; the one before (it fades out).
    picture: Option<(super::console::Behind, String, Instant)>,
    before: Option<super::console::Behind>,
    next: usize,
    /// `systemd-inhibit` while the desktop must stay awake.
    awake: Option<Child>,
    /// Ended downloads that were told about, by key and time.
    told: Vec<(String, u64)>,
    started: bool,
    /// Pads whose low battery was told, until they charge.
    low: Vec<String>,
}

impl Drop for IdleState {
    fn drop(&mut self) {
        if let Some(mut c) = self.awake.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Library {
    /// Something was pressed: `true` when that only woke the screen.
    pub(super) fn idle_input(&mut self) -> bool {
        let s = &mut self.console.idle;
        s.last = Some(Instant::now());
        std::mem::take(&mut s.dimmed)
    }

    fn idle_secs(&self) -> u64 {
        self.console.idle.last.map_or(0, |t| t.elapsed().as_secs())
    }

    /// Something goes on that must not be interrupted.
    fn busy(&self) -> bool {
        !self.running.is_empty()
            || self.console.launching.is_some()
            || arkor_launch::progress::list().iter().any(|e| !e.external)
    }

    fn downloading(&self) -> bool {
        arkor_launch::progress::list().iter().any(|e| !e.external)
    }

    /// Once a second while console mode is on.
    pub(super) fn idle_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.console.idle.started {
            self.console.idle.started = true;
            self.console.idle.last = Some(Instant::now());
            self.console.idle.told = arkor_launch::downloads::history()
                .into_iter()
                .map(|f| (f.key, f.at))
                .collect();
        }
        let idle = self.idle_secs();
        let busy = self.busy();
        let dim = u64::from(self.global.console_dim_mins.unwrap_or(5)) * 60;
        if !busy && dim > 0 && idle >= dim && !self.console.idle.dimmed {
            let s = &mut self.console.idle;
            s.dimmed = true;
            s.dimmed_at = Some(Instant::now());
            s.picture = None;
            s.before = None;
        }
        if self.console.idle.dimmed {
            self.rest_step(cx);
            // Its time and slow movement.
            cx.notify();
        }
        let sleep = u64::from(self.global.console_sleep_mins.unwrap_or(0)) * 60;
        if !busy && sleep > 0 && idle >= sleep {
            // Not again right after waking.
            self.console.idle.last = Some(Instant::now());
            if !cfg!(test) {
                let _ = Command::new("systemctl").arg("suspend").spawn();
            }
        }
        self.keep_awake(idle < KEEP_AWAKE * 60 || self.downloading());
        self.tell_events(window, cx);
    }

    /// Holds or lets go of the desktop's lock and sleep.
    fn keep_awake(&mut self, want: bool) {
        let s = &mut self.console.idle;
        // Ended by itself (or never started): looked at again.
        if let Some(c) = s.awake.as_mut()
            && c.try_wait().ok().flatten().is_some()
        {
            s.awake = None;
        }
        match (want, s.awake.is_some()) {
            (true, false) if !cfg!(test) => {
                use std::os::unix::process::CommandExt;
                let mut cmd = Command::new("systemd-inhibit");
                cmd.args([
                    "--what=idle:sleep",
                    "--who=Arkor",
                    "--why=Console mode is in use",
                    "--mode=block",
                    "sleep",
                    "infinity",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
                // SAFETY: only an async-signal-safe syscall between fork and exec.
                unsafe {
                    cmd.pre_exec(|| {
                        // It must not outlive Arkor.
                        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                        Ok(())
                    });
                }
                s.awake = cmd.spawn().ok();
            }
            (false, true) => {
                if let Some(mut c) = s.awake.take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
            _ => {}
        }
    }

    /// Ended downloads and low batteries: on the screen when Arkor is in front, else as the
    /// desktop's notification (over a game).
    fn tell_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut said: Vec<String> = Vec::new();
        for f in arkor_launch::downloads::history() {
            let key = (f.key.clone(), f.at);
            if self.console.idle.told.contains(&key) {
                continue;
            }
            self.console.idle.told.push(key);
            said.push(if f.ok {
                format!("{} {}", self.tr("Downloaded:", "İndirildi:"), f.label)
            } else {
                format!(
                    "{} {}",
                    self.tr("Download failed:", "İndirilemedi:"),
                    f.label
                )
            });
        }
        for p in super::gamepad::pads() {
            match p.battery {
                Some((percent, false)) if percent <= LOW => {
                    if !self.console.idle.low.contains(&p.name) {
                        self.console.idle.low.push(p.name.clone());
                        said.push(format!(
                            "{}: {}",
                            self.tr("The gamepad's battery is low", "Kolun pili azaldı"),
                            self.percent(f32::from(percent) / 100.)
                        ));
                    }
                }
                Some((_, true)) => self.console.idle.low.retain(|n| *n != p.name),
                _ => {}
            }
        }
        let Some(text) = said.last().cloned() else {
            return;
        };
        if window.is_window_active() {
            self.cue(Cue::Done);
            self.status = Some(text.into());
            cx.notify();
        } else if !cfg!(test) {
            for text in said {
                let _ = Command::new("notify-send")
                    .args(["--app-name=Arkor", "--icon=arkor", "Arkor", &text])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
        }
    }

    /// The games the resting screen shows, one after another: played lately, favourites.
    fn rest_games(&self) -> Vec<String> {
        let mut list: Vec<(u64, &arkor_core::library::LibraryGame)> = self
            .library
            .games
            .iter()
            .filter(|g| !g.hidden)
            .map(|g| (self.playtime(&g.game_id()).1.unwrap_or(0), g))
            .collect();
        list.sort_by_key(|(last, g)| std::cmp::Reverse((g.favorite, *last)));
        list.into_iter()
            .take(12)
            .map(|(_, g)| g.id.clone())
            .collect()
    }

    /// The next picture, now and then while resting.
    fn rest_step(&mut self, cx: &mut Context<Self>) {
        let due = self
            .console
            .idle
            .picture
            .as_ref()
            .is_none_or(|(_, _, at)| at.elapsed() > Duration::from_secs(PICTURE_SECS));
        if !due {
            return;
        }
        let games = self.rest_games();
        if games.is_empty() {
            return;
        }
        let at = self.console.idle.next % games.len();
        self.console.idle.next = at + 1;
        let id = games[at].clone();
        if let Some(b) = self.console_behind(&super::console::Art::Game(id.clone()), cx) {
            let before = self.console.idle.picture.take().map(|(b, _, _)| b);
            self.console.idle.before = before;
            self.console.idle.picture = Some((b, id, Instant::now()));
        }
    }

    /// While it rests: what it moves and fades wants frames.
    pub(super) fn rest_moving(&self) -> bool {
        let s = &self.console.idle;
        s.dimmed
            && (s
                .dimmed_at
                .is_some_and(|t| t.elapsed() < Duration::from_millis(1600))
                || s.picture
                    .as_ref()
                    .is_some_and(|(_, _, at)| at.elapsed() < Duration::from_millis(2400)))
    }

    /// The resting screen: a game's picture drifting behind the time, the date, what goes on
    /// (downloads, the pad's battery) and how to go on.
    pub(super) fn idle_view(&self, vw: f32, vh: f32, u: f32, margin: f32) -> Option<Div> {
        use super::cover_art::{self, Size};
        let s = &self.console.idle;
        if !s.dimmed {
            return None;
        }
        let since = |t: Option<Instant>, ms: f32| {
            t.map_or(1., |t| (t.elapsed().as_secs_f32() * 1000. / ms).min(1.))
        };
        let shown = since(s.dimmed_at, 1400.);
        // Slow movement, so nothing stays burnt into the screen.
        let secs = START.elapsed().as_secs_f32();
        let drift = |period: f32, phase: f32| (secs / period * std::f32::consts::TAU + phase).sin();
        let (dx, dy) = (drift(180., 0.) * 24. * u, drift(240., 1.3) * 16. * u);
        let picture = |b: &super::console::Behind, opacity: f32| {
            let room = div()
                .absolute()
                .left(px(-40. * u + dx * 1.6))
                .top(px(-40. * u + dy * 1.6))
                .w(px(vw + 80. * u))
                .h(px(vh + 80. * u));
            if b.size == Size::Wide {
                // A little larger than the screen: it can move without showing its edge.
                return div()
                    .absolute()
                    .inset_0()
                    .opacity((opacity * 0.7).max(0.001))
                    .child(
                        room.child(cover_art::backdrop(b.path.clone(), Size::Wide, 0.).size_full()),
                    );
            }
            // Only a cover: its colours fill the room, the cover stands at the right.
            let ph = vh * 0.62;
            div()
                .absolute()
                .inset_0()
                .opacity(opacity.max(0.001))
                .child(
                    room.opacity(0.55)
                        .child(cover_art::backdrop(b.path.clone(), Size::Blur, 0.).size_full()),
                )
                .child(
                    div()
                        .absolute()
                        .top(px((vh - ph) * 0.42 - dy))
                        .right(px(margin * 1.6 - dx))
                        .w(px(ph / 1.5))
                        .h(px(ph))
                        .opacity(0.92)
                        .rounded(px(24. * u))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::black().opacity(0.65),
                            offset: gpui::point(px(0.), px(34. * u)),
                            blur_radius: px(110. * u),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .child(cover_art::cover(b.path.clone(), 24. * u, false).size_full()),
                )
        };
        let fade = s
            .picture
            .as_ref()
            .map_or(1., |(_, _, at)| since(Some(*at), 2200.));
        let name = s
            .picture
            .as_ref()
            .and_then(|(_, id, _)| self.library.get(id))
            .map(|g| g.name.clone());
        let mut things = div().flex().items_center().gap(px(28. * u));
        let thing = |icon: &'static str, text: String| {
            div()
                .flex()
                .items_center()
                .gap(px(10. * u))
                .child(super::style::icon(
                    icon,
                    22. * u,
                    pal::text_soft().opacity(0.75),
                ))
                .child(text)
        };
        if let Some(e) = arkor_launch::progress::list().iter().find(|e| !e.external) {
            let text = match e.fraction() {
                Some(f) => format!("{} · {}", e.label, self.percent(f)),
                None => e.label.clone(),
            };
            things = things.child(thing("download", text));
        }
        if let Some((p, (b, _))) = super::gamepad::pad().and_then(|p| p.battery.map(|b| (p, b))) {
            let _ = p;
            things = things.child(thing("gamepad-2", self.percent(f32::from(b) / 100.)));
        }
        let pad = self.console.pad;
        Some(
            div()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .bg(gpui::black().opacity(shown))
                .children(s.before.as_ref().map(|b| picture(b, shown * (1. - fade))))
                .children(s.picture.as_ref().map(|(b, _, _)| picture(b, shown * fade)))
                // Dark where the words are.
                .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                    20.,
                    gpui::linear_color_stop(gpui::black().opacity(0.92), 0.),
                    gpui::linear_color_stop(gpui::black().opacity(0.2), 1.),
                )))
                .child(
                    div()
                        .absolute()
                        .left(px(margin + dx))
                        .bottom(px(110. * u - dy))
                        .opacity(shown)
                        .flex()
                        .flex_col()
                        .gap(px(6. * u))
                        .child(
                            div()
                                .text_size(px(150. * u))
                                .line_height(px(160. * u))
                                .font_weight(FontWeight::LIGHT)
                                .text_color(pal::text().opacity(0.92))
                                .child(super::console_view::clock()),
                        )
                        .child(
                            div()
                                .text_size(px(30. * u))
                                .text_color(pal::text_soft().opacity(0.8))
                                .child(self.today()),
                        )
                        .children(name.map(|n| {
                            div()
                                .mt(px(30. * u))
                                .flex()
                                .flex_col()
                                .gap(px(2. * u))
                                .child(
                                    div()
                                        .text_size(px(17. * u))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(pal::text_soft().opacity(0.55))
                                        .child(
                                            self.tr("FROM YOUR LIBRARY", "KÜTÜPHANENDEN")
                                                .to_string(),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(30. * u))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(pal::text().opacity(0.85))
                                        .child(n),
                                )
                        }))
                        .child(
                            things
                                .mt(px(14. * u))
                                .text_size(px(21. * u))
                                .text_color(pal::text_soft().opacity(0.75)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(margin))
                        .bottom(px(48. * u))
                        .opacity(shown * 0.7)
                        .flex()
                        .items_center()
                        .gap(px(10. * u))
                        .text_size(px(20. * u))
                        .text_color(pal::text_soft())
                        .child(
                            div()
                                .px(px(10. * u))
                                .py(px(3. * u))
                                .rounded(px(8. * u))
                                .border_1()
                                .border_color(pal::text().opacity(0.3))
                                .child(if pad {
                                    self.pad_kind().glyph("A")
                                } else {
                                    self.tr("Any key", "Bir tuş")
                                }),
                        )
                        .child(self.tr("Go on", "Devam et")),
                ),
        )
    }

    /// "Thursday, 27 November" in the user's language.
    fn today(&self) -> String {
        // SAFETY: localtime_r fills the zeroed struct from a valid time.
        let tm = unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&now, &mut tm);
            tm
        };
        let day = tm.tm_wday.clamp(0, 6) as usize;
        let month = tm.tm_mon.clamp(0, 11) as usize;
        match self.global.language {
            arkor_core::Language::Tr => format!(
                "{} {} {}",
                tm.tm_mday,
                [
                    "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos",
                    "Eylül", "Ekim", "Kasım", "Aralık"
                ][month],
                [
                    "Pazar",
                    "Pazartesi",
                    "Salı",
                    "Çarşamba",
                    "Perşembe",
                    "Cuma",
                    "Cumartesi"
                ][day]
            ),
            arkor_core::Language::En => format!(
                "{}, {} {}",
                [
                    "Sunday",
                    "Monday",
                    "Tuesday",
                    "Wednesday",
                    "Thursday",
                    "Friday",
                    "Saturday"
                ][day],
                tm.tm_mday,
                [
                    "January",
                    "February",
                    "March",
                    "April",
                    "May",
                    "June",
                    "July",
                    "August",
                    "September",
                    "October",
                    "November",
                    "December"
                ][month]
            ),
        }
    }

    // ---- Its settings ----

    pub(super) fn idle_opt_key(&self, o: &Opt) -> Option<String> {
        Some(match o {
            Opt::DimAfter => self.global.console_dim_mins.unwrap_or(5).to_string(),
            Opt::SleepAfter => self.global.console_sleep_mins.unwrap_or(0).to_string(),
            _ => return None,
        })
    }

    pub(super) fn idle_opt_choices(&self, o: &Opt) -> Option<Vec<(String, String)>> {
        let mut list: Vec<u32> = match o {
            Opt::DimAfter => DIM_CHOICES.to_vec(),
            Opt::SleepAfter => SLEEP_CHOICES.to_vec(),
            _ => return None,
        };
        // A value written by hand is one of them, too.
        if let Some(now) = self.idle_opt_key(o).and_then(|k| k.parse().ok())
            && !list.contains(&now)
        {
            list.push(now);
            list.sort();
        }
        Some(
            list.iter()
                .map(|m| {
                    let label = match m {
                        0 => self.tr("Never", "Hiçbir zaman").to_string(),
                        m => format!("{m} {}", self.tr("min", "dk")),
                    };
                    (label, m.to_string())
                })
                .collect(),
        )
    }

    pub(super) fn idle_opt_set(&mut self, o: &Opt, key: &str) -> bool {
        let Ok(m) = key.parse::<u32>() else {
            return matches!(o, Opt::DimAfter | Opt::SleepAfter);
        };
        match o {
            Opt::DimAfter => self.global.console_dim_mins = Some(m),
            Opt::SleepAfter => self.global.console_sleep_mins = Some(m),
            _ => return false,
        }
        let r = self.global.save();
        self.report(r);
        true
    }

    pub(super) fn idle_rows_value(&self, o: &Opt) -> Option<Value> {
        let key = self.idle_opt_key(o)?;
        let label = self
            .idle_opt_choices(o)?
            .into_iter()
            .find(|(_, k)| *k == key)
            .map_or(key, |(l, _)| l);
        Some(Value::Choice(label))
    }
}

#[cfg(test)]
impl IdleState {
    /// As if nothing had been pressed for `secs`.
    pub(super) fn pretend_idle(&mut self, secs: u64) {
        self.started = true;
        self.last = Instant::now().checked_sub(std::time::Duration::from_secs(secs));
    }

    pub(super) fn keeps_awake(&self) -> bool {
        self.awake.is_some()
    }
}
