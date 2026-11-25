//! Small reusable building blocks. GPUI ships no widget set, so Arkor keeps its own here.
//! Widgets return elements without click handlers; callers attach `.on_click(cx.listener(..))`.

use gpui::prelude::FluentBuilder;
use gpui::{Div, ElementId, SharedString, Stateful, div, prelude::*, px};

/// Colour names used across the older pages, mapped onto the style palette.
#[allow(non_snake_case)]
pub mod theme {
    use super::super::style::pal;
    use gpui::Hsla;

    pub fn BG() -> Hsla {
        pal::bg()
    }
    pub fn PANEL() -> Hsla {
        pal::panel()
    }
    pub fn ROW_HOVER() -> Hsla {
        pal::raised().opacity(0.6)
    }
    pub fn TEXT() -> Hsla {
        pal::text()
    }
    pub fn MUTED() -> Hsla {
        pal::muted()
    }
    pub fn ACCENT() -> Hsla {
        pal::accent_light()
    }
    pub fn BORDER() -> Hsla {
        pal::glass_spec().border
    }
}

use theme::*;

/// A titled card grouping related settings.
pub fn section(title: impl Into<SharedString>) -> Div {
    super::style::card().flex().flex_col().gap_3().p_5().child(
        div()
            .text_base()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(TEXT())
            .child(title.into()),
    )
}

pub fn label(text: impl Into<SharedString>) -> Div {
    div().text_sm().text_color(MUTED()).child(text.into())
}

/// Pill button; `active` renders it highlighted. Used for toggles and single-choice groups.
pub fn chip(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    use super::style::pal;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1p5()
        .px_3()
        .py_1()
        .rounded_full()
        .text_sm()
        .cursor_pointer()
        .border_1()
        .when(active, |d| {
            d.bg(pal::accent().opacity(0.16))
                .border_color(pal::accent().opacity(0.6))
                .text_color(pal::accent_light())
        })
        .when(!active, |d| {
            d.bg(pal::raised().opacity(0.5))
                .border_color(pal::border_strong())
                .text_color(pal::text_soft())
                .hover(|s| {
                    s.bg(pal::raised())
                        .border_color(pal::accent().opacity(0.45))
                })
        })
        .child(text.into())
}

/// Filled accent button for the main action on a page (e.g. Play).
pub fn primary_button(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    use super::style::{self, pal};
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap_2()
        .px_6()
        .py_2()
        .rounded(px(12.))
        .bg(style::accent_gradient())
        .text_color(pal::on_accent())
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
        .border_color(if active { ACCENT() } else { BORDER() })
        .text_color(if active { ACCENT() } else { MUTED() })
        .child(text.into())
}

/// An on/off switch track.
pub fn switch(on: bool) -> Div {
    use super::style::pal;
    div()
        .flex()
        .flex_none()
        .w(px(38.))
        .h(px(22.))
        .p(px(3.))
        .rounded_full()
        .bg(if on {
            pal::accent()
        } else {
            pal::border_strong()
        })
        .when(on, |d| d.justify_end())
        .child(div().size(px(16.)).rounded_full().bg(if on {
            pal::on_accent()
        } else {
            pal::text_soft()
        }))
}

/// Full-width row with a label, hint and an on/off switch.
pub fn switch_row(
    id: impl Into<ElementId>,
    title: &'static str,
    hint: &'static str,
    on: bool,
) -> Stateful<Div> {
    use super::style::pal;
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_4()
        .py_3()
        .rounded(px(12.))
        .bg(pal::raised().opacity(0.45))
        .border_1()
        .border_color(if on {
            pal::accent().opacity(0.35)
        } else {
            pal::glass_spec().border
        })
        .cursor_pointer()
        .hover(|s| s.bg(pal::raised().opacity(0.8)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .child(
                    div()
                        .text_color(TEXT())
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(title),
                )
                .child(div().text_sm().text_color(MUTED()).child(hint)),
        )
        .child(switch(on))
}

/// Square `-` / `+` button for steppers.
pub fn step_button(id: impl Into<ElementId>, glyph: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(px(8.))
        .border_1()
        .border_color(super::style::pal::border_strong())
        .bg(super::style::pal::raised().opacity(0.5))
        .text_color(TEXT())
        .cursor_pointer()
        .hover(|s| s.bg(ROW_HOVER()))
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
                    .text_color(TEXT())
                    .child(value.into()),
            )
            .child(plus),
    )
}

/// A settings row: name and explanation on the left, the control on the right.
/// Rows inside one `section` read as a list, the way desktop settings do.
pub fn setting_row(
    title: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    let hint: SharedString = hint.into();
    let title: SharedString = title.into();
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_x_6()
        .gap_y_2()
        .py_1()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w(px(220.))
                .flex_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_color(TEXT())
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(title.clone())
                        .when(is_explanation(&hint), |d| {
                            d.child(hint_icon(format!("hint-{title}"), hint.clone()))
                        }),
                )
                .when(!hint.is_empty() && !is_explanation(&hint), |d| {
                    d.child(div().text_sm().text_color(MUTED()).truncate().child(hint))
                }),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(control),
        )
}

/// Whether a row's second line explains the setting (shown on hover) rather than holding a
/// value such as a path or a version (shown under the title).
pub fn is_explanation(text: &str) -> bool {
    text.chars().count() > 44 && text.contains(' ') && !text.starts_with('/')
}

/// A small "i" that shows `text` on hover.
pub fn hint_icon(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text: SharedString = text.into();
    div()
        .id(id)
        .flex_none()
        .child(super::style::icon("info", 13., super::style::pal::faint()))
        .tooltip(move |_, cx| tip(text.clone(), cx))
}

/// What hovering something shows.
pub fn tip(text: SharedString, cx: &mut gpui::App) -> gpui::AnyView {
    cx.new(|_| HintTip(text)).into()
}

struct HintTip(SharedString);

impl gpui::Render for HintTip {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        use super::style::pal;
        div().pl_2().pt_2().child(
            div()
                .max_w(px(360.))
                .px_3()
                .py_2()
                .rounded(px(10.))
                .bg(pal::raised())
                .border_1()
                .border_color(pal::border_strong())
                .shadow_lg()
                .text_sm()
                .text_color(pal::text_soft())
                .child(self.0.clone()),
        )
    }
}

/// A hairline between rows of a section.
pub fn divider() -> Div {
    div().h(px(1.)).w_full().bg(BORDER())
}

/// A small heading inside a section, e.g. "GPU" above its metrics.
pub fn subheading(text: impl Into<SharedString>) -> Div {
    div()
        .pt_1()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(super::style::pal::faint())
        .child(text.into().to_uppercase())
}

/// A check box square.
pub fn checkbox(on: bool) -> Div {
    use super::style::{self, pal};
    div()
        .size(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .border_1()
        .border_color(if on {
            pal::accent()
        } else {
            pal::border_strong()
        })
        .bg(if on {
            pal::accent()
        } else {
            pal::raised().opacity(0.5)
        })
        .when(on, |d| d.child(style::icon("check", 13., pal::on_accent())))
}

/// A check box with its label, for picking several things from a list (fixed width, so a
/// list of them lines up in columns).
pub fn check_item(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    on: bool,
) -> Stateful<Div> {
    use super::style::pal;
    div()
        .id(id)
        .w(px(200.))
        .flex()
        .items_center()
        .gap_2p5()
        .px_2()
        .py_1p5()
        .rounded(px(8.))
        .cursor_pointer()
        .text_sm()
        .text_color(if on { TEXT() } else { pal::text_soft() })
        .hover(|s| s.bg(ROW_HOVER()))
        .child(checkbox(on))
        .child(div().min_w_0().truncate().child(text.into()))
}

/// A choice card with a title and a sentence, for picking one of a few modes.
pub fn choice_card(
    id: impl Into<ElementId>,
    icon: &'static str,
    title: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    use super::style::{self, pal};
    div()
        .id(id)
        .flex_1()
        .min_w(px(180.))
        .flex()
        .gap_3()
        .p_3()
        .rounded(px(12.))
        .border_1()
        .cursor_pointer()
        .border_color(if active {
            pal::accent()
        } else {
            pal::glass_spec().border
        })
        .bg(if active {
            pal::accent().opacity(0.1)
        } else {
            pal::raised().opacity(0.35)
        })
        .hover(|s| s.border_color(pal::accent().opacity(0.6)))
        .child(style::icon(
            icon,
            18.,
            if active {
                pal::accent_light()
            } else {
                pal::muted()
            },
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if active { pal::accent_light() } else { TEXT() })
                        .child(title.into()),
                )
                .child(div().text_xs().text_color(MUTED()).child(hint.into())),
        )
}

/// `[-] value [+]` without a title (for `setting_row`).
pub fn stepper_control(
    value: impl Into<SharedString>,
    minus: impl IntoElement,
    plus: impl IntoElement,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(minus)
        .child(
            div()
                .min_w(px(72.))
                .flex()
                .justify_center()
                .text_color(TEXT())
                .child(value.into()),
        )
        .child(plus)
}
