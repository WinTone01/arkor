//! "Now playing" on the desktop: while a game runs, its page has what console mode's quick
//! menu has (the performance overlay shown or not, a frame limit, the sound, a note for the
//! journal) and the gamepad's middle button brings that menu over the game.

use gpui::{Context, Div, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};
use arkor_launch::quick;

const LIMITS: [u32; 6] = [0, 30, 60, 90, 120, 144];

impl Library {
    /// The card over the running game's page.
    pub(super) fn render_now_playing(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let entry = self.entry()?.clone();
        let gid = entry.game_id();
        if !self.running.contains_key(&gid.to_string()) {
            return None;
        }
        let hud = quick::hud_file(&gid);
        let mut card = style::card()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(style::icon("radio", 16., pal::accent_light()))
                    .child(div().font_weight(gpui::FontWeight::MEDIUM).child(self.tr(
                        "Now playing: changes apply at once",
                        "Şimdi oynanıyor: değişiklikler hemen uygulanır",
                    ))),
            )
            .child(widgets::label(self.tr(
                "The gamepad's middle button brings these over the game too.",
                "Kolun orta tuşu bunları oyunun üstüne de getirir.",
            )));
        if let Some(path) = hud {
            let shown = quick::hud_shown(&path);
            let fps = quick::fps_limit(&path);
            let p1 = path.clone();
            let mut limits = div().flex().flex_wrap().gap_1();
            for (ix, l) in LIMITS.into_iter().enumerate() {
                let p = path.clone();
                let label = if l == 0 {
                    self.tr("No limit", "Sınırsız").to_string()
                } else {
                    format!("{l}")
                };
                limits = limits.child(widgets::chip(("np-fps", ix), label, fps == l).on_click(
                    cx.listener(move |this, _, _, cx| {
                        if let Err(e) = quick::set_fps_limit(&p, l) {
                            this.status = Some(format!("{e:#}").into());
                        }
                        cx.notify();
                    }),
                ));
            }
            card = card
                .child(widgets::setting_row(
                    self.tr("Performance overlay", "Performans göstergesi"),
                    "",
                    widgets::switch(shown)
                        .id("np-hud")
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Err(e) = quick::set_hud_shown(&p1, !shown) {
                                this.status = Some(format!("{e:#}").into());
                            }
                            cx.notify();
                        })),
                ))
                .child(widgets::setting_row(
                    self.tr("Frame limit", "Kare sınırı"),
                    self.tr(
                        "Fewer frames: a cooler, quieter computer and steadier motion.",
                        "Daha az kare: daha serin, daha sessiz bilgisayar ve daha düzenli hareket.",
                    ),
                    limits,
                ));
        }
        let id = entry.id.clone();
        let note = self.submit_field(
            "np-note",
            self.tr(
                "A note for today's journal, then Enter",
                "Bugünün günlüğüne bir not, sonra Enter",
            ),
            cx,
            move |this, text, cx| {
                let r = arkor_core::stats::add_note(&id, text);
                this.report(r);
                cx.notify();
            },
        );
        Some(card.child(widgets::setting_row(
            self.tr("Note", "Not"),
            "",
            div().w(px(360.)).child(note),
        )))
    }
}
