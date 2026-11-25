//! The loading animation: a comet of light circling the ember of Arkor's mark, as in the
//! start-up animation. Used wherever something is being fetched or looked up.

use std::f32::consts::TAU;
use std::sync::LazyLock;
use std::time::Instant;

use gpui::{
    AnyElement, App, Bounds, Div, FontWeight, Pixels, SharedString, Window, canvas, div, fill,
    point, prelude::*, px, size,
};

use super::style::{self, pal};

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Dots of the comet's tail.
const TAIL: usize = 16;

fn paint_comet(bounds: Bounds<Pixels>, still: bool, window: &mut Window) {
    let side = f32::from(bounds.size.width).min(f32::from(bounds.size.height));
    let (cx, cy) = (
        f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.,
        f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.,
    );
    let t = if still {
        0.3
    } else {
        START.elapsed().as_secs_f32()
    };
    let r = side * 0.38;
    let dot = (side * 0.085).max(2.);
    let dot_at = |x: f32, y: f32, d: f32| {
        Bounds::new(point(px(x - d / 2.), px(y - d / 2.)), size(px(d), px(d)))
    };
    // The faint track.
    for i in 0..48 {
        let a = i as f32 * TAU / 48.;
        window.paint_quad(
            fill(
                dot_at(cx + a.cos() * r, cy + a.sin() * r, dot * 0.32),
                pal::text().opacity(0.1),
            )
            .corner_radii(px(dot)),
        );
    }
    // Speeds up and slows down once a turn, like something thrown.
    let turn = t * 0.95;
    let head = TAU * (turn + 0.12 * (TAU * turn).sin());
    for i in 0..TAIL {
        let k = i as f32 / TAIL as f32;
        let a = head - k * 1.9;
        let d = dot * (1. - k * 0.75);
        let color = if i == 0 {
            pal::accent_light()
        } else {
            pal::accent()
        };
        window.paint_quad(
            fill(
                dot_at(cx + a.cos() * r, cy + a.sin() * r, d),
                color.opacity((1. - k).powf(1.6)),
            )
            .corner_radii(px(d)),
        );
    }
    // A glow around the head.
    let g = dot * 2.6;
    window.paint_quad(
        fill(
            dot_at(cx + head.cos() * r, cy + head.sin() * r, g),
            pal::accent_light().opacity(0.18),
        )
        .corner_radii(px(g)),
    );
    if !still {
        window.request_animation_frame();
    }
}

/// Just the circling light, `size` pixels wide.
pub(super) fn spinner(size: f32, still: bool) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window: &mut Window, _: &mut App| paint_comet(bounds, still, window),
    )
    .size(px(size))
    .flex_none()
    .into_any_element()
}

/// The large one: the mark in the middle, the light around it, and what is happening.
pub(super) fn block(text: impl Into<SharedString>, detail: Option<String>, still: bool) -> Div {
    let ring = 112.;
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .py(px(56.))
        .child(
            div()
                .relative()
                .size(px(ring))
                .flex()
                .items_center()
                .justify_center()
                .child(div().absolute().inset_0().child(spinner(ring, still)))
                .child(style::logo(40.)),
        )
        .child(
            div()
                .text_base()
                .font_weight(FontWeight::MEDIUM)
                .text_color(pal::text_soft())
                .child(text.into()),
        )
        .children(detail.map(|d| div().text_sm().text_color(pal::muted()).child(d)))
}

/// A small one in a line of text.
pub(super) fn inline(text: impl Into<SharedString>, still: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_sm()
        .text_color(pal::muted())
        .child(spinner(18., still))
        .child(text.into())
}
