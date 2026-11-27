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
    pub(super) known: HashMap<String, Result<Option<GameInfo>, String>>,
    pictures: HashMap<String, Vec<Picture>>,
    /// What GOG counted for GOG games: minutes and achievements (an error as text).
    gog: HashMap<String, Result<(u64, Vec<arkor_launch::shops::gog::Achievement>), String>>,
}

impl Library {
    pub(super) fn status_name(&self, s: PlayStatus) -> &'static str {
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
        if let Some(id) = self.entry().map(|e| e.id.clone()) {
            arkor_core::stats::record_status(&id, status);
        }
        self.edit_entry(|g| g.status = status);
        cx.notify();
    }

    /// The genres and ways of playing known for a game.
    pub(super) fn genres_of(&self, id: &str) -> Vec<String> {
        match self.info.known.get(id) {
            Some(Ok(Some(i))) => i.genres.iter().chain(&i.modes).cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// Every genre and way of playing in the library, most common first.
    pub(super) fn library_genres(&self) -> Vec<String> {
        let mut count: HashMap<String, usize> = HashMap::new();
        for g in &self.library.games {
            for x in self.genres_of(&g.id) {
                *count.entry(x).or_default() += 1;
            }
        }
        let mut v: Vec<(String, usize)> = count.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().map(|(g, _)| g).collect()
    }

    /// At start: what is known about every game (kept answers at once, the rest looked up
    /// one by one, gently), for the genre shelves.
    pub(super) fn info_fill_all(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || self.global.no_game_info {
            return;
        }
        let games = self.library.games.clone();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let (kept, missing): (Vec<_>, Vec<_>) = bg
                .spawn(async move {
                    let mut kept = Vec::new();
                    let mut missing = Vec::new();
                    for g in games {
                        match info::cached(&g.id) {
                            Some(i) => kept.push((g.id.clone(), i)),
                            None => missing.push(g),
                        }
                    }
                    (kept, missing)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                for (id, i) in kept {
                    this.info.known.entry(id).or_insert(Ok(i));
                }
                this.apply_filter();
                cx.notify();
            });
            for game in missing {
                bg.timer(std::time::Duration::from_millis(1500)).await;
                let id = game.id.clone();
                let found = bg
                    .spawn(async move { info::lookup(&game).map_err(|e| format!("{e:#}")) })
                    .await;
                let stop = this
                    .update(cx, |this, cx| {
                        this.info.known.entry(id).or_insert(found);
                        cx.notify();
                        this.global.no_game_info
                    })
                    .unwrap_or(true);
                if stop {
                    break;
                }
            }
        })
        .detach();
    }

    /// The GOG product id of a library game (`gog:<id>` origin), if it is one.
    fn gog_product(entry: &arkor_core::library::LibraryGame) -> Option<String> {
        let o = entry.origin.as_deref()?;
        let id = o.strip_prefix("gog:")?;
        id.chars()
            .all(|c| c.is_ascii_digit())
            .then(|| id.to_string())
    }

    /// GOG's play time and achievements of a GOG game (read once in the background).
    fn gog_part(
        &mut self,
        entry: &arkor_core::library::LibraryGame,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let product = Self::gog_product(entry)?;
        if !self.info.gog.contains_key(&entry.id) && !cfg!(test) {
            self.info.gog.insert(entry.id.clone(), Ok((0, Vec::new())));
            let id = entry.id.clone();
            let bg = cx.background_executor().clone();
            cx.spawn(async move |this, cx| {
                let r = bg
                    .spawn(async move {
                        arkor_launch::shops::gog::game_stats(&product).map_err(|e| format!("{e:#}"))
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.info.gog.insert(id, r);
                    cx.notify();
                });
            })
            .detach();
        }
        let (minutes, list) = match self.info.gog.get(&entry.id)? {
            Ok(v) => v.clone(),
            Err(e) => return Some(div().child(widgets::label(format!("GOG: {e}")))),
        };
        let got = list.iter().filter(|a| a.unlocked).count();
        let mut part = div().flex().flex_col().gap_2().child(widgets::setting_row(
            "GOG",
            format!(
                "{} · {} {}/{}",
                arkor_core::stats::format_duration(minutes * 60),
                self.tr("achievements", "başarım"),
                got,
                list.len()
            ),
            div(),
        ));
        for a in list.iter().filter(|a| a.unlocked || !a.hidden).take(30) {
            part = part.child(
                div()
                    .flex()
                    .gap_3()
                    .items_center()
                    .child(style::icon(
                        if a.unlocked { "trophy" } else { "lock" },
                        14.,
                        if a.unlocked {
                            pal::accent()
                        } else {
                            pal::muted()
                        },
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(if a.unlocked {
                                        pal::text()
                                    } else {
                                        pal::muted()
                                    })
                                    .child(a.name.clone()),
                            )
                            .child(div().text_xs().text_color(pal::muted()).child(format!(
                                "{} · %{:.1}",
                                a.description,
                                a.rarity as f32 / 10.
                            ))),
                    ),
            );
        }
        Some(part)
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
        if let Some(gog) = self.gog_part(&entry, cx) {
            card = card.child(widgets::divider()).child(gog);
        }

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
        if let Some(b) = &i.beat {
            let h = |m: u32| {
                if m < 90 {
                    format!("{m} {}", self.tr("min", "dk"))
                } else {
                    format!("~{} {}", (m + 30) / 60, self.tr("h", "sa"))
                }
            };
            let mut parts = Vec::new();
            if let Some(m) = b.main {
                parts.push(format!("{} {}", self.tr("Story", "Hikâye"), h(m)));
            }
            if let Some(m) = b.extra {
                parts.push(format!(
                    "{} {}",
                    self.tr("With extras", "Ekstralarla"),
                    h(m)
                ));
            }
            if let Some(m) = b.complete {
                parts.push(format!("{} {}", self.tr("Everything", "Her şeyiyle"), h(m)));
            }
            let played = self.playtime(&entry.game_id()).0 / 60;
            let mut text = format!("{} ({})", parts.join(" · "), b.source);
            if let Some(main) = b.main.filter(|m| *m > 0 && played > 0) {
                let pct = (played as f64 / f64::from(main) * 100.).round() as u64;
                text.push_str(&format!(
                    " — {} %{pct}",
                    self.tr("you played about", "hikâyenin yaklaşık")
                ));
                if self.global.language != arkor_core::Language::Tr {
                    text.push_str(" of the story");
                } else {
                    text.push_str("'ini oynadın");
                }
            }
            facts.push((self.tr("Time to beat", "Bitirme süresi").into(), text));
        }
        for (k, v) in facts {
            card = card.child(widgets::setting_row(k, v, div()));
        }
        // PCGamingWiki: fixes, settings and what the PC version offers.
        let wiki = format!(
            "https://www.pcgamingwiki.com/w/index.php?search={}",
            entry
                .name
                .bytes()
                .map(|b| if b.is_ascii_alphanumeric() {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                })
                .collect::<String>()
        );
        card = card.child(
            div().flex().child(
                widgets::chip(
                    "pcgw",
                    self.tr(
                        "PCGamingWiki: fixes and settings",
                        "PCGamingWiki: düzeltmeler ve ayarlar",
                    ),
                    false,
                )
                .on_click(move |_, _, _| {
                    let _ = std::process::Command::new("xdg-open").arg(&wiki).spawn();
                }),
            ),
        );

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

    /// Settings → Integrations: Arkor's games in Sunshine.
    pub(super) fn render_sunshine(&mut self, cx: &mut Context<Self>) -> Div {
        let on = self.global.sunshine;
        let found = arkor_launch::sunshine::config().is_some();
        widgets::section("Sunshine / Apollo").child(widgets::setting_row(
            self.tr("Games in Sunshine", "Oyunlar Sunshine'da"),
            if found {
                self.tr(
                    "Moonlight lists Arkor's games and starts them with their settings. Only Arkor's entries change; your other applications stay.",
                    "Moonlight Arkor'un oyunlarını listeler ve ayarlarıyla başlatır. Yalnızca Arkor'un girdileri değişir; diğer uygulamaların kalır.",
                )
            } else {
                self.tr(
                    "Sunshine is not set up on this computer.",
                    "Bu bilgisayarda Sunshine kurulu değil.",
                )
            },
            widgets::switch(on)
                .id("sunshine")
                .cursor_pointer()
                .when(!found && !on, |s| s.opacity(0.4))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !found && !on {
                        return;
                    }
                    this.global.sunshine = !on;
                    this.save();
                    this.sunshine_sync(cx);
                })),
        ))
    }

    /// Brings Sunshine's list in line with the library (in the background).
    pub(super) fn sunshine_sync(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || arkor_launch::sunshine::config().is_none() {
            return;
        }
        let on = self.global.sunshine;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let r = bg
                .spawn(async move { arkor_launch::sunshine::sync(on) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = r {
                    this.status = Some(format!("Sunshine: {e:#}").into());
                    cx.notify();
                }
            });
        })
        .detach();
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
