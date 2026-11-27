//! The quick menu: the pad's middle button while a game runs brings Arkor over it with what
//! is wanted then (back to the game, sound, the overlay, a screenshot, stopping it), and the
//! same button or B takes it back to the game.

use std::path::PathBuf;
use std::time::Duration;

use arkor_core::library::GameKind;
use arkor_launch::quick;
use gpui::{Context, Window};

use super::Library;
use super::console::{Act, Item, Kind, Opt, Row, Screen, Value};
use super::console_settings::AppCat;
use super::synth::Cue;

/// The game the menu is about, read when it opens.
#[derive(Debug, Clone, Default)]
pub(super) struct QuickState {
    /// Its library entry and processes.
    entry: String,
    pids: Vec<u32>,
    /// Its MangoHud config, when Arkor gave it one; shown, frame limit.
    hud: Option<PathBuf>,
    shown: bool,
    fps: u32,
    /// Gamescope limits its frames, not MangoHud.
    gamescope: bool,
    /// Opened from the desktop: console mode was turned on just for the menu, and goes off
    /// again when it closes.
    pub(super) from_desktop: bool,
}

const FPS: [u32; 12] = [0, 30, 40, 45, 60, 72, 90, 120, 144, 165, 180, 240];

impl Library {
    /// The game the menu would be about: one Arkor started that Steam does not run (Steam
    /// has its own menu on this button).
    pub(super) fn quick_target(&self) -> Option<(String, u32)> {
        self.quick_game()
    }

    fn quick_game(&self) -> Option<(String, u32)> {
        let mut list: Vec<(&String, &u32)> = self.running.iter().collect();
        list.sort();
        list.into_iter().rev().find_map(|(gid, pid)| {
            let entry = gid.split_once(':')?.1;
            let g = self.library.get(entry)?;
            (!matches!(g.kind, GameKind::Steam { .. })).then(|| (entry.to_string(), *pid))
        })
    }

    /// The pad's middle button while a game is in front: the menu comes over it.
    pub(super) fn quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.quick_game().is_none() {
            return false;
        }
        self.quick_show(window, cx)
    }

    /// From the desktop, over a running game: console mode comes on for the menu only.
    pub(super) fn quick_open_from_desktop(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.console.on || self.quick_game().is_none() {
            return false;
        }
        self.toggle_console(window, cx);
        let shown = self.quick_show(window, cx);
        self.console.quick.from_desktop = shown;
        shown
    }

    /// Console mode turned on only for the menu goes off again.
    fn quick_leave_desktop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.console.quick.from_desktop) && self.console.on {
            self.toggle_console(window, cx);
        }
    }

    /// The menu comes over the game.
    fn quick_show(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((entry, pid)) = self.quick_game() else {
            return false;
        };
        let id = arkor_core::GameId::new(arkor_core::Store::Local, &entry);
        let pids = quick::processes(pid);
        let hud = quick::hud_file(&id);
        let from_desktop = self.console.quick.from_desktop;
        self.console.quick = QuickState {
            from_desktop,
            shown: hud.as_deref().is_some_and(quick::hud_shown),
            fps: hud.as_deref().map_or(0, quick::fps_limit),
            gamescope: quick::in_gamescope(&pids),
            hud,
            pids,
            entry: entry.clone(),
        };
        // The "playing" screen would cover it.
        if let Some(l) = self.console.launching.as_mut() {
            l.dismissed = true;
        }
        self.console.overlay = None;
        if self.console.screen() != Screen::Quick(entry.clone()) {
            self.console_open(Screen::Quick(entry), cx);
        }
        self.sys_load(AppCat::Sound, cx);
        self.sys_load(AppCat::Power, cx);
        self.cue(Cue::Open);
        window.activate_window();
        if !cfg!(test) {
            // Where the desktop has no way, this only notes that Arkor asked for itself.
            cx.background_executor()
                .spawn(async { super::desk::show_arkor() })
                .detach();
        }
        cx.notify();
        true
    }

    /// Back to the game.
    pub(super) fn quick_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.console.overlay = None;
        if matches!(self.console.screen(), Screen::Quick(_)) {
            self.console_back(cx);
        }
        self.quick_leave_desktop(window, cx);
        // As they are now: a launcher may have started the game itself since.
        let pids = match self.quick_game() {
            Some((_, pid)) if !cfg!(test) => quick::processes(pid),
            _ => self.console.quick.pids.clone(),
        };
        self.cue(Cue::Back);
        if cfg!(test) {
            return;
        }
        if super::desk::available() {
            cx.background_executor()
                .spawn(async move {
                    super::desk::show_game(&pids);
                })
                .detach();
        } else {
            super::desk::note(
                "back to the game",
                "no way on this desktop; Arkor minimised",
            );
            window.minimize_window();
        }
    }

    /// The game ended: its menu goes.
    pub(super) fn quick_over(&mut self, entry: &str, cx: &mut Context<Self>) {
        if self.console.screen() == Screen::Quick(entry.to_string()) {
            self.console_back(cx);
        }
    }

    /// A screenshot of the game: back to it first, then the screen is saved.
    fn quick_shot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .library
            .get(&self.console.quick.entry)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        self.quick_back(window, cx);
        if cfg!(test) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            bg.timer(Duration::from_millis(900)).await;
            let result = bg.spawn(async move { quick::screenshot(&name) }).await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok(path) => format!(
                        "{} {}",
                        this.tr("Screenshot saved:", "Ekran görüntüsü kaydedildi:"),
                        path.display()
                    )
                    .into(),
                    Err(e) => format!("{e:#}").into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn quick_act(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        match act {
            Act::QuickBack => self.quick_back(window, cx),
            Act::QuickHome => {
                if self.console.quick.from_desktop {
                    // Back to the desktop window, on the game's page.
                    self.quick_leave_desktop(window, cx);
                } else {
                    self.console_open(Screen::Home, cx)
                }
            }
            Act::QuickShot => self.quick_shot(window, cx),
            Act::QuickClip => self.quick_clip(window, cx),
            _ => {}
        }
        cx.notify();
    }

    // ---- Its settings ----

    pub(super) fn quick_opt_value(&self, o: &Opt) -> Option<Value> {
        let q = &self.console.quick;
        Some(match o {
            Opt::HudShown => Value::Switch(q.shown),
            Opt::FpsLimit => Value::Choice(self.fps_name(q.fps)),
            _ => return None,
        })
    }

    pub(super) fn quick_opt_key(&self, o: &Opt) -> Option<String> {
        (*o == Opt::FpsLimit).then(|| self.console.quick.fps.to_string())
    }

    pub(super) fn quick_opt_choices(&self, o: &Opt) -> Option<Vec<(String, String)>> {
        if *o != Opt::FpsLimit {
            return None;
        }
        let mut list = FPS.to_vec();
        if !list.contains(&self.console.quick.fps) {
            list.push(self.console.quick.fps);
            list.sort();
        }
        Some(
            list.into_iter()
                .map(|f| (self.fps_name(f), f.to_string()))
                .collect(),
        )
    }

    fn fps_name(&self, fps: u32) -> String {
        match fps {
            0 => self.tr("No limit", "Sınırsız").to_string(),
            n => format!("{n} FPS"),
        }
    }

    pub(super) fn quick_opt_set(&mut self, o: &Opt, key: &str) -> bool {
        if *o != Opt::FpsLimit {
            return false;
        }
        let q = &mut self.console.quick;
        if let (Some(path), Ok(fps)) = (&q.hud, key.parse()) {
            match quick::set_fps_limit(path, fps) {
                Ok(()) => q.fps = fps,
                Err(e) => self.status = Some(format!("{e:#}").into()),
            }
        }
        true
    }

    pub(super) fn quick_opt_toggle(&mut self, o: &Opt) -> bool {
        if *o != Opt::HudShown {
            return false;
        }
        let q = &mut self.console.quick;
        if let Some(path) = &q.hud {
            match quick::set_hud_shown(path, !q.shown) {
                Ok(()) => q.shown = !q.shown,
                Err(e) => self.status = Some(format!("{e:#}").into()),
            }
        }
        true
    }

    // ---- The screen ----

    pub(super) fn quick_rows(&self, entry: &str) -> Vec<Row> {
        let Some(g) = self.library.get(entry) else {
            return Vec::new();
        };
        let gid = g.game_id().to_string();
        let line = |act: Act, icon: &'static str, label: &str, value: Value| {
            Row::new(Kind::Line, vec![Item::line(act, icon, label, value)])
        };
        let opt = |o: Opt, icon: &'static str, label: &str| {
            let value = self.opt_value(&o);
            line(Act::Opt(o), icon, label, value)
        };
        // A list down the panel at the side, as the game stays in view.
        let mut rows = vec![
            line(
                Act::QuickBack,
                "play",
                self.tr("Back to the game", "Oyuna dön"),
                Value::None,
            ),
            line(
                Act::QuickHome,
                "house",
                self.tr("Go to Arkor", "Arkor'a geç"),
                Value::None,
            ),
            line(
                Act::Stop(gid),
                "square",
                self.tr("Close the game", "Oyunu kapat"),
                Value::None,
            ),
        ];
        let mut volume = opt(Opt::Volume, "volume-2", self.tr("Volume", "Ses düzeyi"));
        volume.title = Some(self.tr("Quick settings", "Hızlı ayarlar").into());
        rows.push(volume);
        rows.push(opt(Opt::Muted, "volume-x", self.tr("Mute", "Sessiz")));
        let q = &self.console.quick;
        if q.hud.is_some() {
            let mut hud = opt(
                Opt::HudShown,
                "gauge",
                self.tr("Performance overlay", "Performans göstergesi"),
            );
            hud.title = Some(self.tr("This game, now", "Bu oyun, şimdi").into());
            rows.push(hud);
            if !q.gamescope {
                rows.push(opt(
                    Opt::FpsLimit,
                    "timer",
                    self.tr("Frame limit", "Kare sınırı"),
                ));
            }
        }
        if self.sys_has_profiles() {
            rows.push(opt(
                Opt::PowerProfile,
                "zap",
                self.tr("Power profile", "Güç profili"),
            ));
        }
        if arkor_launch::clips::recording() {
            rows.push(line(
                Act::QuickClip,
                "clapperboard",
                self.tr("Save a clip", "Klip kaydet"),
                Value::Text(format!(
                    "{} {}",
                    self.global.clips.seconds(),
                    self.tr("s", "sn")
                )),
            ));
        }
        if quick::can_screenshot() {
            rows.push(line(
                Act::QuickShot,
                "camera",
                self.tr("Take a screenshot", "Ekran görüntüsü al"),
                Value::None,
            ));
        }
        rows
    }
}
