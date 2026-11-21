//! MangoHud layout editor, shared by the global settings page and per-game "Custom" mode.
//! Every control calls `Library::edit_hud`, which applies the change to whichever HUD is
//! being edited and saves it immediately.

use arkor_core::mangohud::{FONT_SIZE_RANGE, HudSettings, Metric, Orientation, Position, Preset};
use gpui::{AnyElement, Context, Div, div, prelude::*, px, rgb, rgba};

use super::Library;
use super::i18n::Strings;
use super::widgets::{self, theme::*};

/// Group titles: "GPU"/"CPU" are the same in every language, the others come from `Strings`.
enum GroupTitle {
    Fixed(&'static str),
    Frame,
    Other,
}

const METRIC_GROUPS: [(GroupTitle, &[Metric]); 4] = [
    (
        GroupTitle::Fixed("GPU"),
        &[
            Metric::GpuStats,
            Metric::GpuTemp,
            Metric::GpuCoreClock,
            Metric::GpuMemClock,
            Metric::GpuPower,
            Metric::Vram,
        ],
    ),
    (
        GroupTitle::Fixed("CPU"),
        &[
            Metric::CpuStats,
            Metric::CpuTemp,
            Metric::CpuMhz,
            Metric::CpuPower,
            Metric::Ram,
        ],
    ),
    (
        GroupTitle::Frame,
        &[Metric::Fps, Metric::Frametime, Metric::FrameTiming],
    ),
    (
        GroupTitle::Other,
        &[
            Metric::Resolution,
            Metric::Wine,
            Metric::EngineVersion,
            Metric::Time,
            Metric::Battery,
        ],
    ),
];

impl Library {
    pub(super) fn render_hud_editor(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.presets(hud, cx))
            .child(
                widgets::section(self.t().position_section).child(
                    div()
                        .flex()
                        .gap_6()
                        .items_start()
                        .child(preview(hud, self.t()))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_4()
                                .child(self.position_grid(hud, cx))
                                .child(self.orientation(hud, cx)),
                        ),
                ),
            )
            .child(self.steppers(hud, cx))
            .child(self.metrics(hud, cx))
    }

    fn presets(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let current = hud.matching_preset();
        let mut row = div().flex().flex_wrap().gap_2();
        for (ix, preset) in Preset::ALL.into_iter().enumerate() {
            row =
                row.child(
                    widgets::chip(
                        ("preset", ix),
                        self.t().preset(preset),
                        current == Some(preset),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit_hud(cx, |h| h.apply_preset(preset))
                    })),
                );
        }
        widgets::section(self.t().presets_section)
            .child(row.child(widgets::badge(self.t().custom_badge, current.is_none())))
    }

    fn position_grid(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_col().gap_1();
        for row in 0..3u8 {
            let mut line = div().flex().gap_1();
            for col in 0..3u8 {
                let cell = div().w(px(34.)).h(px(22.)).rounded_sm();
                line = line.child(match Position::from_cell(row, col) {
                    Some(pos) => {
                        let active = hud.position == pos;
                        cell.id(("pos", (row * 3 + col) as usize))
                            .cursor_pointer()
                            .border_1()
                            .border_color(rgb(if active { ACCENT } else { BORDER }))
                            .bg(rgb(if active { ACCENT } else { BG }))
                            .when(!active, |d| d.hover(|s| s.bg(rgb(ROW_HOVER))))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.edit_hud(cx, |h| h.position = pos)
                            }))
                            .into_any_element()
                    }
                    // The centre is not a MangoHud position: leave a hole.
                    None => cell.into_any_element(),
                });
            }
            grid = grid.child(line);
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(widgets::label(self.t().screen_position))
            .child(grid)
    }

    fn orientation(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let opt = |ix: usize, o: Orientation, text: &'static str, cx: &mut Context<Self>| {
            widgets::chip(("orient", ix), text, hud.orientation == o).on_click(
                cx.listener(move |this, _, _, cx| this.edit_hud(cx, |h| h.orientation = o)),
            )
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(widgets::label(self.t().orientation))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(opt(0, Orientation::Vertical, self.t().vertical, cx))
                    .child(opt(1, Orientation::Horizontal, self.t().horizontal, cx)),
            )
    }

    fn steppers(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let btn = |id: &'static str,
                   glyph: &'static str,
                   f: fn(&mut HudSettings),
                   cx: &mut Context<Self>| {
            widgets::step_button(id, glyph)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_hud(cx, f)))
        };
        let t = self.t();
        let fps = if hud.fps_limit == 0 {
            t.unlimited.to_string()
        } else {
            hud.fps_limit.to_string()
        };
        let (min_font, max_font) = FONT_SIZE_RANGE;
        widgets::section(t.size_section).child(
            div()
                .flex()
                .flex_wrap()
                .gap_6()
                .child(widgets::stepper(
                    t.font_size,
                    format!("{} px", hud.font_size),
                    btn("font-", "−", |h| h.step_font_size(false), cx)
                        .when(hud.font_size <= min_font, |d| d.opacity(0.4)),
                    btn("font+", "+", |h| h.step_font_size(true), cx)
                        .when(hud.font_size >= max_font, |d| d.opacity(0.4)),
                ))
                .child(widgets::stepper(
                    t.bg_opacity,
                    t.percent(hud.background_alpha),
                    btn("alpha-", "−", |h| h.step_alpha(false), cx),
                    btn("alpha+", "+", |h| h.step_alpha(true), cx),
                ))
                .child(widgets::stepper(
                    t.fps_limit,
                    fps,
                    btn("fps-", "−", |h| h.step_fps_limit(false), cx),
                    btn("fps+", "+", |h| h.step_fps_limit(true), cx),
                )),
        )
    }

    fn metrics(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        let mut card = widgets::section(t.metrics_section);
        for (group, metrics) in METRIC_GROUPS {
            let group = match group {
                GroupTitle::Fixed(name) => name,
                GroupTitle::Frame => t.group_frame,
                GroupTitle::Other => t.group_other,
            };
            let mut row = div().flex().flex_wrap().gap_2();
            for &m in metrics {
                row =
                    row.child(
                        widgets::chip(
                            ("metric", m as usize),
                            t.metric(m),
                            hud.metrics.contains(&m),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_hud(cx, |h| h.toggle_metric(m))
                        })),
                    );
            }
            card = card.child(
                div()
                    .flex()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .w(px(48.))
                            .pt_1()
                            .text_sm()
                            .text_color(rgb(TEXT))
                            .child(group),
                    )
                    .child(row.flex_1()),
            );
        }
        card
    }
}

/// A scaled-down mock of the game screen with the HUD where MangoHud will draw it.
/// Values are placeholders; the point is position, orientation, size and what is shown.
fn preview(hud: &HudSettings, t: &Strings) -> Div {
    use Metric::*;
    let has = |m: Metric| hud.metrics.contains(&m);
    let scale = hud.font_size as f32 * 0.42;

    // (label, label colour, values) — colours follow MangoHud's defaults.
    let mut rows: Vec<(&str, u32, Vec<&str>)> = Vec::new();
    let mut gpu = Vec::new();
    for (m, v) in [
        (GpuStats, "45%"),
        (GpuTemp, "62°C"),
        (GpuCoreClock, "2610MHz"),
        (GpuMemClock, "1250MHz"),
        (GpuPower, "220W"),
    ] {
        if has(m) {
            gpu.push(v);
        }
    }
    if !gpu.is_empty() {
        rows.push(("GPU", 0x2e9762, gpu));
    }
    if has(Vram) {
        rows.push(("VRAM", 0xad64c1, vec!["7.1GiB"]));
    }
    let mut cpu = Vec::new();
    for (m, v) in [
        (CpuStats, "23%"),
        (CpuTemp, "58°C"),
        (CpuMhz, "4850MHz"),
        (CpuPower, "65W"),
    ] {
        if has(m) {
            cpu.push(v);
        }
    }
    if !cpu.is_empty() {
        rows.push(("CPU", 0x2e97cb, cpu));
    }
    if has(Ram) {
        rows.push(("RAM", 0xc26693, vec!["11.4GiB"]));
    }
    if has(Battery) {
        rows.push(("BATT", 0x00ff00, vec!["87%"]));
    }
    let mut frame = Vec::new();
    if has(Fps) {
        frame.push("144 FPS");
    }
    if has(Frametime) {
        frame.push("6.9ms");
    }
    if !frame.is_empty() {
        rows.push(("", 0xffffff, frame));
    }
    for (m, l, v) in [
        (Resolution, "RES", "2560x1440"),
        (Wine, "WINE", "Proton 10"),
        (EngineVersion, "DXVK", "2.7"),
        (Time, "", "21:45"),
    ] {
        if has(m) {
            rows.push((l, 0xeb5b5b, vec![v]));
        }
    }

    let horizontal = hud.orientation == Orientation::Horizontal;
    let mut body = div()
        .flex()
        .gap(px(scale * 0.6))
        .when(!horizontal, |d| d.flex_col())
        .when(horizontal, |d| d.items_center());
    for (label, color, values) in rows {
        let mut line = div().flex().gap(px(scale * 0.5)).items_center();
        if !label.is_empty() {
            line = line.child(div().text_color(rgb(color)).child(label));
        }
        for v in values {
            line = line.child(div().text_color(rgb(0xffffff)).child(v));
        }
        body = body.child(line);
    }
    if has(FrameTiming) {
        body = body.child(frametime_graph(scale, horizontal));
    }

    let alpha = (hud.background_alpha * 255 / 100) & 0xff;
    let hud_box: AnyElement = if hud.metrics.is_empty() {
        div().into_any_element()
    } else {
        div()
            .p(px(scale * 0.4))
            .bg(rgba(alpha))
            .text_size(px(scale))
            .line_height(px(scale * 1.25))
            .child(body)
            .into_any_element()
    };

    let (row, col) = hud.position.cell();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(widgets::label(t.preview))
        .child(
            div()
                .flex()
                .flex_col()
                .w(px(384.))
                .h(px(216.))
                .p(px(6.))
                .rounded_md()
                .overflow_hidden()
                .bg(rgb(0x2a3b4f))
                .border_1()
                .border_color(rgb(BORDER))
                .map(|d| match row {
                    0 => d.justify_start(),
                    1 => d.justify_center(),
                    _ => d.justify_end(),
                })
                .map(|d| match col {
                    0 => d.items_start(),
                    1 => d.items_center(),
                    _ => d.items_end(),
                })
                .child(hud_box),
        )
}

fn frametime_graph(scale: f32, horizontal: bool) -> Div {
    const HEIGHTS: [f32; 24] = [
        0.3, 0.35, 0.3, 0.4, 0.3, 0.32, 0.9, 0.3, 0.28, 0.35, 0.3, 0.33, 0.3, 0.45, 0.3, 0.3, 0.31,
        0.6, 0.3, 0.29, 0.34, 0.3, 0.3, 0.36,
    ];
    let h = scale * 1.6;
    let mut graph =
        div()
            .flex()
            .items_end()
            .h(px(h))
            .w(px(if horizontal { scale * 5. } else { scale * 9. }));
    for f in HEIGHTS {
        graph = graph.child(div().flex_1().h(px(h * f)).bg(rgb(0x00ff00)));
    }
    graph
}
