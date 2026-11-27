//! The Statistics page (Tools → Statistics): a year in review (or all time) of what was
//! played, when and how much, and the play journal of the last days. From the session
//! history `arkor run` keeps (see `arkor_core::stats`); play time from before it existed
//! counts in the totals per game only.

use arkor_core::stats::{self, Session, Summary};
use gpui::prelude::FluentBuilder;
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

        let notes = stats::notes();
        let note_input = self.submit_field(
            "journal-note",
            self.tr(
                "A note about today, then Enter",
                "Bugün hakkında bir not, sonra Enter",
            ),
            cx,
            |this, text, cx| {
                let r = stats::add_note("", text);
                this.report(r);
                cx.notify();
            },
        );
        let mut journal = widgets::section(self.tr("Play journal", "Oyun günlüğü"))
            .child(widgets::label(self.tr(
                "What you played each day, and your notes. Notes can also be written from a running game's page.",
                "Her gün ne oynadığın ve notların. Not, çalışan bir oyunun sayfasından da yazılabilir.",
            )))
            .child(div().max_w(px(520.)).child(note_input));
        // Days with notes but no play show too.
        let mut days = stats::journal(&sessions, 30);
        for n in &notes {
            let d = stats::local_day(n.at);
            if !days
                .iter()
                .any(|(x, _)| (x.year, x.month, x.day) == (d.year, d.month, d.day))
            {
                days.push((d, Vec::new()));
            }
        }
        days.sort_by(|a, b| (b.0.year, b.0.month, b.0.day).cmp(&(a.0.year, a.0.month, a.0.day)));
        days.truncate(30);
        for (d, games) in days {
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
            let mut text = games
                .iter()
                .map(|(g, secs)| format!("{} ({})", self.game_name_of(g), fmt(*secs)))
                .collect::<Vec<_>>()
                .join(" · ");
            for n in notes.iter().filter(|n| {
                let x = stats::local_day(n.at);
                (x.year, x.month, x.day) == (d.year, d.month, d.day)
            }) {
                if !text.is_empty() {
                    text.push('\n');
                }
                if n.game.is_empty() {
                    text.push_str(&format!("✎ {}", n.text));
                } else {
                    text.push_str(&format!("✎ {}: {}", self.game_name_of(&n.game), n.text));
                }
            }
            journal = journal.child(fact_row(day, text, div()));
        }

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(header)
            .child(picker)
            .child(tiles)
            .children(year.map(|y| self.render_goal(y, &s, cx)))
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

    /// The year's goal of games to beat, and the year as a picture to share.
    fn render_goal(&mut self, year: i32, s: &Summary, cx: &mut Context<Self>) -> Div {
        let beaten = stats::beaten_in(&stats::status_history(), year);
        let goal = self.global.beat_goal;
        let step = |this: &mut Self, d: i32| {
            this.global.beat_goal = (this.global.beat_goal as i32 + d).clamp(0, 500) as u32;
            let r = this.global.save();
            this.report(r);
        };
        let mut card = widgets::section(format!(
            "{} {year}",
            self.tr("Games beaten in", "Bitirilen oyunlar:")
        ))
        .child(widgets::setting_row(
            self.tr("Goal for the year", "Yılın hedefi"),
            self.tr(
                "How many games you want to finish. A game counts when you set it to Beaten.",
                "Kaç oyun bitirmek istediğin. Bir oyun, durumunu Bitirdim yaptığında sayılır.",
            ),
            widgets::stepper_control(
                if goal == 0 {
                    self.tr("None", "Yok").to_string()
                } else {
                    goal.to_string()
                },
                widgets::step_button("goal-", "−").on_click(cx.listener(move |this, _, _, cx| {
                    step(this, -1);
                    cx.notify();
                })),
                widgets::step_button("goal+", "+").on_click(cx.listener(move |this, _, _, cx| {
                    step(this, 1);
                    cx.notify();
                })),
            ),
        ));
        let n = beaten.len() as u32;
        let frac = if goal > 0 {
            (n as f32 / goal as f32).min(1.)
        } else {
            0.
        };
        card = card.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::BOLD)
                        .child(if goal > 0 {
                            format!("{n} / {goal}")
                        } else {
                            n.to_string()
                        }),
                )
                .when(goal > 0, |d| {
                    d.child(
                        div()
                            .flex_1()
                            .h(px(10.))
                            .rounded_full()
                            .bg(pal::text().opacity(0.08))
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .bg(pal::accent())
                                    .w(gpui::relative(frac)),
                            ),
                    )
                }),
        );
        if !beaten.is_empty() {
            card = card.child(widgets::label(
                beaten
                    .iter()
                    .map(|(g, _)| self.game_name_of(g))
                    .collect::<Vec<_>>()
                    .join(" · "),
            ));
        }
        let svg = self.year_card_svg(year, s, n);
        card.child(
            div().flex().child(
                widgets::chip(
                    "year-card",
                    self.tr(
                        "Save the year as a picture to share",
                        "Yılı paylaşmak için resim olarak kaydet",
                    ),
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    match save_year_card(year, &svg) {
                        Ok(path) => {
                            this.status = Some(path.display().to_string().into());
                            if let Some(dir) = path.parent() {
                                let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                            }
                        }
                        Err(e) => this.status = Some(format!("{e:#}").into()),
                    }
                    cx.notify();
                })),
            ),
        )
    }

    /// The year in numbers, drawn as an SVG card (1080×1350).
    fn year_card_svg(&self, year: i32, s: &Summary, beaten: u32) -> String {
        let esc = |t: &str| {
            t.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        };
        let mut games = String::new();
        for (i, (g, secs)) in s.per_game.iter().take(5).enumerate() {
            games.push_str(&format!(
                r##"<text x="90" y="{}" font-size="40" fill="#e9e4ff">{}. {}</text><text x="990" y="{}" font-size="36" fill="#b9a8ff" text-anchor="end">{}</text>"##,
                760 + i * 80,
                i + 1,
                esc(&self.game_name_of(g)),
                760 + i * 80,
                esc(&stats::format_duration(*secs))
            ));
        }
        let t = |en: &'static str, tr: &'static str| esc(self.tr(en, tr));
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="1080" height="1350" viewBox="0 0 1080 1350">
<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#1b1035"/><stop offset="1" stop-color="#4a1fb8"/></linearGradient></defs>
<rect width="1080" height="1350" fill="url(#g)"/>
<g font-family="Inter, Noto Sans, sans-serif">
<text x="90" y="170" font-size="54" fill="#b9a8ff">{your}</text>
<text x="90" y="290" font-size="130" font-weight="700" fill="#ffffff">{year}</text>
<text x="90" y="420" font-size="72" font-weight="700" fill="#ffffff">{total}</text>
<text x="90" y="470" font-size="34" fill="#b9a8ff">{played}</text>
<text x="560" y="420" font-size="72" font-weight="700" fill="#ffffff">{ngames}</text>
<text x="560" y="470" font-size="34" fill="#b9a8ff">{games_l}</text>
<text x="90" y="590" font-size="72" font-weight="700" fill="#ffffff">{beaten}</text>
<text x="90" y="640" font-size="34" fill="#b9a8ff">{beaten_l}</text>
<text x="560" y="590" font-size="72" font-weight="700" fill="#ffffff">{days}</text>
<text x="560" y="640" font-size="34" fill="#b9a8ff">{days_l}</text>
{games}
<text x="90" y="1280" font-size="30" fill="#8f7fd0">Arkor</text>
</g></svg>"##,
            your = t("Your year in games", "Oyunlarla geçen yılın"),
            total = esc(&stats::format_duration(s.total)),
            played = t("played", "oynandı"),
            ngames = s.per_game.len(),
            games_l = t("games", "oyun"),
            beaten_l = t("beaten", "bitirildi"),
            days = s.days_played,
            days_l = t("days with play", "oyunlu gün"),
        )
    }

    /// The history changed (a game ended): read again when the page shows next.
    pub(super) fn stats_forget(&mut self) {
        self.stats_ui.sessions = None;
    }
}

/// Writes the year card into `~/Pictures/Arkor` (a PNG beside it when a converter is
/// installed); the file to share.
fn save_year_card(year: i32, svg: &str) -> anyhow::Result<std::path::PathBuf> {
    use anyhow::Context as _;
    let dir = dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .context("no pictures folder")?
        .join("Arkor");
    std::fs::create_dir_all(&dir)?;
    let svg_path = dir.join(format!("Arkor {year}.svg"));
    std::fs::write(&svg_path, svg)?;
    let png = dir.join(format!("Arkor {year}.png"));
    let converted = [
        (
            "rsvg-convert",
            vec![
                "-o".to_string(),
                png.display().to_string(),
                svg_path.display().to_string(),
            ],
        ),
        (
            "magick",
            vec![svg_path.display().to_string(), png.display().to_string()],
        ),
    ]
    .into_iter()
    .any(|(tool, args)| {
        std::process::Command::new(tool)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    Ok(if converted && png.is_file() {
        png
    } else {
        svg_path
    })
}
