//! Library page: search, sorting and filters on top, cover tiles of the matching games below.

use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, FontWeight, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, Sort, cover_art, widgets};

impl Library {
    /// A cover image, or a placeholder tile with the game's initial.
    pub(super) fn cover_tile(&self, id: &str, name: &str, w: f32, h: f32) -> gpui::AnyElement {
        let radius = px((w * 0.06).clamp(4., 12.));
        match self.covers.get(id).cloned().flatten() {
            // Tiny covers (the sidebar) stay still.
            Some(path) => cover_art::cover(path, f32::from(radius), w >= 80. && self.moving())
                .w(px(w))
                .h(px(h))
                .flex_none()
                .into_any_element(),
            None => div()
                .w(px(w))
                .h(px(h))
                .flex_none()
                .rounded(radius)
                .bg(style::accent_gradient())
                .opacity(0.85)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(w * 0.38))
                .font_weight(FontWeight::BOLD)
                .text_color(pal::on_accent())
                .child(
                    name.chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default(),
                )
                .into_any_element(),
        }
    }

    /// A cover that fills its tile's width (2:3), animated covers playing.
    fn cover_fill(&self, id: &str, name: &str, h: f32) -> gpui::AnyElement {
        match self.covers.get(id).cloned().flatten() {
            Some(path) => {
                let radius = 12.;
                // While the start-up animation carries this cover, its place stays empty.
                let shown = !self.intro_carries(id);
                let slot = self.intro_slot(id);
                let animate = self.moving();
                gpui::canvas(
                    move |bounds, window, _| {
                        if let Some(slot) = slot {
                            slot(bounds, window.content_mask().bounds);
                        }
                    },
                    move |bounds, _, window, _| {
                        if shown {
                            let animate = animate && window.is_window_active();
                            cover_art::paint(&path, bounds, radius, animate, false, window);
                        }
                    },
                )
                .w(px(h / 1.5))
                .h(px(h))
                .into_any_element()
            }
            None => div()
                .w(px(h / 1.5))
                .h(px(h))
                .rounded(px(12.))
                .bg(style::accent_gradient())
                .opacity(0.85)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(56.))
                .font_weight(FontWeight::BOLD)
                .text_color(pal::on_accent())
                .child(
                    name.chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default(),
                )
                .into_any_element(),
        }
    }

    /// Search, sort, size, categories and hidden games.
    fn library_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let search = self.search_input(cx);
        let mut sorts = style::card().p_1().flex().gap_1();
        for (ix, (sort, icon, label)) in [
            (Sort::Name, "arrow-down-a-z", self.tr("Name", "Ad")),
            (
                Sort::Recent,
                "calendar-clock",
                self.tr("Recent", "Son oynanan"),
            ),
            (
                Sort::Playtime,
                "clock",
                self.tr("Most played", "En çok oynanan"),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let active = self.filter.sort == sort;
            sorts = sorts.child(
                div()
                    .id(("sort", ix))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .px_3()
                    .py_1p5()
                    .rounded(px(9.))
                    .text_sm()
                    .cursor_pointer()
                    .when(active, |s| {
                        s.bg(pal::text().opacity(0.07))
                            .text_color(pal::accent_light())
                    })
                    .when(!active, |s| {
                        s.text_color(pal::muted())
                            .hover(|h| h.text_color(pal::text()))
                    })
                    .child(style::icon(
                        icon,
                        14.,
                        if active {
                            pal::accent_light()
                        } else {
                            pal::muted()
                        },
                    ))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.filter.sort = sort;
                        this.apply_filter();
                        cx.notify();
                    })),
            );
        }
        let mut top = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .child(div().w(px(280.)).child(search))
            .child(sorts)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(style::icon("layout-grid", 14., pal::faint()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(self.tr("Cover size", "Kapak boyutu")),
                    )
                    .child(self.grid_size_chips(cx)),
            );
        if self.filter.show_hidden || self.library.games.iter().any(|g| g.hidden) {
            top = top.child(
                widgets::chip(
                    "show-hidden",
                    self.tr("Hidden games", "Gizli oyunlar"),
                    self.filter.show_hidden,
                )
                .child(style::icon(
                    if self.filter.show_hidden {
                        "eye"
                    } else {
                        "eye-off"
                    },
                    13.,
                    pal::muted(),
                ))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_hidden(cx))),
            );
        }
        let mut bar = div().flex().flex_col().gap_3().child(top);
        let categories = self.categories();
        if !categories.is_empty() {
            let mut row = div().flex().flex_wrap().gap_1p5().child(
                widgets::chip(
                    "cat-all",
                    self.tr("All", "Tümü"),
                    self.filter.category.is_none(),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.filter.category = None;
                    this.apply_filter();
                    cx.notify();
                })),
            );
            for (ix, c) in categories.into_iter().enumerate() {
                let active = self.filter.category.as_ref() == Some(&c);
                row = row.child(widgets::chip(("cat", ix), c.clone(), active).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.filter.category = if this.filter.category.as_ref() == Some(&c) {
                            None
                        } else {
                            Some(c.clone())
                        };
                        this.apply_filter();
                        cx.notify();
                    }),
                ));
            }
            bar = bar.child(row);
        }
        bar
    }

    fn render_empty_library(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .py(px(80.))
            .child(div().size(px(64.)).rounded(px(18.)).bg(pal::raised()).flex().items_center().justify_center().child(style::icon("gamepad-2", 30., pal::accent_light())))
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(self.tr("Your library is empty", "Kütüphanen boş")))
            .child(div().text_sm().text_color(pal::muted()).child(self.tr(
                "Add a game by hand, or pick one Arkor found in Steam, Heroic, Lutris and other launchers.",
                "Bir oyunu elle ekle ya da Arkor'un Steam, Heroic, Lutris ve diğer başlatıcılarda bulduklarından birini seç.",
            )))
            .child(
                widgets::primary_button("empty-add", self.tr("Add game", "Oyun ekle"))
                    .on_click(cx.listener(|this, _, _, cx| this.go(super::shell::Nav::Add, cx))),
            )
    }

    pub(super) fn render_grid(&mut self, cx: &mut Context<Self>) -> Div {
        let title = div()
            .flex()
            .items_end()
            .justify_between()
            .child(Self::header(
                self.tr("Library", "Kütüphane"),
                match self.games.len() {
                    0 => self.tr("No games yet", "Henüz oyun yok").to_string(),
                    n if self.visible.len() == n => format!("{n} {}", self.tr("games", "oyun")),
                    n => format!("{} / {n} {}", self.visible.len(), self.tr("games", "oyun")),
                },
            ))
            .child(
                style::button("grid-add", self.tr("Add game", "Oyun ekle"), true)
                    .child(style::icon("plus", 14., pal::on_accent()))
                    .on_click(cx.listener(|this, _, _, cx| this.go(super::shell::Nav::Add, cx))),
            );
        if self.games.is_empty() {
            return div()
                .flex()
                .flex_col()
                .gap_5()
                .child(title)
                .child(self.render_empty_library(cx));
        }
        let toolbar = self
            .library_toolbar(cx)
            .child(self.library_shelves(cx))
            .child(self.render_views_bar(cx));
        let bar = self.picked_bar(cx);
        let offer = self.render_copies_offer(cx);
        let undo = self.undo_bar(cx);
        let pin = self.pin_card(cx);
        if self.global.library_list {
            let list = if self.visible.is_empty() {
                div().py_8().text_color(pal::muted()).child(self.tr(
                    "No game matches the search or filter.",
                    "Aramaya ya da filtreye uyan oyun yok.",
                ))
            } else {
                self.render_library_list(cx)
            };
            return div()
                .flex()
                .flex_col()
                .gap_5()
                .child(title)
                .children(undo)
                .children(pin)
                .children(offer)
                .child(toolbar)
                .children(bar)
                .child(list);
        }
        // As many columns of about the chosen cover size as fit (covers get a little bigger
        // on large windows); the covers then grow to fill the row, so nothing is left empty.
        let scale = (self.viewport_w / 1320.).clamp(1., 1.45);
        let preferred = self.global.appearance.grid_size.cover_width() * scale;
        // Around the grid: the sidebar and the panel's frame, and six rems of padding.
        let around = 242. + 6. * self.global.appearance.font_size() as f32;
        let available = (self.viewport_w - around).max(preferred);
        let gap = 20.;
        let cols = (((available + gap) / (preferred + gap)).floor() as u16).max(2);
        let tile_w = ((available - gap * f32::from(cols - 1)) / f32::from(cols)).floor();
        let cover_w = tile_w - 6.;
        let cover_h = cover_w * 1.5;
        let mut sections = div().flex().flex_col().gap_6();
        for (gix, (gtitle, gixs)) in self.library_groups().into_iter().enumerate() {
            let mut tiles = div().flex().flex_wrap().gap(px(gap));
            for &ix in &gixs {
                let game = &self.games[ix];
                let played = self.playtime(&game.id).0;
                let running = self.running.contains_key(&game.id.to_string());
                let source = self
                    .library
                    .get(&game.id.id)
                    .map(|g| self.kind_label(g))
                    .unwrap_or_default();
                let hidden = self.library.get(&game.id.id).is_some_and(|g| g.hidden);
                let picked = self.is_picked(ix);
                tiles = tiles.child(
                    div()
                        .id(("tile", ix))
                        .w(px(tile_w))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .cursor_pointer()
                        .group("tile")
                        .when(hidden, |d| d.opacity(0.55))
                        .on_click(cx.listener(move |this, e, _, cx| this.library_click(ix, e, cx)))
                        .child(
                            div()
                                .relative()
                                .rounded(px(14.))
                                .p(px(2.))
                                .border_1()
                                .border_color(if picked {
                                    pal::accent()
                                } else if running {
                                    pal::success().opacity(0.7)
                                } else {
                                    gpui::transparent_black()
                                })
                                .group_hover("tile", |s| s.border_color(pal::accent().opacity(0.7)))
                                .child(self.cover_fill(&game.id.id, &game.name, cover_h))
                                .when(running, |d| {
                                    d.child(
                                        div().absolute().top_2().left_2().child(
                                            style::tag(
                                                self.tr("Running", "Çalışıyor"),
                                                pal::success(),
                                            )
                                            .bg(pal::bg().opacity(0.85)),
                                        ),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .px_1()
                                .truncate()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(pal::text())
                                .child(game.name.clone()),
                        )
                        .child(
                            div()
                                .px_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .text_xs()
                                .text_color(pal::muted())
                                .child(source)
                                .child("·")
                                .child(if played > 0 {
                                    arkor_core::stats::format_duration(played)
                                } else {
                                    self.tr("Not played yet", "Henüz oynanmadı").to_string()
                                }),
                        ),
                );
            }
            let header = gtitle.map(|t| self.group_header(gix, &t, gixs.len(), cx));
            sections = sections.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(header)
                    .child(tiles),
            );
        }
        if self.visible.is_empty() {
            sections = sections.child(div().py_8().text_color(pal::muted()).child(self.tr(
                "No game matches the search or filter.",
                "Aramaya ya da filtreye uyan oyun yok.",
            )));
        }
        div()
            .flex()
            .flex_col()
            .gap_5()
            .child(title)
            .children(undo)
            .children(pin)
            .children(offer)
            .child(toolbar)
            .children(bar)
            .child(sections)
    }
}
