//! Library grid: cover tiles of the (filtered) library, shown when no page is selected.

use gpui::{Context, Div, ObjectFit, StyledImage, div, img, prelude::*, px, rgb};

use super::widgets::{self, theme::*};
use super::{Library, Selection};

impl Library {
    /// A cover image, or a placeholder tile with the game's initial.
    pub(super) fn cover_tile(&self, id: &str, name: &str, w: f32, h: f32) -> gpui::AnyElement {
        match self.covers.get(id).cloned().flatten() {
            Some(path) => img(path)
                .w(px(w))
                .h(px(h))
                .rounded_md()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div()
                .w(px(w))
                .h(px(h))
                .rounded_md()
                .bg(rgb(SELECTED()))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(w * 0.35))
                .text_color(rgb(ACCENT()))
                .child(
                    name.chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default(),
                )
                .into_any_element(),
        }
    }

    pub(super) fn render_grid(&self, cx: &mut Context<Self>) -> Div {
        let cover_w = self.global.appearance.grid_size.cover_width();
        let cover_h = cover_w * 1.5;
        let mut tiles = div().flex().flex_wrap().gap_4();
        for &ix in &self.visible {
            let game = &self.games[ix];
            let played = self.playtime(&game.id).0;
            let running = self.running.contains_key(&game.id.to_string());
            tiles = tiles.child(
                div()
                    .id(("tile", ix))
                    .w(px(cover_w))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .cursor_pointer()
                    .rounded_md()
                    .hover(|s| s.opacity(0.85))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.select(Selection::Game(ix), cx)),
                    )
                    .child(self.cover_tile(&game.id.id, &game.name, cover_w, cover_h))
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .text_color(rgb(TEXT()))
                            .child(game.name.clone()),
                    )
                    .child(widgets::label(if running {
                        format!("▶ {}", self.tr("Running", "Çalışıyor"))
                    } else if played > 0 {
                        arkor_core::stats::format_duration(played)
                    } else {
                        self.tr("Not played yet", "Henüz oynanmadı").to_string()
                    })),
            );
        }
        div()
            .child(Self::header(
                self.tr("Library", "Kütüphane"),
                self.tr(
                    "Pick a game, or use the list on the left.",
                    "Bir oyun seç ya da soldaki listeyi kullan.",
                ),
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .mb_3()
                    .child(self.grid_size_chips(cx)),
            )
            .child(tiles)
    }
}
