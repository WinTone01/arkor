//! Small reusable building blocks. GPUI ships no widget set, so Arkor keeps its own here.
//! Widgets return elements without click handlers; callers attach `.on_click(cx.listener(..))`.

use gpui::{Div, ElementId, Hsla, SharedString, Stateful, div, prelude::*, px, rgb};

/// Colours. Functions rather than constants so the theme and accent can change at runtime.
#[allow(non_snake_case)]
pub mod theme {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    static LIGHT: AtomicBool = AtomicBool::new(false);
    static ACCENT_RGB: AtomicU32 = AtomicU32::new(DEFAULT_ACCENT);

    pub const DEFAULT_ACCENT: u32 = 0xff7a3d; // ember

    /// Accent choices offered in the settings: (colour, English name, Turkish name).
    pub const ACCENTS: [(u32, &str, &str); 6] = [
        (DEFAULT_ACCENT, "Ember", "Kor"),
        (0x4c8dff, "Blue", "Mavi"),
        (0x3ccf7a, "Green", "Yeşil"),
        (0xa77bff, "Purple", "Mor"),
        (0xff5fa2, "Pink", "Pembe"),
        (0x2ec4c4, "Teal", "Turkuaz"),
    ];

    pub fn set(light: bool, accent: u32) {
        LIGHT.store(light, Ordering::Relaxed);
        ACCENT_RGB.store(accent, Ordering::Relaxed);
    }

    pub fn is_light() -> bool {
        LIGHT.load(Ordering::Relaxed)
    }

    fn pick(dark: u32, light: u32) -> u32 {
        if is_light() { light } else { dark }
    }

    /// `a` mixed with `b`; `t` = share of `a`.
    fn mix(a: u32, b: u32, t: f32) -> u32 {
        let ch = |c: u32, s: u32| {
            (((c >> s) & 0xff) as f32 * t + ((b >> s) & 0xff) as f32 * (1. - t)).round() as u32
        };
        (ch(a, 16) << 16) | (ch(a, 8) << 8) | ch(a, 0)
    }

    pub fn BG() -> u32 {
        pick(0x14161a, 0xf3f4f6)
    }
    pub fn PANEL() -> u32 {
        pick(0x1c1f25, 0xe8eaee)
    }
    pub fn CARD() -> u32 {
        pick(0x1a1d23, 0xffffff)
    }
    pub fn ROW_HOVER() -> u32 {
        pick(0x252932, 0xdfe2e8)
    }
    pub fn SELECTED() -> u32 {
        pick(0x2d3340, 0xd4d9e2)
    }
    pub fn TEXT() -> u32 {
        pick(0xe6e8eb, 0x1d2026)
    }
    pub fn MUTED() -> u32 {
        pick(0x8a909c, 0x5c6370)
    }
    pub fn ACCENT() -> u32 {
        ACCENT_RGB.load(Ordering::Relaxed)
    }
    pub fn ACCENT_DIM() -> u32 {
        mix(ACCENT(), BG(), 0.25)
    }
    pub fn BORDER() -> u32 {
        pick(0x2a2e36, 0xcfd4dc)
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn mix_blends_channels() {
            assert_eq!(super::mix(0xff0000, 0x000000, 0.5), 0x800000);
            assert_eq!(super::mix(0x123456, 0xffffff, 1.0), 0x123456);
        }
    }
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
        .bg(rgb(CARD()))
        .border_1()
        .border_color(rgb(BORDER()))
        .child(div().text_sm().text_color(rgb(MUTED())).child(title.into()))
}

pub fn label(text: impl Into<SharedString>) -> Div {
    div().text_xs().text_color(rgb(MUTED())).child(text.into())
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
            d.bg(rgb(ACCENT_DIM()))
                .border_color(rgb(ACCENT()))
                .text_color(rgb(ACCENT()))
        })
        .when(!active, |d| {
            d.border_color(rgb(BORDER()))
                .text_color(rgb(TEXT()))
                .hover(|s| s.bg(rgb(ROW_HOVER())))
        })
        .child(text.into())
}

/// Filled accent button for the main action on a page (e.g. Play).
pub fn primary_button(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px_6()
        .py_2()
        .rounded_lg()
        .bg(rgb(ACCENT()))
        .text_color(rgb(BG()))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
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
        .border_color(rgb(if active { ACCENT() } else { BORDER() }))
        .text_color(rgb(if active { ACCENT() } else { MUTED() }))
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
        rgb(ACCENT()).into()
    } else {
        rgb(BORDER()).into()
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .p_3()
        .rounded_lg()
        .bg(rgb(BG()))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(ROW_HOVER())))
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().text_color(rgb(TEXT())).child(title))
                .child(div().text_xs().text_color(rgb(MUTED())).child(hint)),
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
                .child(div().size(px(16.)).rounded_full().bg(rgb(TEXT()))),
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
        .border_color(rgb(BORDER()))
        .text_color(rgb(TEXT()))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(ROW_HOVER())))
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
                    .text_color(rgb(TEXT()))
                    .child(value.into()),
            )
            .child(plus),
    )
}
