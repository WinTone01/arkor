//! The start-up animation. The arch of the logo is drawn by two beams of light, its core
//! ignites, and the covers of the user's own games burst out of it to circle the logo like a
//! carousel. Then every cover flies from there to its place in the library and the logo
//! settles into the sidebar while the page glides in.
//!
//! All motion follows a clock that never advances more than a thirtieth of a second per
//! frame, so a slow frame delays the animation instead of making it jump. A click or a key
//! skips to the end; with "reduce motion" the page only fades in.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::{PI, TAU};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use gpui::{
    AnyElement, Bounds, ContentMask, Context, Div, FontWeight, Hsla, PathBuilder, Pixels, Window,
    canvas, div, fill, point, prelude::*, px, size,
};

use super::backdrop::{glow, rnd};
use super::style::{self, pal};
use super::{Library, cover_art};

/// Covers in the carousel at most.
const MAX_CARDS: usize = 12;
/// The stage is on screen at least this long (seconds).
const STAGE: f32 = 2.0;
/// And at most this long, even if a cover does not load.
const STAGE_MAX: f32 = 6.0;
/// From the first cover taking off to everything being in place.
const REVEAL: f32 = 1.35;
/// Size of the mark on the stage.
const MARK: f32 = 132.;

/// Where things of the page are, measured while it is laid out.
#[derive(Default)]
struct Places {
    /// Game id → its cover and the area it is visible in.
    covers: HashMap<String, (Bounds<Pixels>, Bounds<Pixels>)>,
    logo: Option<Bounds<Pixels>>,
}

pub(super) struct Intro {
    clock: f32,
    last: Option<Instant>,
    /// Clock value at which the covers took off.
    reveal: Option<f32>,
    /// The loading bar as drawn (0..1).
    bar: f32,
    cards: Vec<(String, PathBuf)>,
    places: Rc<RefCell<Places>>,
    /// Only a fade, no animation.
    plain: bool,
}

// ---- Curves ----

fn clamp01(x: f32) -> f32 {
    x.clamp(0., 1.)
}
/// Progress of something that starts at `from` and takes `secs`.
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
    // Quintic: leaves and lands without a jerk.
    x * x * x * (x * (x * 6. - 15.) + 10.)
}
/// Overshoots a little and settles on 1.
fn out_back(x: f32) -> f32 {
    let c = 1.6;
    1. + (c + 1.) * (x - 1.).powi(3) + c * (x - 1.).powi(2)
}

/// A card of the carousel: centre, size and depth (-1 far … 1 near).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    depth: f32,
}

/// Card `i` of `n` in the carousel at clock `t`, in a window of `w`×`h`.
fn pose(i: usize, n: usize, t: f32, w: f32, h: f32) -> Pose {
    let born = span(t, 0.72 + i as f32 * 0.045, 0.95);
    let out = out_back(born);
    // Spins in fast and slows to a drift.
    let turn = 2.6 * out_cubic(span(t, 0.7, 2.2)) + 0.28 * (t - 0.7).max(0.);
    let a = i as f32 * TAU / n.max(1) as f32 + turn;
    let depth = a.sin();
    let near = (depth + 1.) / 2.;
    let scale = lerp(0.56, 1., near) * lerp(0.15, 1., out_cubic(born));
    let card_h = (h * 0.27).clamp(150., 340.) * scale;
    // Cards face outwards: at the sides they are seen from the edge.
    let face = lerp(0.42, 1., depth.abs());
    let (cx, cy) = centre(w, h);
    Pose {
        x: cx + a.cos() * (w * 0.36).min(680.) * out,
        y: lerp(cy, cy + h * 0.13 + depth * h * 0.15, clamp01(out)),
        w: card_h / 1.5 * face,
        h: card_h,
        depth,
    }
}

/// The core of the logo: everything starts here.
fn centre(w: f32, h: f32) -> (f32, f32) {
    (w / 2., h * 0.36)
}

/// Between the carousel and the cover's place (centre, size), along a lifted arc.
fn flight(from: Pose, to: Pose, k: f32) -> Pose {
    let lift = (PI * k).sin() * 46.;
    Pose {
        x: lerp(from.x, to.x, k),
        y: lerp(from.y, to.y, k) - lift,
        w: lerp(from.w, to.w, k),
        h: lerp(from.h, to.h, k),
        depth: lerp(from.depth, 1., k),
    }
}

fn lerp_bounds(a: Bounds<Pixels>, b: Bounds<Pixels>, k: f32) -> Bounds<Pixels> {
    let mix = |a: Pixels, b: Pixels| px(lerp(a.into(), b.into(), k));
    Bounds::new(
        point(mix(a.origin.x, b.origin.x), mix(a.origin.y, b.origin.y)),
        size(
            mix(a.size.width, b.size.width),
            mix(a.size.height, b.size.height),
        ),
    )
}

/// How far the page under the stage has come in.
#[derive(Clone, Copy)]
pub(super) struct PageIn {
    pub(super) sidebar: f32,
    pub(super) panel: f32,
}

impl Library {
    /// Plays the animation in the first window of this run; later windows just open.
    pub(super) fn start_intro(&mut self) {
        static PLAYED: AtomicBool = AtomicBool::new(false);
        if PLAYED.swap(true, Ordering::Relaxed) {
            return;
        }
        let cards: Vec<_> = self
            .visible
            .iter()
            .filter_map(|&ix| {
                let id = self.games.get(ix)?.id.id.clone();
                let path = self.covers.get(&id)?.clone()?;
                Some((id, path))
            })
            .take(MAX_CARDS)
            .collect();
        for (_, path) in &cards {
            cover_art::ready(path);
        }
        self.intro = Some(Intro {
            clock: 0.,
            last: None,
            reveal: None,
            bar: 0.,
            cards,
            places: Rc::default(),
            plain: self.global.appearance.reduce_motion,
        });
    }

    /// Covers and backgrounds may move.
    pub(super) fn moving(&self) -> bool {
        // The carousel shows the first frames; they start moving once in place.
        !self.global.appearance.reduce_motion && self.intro.is_none()
    }

    /// The animation is drawing this game's cover, so the library leaves its place empty.
    pub(super) fn intro_carries(&self, id: &str) -> bool {
        self.intro
            .as_ref()
            .is_some_and(|i| !i.plain && i.cards.iter().any(|(c, _)| c == id))
    }

    /// Tells the animation where this game's cover belongs.
    pub(super) fn intro_slot(
        &self,
        id: &str,
    ) -> Option<impl Fn(Bounds<Pixels>, Bounds<Pixels>) + 'static> {
        if !self.intro_carries(id) {
            return None;
        }
        let places = self.intro.as_ref()?.places.clone();
        let id = id.to_string();
        Some(move |bounds, visible| {
            places
                .borrow_mut()
                .covers
                .insert(id.clone(), (bounds, visible));
        })
    }

    /// The sidebar's logo; while the animation plays it only marks the place.
    pub(super) fn sidebar_logo(&self) -> AnyElement {
        let Some(intro) = self.intro.as_ref().filter(|i| !i.plain) else {
            return style::logo(32.).into_any_element();
        };
        let places = intro.places.clone();
        canvas(
            move |bounds, _, _| places.borrow_mut().logo = Some(bounds),
            |_, _, _, _| (),
        )
        .size(px(32.))
        .flex_none()
        .into_any_element()
    }

    pub(super) fn skip_intro(&mut self, cx: &mut Context<Self>) {
        if let Some(intro) = &mut self.intro
            && intro.reveal.is_none()
        {
            intro.reveal = Some(intro.clock);
            cx.notify();
        }
    }

    /// Advances the animation; `None` when it is over. Call before drawing the page.
    pub(super) fn step_intro(&mut self, window: &mut Window) -> Option<PageIn> {
        let intro = self.intro.as_mut()?;
        let now = Instant::now();
        let dt = intro
            .last
            .map_or(0., |l| (now - l).as_secs_f32())
            .min(1. / 30.);
        intro.last = Some(now);
        intro.clock += dt;
        window.request_animation_frame();

        let total = intro.cards.len();
        let ready = intro
            .cards
            .iter()
            .filter(|(_, p)| cover_art::ready(p))
            .count();
        let goal = if total == 0 {
            1.
        } else {
            0.15 + 0.85 * ready as f32 / total as f32
        };
        // Glides to the real progress whatever the frame rate, never back.
        let goal = goal.max(intro.bar);
        intro.bar += (goal - intro.bar) * (1. - (-dt * 4.2).exp());
        if intro.bar > 0.992 {
            intro.bar = 1.;
        }
        let t = intro.clock;
        if intro.reveal.is_none()
            && (intro.plain || (t >= STAGE && intro.bar >= 1.) || t >= STAGE_MAX)
        {
            intro.reveal = Some(t);
        }
        let Some(start) = intro.reveal else {
            // Faintly drawn under the opaque stage: text and icons are then ready.
            return Some(PageIn {
                sidebar: 0.,
                panel: 0.,
            });
        };
        let r = t - start;
        if r >= if intro.plain { 0.3 } else { REVEAL } {
            self.intro = None;
            return None;
        }
        Some(if intro.plain {
            let k = out_cubic(span(r, 0., 0.3));
            PageIn {
                sidebar: k,
                panel: k,
            }
        } else {
            PageIn {
                sidebar: out_expo(span(r, 0.3, 0.8)),
                panel: out_expo(span(r, 0.18, 0.9)),
            }
        })
    }

    /// The stage over the page.
    pub(super) fn render_intro(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let intro = self.intro.as_ref().filter(|i| !i.plain)?;
        let view = window.viewport_size();
        let (w, h) = (f32::from(view.width), f32::from(view.height));
        let t = intro.clock;
        let r = intro.reveal.map_or(0., |s| t - s);
        let leaving = intro.reveal.is_some();
        // The stage goes while the covers are in the air.
        let stage = if leaving {
            1. - in_out(span(r, 0.1, 0.62))
        } else {
            1.
        };
        // What is only on the stage leaves faster.
        let words = if leaving {
            1. - out_cubic(span(r, 0., 0.3))
        } else {
            1.
        };
        let (cx0, cy0) = centre(w, h);

        let mut root = div()
            .id("intro")
            .absolute()
            .inset_0()
            .overflow_hidden()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| this.skip_intro(cx)),
            )
            // Once the covers fly, clicks already belong to the page.
            .when(!leaving, |d| d.occlude())
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .opacity(stage)
                    .bg(pal::bg())
                    .child(stage_light(t, w, h))
                    .child(dust(t, r, w, h)),
            );

        // Carousel: far cards, then the logo, then near cards.
        let n = intro.cards.len();
        let mut poses: Vec<(usize, Pose)> = (0..n).map(|i| (i, pose(i, n, t, w, h))).collect();
        poses.sort_by(|a, b| a.1.depth.total_cmp(&b.1.depth));
        let split = poses.partition_point(|(_, p)| p.depth < 0.);
        let card = |&(i, from): &(usize, Pose)| {
            let (id, path) = intro.cards[i].clone();
            let places = intro.places.clone();
            let k = if leaving {
                in_out(span(r, 0.02 + i as f32 * 0.028, 0.92))
            } else {
                0.
            };
            let seen = out_cubic(span(t, 0.72 + i as f32 * 0.045, 0.35));
            card_layer(id, path, places, from, k, seen, (w, h))
        };
        for p in &poses[..split] {
            root = root.child(card(p));
        }
        root = root
            .child(ignition(t, cx0, cy0).opacity(words))
            .child(self.mark(intro, t, r, cx0, cy0));
        for p in &poses[split..] {
            root = root.child(card(p));
        }
        root = root
            .child(wordmark(t, r, cy0).opacity(words))
            .child(self.loading(intro, t, h).opacity(words))
            .when(leaving, |d| d.child(wave(r, cx0, cy0, w, h)));
        Some(root.into_any_element())
    }

    /// The mark: drawn by two beams, then on its way to the sidebar, where it becomes the
    /// logo's tile.
    fn mark(&self, intro: &Intro, t: f32, r: f32, cx0: f32, cy0: f32) -> AnyElement {
        let draw = in_out(span(t, 0.08, 0.62));
        let core = out_back(span(t, 0.6, 0.45));
        // A slow breath once lit.
        let breath = 1. + 0.012 * (t * 2.2).sin() * span(t, 1., 0.6);
        let (mut x, mut y, mut s, mut tile) = (cx0, cy0 - MARK * 0.12, MARK * breath, 0.);
        if intro.reveal.is_some() {
            let k = in_out(span(r, 0.06, 0.95));
            match intro.places.borrow().logo {
                Some(place) => {
                    x = lerp(x, place.center().x.into(), k);
                    y = lerp(y, place.center().y.into(), k) - (PI * k).sin() * 30.;
                    // The mark fills 74 % of the finished tile.
                    s = lerp(s, f32::from(place.size.width) * 0.74, k);
                    tile = in_out(span(r, 0.3, 0.6));
                }
                // No sidebar (big screen): it just goes.
                None => {
                    s *= 1. + k * 0.4;
                    tile = -1.;
                }
            }
        }
        let gone = if tile < 0. {
            1. - out_cubic(span(r, 0., 0.4))
        } else {
            1.
        };
        let tile = tile.max(0.);
        let light = pal::accent_light();
        let top = gpui::white().opacity(gone);
        // On the tile the mark is plain white.
        let foot = Hsla {
            h: light.h,
            s: light.s * (1. - tile),
            l: lerp(light.l, 1., tile),
            a: gone,
        };
        let side = s / 0.74;
        let (left, up) = (x - s / 2., y - s / 2.);
        div()
            .absolute()
            .inset_0()
            .when(tile > 0., |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(x - side / 2.))
                        .top(px(y - side / 2.))
                        .size(px(side))
                        .rounded(px(side * 0.26))
                        .bg(style::accent_gradient())
                        .opacity(tile),
                )
            })
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let at = |mx: f32, my: f32| {
                            point(px(left + mx / 256. * s), px(up + my / 256. * s))
                        };
                        for side in [1., -1.] {
                            let sx = |v: f32| 128. + (v - 128.) * side;
                            let (ox, oy) = (lerp(58., 128., draw), lerp(204., 46., draw));
                            let (ix, iy) = (lerp(92., 128., draw), lerp(204., 120., draw));
                            let mut p = PathBuilder::fill();
                            p.move_to(at(sx(58.), 204.));
                            p.line_to(at(sx(92.), 204.));
                            p.line_to(at(sx(ix), iy));
                            p.line_to(at(sx(ox), oy));
                            p.close();
                            if draw > 0.001
                                && let Ok(path) = p.build()
                            {
                                window.paint_path(
                                    path,
                                    gpui::linear_gradient(
                                        180.,
                                        gpui::linear_color_stop(top, 0.),
                                        gpui::linear_color_stop(foot, 1.),
                                    ),
                                );
                            }
                        }
                        if core > 0. {
                            let mut p = PathBuilder::fill();
                            p.move_to(at(128., 166. - 19. * core));
                            p.line_to(at(128. + 17. * core, 166.));
                            p.line_to(at(128., 166. + 19. * core));
                            p.line_to(at(128. - 17. * core, 166.));
                            p.close();
                            if let Ok(path) = p.build() {
                                window.paint_path(path, top);
                            }
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
            .into_any_element()
    }

    /// What is being loaded, with a thin bar.
    fn loading(&self, intro: &Intro, t: f32, h: f32) -> Div {
        let total = intro.cards.len();
        let ready = intro
            .cards
            .iter()
            .filter(|(_, p)| cover_art::ready(p))
            .count();
        let text = if intro.bar >= 1. {
            self.tr("Ready", "Hazır").to_string()
        } else if t < 0.45 || total == 0 {
            format!(
                "{} · {}",
                self.tr("Reading your library", "Kütüphane okunuyor"),
                self.games.len()
            )
        } else {
            format!(
                "{} · {ready}/{total}",
                self.tr("Getting cover art", "Kapak resimleri getiriliyor")
            )
        };
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(h * 0.915))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(220.))
                    .h(px(3.))
                    .rounded_full()
                    .bg(pal::text().opacity(0.1))
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative(intro.bar))
                            .rounded_full()
                            .bg(style::accent_gradient()),
                    ),
            )
            .child(div().text_xs().text_color(pal::muted()).child(text))
    }
}

/// One cover, in the carousel or on its way to its place in the library.
fn card_layer(
    id: String,
    path: PathBuf,
    places: Rc<RefCell<Places>>,
    from: Pose,
    k: f32,
    seen: f32,
    (w, h): (f32, f32),
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            if seen <= 0. {
                return;
            }
            let window_area = Bounds::new(point(px(0.), px(0.)), size(px(w), px(h)));
            let place = places
                .borrow()
                .covers
                .get(&id)
                .copied()
                .filter(|(b, seen)| b.intersects(seen));
            let (to, area) = match place {
                Some((b, visible)) => (
                    Pose {
                        x: b.center().x.into(),
                        y: b.center().y.into(),
                        w: b.size.width.into(),
                        h: b.size.height.into(),
                        depth: 1.,
                    },
                    visible,
                ),
                // Further down the library, or another page is open: out of the window.
                None => (
                    Pose {
                        y: h + from.h,
                        depth: 1.,
                        ..from
                    },
                    window_area,
                ),
            };
            let p = flight(from, to, k);
            let bounds = if k >= 1. {
                place.map_or(window_area, |(b, _)| b)
            } else {
                Bounds::new(
                    point(px(p.x - p.w / 2.), px(p.y - p.h / 2.)),
                    size(px(p.w), px(p.h)),
                )
            };
            if k >= 1. && place.is_none() {
                return;
            }
            // In the air it may pass over anything; in its place the page cuts it off.
            let mask = ContentMask {
                bounds: lerp_bounds(window_area, area, clamp01((k - 0.7) / 0.3)),
            };
            let radius = lerp(14. * p.h / 260., 12., k);
            // Distance and birth darken it.
            let dark = lerp(0.7, 0., (p.depth + 1.) / 2.).max(1. - seen);
            window.with_content_mask(Some(mask), |window| {
                cover_art::paint(&path, bounds, radius, false, true, window);
                if dark > 0.01 {
                    window.paint_quad(
                        fill(bounds, pal::bg().opacity(dark.min(1.))).corner_radii(px(radius)),
                    );
                }
            });
        },
    )
    .absolute()
    .inset_0()
}

/// Light on the stage: a glow behind the logo and a pool of light under the carousel.
fn stage_light(t: f32, w: f32, h: f32) -> Div {
    let a = pal::accent();
    let soft = if pal::is_light() { 0.4 } else { 1. };
    let lit = out_cubic(span(t, 0.55, 0.9)) * soft;
    let breath = 0.9 + 0.1 * (t * 1.7).sin();
    div()
        .absolute()
        .inset_0()
        .child(glow(
            0.5,
            0.36,
            (w * 0.34).min(560.),
            (h * 0.3).min(320.),
            240.,
            a.opacity(0.42 * lit * breath),
        ))
        .child(glow(
            0.5,
            0.66,
            (w * 0.62).min(1100.),
            46.,
            130.,
            a.opacity(0.5 * lit),
        ))
}

/// Specks of light drifting out from the core; when the covers take off they rush past.
fn dust(t: f32, r: f32, w: f32, h: f32) -> Div {
    let (cx, cy) = centre(w, h);
    let reach = (w * w + h * h).sqrt() * 0.55;
    let lit = span(t, 0.6, 0.6);
    let rush = r * r * 1.6;
    let mut el = div().absolute().inset_0();
    for i in 0..46u32 {
        let a = rnd(i, 11) * TAU;
        let d = (rnd(i, 12) + t * (0.05 + 0.07 * rnd(i, 13)) + rush).fract();
        let far = d * d * reach;
        let s = 1. + 2.4 * d * rnd(i, 14);
        // Fades in near the core and out at the edge.
        let alpha = (d * 5.).min(1.) * (1. - d) * lit * (0.25 + 0.6 * rnd(i, 15));
        let len = s * (1. + (r * 10.).min(5.));
        let color = if i % 4 == 0 {
            pal::text()
        } else {
            pal::accent_light()
        };
        el = el.child(
            div()
                .absolute()
                .left(px(cx + a.cos() * far - len / 2.))
                .top(px(cy + a.sin() * far - s / 2.))
                .w(px(len))
                .h(px(s))
                .rounded_full()
                .bg(color.opacity(alpha)),
        );
    }
    el
}

fn ring(x: f32, y: f32, radius: f32, width: f32, color: Hsla) -> Div {
    div()
        .absolute()
        .left(px(x - radius))
        .top(px(y - radius))
        .size(px(radius * 2.))
        .rounded_full()
        .border(px(width))
        .border_color(color)
}

/// The core lights up: a flash, a ring and sparks.
fn ignition(t: f32, cx: f32, cy: f32) -> Div {
    let mut el = div().absolute().inset_0();
    let k = span(t, 0.6, 0.95);
    if k <= 0. || k >= 1. {
        return el;
    }
    let fade = (1. - k).powi(2);
    let grow = out_expo(k);
    let flash = span(t, 0.6, 0.4);
    if flash < 1. {
        el = el.child(
            div().absolute().left(px(cx)).top(px(cy)).size_0().child(
                div()
                    .absolute()
                    .left(px(-20.))
                    .top(px(-20.))
                    .size(px(40.))
                    .rounded_full()
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::white().opacity(0.85 * (1. - flash).powi(2)),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(90.),
                        spread_radius: px(20. + 60. * out_cubic(flash)),
                        inset: false,
                    }]),
            ),
        );
    }
    el = el
        .child(ring(
            cx,
            cy,
            20. + 330. * grow,
            1. + 2.5 * fade,
            pal::accent_light().opacity(0.8 * fade),
        ))
        .child(ring(
            cx,
            cy,
            10. + 210. * grow,
            1.,
            gpui::white().opacity(0.4 * fade),
        ));
    for i in 0..26u32 {
        let a = rnd(i, 21) * TAU;
        let d = (90. + 260. * rnd(i, 22)) * grow;
        let s = (1.5 + 2.5 * rnd(i, 23)) * (1. - k);
        el = el.child(
            div()
                .absolute()
                .left(px(cx + a.cos() * d - s / 2.))
                // Sparks sink as they cool.
                .top(px(cy + a.sin() * d + 60. * k * k - s / 2.))
                .size(px(s))
                .rounded_full()
                .bg(if i % 3 == 0 {
                    gpui::white()
                } else {
                    pal::accent_light()
                }
                .opacity(fade.sqrt())),
        );
    }
    el
}

/// The wave that sends the covers off.
fn wave(r: f32, cx: f32, cy: f32, w: f32, h: f32) -> Div {
    let reach = (w * w + h * h).sqrt() * 0.75;
    let mut el = div().absolute().inset_0().overflow_hidden();
    for (delay, strength) in [(0., 0.55), (0.09, 0.28)] {
        let k = span(r, delay, 0.8);
        if k > 0. && k < 1. {
            el = el.child(ring(
                cx,
                cy,
                30. + reach * out_cubic(k),
                2. + 26. * (1. - k),
                pal::accent_light().opacity(strength * (1. - k).powi(2)),
            ));
        }
    }
    el
}

/// "Arkor", letter by letter, drawing together.
fn wordmark(t: f32, r: f32, cy: f32) -> Div {
    let gather = out_expo(span(t, 0.78, 1.1));
    let mut row = div()
        .absolute()
        .left_0()
        .right_0()
        .top(px(cy + MARK * 0.2 - 20. * out_cubic(span(r, 0., 0.3))))
        .flex()
        .justify_center()
        .gap(px(lerp(26., 5., gather)))
        .text_size(px(46.))
        .font_weight(FontWeight::BOLD)
        .text_color(pal::text());
    for (i, letter) in ["A", "r", "k", "o", "r"].into_iter().enumerate() {
        let k = out_cubic(span(t, 0.78 + i as f32 * 0.07, 0.5));
        row = row.child(
            div()
                .relative()
                .top(px(26. * (1. - k)))
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
    fn curves_start_and_end_where_they_should() {
        for f in [out_cubic, out_expo, in_out, out_back] {
            assert!(f(0.).abs() < 0.01, "starts at 0");
            assert!((f(1.) - 1.).abs() < 0.001, "ends at 1");
        }
        assert_eq!(span(1., 2., 1.), 0.);
        assert_eq!(span(9., 2., 1.), 1.);
    }

    #[test]
    fn a_cover_lands_exactly_on_its_place() {
        let from = pose(3, 8, 2.4, 1320., 860.);
        let to = Pose {
            x: 430.,
            y: 520.,
            w: 144.,
            h: 216.,
            depth: 1.,
        };
        assert_eq!(flight(from, to, 0.), from, "leaves from the carousel");
        let end = flight(from, to, 1.);
        assert!((end.x - to.x).abs() < 0.01 && (end.y - to.y).abs() < 0.01);
        assert!((end.w - to.w).abs() < 0.01 && (end.h - to.h).abs() < 0.01);
    }

    #[test]
    fn the_carousel_stays_in_the_window() {
        for (w, h) in [(1000., 640.), (1320., 860.), (2560., 1440.)] {
            for i in 0..12 {
                for step in 0..40 {
                    let p = pose(i, 12, 1.9 + step as f32 * 0.1, w, h);
                    assert!(p.x - p.w / 2. > 0. && p.x + p.w / 2. < w, "{w}: {p:?}");
                    assert!(
                        p.y - p.h / 2. > 0. && p.y + p.h / 2. < h * 0.91,
                        "{h}: {p:?}"
                    );
                }
            }
        }
    }
}
