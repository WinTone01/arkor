//! Arkor's visual language (after Modus): near-black surfaces with hairline borders under
//! framed "glass" panels, one accent colour, white figures and grey captions. A light
//! palette exists too. Everything reads the current palette on each render, so changes in
//! Settings → Appearance apply at once.

use arkor_core::profile::Glass;
use gpui::prelude::FluentBuilder;
use gpui::{
    Background, Context, Div, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Svg, div, linear_color_stop,
    linear_gradient, px, rgb, svg,
};

/// Colours. Atomics, so the theme can change without threading state through every view.
pub mod pal {
    use super::Glass;
    use gpui::{Hsla, rgb};
    use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

    pub const DEFAULT_ACCENT: u32 = 0x8b5cf6;

    static ACCENT: AtomicU32 = AtomicU32::new(DEFAULT_ACCENT);
    static GLASS: AtomicU8 = AtomicU8::new(0);
    static LIGHT: AtomicBool = AtomicBool::new(false);

    /// Accent presets: (colour, English name, Turkish name).
    pub const ACCENTS: [(u32, &str, &str); 8] = [
        (DEFAULT_ACCENT, "Violet", "Mor"),
        (0x3b82f6, "Ocean", "Okyanus"),
        (0xef4444, "Crimson", "Kızıl"),
        (0x10b981, "Emerald", "Zümrüt"),
        (0xf59e0b, "Amber", "Kehribar"),
        (0xec4899, "Rose", "Gül"),
        (0x06b6d4, "Cyan", "Camgöbeği"),
        (0xa1a1aa, "Mono", "Gri"),
    ];

    pub fn set(light: bool, accent: u32, glass: Glass) {
        LIGHT.store(light, Ordering::Relaxed);
        ACCENT.store(accent, Ordering::Relaxed);
        GLASS.store(
            Glass::ALL.iter().position(|g| *g == glass).unwrap_or(0) as u8,
            Ordering::Relaxed,
        );
    }

    pub fn is_light() -> bool {
        LIGHT.load(Ordering::Relaxed)
    }

    pub fn glass() -> Glass {
        Glass::ALL
            .get(GLASS.load(Ordering::Relaxed) as usize)
            .copied()
            .unwrap_or_default()
    }

    fn c(hex: u32) -> Hsla {
        rgb(hex).into()
    }
    fn pick(dark: u32, light: u32) -> Hsla {
        c(if is_light() { light } else { dark })
    }
    pub fn lift(c: Hsla, dl: f32) -> Hsla {
        Hsla {
            l: (c.l + dl).clamp(0., 0.95),
            ..c
        }
    }

    pub fn bg() -> Hsla {
        pick(0x030304, 0xeef0f4)
    }
    pub fn panel() -> Hsla {
        pick(0x09090c, 0xf7f8fa)
    }
    pub fn card() -> Hsla {
        pick(0x0d0d11, 0xffffff)
    }
    pub fn raised() -> Hsla {
        pick(0x15151a, 0xe9ebf0)
    }
    pub fn border() -> Hsla {
        pick(0x1f1f25, 0xdcdfe5)
    }
    pub fn border_strong() -> Hsla {
        pick(0x2b2b33, 0xc8ccd4)
    }
    pub fn text() -> Hsla {
        pick(0xf4f4f5, 0x16171b)
    }
    /// Secondary text that still reads as body text.
    pub fn text_soft() -> Hsla {
        pick(0xd4d4d8, 0x33363d)
    }
    pub fn muted() -> Hsla {
        pick(0x8b8b96, 0x5d616b)
    }
    pub fn faint() -> Hsla {
        pick(0x5d5d68, 0x8b909a)
    }
    pub fn accent() -> Hsla {
        c(ACCENT.load(Ordering::Relaxed))
    }
    /// Accent for text and small marks (lighter on dark, darker on light).
    pub fn accent_light() -> Hsla {
        lift(accent(), if is_light() { -0.1 } else { 0.12 })
    }
    pub fn accent_hi() -> Hsla {
        lift(accent(), 0.1)
    }
    pub fn accent_lo() -> Hsla {
        lift(accent(), -0.18)
    }
    /// Readable text on an accent fill.
    pub fn on_accent() -> Hsla {
        if accent().l > 0.68 {
            c(0x09090b)
        } else {
            c(0xffffff)
        }
    }
    pub fn danger() -> Hsla {
        c(0xf0716f)
    }
    pub fn warning() -> Hsla {
        c(0xfbbf24)
    }
    pub fn success() -> Hsla {
        c(0x34d399)
    }

    /// Fill, border and corner radius of panels and cards for the chosen glass style.
    pub struct GlassSpec {
        pub panel: Hsla,
        pub card: Hsla,
        pub border: Hsla,
        pub panel_radius: f32,
        pub card_radius: f32,
    }

    pub fn glass_spec() -> GlassSpec {
        let tint = |a: f32| {
            if is_light() {
                c(0x000000).opacity(a)
            } else {
                c(0xffffff).opacity(a)
            }
        };
        match glass() {
            Glass::Acrylic => GlassSpec {
                panel: panel().opacity(if is_light() { 0.75 } else { 0.6 }),
                card: card().opacity(if is_light() { 0.85 } else { 0.72 }),
                border: border(),
                panel_radius: 22.,
                card_radius: 18.,
            },
            Glass::Crystal => GlassSpec {
                panel: panel().opacity(if is_light() { 0.45 } else { 0.3 }),
                card: card().opacity(if is_light() { 0.55 } else { 0.38 }),
                border: tint(0.13),
                panel_radius: 12.,
                card_radius: 9.,
            },
            Glass::Smoked => GlassSpec {
                panel: panel().opacity(0.92),
                card: card().opacity(0.96),
                border: border_strong(),
                panel_radius: 20.,
                card_radius: 16.,
            },
            Glass::Soft => GlassSpec {
                panel: tint(0.035),
                card: tint(0.05),
                border: tint(0.025),
                panel_radius: 28.,
                card_radius: 24.,
            },
        }
    }
}

/// UI font, bundled so every machine renders the same (Inter, SIL Open Font License).
pub const FONT: &str = "Inter";

pub fn load_fonts(cx: &mut gpui::App) {
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        include_bytes!("../../assets/fonts/Inter-Regular.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../assets/fonts/Inter-Medium.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../assets/fonts/Inter-Bold.ttf")
            .as_slice()
            .into(),
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

pub fn shade(c: Hsla, dl: f32) -> Hsla {
    Hsla {
        l: (c.l + dl).clamp(0., 1.),
        ..c
    }
}

/// A Lucide icon by name (e.g. `"gamepad-2"`).
pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(SharedString::from(format!("arkor/{name}.svg")))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// The outer framed panels (sidebar, content).
pub fn panel() -> Div {
    let g = pal::glass_spec();
    div()
        .rounded(px(g.panel_radius))
        .border_1()
        .border_color(g.border)
        .bg(g.panel)
}

/// A card inside the content panel.
pub fn card() -> Div {
    let g = pal::glass_spec();
    div()
        .rounded(px(g.card_radius))
        .border_1()
        .border_color(g.border)
        .bg(g.card)
}

/// The accent gradient of the logo and highlighted tiles.
pub fn accent_gradient() -> Background {
    linear_gradient(
        135.,
        linear_color_stop(pal::accent_hi(), 0.),
        linear_color_stop(pal::accent_lo(), 1.),
    )
}

/// The Arkor mark: an accent tile with an "A".
pub fn logo(size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(size * 0.3))
        .bg(accent_gradient())
        .text_color(pal::on_accent())
        .text_size(px(size * 0.55))
        .font_weight(gpui::FontWeight::BOLD)
        .child("A")
}

/// Compact rounded button. `primary` is the filled look for a row's main action.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    primary: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap_1p5()
        .px_3()
        .py_1p5()
        .rounded(px(9.))
        .text_sm()
        .font_weight(gpui::FontWeight::MEDIUM)
        .cursor_pointer()
        .border_1()
        .when(primary, |s| {
            s.bg(pal::accent())
                .border_color(pal::accent())
                .text_color(pal::on_accent())
                .hover(|h| h.opacity(0.88))
        })
        .when(!primary, |s| {
            s.border_color(pal::border_strong())
                .bg(pal::raised().opacity(0.6))
                .text_color(pal::text_soft())
                .hover(|h| h.bg(pal::raised()).border_color(pal::accent().opacity(0.5)))
        })
        .child(label.into())
}

/// Segmented tab strip in a framed card. `tabs` are (icon, label).
pub fn tab_strip<V: 'static>(
    id: &'static str,
    tabs: &[(&'static str, &'static str)],
    selected: usize,
    cx: &mut Context<V>,
    on_select: impl Fn(&mut V, usize, &mut Context<V>) + Clone + 'static,
) -> Div {
    card()
        .p_1p5()
        .flex()
        .flex_wrap()
        .gap_1()
        .children(tabs.iter().enumerate().map(|(i, &(icon_name, label))| {
            let active = i == selected;
            let on_select = on_select.clone();
            div()
                .id((id, i))
                .flex()
                .items_center()
                .gap_2()
                .px_3p5()
                .py_2()
                .rounded(px(12.))
                .text_sm()
                .font_weight(gpui::FontWeight::MEDIUM)
                .cursor_pointer()
                .when(active, |s| {
                    s.bg(pal::text().opacity(0.07))
                        .text_color(pal::accent_light())
                })
                .when(!active, |s| {
                    s.text_color(pal::muted())
                        .hover(|h| h.text_color(pal::text()).bg(pal::text().opacity(0.04)))
                })
                .child(icon(
                    icon_name,
                    15.,
                    if active {
                        pal::accent_light()
                    } else {
                        pal::muted()
                    },
                ))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| on_select(this, i, cx)))
        }))
}

/// A small rounded tag, e.g. "On" or "Needs AMD".
pub fn tag(label: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .flex_none()
        .px_2()
        .py_0p5()
        .rounded_full()
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(color)
        .bg(color.opacity(0.12))
        .border_1()
        .border_color(color.opacity(0.25))
        .child(label.into())
}

/// Text colour for plain `u32` hex values (kept for older call sites).
pub fn hex(c: u32) -> Hsla {
    rgb(c).into()
}

/// One entry of a `side_nav`: icon, label and an optional count shown on the right.
pub struct NavEntry {
    pub icon: &'static str,
    pub label: &'static str,
    pub badge: Option<String>,
}

impl NavEntry {
    pub fn new(icon: &'static str, label: &'static str) -> Self {
        Self {
            icon,
            label,
            badge: None,
        }
    }
}

/// A vertical list of a page's sections (Settings, a game's settings, Add game). The page
/// draws the selected section to its right, so every page with sections works the same way.
pub fn side_nav<V: 'static>(
    id: &'static str,
    entries: Vec<NavEntry>,
    selected: Option<usize>,
    cx: &mut Context<V>,
    on_select: impl Fn(&mut V, usize, &mut Context<V>) + Clone + 'static,
) -> Div {
    div()
        .w(px(208.))
        .flex_none()
        .flex()
        .flex_col()
        .gap_0p5()
        .children(entries.into_iter().enumerate().map(|(i, e)| {
            let active = selected == Some(i);
            let on_select = on_select.clone();
            div()
                .id((id, i))
                .flex()
                .items_center()
                .gap_2p5()
                .px_3()
                .py_2()
                .rounded(px(10.))
                .text_sm()
                .cursor_pointer()
                .border_1()
                .when(active, |s| {
                    s.bg(pal::accent().opacity(0.12))
                        .border_color(pal::accent().opacity(0.35))
                        .text_color(pal::text())
                        .font_weight(gpui::FontWeight::MEDIUM)
                })
                .when(!active, |s| {
                    s.border_color(gpui::transparent_black())
                        .text_color(pal::muted())
                        .hover(|h| h.text_color(pal::text()).bg(pal::text().opacity(0.05)))
                })
                .child(icon(
                    e.icon,
                    16.,
                    if active {
                        pal::accent_light()
                    } else {
                        pal::muted()
                    },
                ))
                .child(div().flex_1().min_w_0().truncate().child(e.label))
                .children(e.badge.map(|b| {
                    div()
                        .px_1p5()
                        .rounded_full()
                        .text_xs()
                        .bg(pal::accent().opacity(0.2))
                        .text_color(pal::accent_light())
                        .child(b)
                }))
                .on_click(cx.listener(move |this, _, _, cx| on_select(this, i, cx)))
        }))
}

/// A page with sections: `nav` on the left, the selected section's content on the right.
pub fn with_side_nav(nav: impl IntoElement, content: impl IntoElement) -> Div {
    div().flex().items_start().gap_5().child(nav).child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_4()
            .child(content),
    )
}

/// A back link, e.g. "← Library" above a sub-page.
pub fn back_link(id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .px_2()
        .py_1()
        .rounded(px(8.))
        .text_sm()
        .text_color(pal::muted())
        .cursor_pointer()
        .hover(|s| s.text_color(pal::text()).bg(pal::text().opacity(0.05)))
        .child(icon("arrow-left", 15., pal::muted()))
        .child(label.into())
}
