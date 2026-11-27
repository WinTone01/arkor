//! "Benchmarks": MangoHud frame-time logs (toggle logging in game, default Shift_L+F2):
//! summary numbers, a frame-time chart, and side-by-side comparison of several runs.

use std::collections::HashMap;
use std::path::PathBuf;

use arkor_core::bench::{self, Run};
use gpui::{Context, Div, div, prelude::*, px};

use super::Library;
use super::widgets::{self, theme::*};

#[derive(Default)]
pub(super) struct BenchState {
    runs: Option<Vec<PathBuf>>,
    selected: Vec<PathBuf>,
    parsed: HashMap<PathBuf, Result<Run, String>>,
    /// The log being renamed.
    renaming: Option<PathBuf>,
    error: Option<String>,
}

const CHART_H: f32 = 140.;

impl Library {
    /// The folder MangoHud logs to: the global HUD's `output_folder`, or Arkor's default.
    fn bench_dir(&self) -> Option<PathBuf> {
        match self.global.mangohud.settings.get("output_folder") {
            Some(dir) => Some(match dir.strip_prefix("~/") {
                Some(rest) => dirs::home_dir()?.join(rest),
                None => PathBuf::from(dir),
            }),
            None => bench::default_dir(),
        }
    }

    /// Rescans and selects every run (used by `arkor open benchmarks#all`).
    pub(super) fn rescan_bench_all(&mut self) {
        self.rescan_bench();
        self.bench.selected = self.bench.runs.clone().unwrap_or_default();
    }

    fn rescan_bench(&mut self) {
        self.bench.runs = Some(
            self.bench_dir()
                .map(|d| bench::list(&d))
                .unwrap_or_default(),
        );
        let runs = self.bench.runs.clone().unwrap_or_default();
        self.bench.selected.retain(|p| runs.contains(p));
    }

    fn bench_run(&mut self, path: &std::path::Path) -> Option<&Run> {
        self.bench
            .parsed
            .entry(path.to_path_buf())
            .or_insert_with(|| bench::parse(path).map_err(|e| format!("{e:#}")))
            .as_ref()
            .ok()
    }

    pub(super) fn render_bench(&mut self, cx: &mut Context<Self>) -> Div {
        if self.bench.runs.is_none() {
            self.rescan_bench();
        }
        let dir = self
            .bench_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        let runs = self.bench.runs.clone().unwrap_or_default();
        let mut page = div().child(Self::header(
            self.tr("Benchmarks", "Performans ölçümü"),
            self.tr(
                "Measure how smoothly a game runs. In the game, press Left Shift + F2 to start recording and again to stop (MangoHud must be on). Each recording appears here; pick two or more to compare them, e.g. before and after turning on a setting.",
                "Bir oyunun ne kadar akıcı çalıştığını ölç. Oyunda kaydı başlatmak için Sol Shift + F2'ye bas, durdurmak için yine bas (MangoHud açık olmalı). Her kayıt burada görünür; ikisini ya da daha fazlasını seçip karşılaştır, ör. bir ayarı açmadan önce ve sonra.",
            ),
        ));

        let mut list = widgets::section(format!("{} · {dir}", self.tr("Runs", "Kayıtlar")));
        if runs.is_empty() {
            list = list.child(widgets::label(self.tr("No logs yet.", "Henüz log yok.")));
        }
        if let Some(err) = &self.bench.error {
            list = list.child(widgets::label(format!("⚠ {err}")));
        }
        for (ix, path) in runs.iter().enumerate() {
            let selected = self.bench.selected.contains(path);
            let (p1, p2, p3) = (path.clone(), path.clone(), path.clone());
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if self.bench.renaming.as_ref() == Some(path) {
                let from = path.clone();
                let input = self.submit_field(
                    "bench-rename",
                    self.tr("New name, then Enter", "Yeni ad, sonra Enter"),
                    cx,
                    move |this, text, cx| {
                        match bench::rename(&from, text) {
                            Ok(to) => {
                                for p in this.bench.selected.iter_mut() {
                                    if *p == from {
                                        *p = to.clone();
                                    }
                                }
                                this.bench.parsed.remove(&from);
                                this.bench.error = None;
                            }
                            Err(e) => this.bench.error = Some(format!("{e:#}")),
                        }
                        this.bench.renaming = None;
                        this.fields.remove("bench-rename");
                        this.rescan_bench();
                        cx.notify();
                    },
                );
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(360.)).child(input))
                        .child(
                            widgets::chip(
                                ("bench-rename-cancel", ix),
                                self.tr("Cancel", "Vazgeç"),
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.bench.renaming = None;
                                this.fields.remove("bench-rename");
                                cx.notify();
                            })),
                        ),
                );
                continue;
            }
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        widgets::chip(("bench-run", ix), name, selected).on_click(cx.listener(
                            move |this, _, _, cx| {
                                if !this.bench.selected.contains(&p1) {
                                    this.bench.selected.push(p1.clone());
                                } else {
                                    this.bench.selected.retain(|p| p != &p1);
                                }
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        widgets::chip(
                            ("bench-ren", ix),
                            self.tr("Rename", "Yeniden adlandır"),
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.fields.remove("bench-rename");
                            this.bench.renaming = Some(p3.clone());
                            cx.notify();
                        })),
                    )
                    .child(
                        widgets::chip(("bench-del", ix), self.tr("Delete", "Sil"), false).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.bench.error =
                                    bench::remove(&p2).err().map(|e| format!("{e:#}"));
                                this.bench.parsed.remove(&p2);
                                this.rescan_bench();
                                cx.notify();
                            }),
                        ),
                    ),
            );
        }
        list = list.child(
            div()
                .flex()
                .gap_2()
                .child(
                    widgets::chip("bench-refresh", self.tr("Refresh", "Yenile"), false).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.rescan_bench();
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    widgets::chip("bench-open", self.tr("Open folder", "Klasörü aç"), false)
                        .on_click(cx.listener(|this, _, _, _| {
                            if let Some(d) = this.bench_dir() {
                                let _ = std::fs::create_dir_all(&d);
                                let _ = std::process::Command::new("xdg-open").arg(d).spawn();
                            }
                        })),
                ),
        );
        page = page.child(list);

        let selected = self.bench.selected.clone();
        let mut runs: Vec<Run> = Vec::new();
        for p in &selected {
            match self.bench_run(p).cloned() {
                Some(r) => runs.push(r),
                None => {
                    let why = self
                        .bench
                        .parsed
                        .get(p)
                        .and_then(|r| r.as_ref().err())
                        .cloned()
                        .unwrap_or_default();
                    page = page.child(widgets::label(format!("⚠ {}: {why}", p.display())));
                }
            }
        }
        if runs.is_empty() {
            return page;
        }
        page = page.child(self.bench_table(&runs));
        for run in &runs {
            page = page.child(self.bench_chart(run));
        }
        page
    }

    fn bench_table(&self, runs: &[Run]) -> Div {
        let sums: Vec<_> = runs.iter().map(Run::summary).collect();
        type Row = (&'static str, fn(&bench::Summary) -> f64, bool, &'static str);
        let rows: [Row; 8] = [
            (
                self.tr("Average FPS", "Ortalama FPS"),
                |s| s.avg_fps,
                true,
                "",
            ),
            (self.tr("1% low", "%1 düşük"), |s| s.low_1, true, ""),
            (self.tr("0.1% low", "%0.1 düşük"), |s| s.low_01, true, ""),
            (self.tr("Min FPS", "En düşük FPS"), |s| s.min_fps, true, ""),
            (self.tr("Max FPS", "En yüksek FPS"), |s| s.max_fps, true, ""),
            (
                self.tr("Avg frame time", "Ort. kare süresi"),
                |s| s.avg_frametime,
                false,
                " ms",
            ),
            (
                self.tr("Frame time spread", "Kare süresi sapması"),
                |s| s.frametime_sd,
                false,
                " ms",
            ),
            (self.tr("Length", "Süre"), |s| s.seconds, true, " s"),
        ];
        let cell = |text: String, w: f32| div().w(px(w)).text_sm().child(text);
        let mut head = div().flex().gap_2().child(cell(String::new(), 170.));
        for r in runs {
            head = head.child(
                div()
                    .w(px(150.))
                    .truncate()
                    .text_sm()
                    .text_color(ACCENT())
                    .child(r.name()),
            );
        }
        let mut table = widgets::section(if runs.len() > 1 {
            self.tr(
                "Comparison (vs. the first selected)",
                "Karşılaştırma (ilk seçilene göre)",
            )
        } else {
            self.tr("Summary", "Özet")
        })
        .child(head);
        for (label, get, higher_better, unit) in rows {
            let base = get(&sums[0]);
            let mut line = div()
                .flex()
                .gap_2()
                .px_1()
                .rounded_sm()
                .bg(super::style::pal::raised().opacity(0.45))
                .child(cell(label.into(), 170.));
            for (i, s) in sums.iter().enumerate() {
                let v = get(s);
                let mut text = format!("{v:.1}{unit}");
                let mut color = TEXT();
                if i > 0 && base > 0.0 && label != self.tr("Length", "Süre") {
                    let pct = (v - base) / base * 100.0;
                    let better = if higher_better { pct > 0.5 } else { pct < -0.5 };
                    let worse = if higher_better { pct < -0.5 } else { pct > 0.5 };
                    text.push_str(&format!("  ({pct:+.1}%)"));
                    color = if better {
                        super::style::pal::success()
                    } else if worse {
                        super::style::pal::danger()
                    } else {
                        TEXT()
                    };
                }
                line = line.child(div().w(px(150.)).text_sm().text_color(color).child(text));
            }
            table = table.child(line);
        }
        table
    }

    fn bench_chart(&self, run: &Run) -> Div {
        let points = run.downsample(240);
        let max = points.iter().cloned().fold(16.7, f64::max);
        let mut bars = div()
            .flex()
            .items_end()
            .h(px(CHART_H))
            .w_full()
            .gap(px(1.))
            .p_1()
            .rounded_md()
            .bg(super::style::pal::raised().opacity(0.45));
        for v in points {
            // Green within 60 FPS pacing, yellow up to 30 FPS, red beyond.
            let color = if v <= 16.7 {
                super::style::pal::success()
            } else if v <= 33.4 {
                super::style::pal::warning()
            } else {
                super::style::pal::danger()
            };
            bars = bars.child(
                div()
                    .flex_1()
                    .h(px(((v / max) as f32 * (CHART_H - 8.)).max(1.)))
                    .bg(color),
            );
        }
        let specs = run
            .specs
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("  ·  ");
        widgets::section(format!(
            "{} — {}",
            run.name(),
            self.tr("frame times", "kare süreleri")
        ))
        .child(widgets::label(format!(
            "{} {max:.1} ms",
            self.tr("Tallest bar:", "En yüksek çubuk:")
        )))
        .child(bars)
        .child(widgets::label(specs))
    }
}
