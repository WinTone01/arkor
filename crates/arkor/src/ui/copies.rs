//! One game owned in several stores (GOG and Epic, say): the extra entries are linked to
//! one shown entry as its copies (`LibraryGame::copy_of`), kept out of the lists, and can be
//! played, made the shown one or separated again from the game page. The library page
//! offers to link entries that look like the same game.

use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*};

use super::style::{self, pal};
use super::{Library, widgets};

impl Library {
    /// Where an entry came from, in a word or two ("Epic Games", "Battle.net", "GOG").
    pub(super) fn source_label(&self, id: &str) -> String {
        let Some(g) = self.library.get(id) else {
            return String::new();
        };
        if let Some(origin) = g
            .origin
            .as_deref()
            .and_then(|o| o.parse::<arkor_core::GameId>().ok())
        {
            let name = self.store_name(&origin);
            if !name.is_empty() {
                return name;
            }
        }
        self.kind_label(g).to_string()
    }

    /// Whether the game page has something to say about copies.
    pub(super) fn has_copies(&self, id: &str) -> bool {
        let Some(g) = self.library.get(id) else {
            return false;
        };
        g.copy_of.is_some()
            || !self.library.copies_of(id).is_empty()
            || self
                .library
                .same_games()
                .iter()
                .any(|grp| grp.iter().any(|i| i == id))
    }

    fn edit_copies(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut arkor_core::library::Library) -> anyhow::Result<()>,
    ) {
        let result = f(&mut self.library);
        self.report(result);
        self.save_library();
        self.apply_filter();
        cx.notify();
    }

    /// Links every group of entries that look like the same game; the most played of each
    /// stays shown.
    pub(super) fn link_all_copies(&mut self, cx: &mut Context<Self>) {
        let groups = self.copy_groups();
        for group in groups {
            let main = group
                .iter()
                .max_by_key(|id| {
                    self.playtime(&arkor_core::GameId::new(
                        arkor_core::Store::Local,
                        id.as_str(),
                    ))
                    .0
                })
                .cloned()
                .unwrap_or_default();
            for other in group.iter().filter(|i| **i != main) {
                let _ = self.library.link_copy(&main, other);
            }
        }
        self.save_library();
        self.apply_filter();
        cx.notify();
    }

    /// The library page's offer to link same games, when there are any.
    /// Groups of same-looking entries the user did not call different games.
    fn copy_groups(&self) -> Vec<Vec<String>> {
        self.library
            .same_games()
            .into_iter()
            .filter(|g| {
                let key = self
                    .library
                    .get(&g[0])
                    .map(|e| arkor_core::library::title_key(&e.name))
                    .unwrap_or_default();
                !self.global.not_copies.contains(&key)
            })
            .collect()
    }

    pub(super) fn render_copies_offer(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let n = self.copy_groups().len();
        (n > 0).then(|| {
            style::card()
                .p_4()
                .flex()
                .items_center()
                .gap_3()
                .child(style::icon("copy", 18., pal::accent_light()))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(gpui::FontWeight::MEDIUM).child(format!(
                            "{n} {}",
                            self.tr(
                                "games are in the library more than once",
                                "oyun kütüphanede birden fazla kez var"
                            )
                        )))
                        .child(div().text_sm().text_color(pal::muted()).child(self.tr(
                            "The same game from different stores. Join them into one card; each copy can still be played from the game's page.",
                            "Farklı mağazalardan aynı oyun. Tek kartta birleştir; her kopya yine oyunun sayfasından oynanabilir.",
                        ))),
                )
                .child(
                    widgets::chip("copies-not", self.tr("They are different games", "Farklı oyunlar"), false)
                        .on_click(cx.listener(|this, _, _, cx| {
                            for g in this.copy_groups() {
                                if let Some(e) = this.library.get(&g[0]) {
                                    let key = arkor_core::library::title_key(&e.name);
                                    this.global.not_copies.push(key);
                                }
                            }
                            let r = this.global.save();
                            this.report(r);
                            cx.notify();
                        })),
                )
                .child(
                    widgets::primary_button("copies-link-all", self.tr("Join them", "Birleştir"))
                        .on_click(cx.listener(|this, _, _, cx| this.link_all_copies(cx))),
                )
        })
    }

    /// The game page's card: the copies of this game, or what it is a copy of.
    pub(super) fn render_copies(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let entry = self.entry()?.clone();
        let id = entry.id.clone();
        let mut card = widgets::section(self.tr("Copies in other stores", "Diğer mağazalardaki kopyalar"))
            .child(widgets::label(self.tr(
                "The same game owned in several stores shows as one card. Play goes to this one; any copy can be played or made the one shown.",
                "Birkaç mağazada sahip olunan aynı oyun tek kart olarak görünür. Oyna bunu açar; herhangi bir kopya oynanabilir ya da gösterilen yapılabilir.",
            )));
        if let Some(main) = entry.copy_of.clone() {
            let main_name = self
                .library
                .get(&main)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            let (m1, m2) = (id.clone(), id.clone());
            return Some(
                card.child(div().text_sm().child(format!(
                    "{} {main_name} ({})",
                    self.tr("This is a copy of", "Bu, şunun kopyası:"),
                    self.source_label(&main)
                )))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            widgets::chip(
                                "copy-main",
                                self.tr("Show this one instead", "Bunun yerine bunu göster"),
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let id = m1.clone();
                                    this.edit_copies(cx, |l| l.make_main(&id))
                                },
                            )),
                        )
                        .child(
                            widgets::chip(
                                "copy-split",
                                self.tr("Make it a game of its own", "Ayrı bir oyun yap"),
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let id = m2.clone();
                                    this.edit_copies(cx, |l| {
                                        l.unlink_copy(&id);
                                        Ok(())
                                    })
                                },
                            )),
                        ),
                ),
            );
        }
        card = card.child(div().text_sm().child(format!(
            "{} {}",
            self.tr("Shown and played:", "Gösterilen ve oynanan:"),
            self.source_label(&id)
        )));
        let copies: Vec<(String, String)> = self
            .library
            .copies_of(&id)
            .iter()
            .map(|g| (g.id.clone(), g.name.clone()))
            .collect();
        for (ix, (cid, name)) in copies.into_iter().enumerate() {
            let (c1, c2, c3) = (cid.clone(), cid.clone(), cid.clone());
            card = card.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(format!("{name} · {}", self.source_label(&cid))),
                    )
                    .child(
                        widgets::chip(
                            ("copy-play", ix),
                            self.tr("Play this copy", "Bu kopyayı oyna"),
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let gid =
                                arkor_core::GameId::new(arkor_core::Store::Local, c1.as_str());
                            this.select_game(&gid, cx);
                            this.play(cx);
                        })),
                    )
                    .child(
                        widgets::chip(
                            ("copy-use", ix),
                            self.tr("Show this one", "Bunu göster"),
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = c2.clone();
                            this.edit_copies(cx, |l| l.make_main(&id));
                            let gid =
                                arkor_core::GameId::new(arkor_core::Store::Local, c2.as_str());
                            this.select_game(&gid, cx);
                        })),
                    )
                    .child(
                        widgets::chip(("copy-split", ix), self.tr("Separate", "Ayır"), false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let id = c3.clone();
                                this.edit_copies(cx, |l| {
                                    l.unlink_copy(&id);
                                    Ok(())
                                })
                            })),
                    ),
            );
        }
        // Entries that look like the same game and are not linked yet.
        let same: Vec<String> = self
            .library
            .same_games()
            .into_iter()
            .find(|g| g.contains(&id))
            .unwrap_or_default()
            .into_iter()
            .filter(|o| *o != id)
            .collect();
        for (ix, other) in same.into_iter().enumerate() {
            let name = self
                .library
                .get(&other)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            let (main, o) = (id.clone(), other.clone());
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(format!(
                                "{} {name} · {}",
                                self.tr("Looks like the same game:", "Aynı oyun gibi görünüyor:"),
                                self.source_label(&other)
                            )),
                    )
                    .child(
                        widgets::chip(
                            ("copy-link", ix),
                            self.tr("Join as a copy", "Kopya olarak birleştir"),
                            true,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let (m, c) = (main.clone(), o.clone());
                            this.edit_copies(cx, |l| l.link_copy(&m, &c))
                        })),
                    ),
            );
        }
        Some(card.when(false, |d| d))
    }
}
