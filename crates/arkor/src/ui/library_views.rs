//! The library's other ways of showing games: a list instead of covers, ready-made shelves
//! (favourites, never played, played lately, a store or a kind of game), and several games
//! picked at once (Ctrl+click, Shift+click, Ctrl+A) for actions on all of them.

use arkor_core::library::{GameKind, LibraryGame};
use gpui::prelude::FluentBuilder;
use gpui::{ClickEvent, Context, Div, FontWeight, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, Selection, widgets};

/// A ready-made shelf of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Smart {
    Favorites,
    NeverPlayed,
    /// Played in the last two weeks.
    Recent,
    /// Played ten hours or more.
    LongPlayed,
    Steam,
    Epic,
    Gog,
    Amazon,
    /// Windows programs and Linux programs added by hand or from other launchers.
    Programs,
    Emulated,
}

impl Smart {
    const ALL: [Smart; 10] = [
        Smart::Favorites,
        Smart::Recent,
        Smart::NeverPlayed,
        Smart::LongPlayed,
        Smart::Steam,
        Smart::Epic,
        Smart::Gog,
        Smart::Amazon,
        Smart::Programs,
        Smart::Emulated,
    ];
}

const DAY: u64 = 24 * 3600;

impl Library {
    fn smart_name(&self, s: Smart) -> &'static str {
        match s {
            Smart::Favorites => self.tr("Favourites", "Favoriler"),
            Smart::NeverPlayed => self.tr("Never played", "Hiç oynanmamış"),
            Smart::Recent => self.tr("Played lately", "Son oynananlar"),
            Smart::LongPlayed => self.tr("10+ hours", "10+ saat"),
            Smart::Steam => "Steam",
            Smart::Epic => "Epic",
            Smart::Gog => "GOG",
            Smart::Amazon => "Amazon",
            Smart::Programs => self.tr("Windows & Linux", "Windows ve Linux"),
            Smart::Emulated => self.tr("Emulators", "Emülatörler"),
        }
    }

    /// Whether `game` belongs on shelf `s`.
    pub(super) fn smart_matches(&self, s: Smart, game: &LibraryGame) -> bool {
        let (played, last) = self.playtime(&game.game_id());
        let from_gog = game.origin.as_deref().is_some_and(|o| o.starts_with("gog"));
        match s {
            Smart::Favorites => game.favorite,
            Smart::NeverPlayed => played == 0,
            Smart::Recent => {
                last.is_some_and(|t| arkor_core::stats::now().saturating_sub(t) < 14 * DAY)
            }
            Smart::LongPlayed => played >= 10 * 3600,
            Smart::Steam => matches!(game.kind, GameKind::Steam { .. }),
            Smart::Epic => matches!(game.kind, GameKind::Epic { .. }),
            Smart::Amazon => matches!(game.kind, GameKind::Amazon { .. }),
            Smart::Gog => from_gog,
            Smart::Programs => {
                matches!(game.kind, GameKind::Windows | GameKind::Linux) && !from_gog
            }
            Smart::Emulated => matches!(game.kind, GameKind::Emulated { .. }),
        }
    }

    /// The shelves that hold at least one game.
    fn smart_shelves(&self) -> Vec<Smart> {
        Smart::ALL
            .into_iter()
            .filter(|s| self.library.games.iter().any(|g| self.smart_matches(*s, g)))
            .collect()
    }

    /// The shelves row, and the switch between covers and a list.
    pub(super) fn library_shelves(&mut self, cx: &mut Context<Self>) -> Div {
        let mut row = div().flex().flex_wrap().items_center().gap_1p5();
        for (ix, s) in self.smart_shelves().into_iter().enumerate() {
            let on = self.filter.smart == Some(s);
            row = row.child(
                widgets::chip(("smart", ix), self.smart_name(s), on).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.filter.smart = if this.filter.smart == Some(s) {
                            None
                        } else {
                            Some(s)
                        };
                        this.apply_filter();
                        cx.notify();
                    },
                )),
            );
        }
        // Genres and ways of playing, and where the user is with games.
        let genres = self.library_genres();
        if !genres.is_empty() || self.filter.genre.is_some() {
            let current = self.filter.genre.clone();
            let mut names = genres;
            if let Some(c) = &current
                && !names.contains(c)
            {
                names.insert(0, c.clone());
            }
            for (ix, g) in names.into_iter().take(12).enumerate() {
                let on = current.as_ref() == Some(&g);
                row = row.child(
                    widgets::chip(("genre", ix), g.clone(), on).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.filter.genre = (!on).then(|| g.clone());
                            this.apply_filter();
                            cx.notify();
                        },
                    )),
                );
            }
        }
        for (ix, s) in arkor_core::library::PlayStatus::ALL.into_iter().enumerate() {
            if !self.library.games.iter().any(|g| g.status == Some(s)) {
                continue;
            }
            let on = self.filter.status == Some(s);
            row = row.child(
                widgets::chip(("pstatus", ix), self.status_name(s), on).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.filter.status = (!on).then_some(s);
                        this.apply_filter();
                        cx.notify();
                    },
                )),
            );
        }
        let list = self.global.library_list;
        let view = |id: &'static str, icon: &'static str, on: bool| {
            div()
                .id(id)
                .p_1p5()
                .rounded(px(8.))
                .cursor_pointer()
                .when(on, |d| d.bg(pal::text().opacity(0.08)))
                .child(style::icon(
                    icon,
                    15.,
                    if on {
                        pal::accent_light()
                    } else {
                        pal::muted()
                    },
                ))
        };
        row.child(div().flex_1()).child(
            style::card()
                .p_1()
                .flex()
                .gap_1()
                .child(
                    view("view-grid", "layout-grid", !list)
                        .on_click(cx.listener(|this, _, _, cx| this.set_library_list(false, cx))),
                )
                .child(
                    view("view-list", "list", list)
                        .on_click(cx.listener(|this, _, _, cx| this.set_library_list(true, cx))),
                ),
        )
    }

    fn set_library_list(&mut self, on: bool, cx: &mut Context<Self>) {
        self.global.library_list = on;
        let r = self.global.save();
        self.report(r);
        cx.notify();
    }

    /// A click on a game in the library: Ctrl picks it (or not), Shift picks the run from
    /// the last picked one, a plain click opens it.
    pub(super) fn library_click(&mut self, ix: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        let m = event.modifiers();
        let Some(id) = self.games.get(ix).map(|g| g.id.id.clone()) else {
            return;
        };
        if m.control || m.platform {
            match self.picked.iter().position(|p| *p == id) {
                Some(at) => {
                    self.picked.remove(at);
                }
                None => self.picked.push(id),
            }
        } else if m.shift {
            let order: Vec<String> = self
                .visible
                .iter()
                .filter_map(|&v| self.games.get(v).map(|g| g.id.id.clone()))
                .collect();
            let to = order.iter().position(|g| *g == id);
            let from = self
                .picked
                .last()
                .and_then(|last| order.iter().position(|g| g == last))
                .or(to);
            if let (Some(a), Some(b)) = (from, to) {
                for g in &order[a.min(b)..=a.max(b)] {
                    if !self.picked.contains(g) {
                        self.picked.push(g.clone());
                    }
                }
            }
        } else if !self.picked.is_empty() {
            self.picked.clear();
        } else {
            self.select(Selection::Game(ix), cx);
            return;
        }
        cx.notify();
    }

    /// Ctrl+A in the library: every game shown is picked (again: none).
    pub(super) fn library_pick_all(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.selection, None | Some(Selection::Grid)) {
            return;
        }
        let all: Vec<String> = self
            .visible
            .iter()
            .filter_map(|&v| self.games.get(v).map(|g| g.id.id.clone()))
            .collect();
        self.picked = if self.picked.len() == all.len() {
            Vec::new()
        } else {
            all
        };
        cx.notify();
    }

    pub(super) fn is_picked(&self, ix: usize) -> bool {
        self.games
            .get(ix)
            .is_some_and(|g| self.picked.contains(&g.id.id))
    }

    /// Does `change` to every picked game and saves the library.
    pub(super) fn change_picked(
        &mut self,
        change: impl Fn(&mut LibraryGame),
        cx: &mut Context<Self>,
    ) {
        for id in self.picked.clone() {
            if let Some(g) = self.library.get_mut(&id) {
                change(g);
            }
        }
        self.save_library();
        self.apply_filter();
        cx.notify();
    }

    /// The bar of actions for the picked games.
    pub(super) fn picked_bar(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        if self.picked.is_empty() {
            return None;
        }
        let n = self.picked.len();
        let all_fav = self
            .picked
            .iter()
            .all(|id| self.library.get(id).is_some_and(|g| g.favorite));
        let all_hidden = self
            .picked
            .iter()
            .all(|id| self.library.get(id).is_some_and(|g| g.hidden));
        let mut cats = div().flex().flex_wrap().gap_1();
        for (ix, c) in std::iter::once(None)
            .chain(self.categories().into_iter().map(Some))
            .enumerate()
        {
            let label = c
                .clone()
                .unwrap_or_else(|| self.tr("No category", "Kategorisiz").into());
            cats = cats.child(
                widgets::chip(("pick-cat", ix), label, false).on_click(cx.listener(
                    move |this, _, _, cx| {
                        let c = c.clone();
                        this.change_picked(move |g| g.category.clone_from(&c), cx)
                    },
                )),
            );
        }
        let removing = self.game_ui.confirm_remove_picked;
        Some(
            style::card()
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .border_1()
                .border_color(pal::accent().opacity(0.5))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{n} {}", self.tr("games picked", "oyun seçildi"))),
                        )
                        .child(div().flex_1())
                        .child(
                            style::button(
                                "pick-fav",
                                if all_fav {
                                    self.tr("Not favourites", "Favorilerden çıkar")
                                } else {
                                    self.tr("Favourites", "Favorilere ekle")
                                },
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.change_picked(move |g| g.favorite = !all_fav, cx)
                                },
                            )),
                        )
                        .child(
                            style::button(
                                "pick-hide",
                                if all_hidden {
                                    self.tr("Show", "Göster")
                                } else {
                                    self.tr("Hide", "Gizle")
                                },
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.change_picked(move |g| g.hidden = !all_hidden, cx)
                                },
                            )),
                        )
                        .child(
                            style::button(
                                "pick-remove",
                                if removing {
                                    self.tr(
                                        "Remove them? Press again",
                                        "Kaldırılsın mı? Tekrar bas",
                                    )
                                } else {
                                    self.tr("Remove from library", "Kütüphaneden kaldır")
                                },
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.remove_picked(cx))),
                        )
                        .child(
                            style::button("pick-none", self.tr("Done", "Bitti"), true).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.picked.clear();
                                    this.game_ui.confirm_remove_picked = false;
                                    cx.notify();
                                }),
                            ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(pal::muted())
                                .child(self.tr("Category:", "Kategori:")),
                        )
                        .child(cats),
                ),
        )
    }

    /// Removes the picked games from the library (their files stay), after a second press.
    pub(super) fn remove_picked(&mut self, cx: &mut Context<Self>) {
        if !self.game_ui.confirm_remove_picked {
            self.game_ui.confirm_remove_picked = true;
            cx.notify();
            return;
        }
        self.game_ui.confirm_remove_picked = false;
        for id in std::mem::take(&mut self.picked) {
            self.emu_note_removed(&id);
            self.library.remove(&id);
        }
        let r = self.library.save();
        self.report(r);
        self.reload_games();
        cx.notify();
    }

    /// The library as a list: cover, name, kind, category, play time, last played.
    pub(super) fn render_library_list(&mut self, cx: &mut Context<Self>) -> Div {
        let mut list = style::card().flex().flex_col().p_1();
        let head = |t: &'static str, w: f32| {
            div()
                .w(px(w))
                .flex_none()
                .text_xs()
                .text_color(pal::faint())
                .child(t)
        };
        list = list.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .child(div().w(px(36.)).flex_none())
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(pal::faint())
                        .child(self.tr("Name", "Ad")),
                )
                .child(head(self.tr("Kind", "Tür"), 130.))
                .child(head(self.tr("Category", "Kategori"), 150.))
                .child(head(self.tr("Played", "Süre"), 90.))
                .child(head(self.tr("Last played", "Son oynanma"), 110.)),
        );
        for &ix in &self.visible.clone() {
            let Some(game) = self.games.get(ix).cloned() else {
                continue;
            };
            let entry = self.library.get(&game.id.id).cloned();
            let (played, last) = self.playtime(&game.id);
            let picked = self.is_picked(ix);
            let running = self.running.contains_key(&game.id.to_string());
            let kind = entry
                .as_ref()
                .map(|e| self.kind_label(e))
                .unwrap_or_default();
            let category = entry
                .as_ref()
                .and_then(|e| e.category.clone())
                .unwrap_or_default();
            let last = last
                .map(|t| {
                    super::game_page::ago(
                        arkor_core::stats::now().saturating_sub(t),
                        self.global.language,
                    )
                })
                .unwrap_or_else(|| "—".into());
            list = list.child(
                div()
                    .id(("list-row", ix))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_1p5()
                    .rounded(px(10.))
                    .cursor_pointer()
                    .when(picked, |d| d.bg(pal::accent().opacity(0.18)))
                    .when(!picked, |d| d.hover(|h| h.bg(pal::text().opacity(0.05))))
                    .when(entry.as_ref().is_some_and(|e| e.hidden), |d| {
                        d.opacity(0.55)
                    })
                    .on_click(cx.listener(move |this, e, _, cx| this.library_click(ix, e, cx)))
                    .child(div().w(px(36.)).flex_none().child(self.cover_tile(
                        &game.id.id,
                        &game.name,
                        36.,
                        54.,
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(game.name.clone()),
                            )
                            .when(entry.as_ref().is_some_and(|e| e.favorite), |d| {
                                d.child(style::icon("star", 13., pal::accent_light()))
                            })
                            .when(running, |d| {
                                d.child(style::tag(self.tr("Running", "Çalışıyor"), pal::success()))
                            }),
                    )
                    .child(
                        div()
                            .w(px(130.))
                            .flex_none()
                            .truncate()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(kind),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .flex_none()
                            .truncate()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(category),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .flex_none()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(if played > 0 {
                                arkor_core::stats::format_duration(played)
                            } else {
                                "—".to_string()
                            }),
                    )
                    .child(
                        div()
                            .w(px(110.))
                            .flex_none()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(last),
                    ),
            );
        }
        list
    }
}
