//! Small reusable building blocks. GPUI ships no widget set, so Arkor keeps its own here.
//! Widgets return elements without click handlers; callers attach `.on_click(cx.listener(..))`.

use gpui::{Div, ElementId, Hsla, SharedString, Stateful, div, prelude::*, px, rgb};

pub mod theme {
    pub const BG: u32 = 0x14161a;
    pub const PANEL: u32 = 0x1c1f25;
    pub const CARD: u32 = 0x1a1d23;
    pub const ROW_HOVER: u32 = 0x252932;
    pub const SELECTED: u32 = 0x2d3340;
    pub const TEXT: u32 = 0xe6e8eb;
    pub const MUTED: u32 = 0x8a909c;
    pub const ACCENT: u32 = 0xff7a3d; // ember
    pub const ACCENT_DIM: u32 = 0x4a2a1c;
    pub const BORDER: u32 = 0x2a2e36;
}

use theme::*;

/// A titled card grouping related settings.
pub fn section(title: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_lg()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgb(BORDER))
        .child(div().text_sm().text_color(rgb(MUTED)).child(title.into()))
}

pub fn label(text: impl Into<SharedString>) -> Div {
    div().text_xs().text_color(rgb(MUTED)).child(text.into())
}

/// Pill button; `active` renders it highlighted. Used for toggles and single-choice groups.
pub fn chip(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_full()
        .text_sm()
        .cursor_pointer()
        .border_1()
        .when(active, |d| {
            d.bg(rgb(ACCENT_DIM))
                .border_color(rgb(ACCENT))
                .text_color(rgb(ACCENT))
        })
        .when(!active, |d| {
            d.border_color(rgb(BORDER))
                .text_color(rgb(TEXT))
                .hover(|s| s.bg(rgb(ROW_HOVER)))
        })
        .child(text.into())
}

/// Non-clickable chip for state that cannot be chosen directly (e.g. "Custom" preset).
pub fn badge(text: impl Into<SharedString>, active: bool) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded_full()
        .text_sm()
        .border_1()
        .border_dashed()
        .border_color(rgb(if active { ACCENT } else { BORDER }))
        .text_color(rgb(if active { ACCENT } else { MUTED }))
        .child(text.into())
}

/// Full-width row with a label, hint and an on/off switch.
pub fn switch_row(
    id: impl Into<ElementId>,
    title: &'static str,
    hint: &'static str,
    on: bool,
) -> Stateful<Div> {
    let track: Hsla = if on {
        rgb(ACCENT).into()
    } else {
        rgb(BORDER).into()
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .p_3()
        .rounded_lg()
        .bg(rgb(BG))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(ROW_HOVER)))
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().text_color(rgb(TEXT)).child(title))
                .child(div().text_xs().text_color(rgb(MUTED)).child(hint)),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .w(px(40.))
                .h(px(22.))
                .p(px(3.))
                .rounded_full()
                .bg(track)
                .when(on, |d| d.justify_end())
                .child(div().size(px(16.)).rounded_full().bg(rgb(TEXT))),
        )
}

/// Square `-` / `+` button for steppers.
pub fn step_button(id: impl Into<ElementId>, glyph: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.))
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .text_color(rgb(TEXT))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(ROW_HOVER)))
        .child(glyph)
}

/// `label  [-] value [+]` — the caller passes the two buttons with handlers attached.
pub fn stepper(
    title: &'static str,
    value: impl Into<SharedString>,
    minus: impl IntoElement,
    plus: impl IntoElement,
) -> Div {
    div().flex().flex_col().gap_1().child(label(title)).child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(minus)
            .child(
                div()
                    .min_w(px(64.))
                    .flex()
                    .justify_center()
                    .text_color(rgb(TEXT))
                    .child(value.into()),
            )
            .child(plus),
    )
}
