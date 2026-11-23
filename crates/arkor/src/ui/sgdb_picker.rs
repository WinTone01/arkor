//! Picking a cover from SteamGridDB on a game's page: search the game by name, switch
//! between still pictures and animated ones, click a cover to use it. Previews load one by
//! one in the background. Also the automatic cover for games added without one.

use std::path::PathBuf;

use arkor_launch::covers::{self, SgdbImage, SgdbMatch};
use gpui::prelude::FluentBuilder;
use gpui::{Context, ObjectFit, div, img, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};

#[derive(Default)]
pub(super) struct Picker {
    /// The library game the cover is for, and its name.
    game_id: String,
    name: String,
    matches: Vec<SgdbMatch>,
    game: Option<u64>,
    animated: bool,
    /// Covers with their preview once downloaded.
    images: Vec<(SgdbImage, Option<PathBuf>)>,
    loading: bool,
    error: Option<String>,
    /// The cover being applied.
    applying: Option<u64>,
    /// Bumped with each search, so results of an older one are dropped.
    request: u64,
}

impl Library {
    /// Opens the picker and searches SteamGridDB for `name`.
    pub(super) fn sgdb_open(&mut self, game_id: String, name: String, cx: &mut Context<Self>) {
        if self.global.steamgriddb_key.trim().is_empty() {
            self.status = Some(
                self.tr(
                    "Add a SteamGridDB API key in Settings → Integrations first.",
                    "Önce Ayarlar → Entegrasyonlar'a bir SteamGridDB API anahtarı ekle.",
                )
                .into(),
            );
            cx.notify();
            return;
        }
        self.sgdb = Some(Picker {
            game_id,
            name: name.clone(),
            ..Picker::default()
        });
        self.fields.remove("sgdb-search");
        self.sgdb_search(name, cx);
    }

    fn sgdb_search(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(p) = self.sgdb.as_mut() else { return };
        p.request += 1;
        let request = p.request;
        p.loading = true;
        p.error = None;
        p.matches.clear();
        p.images.clear();
        p.game = None;
        cx.notify();
        let key = self.global.steamgriddb_key.clone();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move { covers::sgdb_search(&name, &key) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(p) = this.sgdb.as_mut().filter(|p| p.request == request) else {
                    return;
                };
                p.loading = false;
                match result {
                    Ok(found) => {
                        let first = found.first().map(|m| m.id);
                        p.matches = found;
                        if let Some(game) = first {
                            let animated = p.animated;
                            this.sgdb_load(game, animated, cx);
                        }
                    }
                    Err(e) => p.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Lists the covers of `game`, then downloads their previews one by one.
    fn sgdb_load(&mut self, game: u64, animated: bool, cx: &mut Context<Self>) {
        let Some(p) = self.sgdb.as_mut() else { return };
        p.request += 1;
        let request = p.request;
        p.game = Some(game);
        p.animated = animated;
        p.loading = true;
        p.error = None;
        p.images.clear();
        cx.notify();
        let key = self.global.steamgriddb_key.clone();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let list = bg
                .spawn(async move { covers::sgdb_covers(game, animated, &key) })
                .await;
            let images = match list {
                Ok(images) => images.into_iter().take(50).collect::<Vec<_>>(),
                Err(e) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(p) = this.sgdb.as_mut().filter(|p| p.request == request) {
                            p.loading = false;
                            p.error = Some(format!("{e:#}"));
                            cx.notify();
                        }
                    });
                    return;
                }
            };
            let alive = this.update(cx, |this, cx| {
                let Some(p) = this.sgdb.as_mut().filter(|p| p.request == request) else {
                    return false;
                };
                p.loading = false;
                p.images = images.iter().cloned().map(|i| (i, None)).collect();
                cx.notify();
                true
            });
            if !matches!(alive, Ok(true)) {
                return;
            }
            // All previews are fetched at once on the background pool (animated ones also go
            // through ffmpeg), then shown in order as they arrive.
            let tasks: Vec<_> = images
                .into_iter()
                .map(|image| bg.spawn(async move { covers::sgdb_thumb(&image) }))
                .collect();
            for (ix, task) in tasks.into_iter().enumerate() {
                let Ok(path) = task.await else {
                    continue;
                };
                let still = this.update(cx, |this, cx| {
                    let Some(p) = this.sgdb.as_mut().filter(|p| p.request == request) else {
                        return false;
                    };
                    if let Some(slot) = p.images.get_mut(ix) {
                        slot.1 = Some(path);
                    }
                    cx.notify();
                    true
                });
                if !matches!(still, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    fn sgdb_apply(&mut self, id: String, image: SgdbImage, cx: &mut Context<Self>) {
        if let Some(p) = self.sgdb.as_mut() {
            p.applying = Some(image.id);
        }
        self.status = Some(self.tr("Getting the cover…", "Kapak alınıyor…").into());
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let gid = id.clone();
            let result = bg
                .spawn(async move { covers::set_from_sgdb(&gid, &image) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(p) = this.sgdb.as_mut() {
                    p.applying = None;
                }
                if let Some(g) = this.library.get(&id) {
                    this.covers.insert(id.clone(), covers::cover_for(g));
                }
                this.report(result.map(|_| ()));
                cx.notify();
            });
        })
        .detach();
    }

    /// For a game just added: SteamGridDB's cover when it knows the exact name.
    pub(super) fn auto_cover(&mut self, id: &str, cx: &mut Context<Self>) {
        let key = self.global.steamgriddb_key.clone();
        let Some(game) = self.library.get(id).cloned() else {
            return;
        };
        if key.trim().is_empty() {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let found = bg
                .spawn(async move { covers::auto_cover(&game, &key).map(|p| (game, p)) })
                .await;
            if let Ok((game, Some(_))) = found {
                let _ = this.update(cx, |this, cx| {
                    this.covers
                        .insert(game.id.clone(), covers::cover_for(&game));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The picker as a window over the page.
    pub(super) fn render_sgdb_picker(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let (id, name) = {
            let p = self.sgdb.as_ref()?;
            (p.game_id.clone(), p.name.clone())
        };
        let (id, name) = (id.as_str(), name.as_str());
        let search = self.submit_field(
            "sgdb-search",
            self.tr("Game name, then Enter", "Oyun adı, sonra Enter"),
            cx,
            |this, text, cx| this.sgdb_search(text.to_string(), cx),
        );
        let p = self.sgdb.as_ref()?;
        let (game, animated) = (p.game, p.animated);

        let mut head = div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().w(px(260.)).child(search))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        widgets::chip("sgdb-still", self.tr("Pictures", "Resimler"), !animated)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(g) = game {
                                    this.sgdb_load(g, false, cx);
                                }
                            })),
                    )
                    .child(
                        widgets::chip("sgdb-animated", self.tr("Animated", "Hareketli"), animated)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(g) = game {
                                    this.sgdb_load(g, true, cx);
                                }
                            })),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("sgdb-close")
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded(px(8.))
                    .text_xs()
                    .text_color(pal::muted())
                    .cursor_pointer()
                    .hover(|s| s.bg(pal::raised()).text_color(pal::text()))
                    .child(self.tr("Close", "Kapat"))
                    .child(style::icon("x", 13., pal::muted()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sgdb = None;
                        cx.notify();
                    })),
            );
        if p.matches.is_empty() && !p.loading && p.error.is_none() {
            head = head.child(widgets::label(format!("“{name}”")));
        }

        let mut matches = div().flex().flex_wrap().gap_2();
        for (ix, m) in p.matches.iter().take(8).enumerate() {
            let gid = m.id;
            let text = match m.year() {
                Some(y) => format!("{} ({y})", m.name),
                None => m.name.clone(),
            };
            matches = matches.child(
                widgets::chip(("sgdb-match", ix), text, p.game == Some(m.id)).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let animated = this.sgdb.as_ref().is_some_and(|p| p.animated);
                        this.sgdb_load(gid, animated, cx);
                    }),
                ),
            );
        }

        let mut grid = div().flex().flex_wrap().justify_center().gap_3();
        for (ix, (image, thumb)) in p.images.iter().enumerate() {
            let applying = p.applying == Some(image.id);
            let (id, image_c) = (id.to_string(), image.clone());
            let tile = div()
                .id(("sgdb-cover", ix))
                .relative()
                .w(px(104.))
                .h(px(156.))
                .rounded(px(10.))
                .overflow_hidden()
                .border_2()
                .border_color(if applying {
                    pal::accent()
                } else {
                    gpui::transparent_black()
                })
                .bg(pal::raised().opacity(0.6))
                .cursor_pointer()
                .hover(|s| s.border_color(pal::accent().opacity(0.6)))
                .child(match thumb {
                    Some(path) => img(path.clone())
                        .w(px(104.))
                        .h(px(156.))
                        .object_fit(ObjectFit::Cover)
                        // An id keeps the frame position, so animated covers play.
                        .id(("sgdb-thumb", ix))
                        .into_any_element(),
                    None => div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(style::icon("image", 20., pal::faint()))
                        .into_any_element(),
                })
                .when(image.animated, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top_1()
                            .left_1()
                            .px_1p5()
                            .rounded(px(5.))
                            .bg(gpui::black().opacity(0.6))
                            .text_xs()
                            .text_color(gpui::white())
                            .child(self.tr("Animated", "Hareketli")),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.sgdb_apply(id.clone(), image_c.clone(), cx)
                }));
            grid = grid.child(tile);
        }

        let mut out = style::card()
            .id("sgdb-window")
            .w(px(900.))
            .max_w(gpui::relative(0.92))
            .max_h(gpui::relative(0.86))
            .overflow_y_scroll()
            .bg(pal::bg())
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            // Clicks inside stay inside (the backdrop closes the window).
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(style::icon("images", 16., pal::accent_light()))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child("SteamGridDB"),
                    )
                    .child(widgets::label(self.tr(
                        "· click a cover to use it",
                        "· kullanmak için bir kapağa tıkla",
                    ))),
            )
            .child(head)
            .when(!p.matches.is_empty(), |d| d.child(matches));
        if let Some(e) = &p.error {
            out = out.child(
                div()
                    .text_sm()
                    .text_color(pal::warning())
                    .child(format!("⚠ {e}")),
            );
        } else if p.loading {
            out = out.child(widgets::label(self.tr("Looking…", "Bakılıyor…")));
        } else if p.game.is_some() && p.images.is_empty() {
            out = out.child(widgets::label(if animated {
                self.tr(
                    "No animated covers for this game.",
                    "Bu oyun için hareketli kapak yok.",
                )
            } else {
                self.tr("No covers for this game.", "Bu oyun için kapak yok.")
            }));
        } else if p.matches.is_empty() && p.game.is_none() {
            out = out.child(widgets::label(self.tr(
                "Not found on SteamGridDB: try another name.",
                "SteamGridDB'de bulunamadı: başka bir ad dene.",
            )));
        }
        let window = out.child(grid);
        Some(
            div()
                .id("sgdb-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .bg(gpui::black().opacity(0.55))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.sgdb = None;
                        cx.notify();
                    }),
                )
                .child(window)
                .into_any_element(),
        )
    }
}
