//! Console mode's start-up animation. Embers are drawn together into a spark, the arch of the
//! logo is struck in light around it, and the camera dives through the arch into a tunnel of
//! arches with the covers of the user's own games (and a gamepad's buttons) rushing past. At
//! its end one last arch comes to rest and its core ignites; two lights run around it and
//! draw a gamepad, its sticks and buttons light up one after the other, the name draws
//! together, and the logo flies to its place in the console while the screen opens under it.
//!
//! The first time it plays in full; coming back to console mode plays its end only. A button,
//! key or click skips it. Its music (`synth`) follows the same times.

use std::f32::consts::{PI, TAU};
use std::path::PathBuf;
use std::time::Instant;

use gpui::{
    AnyElement, Bounds, Context, Hsla, PathBuilder, Pixels, Window, canvas, div, fill, point,
    prelude::*, px, size,
};

use super::backdrop::rnd;
use super::console_sound::{self, Cue};
use super::style::{self, pal};
use super::{Library, cover_art};

/// The spark catches and the arch is struck.
pub(super) const IGNITE: f32 = 0.95;
/// The camera starts to move.
pub(super) const DIVE: f32 = 1.5;
/// The last arch comes to rest.
pub(super) const ARRIVE: f32 = 3.2;
/// The logo takes off and the console opens.
const REVEAL: f32 = 5.05;
/// From then to everything being in place.
const OPENING: f32 = 0.95;
/// Where the short animation starts.
pub(super) const SHORT_FROM: f32 = 2.2;
/// Its reveal comes sooner after the arrival.
const SHORT_REVEAL: f32 = ARRIVE + 1.25;
/// Two lights draw the gamepad.
pub(super) const TRACE: f32 = ARRIVE + 0.12;
pub(super) const TRACE_SECS: f32 = 0.8;
/// Its parts light up, one after the other.
pub(super) const PARTS: f32 = TRACE + 0.42;
pub(super) const PART_STEP: f32 = 0.07;

/// How deep the tunnel is, in arches.
const DEEP: f32 = 12.;
/// The distance from which an arch is drawn at its own size.
const EYE: f32 = 1.;
/// Covers in the tunnel at most.
const MAX_CARDS: usize = 18;
/// The tunnel's arches, beside the logo's size, and how far they are from each other.
const GATE: f32 = 2.7;
const STEP: f32 = 0.62;

pub(super) struct ConsoleIntro {
    clock: f32,
    last: Option<Instant>,
    waited: f32,
    short: bool,
    /// The clock when the console began to open.
    reveal: Option<f32>,
    sounded: bool,
    cards: Vec<PathBuf>,
}

fn clamp01(x: f32) -> f32 {
    x.clamp(0., 1.)
}
fn span(t: f32, from: f32, secs: f32) -> f32 {
    clamp01((t - from) / secs)
}
fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}
fn out_cubic(x: f32) -> f32 {
    1. - (1. - x).powi(3)
}
fn out_expo(x: f32) -> f32 {
    if x >= 1. { 1. } else { 1. - (-9. * x).exp2() }
}
fn in_out(x: f32) -> f32 {
    x * x * x * (x * (x * 6. - 15.) + 10.)
}
fn out_back(x: f32) -> f32 {
    let c = 1.6;
    1. + (c + 1.) * (x - 1.).powi(3) + c * (x - 1.).powi(2)
}

/// Where the camera is in the tunnel: a small step back, then off, and a soft landing.
fn camera(t: f32) -> f32 {
    let back = 0.16 * (PI * span(t, DIVE - 0.25, 0.5)).sin();
    DEEP * in_out(span(t, DIVE, ARRIVE - DIVE)) - back
}

/// How far the mark's drawing has come, from the feet up.
fn struck(t: f32) -> f32 {
    in_out(span(t, IGNITE, 0.55))
}

/// What the stage looks like at one moment.
#[derive(Clone, Copy)]
struct Stage {
    t: f32,
    /// Seconds since the console began to open.
    r: f32,
    leaving: bool,
    w: f32,
    h: f32,
    /// The size of an arch at its own distance.
    mark: f32,
    /// The middle: where the tunnel ends, and the core of the logo.
    cx: f32,
    cy: f32,
}

impl Stage {
    fn new(t: f32, reveal: Option<f32>, w: f32, h: f32) -> Self {
        let mark = (h * 0.25).clamp(140., 420.);
        Stage {
            t,
            r: reveal.map_or(0., |s| t - s),
            leaving: reveal.is_some(),
            w,
            h,
            mark,
            cx: w / 2.,
            cy: h * 0.47,
        }
    }

    /// Something `off` the middle at depth `z` as seen by the camera: where, and how large.
    fn project(&self, z: f32, off: (f32, f32)) -> Option<(f32, f32, f32)> {
        let d = z - camera(self.t);
        if d < 0.03 {
            return None;
        }
        let k = EYE / d;
        Some((self.cx + off.0 * k, self.cy + off.1 * k, k))
    }

    /// Far things are lost in the dark, and what comes very near in the light.
    fn fog(&self, k: f32) -> f32 {
        let d = EYE / k;
        (1. - (d / 7.5)).clamp(0., 1.).powf(1.4) * span(d, 0.04, 0.3)
    }
}

/// The arch with the middle of its core at (x, y): `grow` from its feet to its tip, `thin` 1
/// (the logo) … 0.2 (a line).
fn arch(
    (x, y): (f32, f32),
    s: f32,
    grow: f32,
    thin: f32,
    color: (Hsla, Hsla),
    window: &mut Window,
) {
    // The mark's own coordinates, its core (128, 166) in the middle.
    let at = |mx: f32, my: f32| {
        point(
            px(x + (mx - 128.) / 256. * s),
            px(y + (my - 166.) / 256. * s),
        )
    };
    let foot = lerp(58., 92., thin);
    let tip = lerp(46., 120., thin);
    let mut p = PathBuilder::fill();
    if grow >= 1. {
        // In one piece: no seam at its tip.
        p.move_to(at(128., 46.));
        p.line_to(at(198., 204.));
        p.line_to(at(256. - foot, 204.));
        p.line_to(at(128., tip));
        p.line_to(at(foot, 204.));
        p.line_to(at(58., 204.));
        p.close();
    } else {
        for side in [1., -1.] {
            let sx = |v: f32| 128. + (v - 128.) * side;
            p.move_to(at(sx(58.), 204.));
            p.line_to(at(sx(foot), 204.));
            p.line_to(at(sx(lerp(foot, 128., grow)), lerp(204., tip, grow)));
            p.line_to(at(sx(lerp(58., 128., grow)), lerp(204., 46., grow)));
            p.close();
        }
    }
    if let Ok(path) = p.build() {
        window.paint_path(
            path,
            gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(color.0, 0.),
                gpui::linear_color_stop(color.1, 1.),
            ),
        );
    }
}

fn core(x: f32, y: f32, s: f32, grow: f32, color: Hsla, window: &mut Window) {
    let k = s / 256. * grow;
    let mut p = PathBuilder::fill();
    p.move_to(point(px(x), px(y - 19. * k)));
    p.line_to(point(px(x + 17. * k), px(y)));
    p.line_to(point(px(x), px(y + 19. * k)));
    p.line_to(point(px(x - 17. * k), px(y)));
    p.close();
    if let Ok(path) = p.build() {
        window.paint_path(path, color);
    }
}

fn disc(x: f32, y: f32, d: f32, color: Hsla, window: &mut Window) {
    window.paint_quad(
        fill(
            Bounds::new(point(px(x - d / 2.), px(y - d / 2.)), size(px(d), px(d))),
            color,
        )
        .corner_radii(px(d / 2.)),
    );
}

/// A line of light along (dx, dy), `len` long and `wide` thick, added to `p`.
fn streak(p: &mut PathBuilder, (x, y): (f32, f32), (x2, y2): (f32, f32), wide: f32) {
    let (dx, dy) = (x2 - x, y2 - y);
    let len = dx.hypot(dy).max(0.001);
    let (nx, ny) = (-dy / len * wide / 2., dx / len * wide / 2.);
    // Pointed at its tail.
    p.move_to(point(px(x), px(y)));
    p.line_to(point(px(x2 + nx), px(y2 + ny)));
    p.line_to(point(px(x2 - nx), px(y2 - ny)));
    p.close();
}

/// Embers drawn together, in three shades.
fn embers(st: Stage, window: &mut Window) {
    let t = st.t;
    if t >= IGNITE + 0.1 {
        return;
    }
    let reach = st.w.hypot(st.h) * 0.5;
    for shade in 0..3u32 {
        let mut p = PathBuilder::fill();
        let mut any = false;
        for i in (shade..150).step_by(3) {
            let from = rnd(i, 31) * 0.45;
            let k = span(t, from, IGNITE - from);
            if k <= 0. || k >= 1. {
                continue;
            }
            let place = |k: f32| {
                let pull = k * k * (3. - 2. * k);
                let d = reach * (0.25 + 0.75 * rnd(i, 32)) * (1. - pull).powf(1.5);
                let a =
                    rnd(i, 33) * TAU + 2.4 * pull * pull + 0.25 * (t * 2. + rnd(i, 34) * 9.).sin();
                (st.cx + a.cos() * d, st.cy + a.sin() * d * 0.82)
            };
            let tail = place((k - 0.035 - 0.04 * k).max(0.));
            let size = st.h / 1080. * (1.4 + 2.6 * rnd(i, 35)) * (1. - 0.4 * k);
            streak(&mut p, tail, place(k), size);
            any = true;
        }
        if any && let Ok(path) = p.build() {
            let rise = span(t, 0.05, 0.4);
            let color = match shade {
                0 => gpui::white().opacity(0.85 * rise),
                1 => pal::accent_light().opacity(0.8 * rise),
                _ => pal::accent().opacity(0.9 * rise),
            };
            window.paint_path(path, color);
        }
    }
    // The spark they make.
    let lit = span(t, 0.2, IGNITE - 0.2);
    let d = st.mark * 0.055 * lit * (1. + 0.16 * (t * 31.).sin());
    disc(
        st.cx,
        st.cy,
        d * 3.4,
        pal::accent().opacity(0.16 * lit),
        window,
    );
    disc(
        st.cx,
        st.cy,
        d * 2.,
        pal::accent_light().opacity(0.35 * lit),
        window,
    );
    disc(st.cx, st.cy, d, gpui::white().opacity(lit), window);
}

/// Light rushing past in the tunnel.
fn speed(st: Stage, window: &mut Window) {
    let t = st.t;
    let v = (camera(t + 0.012) - camera(t - 0.012)) / 0.024;
    if t < DIVE || v < 0.15 {
        return;
    }
    let stretch = (v * 0.035).min(0.9);
    for shade in 0..2u32 {
        let mut p = PathBuilder::fill();
        let mut any = false;
        for i in (shade..120).step_by(2) {
            let z = 0.6 + rnd(i, 41) * (DEEP + 4.);
            let a = rnd(i, 42) * TAU;
            let d = st.mark * (0.55 + 2.4 * rnd(i, 43));
            let off = (a.cos() * d, a.sin() * d);
            let (Some(near), Some(far)) = (st.project(z, off), st.project(z + stretch, off)) else {
                continue;
            };
            if st.fog(near.2) < 0.06 {
                continue;
            }
            let wide = (st.h / 1080. * 1.5 * near.2.sqrt()).min(st.h * 0.008);
            streak(&mut p, (far.0, far.1), (near.0, near.1), wide);
            any = true;
        }
        if any && let Ok(path) = p.build() {
            let a = (v / 6.).min(1.) * 0.6;
            let color = if shade == 0 {
                gpui::white().opacity(a * 0.8)
            } else {
                pal::accent_light().opacity(a)
            };
            window.paint_path(path, color);
        }
    }
}

/// A part of a gamepad, floating by: a ring, a cross, four buttons, a stick.
fn button(i: u32, (x, y): (f32, f32), r: f32, a: f32, window: &mut Window) {
    let light = pal::accent_light().opacity(a);
    match i % 4 {
        0 => {
            ring(x, y, r, r * 0.14, light, window);
            disc(x, y, r * 0.5, gpui::white().opacity(0.8 * a), window);
        }
        1 => cross(x, y, r, light, window),
        2 => {
            for (dx, dy) in [(0., -1.), (1., 0.), (0., 1.), (-1., 0.)] {
                ring(
                    x + dx * r * 0.66,
                    y + dy * r * 0.66,
                    r * 0.36,
                    r * 0.09,
                    light,
                    window,
                );
            }
        }
        _ => {
            ring(x, y, r, r * 0.1, light, window);
            ring(
                x,
                y,
                r * 0.64,
                r * 0.07,
                gpui::white().opacity(0.6 * a),
                window,
            );
        }
    }
}

fn ring(x: f32, y: f32, r: f32, wide: f32, color: Hsla, window: &mut Window) {
    window.paint_quad(gpui::quad(
        Bounds::new(point(px(x - r), px(y - r)), size(px(r * 2.), px(r * 2.))),
        px(r),
        gpui::transparent_black(),
        px(wide.max(1.)),
        color,
        gpui::BorderStyle::Solid,
    ));
}

/// The direction cross.
fn cross(x: f32, y: f32, r: f32, color: Hsla, window: &mut Window) {
    let w = r * 0.34;
    let mut p = PathBuilder::stroke(px((r * 0.13).max(1.)));
    let at = |dx: f32, dy: f32| point(px(x + dx), px(y + dy));
    p.move_to(at(-w, -r));
    for (dx, dy) in [
        (w, -r),
        (w, -w),
        (r, -w),
        (r, w),
        (w, w),
        (w, r),
        (-w, r),
        (-w, w),
        (-r, w),
        (-r, -w),
        (-w, -w),
    ] {
        p.line_to(at(dx, dy));
    }
    p.close();
    if let Ok(path) = p.build() {
        window.paint_path(path, color);
    }
}

/// The right half of a gamepad's outline around the logo, from the middle of its lower edge
/// around the grip to the middle of its top (in marks, the core of the logo in the middle).
const OUTLINE: [(f32, f32); 17] = [
    (0., 0.5),
    (0.42, 0.5),
    (0.78, 0.54),
    (1.0, 0.72),
    (1.2, 1.0),
    (1.45, 1.17),
    (1.72, 1.1),
    (1.86, 0.8),
    (1.83, 0.4),
    (1.72, -0.06),
    (1.57, -0.48),
    (1.36, -0.76),
    (1.08, -0.86),
    (0.8, -0.82),
    (0.6, -0.72),
    (0.3, -0.7),
    (0., -0.7),
];

/// The outline as a smooth line, and how long it is up to each of its points.
fn outline() -> (Vec<(f32, f32)>, Vec<f32>) {
    let n = OUTLINE.len() as isize;
    // Mirrored beyond its ends, so it goes on smoothly on the other side.
    let at = |i: isize| -> (f32, f32) {
        if i < 0 {
            let (x, y) = OUTLINE[(-i) as usize];
            (-x, y)
        } else if i >= n {
            let (x, y) = OUTLINE[(2 * (n - 1) - i) as usize];
            (-x, y)
        } else {
            OUTLINE[i as usize]
        }
    };
    let mut points = Vec::new();
    for i in 0..n - 1 {
        let (a, b, c, d) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for step in 0..8 {
            let t = step as f32 / 8.;
            let f = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2. * b
                    + (c - a) * t
                    + (2. * a - 5. * b + 4. * c - d) * t * t
                    + (3. * b - a - 3. * c + d) * t * t * t)
            };
            points.push((f(a.0, b.0, c.0, d.0), f(a.1, b.1, c.1, d.1)));
        }
    }
    points.push(OUTLINE[OUTLINE.len() - 1]);
    let mut length = vec![0f32];
    for w in points.windows(2) {
        let last = length[length.len() - 1];
        length.push(last + (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1));
    }
    (points, length)
}

/// The gamepad of light around the logo.
fn gamepad(st: Stage, words: f32, window: &mut Window) {
    let t = st.t;
    let k = in_out(span(t, TRACE, TRACE_SECS));
    if k <= 0. || words <= 0.01 {
        return;
    }
    // It opens outwards when the console comes.
    let m = st.mark * (1. + 0.35 * out_cubic(span(st.r, 0., 0.5)));
    let place = |(x, y): (f32, f32)| (st.cx + x * m, st.cy + y * m);
    let (points, length) = outline();
    let reach = length[length.len() - 1] * k;
    let wide = (st.h / 1080. * 3.).max(1.2);
    let done = span(t, TRACE + TRACE_SECS, 0.4);
    let breath = 0.9 + 0.1 * (t * 3.).sin();
    for side in [1., -1.] {
        let mut seen: Vec<(f32, f32)> = Vec::new();
        for (i, p) in points.iter().enumerate() {
            if length[i] <= reach {
                seen.push(place((p.0 * side, p.1)));
            } else {
                // The end of the light, between two points.
                let (a, b) = (points[i - 1], *p);
                let f = (reach - length[i - 1]) / (length[i] - length[i - 1]).max(1e-4);
                seen.push(place((lerp(a.0, b.0, f) * side, lerp(a.1, b.1, f))));
                break;
            }
        }
        if seen.len() < 2 {
            continue;
        }
        let line = |from: usize, w: f32, color: Hsla, window: &mut Window| {
            let mut p = PathBuilder::stroke(px(w));
            p.move_to(point(px(seen[from].0), px(seen[from].1)));
            for (x, y) in &seen[from + 1..] {
                p.line_to(point(px(*x), px(*y)));
            }
            if let Ok(path) = p.build() {
                window.paint_path(path, color);
            }
        };
        for (w, a) in [(9., 0.05), (5.5, 0.09), (3., 0.16)] {
            line(
                0,
                wide * w,
                pal::accent().opacity(a * words * breath),
                window,
            );
        }
        line(
            0,
            wide,
            pal::accent_light().opacity(0.92 * words * breath),
            window,
        );
        // The light itself, bright while it runs.
        let run = (1. - done) * words;
        if run > 0.01 {
            let tail = seen.len().saturating_sub(14);
            if seen.len() - tail >= 2 {
                line(tail, wide * 1.5, gpui::white().opacity(0.9 * run), window);
            }
            let (x, y) = seen[seen.len() - 1];
            for (d, a) in [(16., 0.12), (9., 0.25), (4., 1.)] {
                disc(x, y, wide * d, gpui::white().opacity(a * run), window);
            }
        }
    }
    // Where the two lights meet.
    let meet = span(t, TRACE + TRACE_SECS * 0.93, 0.5);
    if meet > 0. && meet < 1. {
        let (x, y) = place((0., -0.7));
        ring(
            x,
            y,
            m * 0.5 * out_expo(meet),
            wide * (1. - meet),
            gpui::white().opacity(0.7 * (1. - meet) * words),
            window,
        );
    }
    // Its parts.
    for n in 0..5u32 {
        let from = PARTS + n as f32 * PART_STEP;
        let born = span(t, from, 0.42);
        if born <= 0. {
            continue;
        }
        let grow = out_back(born).max(0.);
        let a = (born * 4.).min(1.) * words;
        // It flares as it lights up, then glows.
        let flare = (1. - span(t, from, 0.5)).powi(2);
        let light = pal::accent_light().opacity(a);
        let white = gpui::white().opacity((0.35 + 0.65 * flare) * a);
        let thin = wide * (0.8 + flare);
        match n {
            0 => {
                let (x, y) = place((-1.17, -0.3));
                cross(x, y, m * 0.2 * grow, light, window);
                disc(x, y, m * 0.07 * grow, white, window);
            }
            1 | 2 => {
                let side = if n == 1 { -1. } else { 1. };
                let (x, y) = place((side * 0.68, 0.16));
                ring(x, y, m * 0.2 * grow, thin, light, window);
                ring(x, y, m * 0.13 * grow, thin * 0.7, white, window);
            }
            3 => {
                let (x, y) = place((1.17, -0.3));
                for (i, (dx, dy)) in [(0., 1.), (1., 0.), (-1., 0.), (0., -1.)]
                    .into_iter()
                    .enumerate()
                {
                    let late = out_back(span(t, from + i as f32 * 0.05, 0.4)).max(0.);
                    let (bx, by) = (x + dx * m * 0.16, y + dy * m * 0.16);
                    ring(bx, by, m * 0.083 * late, thin, light, window);
                    disc(bx, by, m * 0.07 * late, white, window);
                }
            }
            _ => {
                for side in [-1., 1.] {
                    let (x, y) = place((side * 0.45, -0.38));
                    let (w, h) = (m * 0.13 * grow, m * 0.045 * grow);
                    window.paint_quad(
                        fill(
                            Bounds::new(point(px(x - w / 2.), px(y - h / 2.)), size(px(w), px(h))),
                            light,
                        )
                        .corner_radii(px(h / 2.)),
                    );
                }
            }
        }
    }
}

/// The floor of the tunnel: two rails of light under the arches' feet, and bars across that
/// come towards the eye.
fn floor(st: Stage, window: &mut Window) {
    let t = st.t;
    let a = span(t, DIVE - 0.1, 0.5) * (1. - span(t, ARRIVE - 0.45, 0.45));
    if a <= 0.01 {
        return;
    }
    let cam = camera(t);
    let foot = st.mark * GATE;
    let y = (204. - 166.) / 256. * foot;
    let rail = |x: f32, wide: f32, color: Hsla, window: &mut Window| {
        let at = |d: f32, off: f32| {
            let k = EYE / d;
            point(px(st.cx + (x + off) * k), px(st.cy + y * k))
        };
        let mut p = PathBuilder::fill();
        p.move_to(at(0.12, -wide));
        p.line_to(at(0.12, wide));
        p.line_to(at(9., wide));
        p.line_to(at(9., -wide));
        p.close();
        if let Ok(path) = p.build() {
            window.paint_path(
                path,
                gpui::linear_gradient(
                    180.,
                    gpui::linear_color_stop(color.opacity(0.), 0.),
                    gpui::linear_color_stop(color, 0.12),
                ),
            );
        }
    };
    for side in [-1., 1.] {
        let x = side * (70. - 6.) / 256. * foot;
        rail(x, foot * 0.016, pal::accent().opacity(0.35 * a), window);
        rail(
            x,
            foot * 0.004,
            pal::accent_light().opacity(0.9 * a),
            window,
        );
    }
    // Bars across, one between every two arches.
    let mut p = PathBuilder::fill();
    let mut any = false;
    let mut z = EYE + 0.6;
    while z < EYE + DEEP {
        let d = z - cam;
        if (0.14..6.).contains(&d) {
            let half = 58. / 256. * foot;
            let (near, far) = (EYE / d, EYE / (d + 0.035));
            p.move_to(point(px(st.cx - half * near), px(st.cy + y * near)));
            p.line_to(point(px(st.cx + half * near), px(st.cy + y * near)));
            p.line_to(point(px(st.cx + half * far), px(st.cy + y * far)));
            p.line_to(point(px(st.cx - half * far), px(st.cy + y * far)));
            p.close();
            any = true;
        }
        z += STEP / 2.;
    }
    if any && let Ok(path) = p.build() {
        window.paint_path(path, pal::accent().opacity(0.28 * a));
    }
}

/// One thing in the tunnel.
enum Thing {
    /// A button of a gamepad, and which.
    Button(u32),
    Arch(f32),
    Card(usize, f32),
}

/// The tunnel, from far to near.
fn tunnel(st: Stage, cards: &[PathBuf], window: &mut Window) {
    let t = st.t;
    let mut things: Vec<(f32, Thing)> = Vec::new();
    if t < ARRIVE + 0.02 {
        // The first one is the arch that was struck.
        things.push((EYE, Thing::Arch(0.)));
        let mut z = EYE + 0.9;
        while z < EYE + DEEP - 0.5 {
            things.push((z, Thing::Arch(1.)));
            z += STEP;
        }
        for i in 0..16u32 {
            let z = EYE + 0.8 + i as f32 * (DEEP - 1.6) / 16.;
            things.push((z, Thing::Button(i)));
        }
        let n = cards.len().max(1) as f32;
        for i in 0..cards.len() {
            let z = EYE + 1.1 + i as f32 * (DEEP - 1.9) / n;
            things.push((z, Thing::Card(i, z)));
        }
    }
    things.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (z, thing) in things {
        match thing {
            Thing::Button(i) => {
                // High and wide of the way, turning slowly.
                let side = if i % 2 == 0 { -1. } else { 1. };
                let off = (
                    side * st.mark * (0.85 + 1.5 * rnd(i, 55)),
                    -st.mark * (0.55 + 1.1 * rnd(i, 56)),
                );
                let Some((x, y, k)) = st.project(z, off) else {
                    continue;
                };
                let a = st.fog(k) * span(t, DIVE - 0.1, 0.5) * 0.8;
                if a > 0.03 {
                    button(i, (x, y), st.mark * 0.17 * k, a, window);
                }
            }
            Thing::Arch(n) => {
                let Some((x, y, k)) = st.project(z, (0., 0.)) else {
                    continue;
                };
                let grow = if n == 0. { struck(t) } else { 1. };
                let seen = if n == 0. {
                    span(t, IGNITE, 0.1)
                } else {
                    span(t, DIVE - 0.3, 0.5)
                };
                let a = st.fog(k) * seen * if n == 0. { 1. } else { 0.85 };
                if grow <= 0.001 || a <= 0.01 {
                    continue;
                }
                // From the logo's weight to a frame of light as it comes by.
                let thin = if n == 0. {
                    lerp(1., 0.25, span(t, DIVE, 0.45))
                } else {
                    0.17
                };
                let s = st.mark * k * if n == 0. { 1. } else { GATE };
                // A halo: the same arch, larger and faint.
                arch(
                    (x, y),
                    s * 1.05,
                    grow,
                    (thin * 1.25).min(1.),
                    (
                        pal::accent_light().opacity(0.14 * a),
                        pal::accent().opacity(0.22 * a),
                    ),
                    window,
                );
                arch(
                    (x, y),
                    s,
                    grow,
                    thin,
                    (
                        gpui::white().opacity(a),
                        pal::accent_light().opacity(0.9 * a),
                    ),
                    window,
                );
            }
            Thing::Card(i, z) => {
                let side = if i % 2 == 0 { -1. } else { 1. };
                let off = (
                    side * st.mark * (1.5 + 0.7 * rnd(i as u32, 51)),
                    st.mark * ((rnd(i as u32, 52) - 0.5) * 1.1 - 0.3),
                );
                let Some((x, y, k)) = st.project(z, off) else {
                    continue;
                };
                let a = st.fog(k) * span(t, DIVE - 0.1, 0.5);
                if a <= 0.02 {
                    continue;
                }
                // Turned towards the way.
                let ch = st.mark * 1.5 * k;
                let cw = ch / 1.5 * 0.62;
                let b = Bounds::new(
                    point(px(x - cw / 2.), px(y - ch / 2.)),
                    size(px(cw), px(ch)),
                );
                if f32::from(b.right()) < 0. || f32::from(b.left()) > st.w {
                    continue;
                }
                let radius = (12. * k).min(40.);
                cover_art::paint(&cards[i], b, radius, false, true, window);
                window.paint_quad(
                    fill(b, gpui::black().opacity((1. - a) * 0.96)).corner_radii(px(radius)),
                );
            }
        }
    }
    // The light at the end, seen until the last arch stands in front of it.
    let d = DEEP + EYE - camera(t);
    let glow = span(t, DIVE - 0.2, 0.6);
    let glow = glow * (1. - span(t, ARRIVE - 0.4, 0.35));
    if glow > 0. {
        let s = st.mark * 0.5 / d.max(1.);
        for (k, a) in [(0.9, 0.12), (0.7, 0.16), (0.52, 0.3)] {
            disc(
                st.cx,
                st.cy,
                s * k,
                pal::accent_light().opacity(a * glow),
                window,
            );
        }
        disc(st.cx, st.cy, s * 0.35, gpui::white().opacity(glow), window);
    }
}

/// The flash and the rings when the spark catches and when the last arch lands.
fn burst(st: Stage, at: f32, power: f32, window: &mut Window) {
    let k = span(st.t, at, 1.1);
    if st.t < at || k >= 1. {
        return;
    }
    let grow = out_expo(k);
    let fade = (1. - k).powi(2);
    // A line of light across the whole screen.
    let line = span(st.t, at, 0.75);
    if line < 1. {
        let wide = st.w * (0.25 + 0.75 * out_expo(line)) * power.min(1.);
        let tall = (st.h * 0.007 * power * (1. - line).powi(2)).max(0.6);
        for (n, a) in [(5., 0.1), (2.4, 0.25), (1., 0.95)] {
            let (hh, color) = (tall * n, pal::accent_light());
            let color = if n == 1. { gpui::white() } else { color };
            for (from, left, right) in [
                (st.cx - wide / 2., 0., a * (1. - line)),
                (st.cx, a * (1. - line), 0.),
            ] {
                window.paint_quad(fill(
                    Bounds::new(
                        point(px(from), px(st.cy - hh / 2.)),
                        size(px(wide / 2.), px(hh)),
                    ),
                    gpui::linear_gradient(
                        90.,
                        gpui::linear_color_stop(color.opacity(left), 0.),
                        gpui::linear_color_stop(color.opacity(right), 1.),
                    ),
                ));
            }
        }
    }
    for (reach, width, a) in [(0.62, 5., 0.75), (0.4, 2., 0.4)] {
        let r = st.h * reach * power * grow;
        window.paint_quad(gpui::quad(
            Bounds::new(
                point(px(st.cx - r), px(st.cy - r)),
                size(px(r * 2.), px(r * 2.)),
            ),
            px(r),
            gpui::transparent_black(),
            px((width * fade * st.h / 1080.).max(0.5)),
            pal::accent_light().opacity(a * fade),
            gpui::BorderStyle::Solid,
        ));
    }
    let mut p = PathBuilder::fill();
    for i in 0..(40. * power) as u32 {
        let a = rnd(i, 61) * TAU;
        let d = st.h * (0.12 + 0.5 * rnd(i, 62)) * power;
        let place = |k: f32| {
            let d = d * out_expo(k);
            (
                st.cx + a.cos() * d,
                st.cy + a.sin() * d + st.h * 0.1 * k * k,
            )
        };
        streak(
            &mut p,
            place((k - 0.05).max(0.)),
            place(k + 0.002),
            st.h / 1080. * (1.5 + 3. * rnd(i, 63)) * (1. - k),
        );
    }
    if let Ok(path) = p.build() {
        window.paint_path(path, gpui::white().opacity(fade.sqrt()));
    }
    // And for a moment everything is bright.
    let flash = (1. - span(st.t, at, 0.32)).powi(2) * 0.55 * power.min(1.);
    if flash > 0.01 {
        window.paint_quad(fill(
            Bounds::new(point(px(0.), px(0.)), size(px(st.w), px(st.h))),
            Hsla {
                l: 0.82,
                ..pal::accent_light()
            }
            .opacity(flash * 0.6),
        ));
    }
}

impl Library {
    /// Plays the animation over console mode: in full the first time, its end afterwards.
    pub(super) fn start_console_intro(&mut self) {
        use std::sync::atomic::{AtomicBool, Ordering};
        static PLAYED: AtomicBool = AtomicBool::new(false);
        if cfg!(test) {
            return;
        }
        self.play_console_intro(PLAYED.swap(true, Ordering::Relaxed));
    }

    pub(super) fn play_console_intro(&mut self, short: bool) {
        if self.global.console_no_intro || self.global.appearance.reduce_motion {
            return;
        }
        // What was played last comes first.
        let mut games: Vec<(Option<u64>, PathBuf)> = self
            .library
            .games
            .iter()
            .filter(|g| !g.hidden)
            .filter_map(|g| {
                let path = self.covers.get(&g.id)?.clone()?;
                Some((self.playtime(&g.game_id()).1, path))
            })
            .collect();
        games.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
        let cards: Vec<PathBuf> = games.into_iter().map(|(_, p)| p).take(MAX_CARDS).collect();
        for path in &cards {
            cover_art::ready(path);
        }
        // A small library comes by more than once.
        let cards: Vec<PathBuf> = match cards.len() {
            0 => cards,
            n => cards
                .into_iter()
                .cycle()
                .take(MAX_CARDS.min(n * 4))
                .collect(),
        };
        console_sound::warm();
        self.console.intro = Some(ConsoleIntro {
            clock: if short { SHORT_FROM } else { 0. },
            last: None,
            waited: 0.,
            short,
            reveal: None,
            sounded: false,
            cards,
        });
    }

    /// Any input ends it; `true` while there was something to skip.
    pub(super) fn skip_console_intro(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(i) = self.console.intro.as_mut() else {
            return false;
        };
        if i.reveal.is_some() {
            return false;
        }
        i.clock = i.clock.max(ARRIVE + 0.3);
        i.reveal = Some(i.clock);
        console_sound::fade(0.5);
        cx.notify();
        true
    }

    /// Advances it by one frame. `false` when there is none.
    pub(super) fn step_console_intro(&mut self, window: &mut Window) -> bool {
        let volume = self.console_volume();
        let Some(i) = self.console.intro.as_mut() else {
            return false;
        };
        window.request_animation_frame();
        let now = Instant::now();
        let dt = i.last.map_or(0., |l| (now - l).as_secs_f32()).min(1. / 30.);
        i.last = Some(now);
        let cue = if i.short { Cue::Hello } else { Cue::Welcome };
        if !i.sounded {
            // The covers and the music are there first (never for long).
            i.waited += dt;
            let ready = i.cards.iter().all(|p| cover_art::ready(p))
                && (volume <= 0. || console_sound::ready(cue));
            if i.waited < 0.12 || (!ready && i.waited < 1.2) {
                return true;
            }
            i.sounded = true;
            console_sound::play(cue, volume, 0.);
        }
        i.clock += dt;
        let due = if i.short { SHORT_REVEAL } else { REVEAL };
        if i.reveal.is_none() && i.clock >= due {
            i.reveal = Some(i.clock);
        }
        if let Some(at) = i.reveal
            && i.clock - at >= OPENING
        {
            self.console.intro = None;
            return false;
        }
        true
    }

    /// The moment the console under it comes in.
    pub(super) fn console_intro_opens(&self) -> Option<bool> {
        self.console.intro.as_ref().map(|i| i.reveal.is_some())
    }

    /// The stage over the console. `logo`: where the console's logo is (its middle and size).
    pub(super) fn render_console_intro(
        &self,
        (w, h): (f32, f32),
        logo: (f32, f32, f32),
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let i = self.console.intro.as_ref()?;
        let st = Stage::new(i.clock, i.reveal, w, h);
        let cards = i.cards.clone();
        // The stage goes while the logo is on its way.
        let stage = if st.leaving {
            1. - in_out(span(st.r, 0.08, 0.6))
        } else {
            1.
        };
        let words = if st.leaving {
            1. - out_cubic(span(st.r, 0., 0.25))
        } else {
            1.
        };
        let night = Hsla {
            h: pal::accent().h,
            s: 0.55,
            l: 0.016,
            a: 1.,
        };
        let root = div()
            .id("console-intro")
            .absolute()
            .inset_0()
            .overflow_hidden()
            .when(!st.leaving, |d| d.occlude())
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.skip_console_intro(cx);
                }),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .opacity(stage)
                    .bg(night)
                    .child(sky(st))
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |_, _, window, _| {
                                embers(st, window);
                                floor(st, window);
                                speed(st, window);
                                tunnel(st, &cards, window);
                                burst(st, IGNITE, 0.6, window);
                                burst(st, ARRIVE, 1.1, window);
                            },
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .child(console_glow(st).opacity(stage))
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| gamepad(st, words, window),
                )
                .absolute()
                .inset_0(),
            )
            .child(landed(st, logo))
            .child(name(st).opacity(words));
        Some(root.into_any_element())
    }
}

/// The colour of the dark: light gathers at the tunnel's end and opens up with the flight.
fn sky(st: Stage) -> gpui::Div {
    let t = st.t;
    let warm = span(t, 0.15, IGNITE) * 0.35;
    let fly = in_out(span(t, DIVE - 0.2, ARRIVE - DIVE)) * (1. - span(t, ARRIVE, 0.5));
    let near = warm * (1. - span(t, ARRIVE - 0.3, 0.6)) + fly;
    let a = pal::accent();
    // To the sides the colour leans to its neighbours.
    let lean = |by: f32| Hsla {
        h: (a.h + by).rem_euclid(1.),
        ..a
    };
    div().absolute().inset_0().when(near > 0.01, |d| {
        d.child(super::backdrop::glow(
            0.5,
            0.47,
            st.mark * (0.5 + 1.6 * fly),
            st.mark * (0.4 + fly),
            st.mark * (0.7 + 0.8 * fly),
            a.opacity(0.5 * near),
        ))
        .child(super::backdrop::glow(
            0.12,
            0.35,
            st.w * 0.1,
            st.h * 0.4,
            st.h * 0.3,
            lean(-0.09).opacity(0.5 * fly),
        ))
        .child(super::backdrop::glow(
            0.88,
            0.35,
            st.w * 0.1,
            st.h * 0.4,
            st.h * 0.3,
            lean(0.1).opacity(0.5 * fly),
        ))
    })
}

/// Soft light behind the logo once it has landed.
fn console_glow(st: Stage) -> gpui::Div {
    let lit = out_cubic(span(st.t, ARRIVE - 0.05, 0.5));
    let breath = 0.92 + 0.08 * (st.t * 2.1).sin();
    div().absolute().inset_0().when(lit > 0., |d| {
        d.child(super::backdrop::glow(
            0.5,
            0.47,
            st.mark * 1.5,
            st.mark * 1.1,
            st.mark * 0.9,
            pal::accent().opacity(0.5 * lit * breath),
        ))
        .child(super::backdrop::glow(
            0.5,
            0.47,
            st.w * 0.55,
            st.h * 0.03,
            st.h * 0.14,
            pal::accent().opacity(0.34 * lit),
        ))
    })
}

/// The last arch: the logo, and then its way to its place in the console.
fn landed(st: Stage, (lx, ly, ls): (f32, f32, f32)) -> AnyElement {
    let t = st.t;
    let Some((_, _, k)) = st.project(DEEP + EYE, (0., 0.)) else {
        return div().into_any_element();
    };
    let seen = span(t, DIVE, 0.5) * if t < ARRIVE { st.fog(k).max(0.12) } else { 1. };
    let hit = span(t, ARRIVE, 0.5);
    // It swells with the hit and settles.
    let punch = 1. + 0.1 * (PI * span(t, ARRIVE, 0.28)).sin() * (1. - hit);
    let (mut x, mut y) = (st.cx, st.cy);
    let mut s = st.mark * k.min(1.) * punch;
    let mut tile = 0.;
    if st.leaving {
        let go = in_out(span(st.r, 0.02, 0.8));
        // On the tile the mark fills 74 %; its core lies below its middle.
        let end = ls * 0.74;
        let end_y = ly + (166. - 128.) / 256. * end;
        x = lerp(x, lx, go);
        y = lerp(y, end_y, go) + (PI * go).sin() * st.h * 0.035;
        s = lerp(s, end, go);
        tile = in_out(span(st.r, 0.32, 0.5));
    }
    let gone = if st.leaving {
        1. - span(st.r, OPENING - 0.12, 0.12)
    } else {
        1.
    };
    let light = pal::accent_light();
    let top = gpui::white().opacity(seen * gone);
    let foot = Hsla {
        h: light.h,
        s: light.s * (1. - tile),
        l: lerp(light.l, 1., tile),
        a: seen * gone,
    };
    let lit = out_back(span(t, ARRIVE - 0.04, 0.4));
    let side = s / 0.74;
    let middle = y - (166. - 128.) / 256. * s;
    div()
        .absolute()
        .inset_0()
        .when(tile > 0., |d| {
            d.child(
                div()
                    .absolute()
                    .left(px(x - side / 2.))
                    .top(px(middle - side / 2.))
                    .size(px(side))
                    .rounded(px(side * 0.26))
                    .bg(style::accent_gradient())
                    .opacity(tile * gone),
            )
        })
        .child(
            canvas(
                |_: Bounds<Pixels>, _, _| (),
                move |_, _, window, _| {
                    if seen <= 0.01 {
                        return;
                    }
                    arch((x, y), s, 1., 1., (top, foot), window);
                    if lit > 0. {
                        core(x, y, s, lit, top, window);
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .into_any_element()
}

/// The name, letter by letter, drawing together.
fn name(st: Stage) -> gpui::Div {
    let t = st.t - ARRIVE;
    let gather = out_expo(span(t, 0.05, 1.1));
    let u = st.mark / 270.;
    let mut row = div()
        .absolute()
        .left_0()
        .right_0()
        .top(px(st.cy + st.mark * 0.66))
        .flex()
        .justify_center()
        .gap(px(lerp(60., 16., gather) * u))
        .text_size(px(58. * u))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(gpui::white());
    if t <= 0. {
        return row;
    }
    for (i, letter) in ["A", "R", "K", "O", "R"].into_iter().enumerate() {
        let k = out_cubic(span(t, 0.08 + i as f32 * 0.085, 0.5));
        row = row.child(
            div()
                .relative()
                .top(px(34. * u * (1. - k)))
                .opacity(k)
                .child(letter),
        );
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_ends_in_front_of_the_last_arch() {
        assert!(camera(0.).abs() < 1e-6);
        assert!((camera(ARRIVE) - DEEP).abs() < 1e-4);
        assert!((camera(9.) - DEEP).abs() < 1e-4);
        assert!(camera(DIVE) < 0., "a step back first");
        let st = Stage::new(ARRIVE + 1., None, 2560., 1440.);
        let (x, y, k) = st.project(DEEP + EYE, (0., 0.)).unwrap();
        assert!((k - 1.).abs() < 1e-3, "the logo stands at its own size");
        assert_eq!((x, y), (st.cx, st.cy));
        // The way only goes forward once it has begun.
        let mut last = camera(DIVE + 0.25);
        for n in 1..60 {
            let now = camera(DIVE + 0.25 + n as f32 * 0.03);
            assert!(now >= last - 1e-4);
            last = now;
        }
    }

    #[test]
    fn the_gamepad_is_one_closed_line_around_the_logo() {
        let (points, length) = outline();
        assert_eq!((points[0].0, points[points.len() - 1].0), (0., 0.));
        assert!(length.is_sorted() && length.len() == points.len());
        for (x, y) in &points {
            assert!(*x >= -0.01, "one half stays on its side: {x}");
            // Clear of the logo: 0.28 wide and from -0.47 to 0.15.
            assert!(*x > 0.28 || *y > 0.4 || *y < -0.6, "({x}, {y})");
        }
        // Smooth: no step turns sharply.
        for w in points.windows(3) {
            let (a, b) = (
                (w[1].1 - w[0].1).atan2(w[1].0 - w[0].0),
                (w[2].1 - w[1].1).atan2(w[2].0 - w[1].0),
            );
            let turn = (b - a + PI).rem_euclid(TAU) - PI;
            assert!(turn.abs() < 0.5, "{w:?}");
        }
    }

    #[test]
    fn the_times_are_in_order() {
        let order = [
            0.,
            IGNITE,
            DIVE,
            SHORT_FROM,
            ARRIVE,
            TRACE,
            PARTS,
            TRACE + TRACE_SECS,
            SHORT_REVEAL,
            REVEAL,
        ];
        assert!(order.is_sorted());
    }
}
