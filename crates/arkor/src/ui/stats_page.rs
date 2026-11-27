//! The Statistics page (Tools → Statistics): a year in review (or all time) of what was
//! played, when and how much, and the play journal of the last days. From the session
//! history `arkor run` keeps (see `arkor_core::stats`); play time from before it existed
//! counts in the totals per game only.

use arkor_core::stats::{self, Session, Summary};
use gpui::{Context, Div, FontWeight, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};

#[derive(Default)]
pub(super) struct StatsUi {
    sessions: Option<Vec<Session>>,
    /// The year shown (`None`: all time).
    year: Option<i32>,
    picked_year: bool,
}

const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_TR: [&str; 12] = [
    "Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara",
];
const DAYS_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const DAYS_TR: [&str; 7] = ["Pzt", "Sal", "Çar", "Per", "Cum", "Cmt", "Paz"];

/// A title with a value under it, the value shown whole (however long).
fn fact_row(title: impl Into<gpui::SharedString>, value: String, _: Div) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .py_1()
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(pal::text())
                .child(title.into()),
        )
        .child(div().text_sm().text_color(pal::muted()).child(value))
}

impl Library {
    fn game_name_of(&self, game: &str) -> String {
        let id = game.split_once(':').map_or(game, |(_, id)| id);
        self.library
            .get(id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| game.to_string())
    }

    /// A row of bars: (label, value).
    fn bars(&self, items: &[(String, u64)]) -> Div {
        let max = items.iter().map(|(_, v)| *v).max().unwrap_or(0).max(1);
        let mut row = div().flex().items_end().gap_1().h(px(140.));
        for (label, v) in items {
            let h = (*v as f32 / max as f32 * 110.).max(if *v > 0 { 3. } else { 1. });
            row = row.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_end()
                    .gap_1()
                    .h_full()
                    .child(div().w_full().h(px(h)).rounded_t(px(4.)).bg(if *v > 0 {
                        pal::accent().opacity(0.85)
                    } else {
                        pal::text().opacity(0.08)
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(pal::faint())
                            .child(label.clone()),
                    ),
            );
        }
        row
    }

    fn tile(&self, value: String, label: &'static str) -> Div {
        style::card()
            .flex_1()
            .min_w(px(150.))
            .p_4()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(pal::text())
                    .child(value),
            )
            .child(div().text_sm().text_color(pal::muted()).child(label))
    }

    pub(super) fn render_stats_page(&mut self, cx: &mut Context<Self>) -> Div {
        if self.stats_ui.sessions.is_none() {
            self.stats_ui.sessions = Some(stats::sessions());
        }
        let sessions = self.stats_ui.sessions.clone().unwrap_or_default();
        let years = stats::years(&sessions);
        if !self.stats_ui.picked_year {
            self.stats_ui.year = years.first().copied();
        }
        let year = self.stats_ui.year;
        let tr = self.global.language == arkor_core::Language::Tr;
        let header = Self::header(
            self.tr("Statistics", "İstatistikler"),
            match year {
                Some(y) => format!("{} {y}", self.tr("Year in review", "Yılın özeti")),
                None => self.tr("All time", "Tüm zamanlar").to_string(),
            },
        );
        if sessions.is_empty() {
            return div().child(header).child(widgets::label(self.tr(
                "Play a game: its sessions are counted from now on (play time from before shows on each game's page).",
                "Bir oyun oyna: oturumlar bundan sonra sayılır (önceki oynama süreleri her oyunun sayfasında görünür).",
            )));
        }
        let mut picker = div().flex().flex_wrap().gap_1p5();
        for (ix, y) in years.iter().copied().enumerate() {
            picker = picker.child(
                widgets::chip(("stats-year", ix), y.to_string(), year == Some(y)).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.stats_ui.year = Some(y);
                        this.stats_ui.picked_year = true;
                        cx.notify();
                    }),
                ),
            );
        }
        picker = picker.child(
            widgets::chip(
                "stats-all",
                self.tr("All time", "Tüm zamanlar"),
                year.is_none(),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.stats_ui.year = None;
                this.stats_ui.picked_year = true;
                cx.notify();
            })),
        );
        let s: Summary = stats::summarize(&sessions, year);
        let fmt = stats::format_duration;
        let tiles = div()
            .flex()
            .flex_wrap()
            .gap_3()
            .child(self.tile(fmt(s.total), self.tr("played", "oynandı")))
            .child(self.tile(s.per_game.len().to_string(), self.tr("games", "oyun")))
            .child(self.tile(s.sessions.to_string(), self.tr("sessions", "oturum")))
            .child(self.tile(
                s.days_played.to_string(),
                self.tr("days with play", "oyunlu gün"),
            ))
            .child(self.tile(
                s.best_streak.to_string(),
                self.tr("days in a row at most", "en uzun seri (gün)"),
            ));
        let months: Vec<(String, u64)> = (0..12)
            .map(|m| {
                (
                    if tr { MONTHS_TR[m] } else { MONTHS_EN[m] }.to_string(),
                    s.per_month[m],
                )
            })
            .collect();
        let weekdays: Vec<(String, u64)> = (0..7)
            .map(|d| {
                (
                    if tr { DAYS_TR[d] } else { DAYS_EN[d] }.to_string(),
                    s.per_weekday[d],
                )
            })
            .collect();
        let hours: Vec<(String, u64)> = (0..24)
            .map(|h| {
                (
                    if h % 6 == 0 {
                        h.to_string()
                    } else {
                        String::new()
                    },
                    s.per_hour[h],
                )
            })
            .collect();

        let mut top = widgets::section(self.tr("Most played", "En çok oynananlar"));
        let most = s.per_game.first().map(|(_, v)| *v).unwrap_or(1).max(1);
        for (game, secs) in s.per_game.iter().take(10) {
            top = top.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().w(px(220.)).truncate().child(self.game_name_of(game)))
                    .child(
                        div()
                            .flex_1()
                            .h(px(8.))
                            .rounded_full()
                            .bg(pal::text().opacity(0.06))
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .w(gpui::relative(*secs as f32 / most as f32))
                                    .bg(pal::accent()),
                            ),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .text_sm()
                            .text_color(pal::muted())
                            .child(fmt(*secs)),
                    ),
            );
        }
        let mut facts = widgets::section(self.tr("Highlights", "Öne çıkanlar"));
        if let Some(l) = &s.longest {
            let d = stats::local_day(l.start);
            facts = facts.child(fact_row(
                self.tr("Longest session", "En uzun oturum"),
                format!(
                    "{} · {} · {:02}.{:02}.{}",
                    self.game_name_of(&l.game),
                    fmt(l.secs()),
                    d.day,
                    d.month,
                    d.year
                ),
                div(),
            ));
        }
        if let Some((m, v)) = s.per_month.iter().enumerate().max_by_key(|(_, v)| **v)
            && *v > 0
        {
            facts = facts.child(fact_row(
                self.tr("Busiest month", "En yoğun ay"),
                format!(
                    "{} · {}",
                    if tr { MONTHS_TR[m] } else { MONTHS_EN[m] },
                    fmt(*v)
                ),
                div(),
            ));
        }
        if year.is_some() && !s.new_games.is_empty() {
            facts = facts.child(fact_row(
                self.tr("Started this year", "Bu yıl başlananlar"),
                s.new_games
                    .iter()
                    .take(6)
                    .map(|g| self.game_name_of(g))
                    .collect::<Vec<_>>()
                    .join(", "),
                div(),
            ));
        }

        let mut journal = widgets::section(self.tr("Play journal", "Oyun günlüğü"));
        for (d, games) in stats::journal(&sessions, 30) {
            let day = format!(
                "{:02}.{:02}.{} {}",
                d.day,
                d.month,
                d.year,
                if tr {
                    DAYS_TR[d.weekday as usize]
                } else {
                    DAYS_EN[d.weekday as usize]
                }
            );
            let text = games
                .iter()
                .map(|(g, secs)| format!("{} ({})", self.game_name_of(g), fmt(*secs)))
                .collect::<Vec<_>>()
                .join(" · ");
            journal = journal.child(fact_row(day, text, div()));
        }

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(header)
            .child(picker)
            .child(tiles)
            .child(widgets::section(self.tr("By month", "Aylara göre")).child(self.bars(&months)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(
                        div().flex_1().min_w(px(300.)).child(
                            widgets::section(
                                self.tr("By day of the week", "Haftanın günlerine göre"),
                            )
                            .child(self.bars(&weekdays)),
                        ),
                    )
                    .child(
                        div().flex_1().min_w(px(300.)).child(
                            widgets::section(self.tr("By hour", "Saatlere göre"))
                                .child(self.bars(&hours)),
                        ),
                    ),
            )
            .child(top)
            .child(facts)
            .child(journal)
    }

    /// The history changed (a game ended): read again when the page shows next.
    pub(super) fn stats_forget(&mut self) {
        self.stats_ui.sessions = None;
    }
}
