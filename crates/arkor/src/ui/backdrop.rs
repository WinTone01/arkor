//! Window backgrounds (from Modus). Everything is drawn with GPUI primitives: soft glows
//! are blurred box shadows, so no image assets are needed.

use arkor_core::profile::Backdrop as Background;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::f32::consts::TAU;

use super::style::pal;

/// Deterministic pseudo-random number in 0..1 for particle `i`.
pub(super) fn rnd(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297a_2d39);
    x ^= x >> 15;
    (x & 0xffff) as f32 / 65535.
}

fn hue_shift(c: Hsla, dh: f32) -> Hsla {
    Hsla {
        h: (c.h + dh).rem_euclid(1.),
        ..c
    }
}

/// A blurred ellipse centred at (`x`, `y`) given as fractions of the window.
pub(super) fn glow(x: f32, y: f32, w: f32, h: f32, blur: f32, color: Hsla) -> Div {
    div()
        .absolute()
        .left(relative(x))
        .top(relative(y))
        .size_0()
        .child(
            div()
                .absolute()
                .left(px(-w / 2.))
                .top(px(-h / 2.))
                .w(px(w))
                .h(px(h))
                .rounded_full()
                .shadow(vec![BoxShadow {
                    color,
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(blur),
                    spread_radius: px(0.),
                    inset: false,
                }]),
        )
}

/// A circle outline centred at (`x`, `y`).
fn ring(x: f32, y: f32, size: f32, color: Hsla) -> Div {
    div()
        .absolute()
        .left(relative(x))
        .top(relative(y))
        .size_0()
        .child(
            div()
                .absolute()
                .left(px(-size / 2.))
                .top(px(-size / 2.))
                .size(px(size))
                .rounded_full()
                .border_1()
                .border_color(color),
        )
}

fn layer() -> Div {
    div().absolute().inset_0().overflow_hidden()
}

/// The background layer for `kind`. With `still`, animated kinds stay in place.
/// `secs` is how long it has been moving.
pub fn render(
    kind: Background,
    still: bool,
    image: Option<&std::path::Path>,
    secs: f32,
) -> AnyElement {
    let turn = |period: f32| (secs / period).fract();
    let a = pal::accent();
    // Glows are softer on the light palette.
    let a = if pal::is_light() { a.opacity(0.45) } else { a };
    if still && kind.animated() {
        return layer()
            .child(glow(0.5, -0.08, 1100., 420., 260., a.opacity(0.75)))
            .into_any_element();
    }
    match kind {
        Background::Nebula => layer()
            .child(glow(0.5, -0.08, 1100., 420., 260., a.opacity(0.75)))
            .child(glow(0.62, 0.5, 700., 520., 320., a.opacity(0.08)))
            .into_any_element(),
        Background::Eclipse => layer()
            .child(glow(0.6, 0.46, 560., 560., 140., a.opacity(0.7)))
            .child(glow(
                0.6,
                0.46,
                520.,
                520.,
                40.,
                hue_shift(a, 0.04).opacity(0.5),
            ))
            .child(
                div()
                    .absolute()
                    .left(relative(0.6))
                    .top(relative(0.46))
                    .size_0()
                    .child(
                        div()
                            .absolute()
                            .left(px(-262.))
                            .top(px(-246.))
                            .size(px(524.))
                            .rounded_full()
                            .bg(pal::bg())
                            .shadow(vec![BoxShadow {
                                color: pal::bg(),
                                offset: point(px(0.), px(0.)),
                                blur_radius: px(12.),
                                spread_radius: px(0.),
                                inset: false,
                            }]),
                    ),
            )
            .into_any_element(),
        Background::Pulse => {
            let mut el = layer().child(glow(0.55, 0.55, 260., 260., 180., a.opacity(0.5)));
            for i in 0..9 {
                let size = 180. + i as f32 * 150.;
                el = el.child(ring(
                    0.55,
                    0.55,
                    size,
                    a.opacity((0.34 - i as f32 * 0.034).max(0.03)),
                ));
            }
            el.into_any_element()
        }
        Background::Grid => {
            let line = pal::text().opacity(0.035);
            let mut el = layer();
            for i in 0..48 {
                el = el.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(i as f32 * 56.))
                        .w(px(1.))
                        .bg(line),
                );
            }
            for i in 0..30 {
                el = el.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(i as f32 * 56.))
                        .h(px(1.))
                        .bg(line),
                );
            }
            el.child(glow(0.55, 0.5, 600., 420., 300., a.opacity(0.22)))
                // Vignette: fade the grid out towards the edges.
                .child(div().absolute().inset_0().shadow(vec![BoxShadow {
                    color: pal::bg(),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(260.),
                    spread_radius: px(120.),
                    inset: true,
                }]))
                .into_any_element()
        }
        Background::Aurora => layer()
            .map(|el| {
                let d = turn(28.);
                let p = d * TAU;
                el.child(glow(
                    0.5 + 0.28 * p.sin(),
                    0.3 + 0.14 * (2. * p).sin(),
                    760.,
                    420.,
                    260.,
                    a.opacity(0.8),
                ))
                .child(glow(
                    0.45 - 0.3 * (p + 1.9).sin(),
                    0.62 + 0.12 * (p + 0.7).cos(),
                    640.,
                    380.,
                    260.,
                    hue_shift(a, 0.12).opacity(0.55),
                ))
                .child(glow(
                    0.6 + 0.22 * (p + 4.).cos(),
                    0.45 + 0.2 * (3. * p).sin() * 0.5,
                    520.,
                    520.,
                    280.,
                    hue_shift(a, -0.08).opacity(0.4),
                ))
            })
            .into_any_element(),
        Background::Tide => layer()
            .map(|el| {
                let d = turn(20.);
                let p = d * TAU;
                el.child(glow(
                    0.5 + 0.06 * p.cos(),
                    0.28 + 0.09 * p.sin(),
                    1500.,
                    200.,
                    180.,
                    a.opacity(0.65),
                ))
                .child(glow(
                    0.5 - 0.06 * p.cos(),
                    0.74 - 0.09 * p.sin(),
                    1500.,
                    180.,
                    180.,
                    hue_shift(a, 0.1).opacity(0.5),
                ))
            })
            .into_any_element(),
        Background::Drift => layer()
            .child(glow(0.5, 1.05, 1200., 300., 260., a.opacity(0.35)))
            .map(|mut el| {
                let d = turn(36.);
                for i in 0..22u32 {
                    let size = 10. + 34. * rnd(i, 1);
                    let laps = 1. + (rnd(i, 2) * 2.).floor();
                    let y = 1.1 - ((rnd(i, 3) + d * laps).fract() * 1.25);
                    let x = rnd(i, 4) + 0.03 * ((d * 3. + rnd(i, 5)) * TAU).sin();
                    let color = if i % 3 == 0 { hue_shift(a, 0.1) } else { a };
                    el = el.child(glow(
                        x,
                        y,
                        size,
                        size,
                        size * 0.9,
                        color.opacity(0.25 + 0.3 * rnd(i, 6)),
                    ));
                }
                el
            })
            .into_any_element(),
        Background::Plain => layer().into_any_element(),
        Background::Image => match image {
            Some(path) => layer()
                .child(
                    img(path.to_path_buf())
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
                // Darken (or lighten) it a little so text on the panels stays readable.
                .child(div().absolute().inset_0().bg(pal::bg().opacity(0.35)))
                .into_any_element(),
            None => layer()
                .child(glow(0.5, -0.08, 1100., 420., 260., a.opacity(0.75)))
                .into_any_element(),
        },
    }
}
