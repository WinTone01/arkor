//! The game page's "About" card: the user's rating and where they are with the game, and
//! what is known about it (see `arkor_launch::info`): description, genres, how it is played,
//! developer, release, age ratings, screenshots and trailers. Settings → Integrations turns
//! the look-ups off and holds the user's IGDB application.

use std::collections::HashMap;
use std::path::PathBuf;

use arkor_core::library::PlayStatus;
use arkor_launch::info::{self, GameInfo};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, ObjectFit, div, img, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};

/// A picture of the game: its file, what it opens, whether it is a trailer.
type Picture = (PathBuf, String, bool);

#[derive(Default)]
pub(super) struct InfoUi {
    /// What is known, by library entry (an error as text).
    known: HashMap<String, Result<Option<GameInfo>, String>>,
    pictures: HashMap<String, Vec<Picture>>,
}

impl Library {
    fn status_name(&self, s: PlayStatus) -> &'static str {
        match s {
            PlayStatus::PlanToPlay => self.tr("Plan to play", "Oynanacak"),
            PlayStatus::Playing => self.tr("Playing", "Oynuyorum"),
            PlayStatus::OnHold => self.tr("On hold", "Ara verdim"),
            PlayStatus::Beaten => self.tr("Beaten", "Bitirdim"),
            PlayStatus::Abandoned => self.tr("Abandoned", "Bıraktım"),
        }
    }

    /// Looks the game up in the background (once per session; the answer is kept a month).
    fn info_load(&mut self, cx: &mut Context<Self>) {
        let Some(game) = self.entry().cloned() else {
            return;
        };
        if cfg!(test) || self.global.no_game_info || self.info.known.contains_key(&game.id) {
            return;
        }
        if let Some(c) = info::cached(&game.id) {
            self.info.known.insert(game.id.clone(), Ok(c.clone()));
            if let Some(i) = c {
                self.info_pictures(&game.id, i, cx);
            }
            return;
        }
        self.info.known.insert(game.id.clone(), Ok(None));
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let id = game.id.clone();
            let found = bg
                .spawn(async move { info::lookup(&game).map_err(|e| format!("{e:#}")) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(Some(i)) = &found {
                    this.info_pictures(&id, i.clone(), cx);
                }
                this.info.known.insert(id, found);
                cx.notify();
            });
        })
        .detach();
    }

    fn info_pictures(&mut self, id: &str, i: GameInfo, cx: &mut Context<Self>) {
        let bg = cx.background_executor().clone();
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let key = id.clone();
            let pics = bg.spawn(async move { info::pictures(&key, &i) }).await;
            let _ = this.update(cx, |this, cx| {
                this.info.pictures.insert(id, pics);
                cx.notify();
            });
        })
        .detach();
    }

    fn set_rating(&mut self, stars: Option<u8>, cx: &mut Context<Self>) {
        self.edit_entry(|g| g.rating = stars);
        cx.notify();
    }

    fn set_status(&mut self, status: Option<PlayStatus>, cx: &mut Context<Self>) {
        self.edit_entry(|g| g.status = status);
        cx.notify();
    }

    /// The "About" card of the selected game.
    pub(super) fn render_about(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let entry = self.entry().cloned()?;
        self.info_load(cx);
        let mut card = widgets::section(self.tr("About", "Hakkında"));

        // The user's own: stars and status.
        let rating = entry.rating.unwrap_or(0);
        let mut stars = div().flex().gap_1();
        for n in 1..=5u8 {
            stars = stars.child(
                div()
                    .id(("star", n as usize))
                    .cursor_pointer()
                    .child(style::icon(
                        "star",
                        18.,
                        if n <= rating {
                            pal::accent_light()
                        } else {
                            pal::faint()
                        },
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // The same star again clears the rating.
                        this.set_rating((n != rating).then_some(n), cx)
                    })),
            );
        }
        let mut statuses = div().flex().flex_wrap().gap_1();
        for (ix, s) in PlayStatus::ALL.into_iter().enumerate() {
            let on = entry.status == Some(s);
            statuses = statuses.child(
                widgets::chip(("status", ix), self.status_name(s), on).on_click(
                    cx.listener(move |this, _, _, cx| this.set_status((!on).then_some(s), cx)),
                ),
            );
        }
        card = card
            .child(widgets::setting_row(
                self.tr("Your rating", "Puanın"),
                "",
                stars,
            ))
            .child(widgets::setting_row(
                self.tr("Status", "Durum"),
                "",
                statuses,
            ));

        let known = self.info.known.get(&entry.id).cloned();
        let Some(Ok(Some(i))) = known else {
            if let Some(Err(e)) = known {
                card = card.child(widgets::label(e));
            }
            return Some(card);
        };
        card = card.child(widgets::divider());
        if !i.description.is_empty() {
            card = card.child(
                div()
                    .text_sm()
                    .text_color(pal::muted())
                    .child(i.description.clone()),
            );
        }
        let mut tags = div().flex().flex_wrap().gap_1p5();
        for g in i.genres.iter().chain(&i.modes) {
            tags = tags.child(style::tag(g.clone(), pal::accent_light()));
        }
        for a in &i.ages {
            tags = tags.child(style::tag(a.clone(), pal::muted()));
        }
        card = card.child(tags);
        let mut facts: Vec<(String, String)> = Vec::new();
        if !i.developers.is_empty() {
            facts.push((
                self.tr("Developer", "Geliştirici").into(),
                i.developers.join(", "),
            ));
        }
        if !i.release.is_empty() {
            facts.push((self.tr("Released", "Çıkış").into(), i.release.clone()));
        }
        if let Some(s) = i.score {
            facts.push((
                self.tr("Critics", "Eleştirmenler").into(),
                format!("{s}/100"),
            ));
        }
        if let Some(h) = i.hours {
            facts.push((self.tr("Length", "Süre").into(), format!("~{h} h")));
        }
        for (k, v) in facts {
            card = card.child(widgets::setting_row(k, v, div()));
        }

        // Trailers and screenshots, a row of pictures; a click opens the full one.
        let pics = self
            .info
            .pictures
            .get(&entry.id)
            .cloned()
            .unwrap_or_default();
        if !pics.is_empty() {
            let mut row = div().flex().flex_wrap().gap_2();
            for (ix, (file, target, trailer)) in pics.into_iter().enumerate() {
                row = row.child(
                    div()
                        .id(("shot", ix))
                        .relative()
                        .w(px(176.))
                        .h(px(99.))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .cursor_pointer()
                        .border_1()
                        .border_color(pal::text().opacity(0.08))
                        .hover(|h| h.border_color(pal::accent().opacity(0.7)))
                        .child(img(file).size_full().object_fit(ObjectFit::Cover))
                        .when(trailer, |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(pal::bg().opacity(0.35))
                                    .child(style::icon("play", 26., pal::text())),
                            )
                        })
                        .on_click(move |_, _, _| open_picture(&target, trailer)),
                );
            }
            card = card.child(row);
        }
        if !i.link.is_empty() {
            let link = i.link.clone();
            card = card.child(
                div().flex().child(
                    style::button("info-source", i.source.clone(), false)
                        .child(style::icon("external-link", 13., pal::muted()))
                        .on_click(move |_, _, _| {
                            let _ = std::process::Command::new("xdg-open").arg(&link).spawn();
                        }),
                ),
            );
        }
        Some(card)
    }

    /// Settings → Integrations: game information and the user's IGDB application.
    pub(super) fn render_info_settings(&mut self, cx: &mut Context<Self>) -> Div {
        let on = !self.global.no_game_info;
        let mut card = widgets::section(self.tr("Game information", "Oyun bilgileri"));
        card = card.child(widgets::setting_row(
            self.tr("Look games up", "Oyunları araştır"),
            self.tr(
                "Description, genres, age ratings, screenshots and trailers on the game page, from Steam's store (and VNDB for visual novels). Only the game's name is sent.",
                "Oyun sayfasında açıklama, türler, yaş sınırları, ekran görüntüleri ve fragmanlar; Steam mağazasından (görsel romanlar için VNDB). Yalnızca oyunun adı gönderilir.",
            ),
            widgets::switch(on)
                .id("game-info")
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.global.no_game_info = on;
                    this.save();
                    cx.notify();
                })),
        ));
        if !on {
            return card;
        }
        let has_igdb = self
            .info_igdb
            .unwrap_or_else(|| info::igdb_account().is_some());
        self.info_igdb = Some(has_igdb);
        if has_igdb {
            return card.child(widgets::setting_row(
                "IGDB",
                self.tr(
                    "Your Twitch application is used first.",
                    "Önce senin Twitch uygulaman kullanılır.",
                ),
                style::button("igdb-out", self.tr("Forget", "Unut"), false).on_click(cx.listener(
                    |this, _, _, cx| {
                        info::igdb_logout();
                        this.info_igdb = Some(false);
                        cx.notify();
                    },
                )),
            ));
        }
        let id = self.field("igdb-id", |_| String::new(), "Client ID", cx, |_, _| {});
        let secret = self.secret_field("igdb-secret", "Client secret", cx);
        card.child(widgets::setting_row(
            self.tr("IGDB (optional)", "IGDB (isteğe bağlı)"),
            self.tr(
                "More games, and age ratings of every country: create an application at dev.twitch.tv/console and enter its ID and secret (kept encrypted).",
                "Daha çok oyun ve her ülkenin yaş sınırları: dev.twitch.tv/console'da bir uygulama oluştur, kimliğini ve gizli anahtarını gir (şifreli saklanır).",
            ),
            div()
                .flex()
                .flex_col()
                .gap_1()
                .w(px(300.))
                .child(id)
                .child(secret)
                .child(
                    style::button("igdb-save", self.tr("Save", "Kaydet"), true).on_click(
                        cx.listener(|this, _, _, cx| this.igdb_save(cx)),
                    ),
                ),
        ))
    }

    fn igdb_save(&mut self, cx: &mut Context<Self>) {
        let id = self.field_text("igdb-id", cx).unwrap_or_default();
        let secret = self.field_text("igdb-secret", cx).unwrap_or_default();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let r = bg
                .spawn(async move { info::igdb_login(&id, &secret) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(()) => {
                        this.info_igdb = Some(true);
                        this.fields.inputs.remove("igdb-secret");
                        this.info.known.clear();
                        this.status = Some(this.tr("IGDB is set up.", "IGDB ayarlandı.").into());
                    }
                    Err(e) => this.status = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Opens a screenshot in the image viewer, or plays a trailer (mpv when it is there, else
/// the browser).
fn open_picture(target: &str, trailer: bool) {
    if trailer && !target.contains("youtube.com") && arkor_launch::system::has("mpv") {
        let _ = std::process::Command::new("mpv")
            .arg("--really-quiet")
            .arg(target)
            .spawn();
        return;
    }
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}
