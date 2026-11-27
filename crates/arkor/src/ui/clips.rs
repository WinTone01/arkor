//! Clips of games (see `arkor_launch::clips`): their settings, on the desktop's settings page
//! and console mode's Games settings, and saving one from the quick menu.

use std::time::Duration;

use arkor_launch::clips;
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, Window, div, prelude::*};

use super::console::{Act, Item, Kind, Opt, Row, Value};
use super::style::{self, pal};
use super::{Library, widgets};

/// How long a clip can be, in seconds.
const LENGTHS: [u32; 5] = [15, 30, 60, 120, 300];

impl Library {
    fn clip_length_name(&self, s: u32) -> String {
        if s < 60 {
            format!("{s} {}", self.tr("s", "sn"))
        } else {
            format!("{} {}", s / 60, self.tr("min", "dk"))
        }
    }

    fn clips_text(&self) -> &'static str {
        self.tr(
            "While a game runs, its last moments are kept in memory (GPU Screen Recorder, on the graphics card). Save them from the quick menu (the gamepad's middle button) or with `arkor clip` on a keyboard shortcut; clips go to Videos/Arkor.",
            "Oyun açıkken son anları bellekte tutulur (GPU Screen Recorder, ekran kartıyla). Hızlı menüden (oyun kolunun orta tuşu) ya da bir klavye kısayoluna atanmış `arkor clip` ile kaydet; klipler Videolar/Arkor klasörüne gider.",
        )
    }

    // ---- Console mode ----

    /// The clip lines of console mode's Games settings.
    pub(super) fn clip_rows(&self) -> Vec<Row> {
        let line = |act: Act, icon, label: &str, value: Value| {
            Row::new(Kind::Line, vec![Item::line(act, icon, label, value)])
        };
        if !clips::available() {
            return vec![line(
                Act::InstallTools(vec!["gpu-screen-recorder".into()]),
                "clapperboard",
                self.tr("Clips of games", "Oyun klipleri"),
                Value::Text(
                    self.tr(
                        "needs GPU Screen Recorder: install",
                        "GPU Screen Recorder gerekli: kur",
                    )
                    .into(),
                ),
            )];
        }
        let mut on = line(
            Act::Opt(Opt::Clips),
            "clapperboard",
            self.tr("Clips of games", "Oyun klipleri"),
            self.opt_value(&Opt::Clips),
        );
        on.items[0].sub = self
            .tr(
                "Save the last moments from the quick menu.",
                "Son anları hızlı menüden kaydet.",
            )
            .into();
        let mut rows = vec![on];
        if self.global.clips.enabled {
            rows.push(line(
                Act::Opt(Opt::ClipLength),
                "timer",
                self.tr("Clip length", "Klip uzunluğu"),
                self.opt_value(&Opt::ClipLength),
            ));
            rows.push(line(
                Act::Opt(Opt::ClipSound),
                "volume-2",
                self.tr("With the game's sound", "Oyunun sesiyle"),
                self.opt_value(&Opt::ClipSound),
            ));
        }
        rows
    }

    pub(super) fn clips_opt_value(&self, o: &Opt) -> Option<Value> {
        let c = &self.global.clips;
        Some(match o {
            Opt::Clips => Value::Switch(c.enabled),
            Opt::ClipSound => Value::Switch(!c.no_sound),
            Opt::ClipLength => Value::Choice(self.clip_length_name(c.seconds())),
            _ => return None,
        })
    }

    pub(super) fn clips_opt_key(&self, o: &Opt) -> Option<String> {
        (*o == Opt::ClipLength).then(|| self.global.clips.seconds().to_string())
    }

    pub(super) fn clips_opt_choices(&self, o: &Opt) -> Option<Vec<(String, String)>> {
        if *o != Opt::ClipLength {
            return None;
        }
        let mut list = LENGTHS.to_vec();
        let now = self.global.clips.seconds();
        if !list.contains(&now) {
            list.push(now);
            list.sort();
        }
        Some(
            list.into_iter()
                .map(|s| (self.clip_length_name(s), s.to_string()))
                .collect(),
        )
    }

    pub(super) fn clips_opt_set(&mut self, o: &Opt, key: &str) -> bool {
        if *o != Opt::ClipLength {
            return false;
        }
        if let Ok(s) = key.parse() {
            self.global.clips.seconds = Some(s);
            let r = self.global.save();
            self.report(r);
        }
        true
    }

    /// Switches a clip setting; `false` when it is none.
    pub(super) fn clips_opt_toggle(&mut self, o: &Opt) -> bool {
        let c = &mut self.global.clips;
        match o {
            Opt::Clips => c.enabled = !c.enabled,
            Opt::ClipSound => c.no_sound = !c.no_sound,
            _ => return false,
        }
        let r = self.global.save();
        self.report(r);
        true
    }

    /// Saves a clip of the game: back to it first (the menu is not in the clip's end).
    pub(super) fn quick_clip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quick_back(window, cx);
        if cfg!(test) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            bg.timer(Duration::from_millis(600)).await;
            let result = bg.spawn(async move { clips::save() }).await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok(path) => format!(
                        "{} {}",
                        this.tr("Clip saved:", "Klip kaydedildi:"),
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

    // ---- The desktop ----

    /// The Clips section of the settings page.
    pub(super) fn render_clips(&self, cx: &mut Context<Self>) -> Div {
        let mut card = widgets::section(self.tr("Clips", "Klipler"));
        if !clips::available() {
            return card.child(widgets::setting_row(
                self.tr("Clips of games", "Oyun klipleri"),
                self.tr(
                    "Needs GPU Screen Recorder (the gpu-screen-recorder package).",
                    "GPU Screen Recorder gerekli (gpu-screen-recorder paketi).",
                ),
                div(),
            ));
        }
        let c = self.global.clips.clone();
        card = card.child(widgets::setting_row(
            self.tr("Clips of games", "Oyun klipleri"),
            self.clips_text(),
            widgets::switch(c.enabled)
                .id("clips")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clips_opt_toggle(&Opt::Clips);
                    cx.notify();
                })),
        ));
        if !c.enabled {
            return card;
        }
        let mut lengths = div().flex().gap_1();
        for (ix, s) in LENGTHS.into_iter().enumerate() {
            lengths = lengths.child(
                widgets::chip(
                    ("clip-length", ix),
                    self.clip_length_name(s),
                    c.seconds() == s,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.clips_opt_set(&Opt::ClipLength, &s.to_string());
                    cx.notify();
                })),
            );
        }
        card.child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("Clip length", "Klip uzunluğu"),
                self.tr(
                    "Longer clips keep more in memory.",
                    "Uzun klipler bellekte daha çok yer tutar.",
                ),
                lengths,
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("With the game's sound", "Oyunun sesiyle"),
                "",
                widgets::switch(!c.no_sound)
                    .id("clip-sound")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clips_opt_toggle(&Opt::ClipSound);
                        cx.notify();
                    })),
            ))
            .when_some(
                clips::folder("").and_then(|f| f.parent().map(|p| p.to_path_buf())),
                |card, dir| {
                    card.child(widgets::divider()).child(widgets::setting_row(
                        self.tr("Clips folder", "Klip klasörü"),
                        dir.display().to_string(),
                        style::button("clips-folder", self.tr("Open", "Aç"), false)
                            .child(style::icon("folder-open", 13., pal::muted()))
                            .on_click(move |_, _, _| {
                                let _ = std::fs::create_dir_all(&dir);
                                let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
                            }),
                    ))
                },
            )
    }
}
