//! The Downloads overview, laid out like Steam's download page: the current download as a
//! large card with a speed graph, then "Up next" (the queue, reorderable) and "Completed"
//! (what finished in this session). Rows are generic (art, name, state, actions), so store
//! game downloads can join the same queue later with their cover art.

use std::collections::VecDeque;

use arkor_launch::downloads::{self, State};
use arkor_launch::progress::{self, Phase};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};

/// Speed samples kept for the graph (one per 0.3 s: about 45 s).
pub(super) const SPEED_SAMPLES: usize = 150;

/// Recent download speeds for the graph, sampled by the progress watcher.
#[derive(Default)]
pub(super) struct SpeedGraph {
    pub(super) samples: VecDeque<f64>,
    pub(super) peak: f64,
}

impl SpeedGraph {
    /// Adds the current total speed; an idle queue clears the graph.
    pub(super) fn sample(&mut self) -> bool {
        let active = progress::list();
        let downloading: Vec<_> = active
            .iter()
            .filter(|e| e.phase == Phase::Downloading)
            .collect();
        if active.is_empty() {
            let changed = !self.samples.is_empty();
            self.samples.clear();
            self.peak = 0.;
            return changed;
        }
        let speed: f64 = downloading.iter().map(|e| e.speed).sum();
        self.samples.push_back(speed);
        while self.samples.len() > SPEED_SAMPLES {
            self.samples.pop_front();
        }
        self.peak = self.peak.max(speed);
        true
    }
}

/// An icon for a queue key (store downloads will bring their own art).
fn icon_for(key: &str) -> &'static str {
    if arkor_launch::shops::parse_key(key).is_some() {
        "gamepad-2"
    } else if key.starts_with("latest:") || key.contains("proton") {
        "wine"
    } else if key.starts_with("component:") {
        "package"
    } else if ["mako", "afmf", "vknemu", "vksumi"].contains(&key) {
        "layers"
    } else {
        "puzzle"
    }
}

/// The art tile at the start of a row (or the large one in the current-download card):
/// a store game's picture, else an icon.
fn art(lib: &mut Library, key: &str, w: f32, h: f32, cx: &mut Context<Library>) -> Div {
    if let Some((shop, id, _)) = arkor_launch::shops::parse_key(key)
        && let Some(path) = lib
            .shop_wide(shop, id, cx)
            .or_else(|| lib.shop_cover(shop, id, cx))
    {
        let radius = (h * 0.14).clamp(8., 16.);
        return div().w(px(w)).h(px(h)).flex_none().child(
            super::cover_art::cover(path, radius, false)
                .w(px(w))
                .h(px(h)),
        );
    }
    div()
        .w(px(w))
        .h(px(h))
        .flex_none()
        .rounded(px((h * 0.14).clamp(8., 16.)))
        .flex()
        .items_center()
        .justify_center()
        .bg(style::accent_gradient())
        .child(style::icon(
            icon_for(key),
            (h * 0.42).min(48.),
            pal::on_accent(),
        ))
}

fn eta_text(secs: u64, lib: &Library) -> String {
    if secs >= 3600 {
        format!(
            "{} {} {} {}",
            secs / 3600,
            lib.tr("h", "sa"),
            (secs % 3600) / 60,
            lib.tr("min", "dk")
        )
    } else if secs >= 60 {
        format!("{} {}", secs / 60, lib.tr("min", "dk"))
    } else {
        format!("{secs} {}", lib.tr("s", "sn"))
    }
}

fn section_title(text: impl Into<gpui::SharedString>, count: Option<usize>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(pal::text_soft())
                .child(text.into()),
        )
        .children(count.map(|n| {
            div()
                .px_2()
                .rounded_full()
                .bg(pal::raised())
                .text_xs()
                .text_color(pal::muted())
                .child(n.to_string())
        }))
}

impl Library {
    /// The current download: large art, name, progress and figures, and the speed graph.
    fn current_download(&mut self, e: &progress::Entry, cx: &mut Context<Self>) -> Div {
        let downloading = e.phase == Phase::Downloading;
        let status = match e.phase {
            Phase::Preparing if arkor_launch::shops::parse_key(&e.key).is_some() => {
                self.tr("Preparing", "Hazırlanıyor").to_string()
            }
            Phase::Preparing => self
                .tr("Checking for updates", "Güncellemeler denetleniyor")
                .to_string(),
            Phase::Finishing => self.tr("Installing", "Kuruluyor").to_string(),
            Phase::Downloading => self.tr("Downloading", "İndiriliyor").to_string(),
        };
        let size = match (downloading, e.total) {
            (true, Some(t)) => format!(
                "{} / {}",
                progress::human_bytes(e.done),
                progress::human_bytes(t)
            ),
            (true, None) => progress::human_bytes(e.done),
            _ => String::new(),
        };
        let percent = e
            .fraction()
            .filter(|_| downloading)
            .map(|f| format!("{:.0}%", f * 100.));
        let eta = e
            .eta_secs()
            .filter(|_| downloading)
            .map(|s| format!("{} {}", eta_text(s, self), self.tr("left", "kaldı")));
        let queued = downloads::items().iter().any(|i| i.key == e.key);

        // Speed graph: one bar per sample, scaled to the peak.
        let graph = &self.downloads.speed;
        let peak = graph.peak.max(1.);
        let mut bars = div()
            .h(px(84.))
            .flex()
            .items_end()
            // The newest sample stays in view.
            .justify_end()
            .gap(px(1.))
            .overflow_hidden();
        for &v in graph.samples.iter() {
            let h = ((v / peak) as f32 * 80.).max(1.);
            bars = bars.child(
                div()
                    .w(px(2.))
                    .h(px(h))
                    .rounded_t(px(1.))
                    .bg(pal::accent().opacity(0.75)),
            );
        }
        let now = graph.samples.back().copied().unwrap_or(0.);
        let stat = |label: &'static str, value: String| {
            div()
                .flex()
                .flex_col()
                .child(div().text_xs().text_color(pal::faint()).child(label))
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(value),
                )
        };
        let graph_panel = div()
            // Narrower windows leave the room to the name and the figures.
            .w(px(if self.viewport_w < 1500. { 220. } else { 340. }))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(px(12.))
            .bg(pal::raised().opacity(0.35))
            .child(bars)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(stat(
                        self.tr("Network", "Ağ"),
                        format!("{}/s", progress::human_bytes(now as u64)),
                    ))
                    .child(stat(
                        self.tr("Peak", "Tepe"),
                        format!("{}/s", progress::human_bytes(graph.peak as u64)),
                    )),
            );

        let key = e.key.clone();
        let key2 = e.key.clone();
        style::card()
            .p_5()
            .flex()
            .items_center()
            .gap_5()
            .child(art(self, &e.key, 200., 112., cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal::accent_light())
                            .child(status.to_uppercase()),
                    )
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .truncate()
                            .child(e.label.clone()),
                    )
                    .child(
                        super::downloads_page::progress_bar(if downloading {
                            e.fraction()
                        } else {
                            None
                        })
                        .h(px(8.)),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_3()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(size)
                            .child(div().flex().gap_3().children(eta).children(
                                percent.map(|p| div().text_color(pal::accent_light()).child(p)),
                            )),
                    )
                    .when(queued, |d| {
                        d.child(
                            div()
                                .flex()
                                .gap_2()
                                .pt_1()
                                .child(
                                    style::button("cur-pause", self.tr("Pause", "Duraklat"), false)
                                        .child(style::icon("pause", 13., pal::muted()))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            downloads::pause(&key);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    style::button("cur-cancel", self.tr("Cancel", "İptal"), false)
                                        .child(style::icon("x", 13., pal::muted()))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            downloads::cancel(&key2);
                                            cx.notify();
                                        })),
                                ),
                        )
                    }),
            )
            .child(graph_panel)
    }

    /// One row of "Up next" or "Completed".
    fn queue_row(
        &self,
        art: Div,
        title: String,
        detail: String,
        detail_color: gpui::Hsla,
        actions: Div,
        last: bool,
    ) -> Div {
        div()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .gap_4()
            .when(!last, |d| {
                d.border_b_1().border_color(pal::glass_spec().border)
            })
            .child(art)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(detail_color)
                            .truncate()
                            .child(detail),
                    ),
            )
            .child(actions.flex_none())
    }

    /// Waiting, paused and failed downloads in the order they will run.
    fn up_next(&mut self, cx: &mut Context<Self>) -> Div {
        let items: Vec<downloads::Item> = downloads::items()
            .into_iter()
            .filter(|i| !progress::running(&i.key))
            .collect();
        let mut out = div().flex().flex_col().gap_3().child(section_title(
            self.tr("Up next", "Sıradakiler"),
            Some(items.len()),
        ));
        if items.is_empty() {
            return out.child(
                style::card()
                    .px_4()
                    .py_3()
                    .text_sm()
                    .text_color(pal::muted())
                    .child(self.tr("Nothing waiting.", "Bekleyen bir şey yok.")),
            );
        }
        let n = items.len();
        let mut list = style::card().overflow_hidden().flex().flex_col();
        for (ix, item) in items.into_iter().enumerate() {
            let (detail, color) = match &item.state {
                State::Queued => (self.tr("Waiting", "Bekliyor").to_string(), pal::muted()),
                State::Paused => (
                    self.tr("Paused", "Duraklatıldı").to_string(),
                    pal::warning(),
                ),
                State::Failed(e) => (
                    format!("{} · {e}", self.tr("Failed", "Başarısız")),
                    pal::danger(),
                ),
            };
            let (k1, k2, k3) = (item.key.clone(), item.key.clone(), item.key.clone());
            let mut actions = div().flex().items_center().gap_2();
            if ix > 0 {
                actions = actions.child(
                    style::icon_button(("q-top", ix), "arrow-up-to-line", false).on_click(
                        cx.listener(move |_, _, _, cx| {
                            downloads::move_to_front(&k1);
                            cx.notify();
                        }),
                    ),
                );
            }
            actions = actions.child(match item.state {
                State::Queued => style::icon_button(("q-pause", ix), "pause", false).on_click(
                    cx.listener(move |_, _, _, cx| {
                        downloads::pause(&k2);
                        cx.notify();
                    }),
                ),
                State::Paused => style::icon_button(("q-resume", ix), "play", true)
                    .on_click(cx.listener(move |this, _, _, cx| this.enqueue_download(&k2, cx))),
                State::Failed(_) => style::icon_button(("q-retry", ix), "rotate-ccw", true)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        downloads::retry(&k2);
                        this.watch_progress(cx);
                        cx.notify();
                    })),
            });
            actions = actions.child(style::icon_button(("q-remove", ix), "x", false).on_click(
                cx.listener(move |_, _, _, cx| {
                    downloads::cancel(&k3);
                    cx.notify();
                }),
            ));
            let picture = art(self, &item.key, 64., 36., cx);
            list = list.child(self.queue_row(
                picture,
                item.label,
                detail,
                color,
                actions,
                ix + 1 == n,
            ));
        }
        out = out.child(list);
        out
    }

    /// What finished in this session, newest first.
    fn completed(&mut self, cx: &mut Context<Self>) -> Div {
        let done = downloads::history();
        let mut head = div()
            .flex()
            .items_center()
            .justify_between()
            .child(section_title(
                self.tr("Completed", "Tamamlananlar"),
                Some(done.len()),
            ));
        if !done.is_empty() {
            head = head.child(
                style::button("done-clear", self.tr("Clear", "Temizle"), false).on_click(
                    cx.listener(|_, _, _, cx| {
                        downloads::clear_history();
                        cx.notify();
                    }),
                ),
            );
        }
        let mut out = div().flex().flex_col().gap_3().child(head);
        if done.is_empty() {
            return out.child(
                style::card()
                    .px_4()
                    .py_3()
                    .text_sm()
                    .text_color(pal::muted())
                    .child(self.tr(
                        "Nothing finished yet in this session.",
                        "Bu oturumda henüz biten bir şey yok.",
                    )),
            );
        }
        let now = arkor_core::stats::now();
        let n = done.len();
        let mut list = style::card().overflow_hidden().flex().flex_col();
        for (ix, f) in done.into_iter().enumerate() {
            let when = super::game_page::ago(now.saturating_sub(f.at), self.global.language);
            let (detail, color, icon, icolor) = if f.ok {
                (
                    format!("{} · {when}", f.report),
                    pal::muted(),
                    "circle-check",
                    pal::success(),
                )
            } else {
                (
                    format!("{} · {when}", f.report),
                    pal::danger(),
                    "circle-alert",
                    pal::danger(),
                )
            };
            let actions = div().child(style::icon(icon, 16., icolor));
            let picture = art(self, &f.key, 64., 36., cx);
            list =
                list.child(self.queue_row(picture, f.label, detail, color, actions, ix + 1 == n));
        }
        out = out.child(list);
        out
    }

    /// The overview: current download, up next, completed.
    pub(super) fn downloads_overview(&mut self, cx: &mut Context<Self>) -> Div {
        let active = progress::list();
        let mut out = div().flex().flex_col().gap_5();
        // Real downloads first; parts only being checked come after.
        let mut shown: Vec<_> = active.iter().collect();
        shown.sort_by_key(|e| e.phase == Phase::Preparing);
        match shown.first() {
            Some(e) => out = out.child(self.current_download(e, cx)),
            None => {
                out = out.child(
                    style::card()
                        .p_5()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(
                            div()
                                .size(px(44.))
                                .rounded(px(12.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(pal::success().opacity(0.12))
                                .child(style::icon("circle-check", 22., pal::success())),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(div().font_weight(gpui::FontWeight::MEDIUM).child(
                                    self.tr("Nothing is downloading", "Şu an bir şey inmiyor"),
                                ))
                                .child(widgets::label(
                                    self.tr("Everything is up to date.", "Her şey güncel."),
                                )),
                        ),
                )
            }
        }
        // Further transfers at the same time, compact.
        for e in shown.iter().skip(1) {
            let detail = match e.fraction() {
                Some(f) if e.phase == Phase::Downloading => format!("{:.0}%", f * 100.),
                _ => self
                    .tr("Checking for updates", "Güncellemeler denetleniyor")
                    .to_string(),
            };
            let picture = art(self, &e.key, 64., 36., cx);
            out = out.child(style::card().child(self.queue_row(
                picture,
                e.label.clone(),
                detail,
                pal::muted(),
                div(),
                true,
            )));
        }
        out.child(self.up_next(cx)).child(self.completed(cx))
    }
}
