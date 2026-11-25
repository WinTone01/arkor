//! How console mode looks. Sizes are in units of a 1080-line screen (`u`), so it fills a
//! television and a small window alike.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, BoxShadow, Context, Div, FontWeight, Window, div, linear_color_stop,
    linear_gradient, point, prelude::*, px, relative,
};

use super::console::{Act, Art, Item, Kind, Look, Nav, Overlay, Row, Screen, Value};
use super::cover_art::{self, Size};
use super::style::{self, pal};
use super::{Library, downloads_page, gamepad};

/// How much bigger the focused item is.
const GROW: f32 = 0.07;
const FOCUS_TIME: f32 = 0.15;
const ENTER_TIME: f32 = 0.24;
const FADE_TIME: f32 = 0.5;
/// The focus has to rest this long before the picture behind changes.
const REST: Duration = Duration::from_millis(70);

/// Where things are on this window.
#[derive(Clone, Copy)]
struct Frame {
    vw: f32,
    vh: f32,
    u: f32,
    margin: f32,
    width: f32,
    /// Share of the way eased positions move in this frame.
    ease: f32,
    still: bool,
}

/// Several lines beside each other: the column in a line's id.
const LINE_COL: usize = 1_000_000;

impl Frame {
    /// One tile of a grid, and the gap between two.
    fn tile(&self, cols: usize) -> (f32, f32) {
        let gap = 28. * self.u;
        let w = (self.width - gap * (cols.max(1) - 1) as f32) / cols.max(1) as f32;
        (w.floor(), gap)
    }

    fn cols(&self) -> usize {
        let gap = 28. * self.u;
        (((self.width + gap) / (206. * self.u + gap)).floor() as usize).max(3)
    }

    fn shelf_tile(&self) -> (f32, f32) {
        (214. * self.u, 321. * self.u)
    }

    fn row_height(&self, row: &Row, cols: usize) -> f32 {
        let u = self.u;
        let body = match row.kind {
            Kind::Shelf => self.shelf_tile().1 + 92. * u,
            Kind::Grid => self.tile(cols).0 * 1.5 + 88. * u,
            Kind::Pills => 88. * u,
            Kind::Keys => 78. * u,
            Kind::Line => {
                let tall = row.items.iter().any(|i| {
                    !i.sub.is_empty() || matches!(i.look, Look::Line { art: Some(_), .. })
                });
                if tall { 108. * u } else { 84. * u }
            }
        };
        body + if row.title.is_some() { 56. * u } else { 0. }
    }
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// The share of `secs` that went by since `at` (1 when it never began).
fn since(at: Option<Instant>, secs: f32) -> f32 {
    at.map_or(1., |t| (t.elapsed().as_secs_f32() / secs).min(1.))
}

impl Library {
    pub(super) fn render_console(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let size = window.viewport_size();
        let (vw, vh) = (f32::from(size.width), f32::from(size.height));
        let u = (vh / 1080.).min(vw / 1720.).max(0.42);
        let scale = window.scale_factor();
        self.console.screen_px = ((vw * scale).round() as u32, (vh * scale).round() as u32);
        if std::mem::take(&mut self.console.come_back) && !window.is_window_active() {
            window.activate_window();
        }
        let still = self.global.appearance.reduce_motion;
        let now = Instant::now();
        // After a rest the time since the last frame says nothing: a move begins with one
        // frame's step instead of jumping half the way.
        let dt = match self.console.drawn {
            None => 1.,
            Some(_) if !self.console.unsettled => 0.008,
            Some(t) => (now - t).as_secs_f32().min(0.05),
        };
        self.console.drawn = Some(now);
        let f = Frame {
            vw,
            vh,
            u,
            margin: 88. * u,
            width: vw - 176. * u,
            ease: if still { 1. } else { 1. - (-dt * 13.).exp() },
            still,
        };
        self.console.cols = f.cols();
        self.console.unsettled = false;
        if self
            .console
            .trimmed
            .is_none_or(|t| t.elapsed() > Duration::from_secs(3))
        {
            self.console.trimmed = Some(now);
            cover_art::trim_wide(window);
            cover_art::trim("arkor/stores", 140, window);
        }
        if self.console_shows_disks() {
            self.console_look_drives(cx);
        }
        let rows = self.console_rows();
        // Never lost: what had the focus may be gone.
        self.console_focus(&rows).step(&rows, 0, 0);
        let item = self.focused_item(&rows);
        self.step_behind(&item, f, cx);
        self.warm_behind(&rows);
        for (at, secs) in [
            (self.console.moved, FOCUS_TIME),
            (self.console.entered, ENTER_TIME),
            (self.console.behind_at, FADE_TIME),
        ] {
            if !still && since(at, secs) < 1. {
                self.console.unsettled = true;
            }
        }
        let entered = if still {
            1.
        } else {
            ease_out(since(self.console.entered, ENTER_TIME))
        };
        let spot = self.spotlight(&item, f);
        let top = if spot.is_some() { 470. * u } else { 150. * u };
        let body = match (self.console.screen(), self.shops_preparing()) {
            (Screen::Stores, Some((done, total))) => div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(top))
                .bottom(px(78. * u))
                .flex()
                .items_center()
                .justify_center()
                .child(super::loader::block(
                    self.tr("Getting your games ready", "Oyunların hazırlanıyor"),
                    Some(format!("{done} / {total}")),
                    still,
                ))
                .into_any_element(),
            _ => self.console_body(&rows, top, f, cx),
        };
        // The start-up animation: the console comes in when it opens.
        if self.step_console_intro(window) {
            self.console.unsettled = true;
        }
        match self.console_intro_opens() {
            Some(true) if std::mem::take(&mut self.console.intro_covers) => {
                self.console.entered = Some(now);
            }
            Some(false) => self.console.intro_covers = true,
            _ => {}
        }
        let logo = 48. * u;
        let intro =
            self.render_console_intro((vw, vh), (f.margin + logo / 2., self.logo_y(u), logo), cx);
        let over = self.console_overlay_view(f, cx);
        let launching = self.console_launching(f, window, cx);
        if self.console.unsettled {
            if still {
                window.request_animation_frame();
            } else {
                self.keep_moving(window, cx);
            }
        }
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .text_size(px(22. * u))
            .on_scroll(cx.listener(|this, e: &gpui::ScrollEvent, w, cx| {
                let dy = f32::from(e.delta.pixel_delta(px(40.)).y);
                // Touchpads send many small steps.
                let due = this
                    .console
                    .moved
                    .is_none_or(|t| t.elapsed() > Duration::from_millis(110));
                if dy.abs() > 1. && due {
                    this.console.pad = false;
                    this.nav(if dy > 0. { Nav::Up } else { Nav::Down }, w, cx);
                }
            }))
            .child(self.render_behind(f, spot.is_some()))
            .child(
                div().absolute().inset_0().opacity(entered.max(0.02)).child(
                    div()
                        .absolute()
                        .inset_0()
                        .top(px(26. * u * (1. - entered)))
                        .children(spot)
                        .child(body),
                ),
            )
            .child(self.console_top(f, cx))
            .children(over)
            .children(self.render_shop_login(f, cx))
            .child(self.console_footer(&item, f, cx))
            .children(launching)
            .children(intro)
    }

    /// A store's login (its page is a window of its own; this is what is around it).
    fn render_shop_login(&mut self, f: Frame, cx: &mut Context<Self>) -> Option<AnyElement> {
        use super::shops::LoginMode;
        let u = f.u;
        let l = self.shops.login.as_ref()?;
        let (shop, mode, busy, error) = (l.shop, l.mode, l.busy, l.error.clone());
        let (no_webkit, known) = (l.no_webkit, l.login.is_some());
        let waiting = busy || (error.is_none() && mode != LoginMode::Closed);
        let (title, text) = if no_webkit {
            (
                self.tr(
                    "The login window needs WebKitGTK",
                    "Giriş penceresi WebKitGTK istiyor",
                ),
                self.tr(
                    "The package webkit2gtk-4.1 is not installed.",
                    "webkit2gtk-4.1 paketi kurulu değil.",
                ),
            )
        } else if busy && known {
            (
                self.tr("Logging in…", "Giriş yapılıyor…"),
                self.tr("Almost there.", "Neredeyse bitti."),
            )
        } else if waiting && mode == LoginMode::Browser {
            (
                self.tr("Log in in your browser", "Tarayıcında giriş yap"),
                self.tr(
                    "Then copy the page (Epic) or its address (GOG, Amazon): Arkor takes it.",
                    "Sonra sayfayı (Epic) ya da adresini (GOG, Amazon) kopyala: Arkor alır.",
                ),
            )
        } else if waiting {
            (
                self.tr("Opening the login window…", "Giriş penceresi açılıyor…"),
                "",
            )
        } else if error.is_some() {
            (self.tr("The login did not work", "Giriş olmadı"), "")
        } else {
            (
                self.tr("The login window was closed", "Giriş penceresi kapatıldı"),
                self.tr("Open it again to log in.", "Giriş yapmak için yeniden aç."),
            )
        };
        let error = error.filter(|_| !no_webkit).map(|e| self.shop_error(&e));
        let rows = self.login_rows();
        self.console.over_focus.step(&rows, 0, 0);
        let col = self.console.over_focus.col();
        let mut pills = div().flex().flex_wrap().justify_center().gap(px(14. * u));
        for (c, item) in rows[0].items.iter().enumerate() {
            let glow = if c == col { 1. } else { 0. };
            pills = pills.child(self.console_pill(item, ("over-login", 0, c), glow, f, cx));
        }
        let color = super::shops_page::shop_color(shop);
        Some(
            div()
                .id("console-login")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.68))
                .child(
                    div()
                        .w(px((860. * u).min(f.vw * 0.9)))
                        .rounded(px(28. * u))
                        .overflow_hidden()
                        .border_1()
                        .border_color(pal::border_strong())
                        .bg(pal::bg())
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .w_full()
                                .h(px(230. * u))
                                .rounded_t(px(28. * u))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(linear_gradient(
                                    180.,
                                    linear_color_stop(color.opacity(0.55), 0.),
                                    linear_color_stop(color.opacity(0.), 1.),
                                ))
                                .child(
                                    div()
                                        .relative()
                                        .size(px(180. * u))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(waiting, |d| {
                                            d.child(
                                                div().absolute().inset_0().child(
                                                    super::loader::spinner(180. * u, f.still),
                                                ),
                                            )
                                        })
                                        .child(super::shops_page::shop_mark(shop, 92. * u)),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .px(px(48. * u))
                                .pb(px(48. * u))
                                .flex()
                                .flex_col()
                                .items_center()
                                .text_center()
                                .gap(px(16. * u))
                                .child(
                                    div()
                                        .text_size(px(34. * u))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(title),
                                )
                                .when(!text.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .text_size(px(22. * u))
                                            .text_color(pal::muted())
                                            .child(text),
                                    )
                                })
                                .children(error.map(|e| {
                                    div()
                                        .w_full()
                                        .text_size(px(21. * u))
                                        .text_color(pal::warning())
                                        .child(e)
                                }))
                                .child(div().pt(px(18. * u)).child(pills)),
                        ),
                )
                .into_any_element(),
        )
    }

    // ---- Behind everything ----

    /// Decodes the pictures of what is next to the focus, so they are there when it moves.
    fn warm_behind(&mut self, rows: &[Row]) {
        if !matches!(
            self.console.screen(),
            Screen::Home | Screen::Library | Screen::Stores
        ) {
            return;
        }
        let f = self.console.focus();
        let mut near = Vec::new();
        for (dr, dc) in [(0i32, -1i32), (0, 1), (-1, 0), (1, 0), (0, 2)] {
            let r = f.row as i32 + dr;
            let Some(row) = usize::try_from(r).ok().and_then(|r| rows.get(r)) else {
                continue;
            };
            let col = if dr == 0 {
                f.col() as i32 + dc
            } else {
                // Another row remembers its own column; near enough.
                f.col() as i32
            };
            if let Some(Item {
                look: Look::Cover(art),
                ..
            }) = usize::try_from(col).ok().and_then(|c| row.items.get(c))
            {
                near.push(art.clone());
            }
        }
        // Only a decode is started here, no download (`shop_wide` fetches when missing).
        let mut cx_free = Vec::new();
        for art in near {
            let path = match &art {
                Art::Game(id) => match self.library.get(id).map(|g| g.kind.clone()) {
                    Some(arkor_core::library::GameKind::Steam { appid }) => {
                        self.steam_hero(&appid).map(|p| (p, Size::Wide))
                    }
                    _ => None,
                },
                Art::Shop(shop, id) => {
                    arkor_launch::shops::covers::wide(*shop, id).map(|p| (p, Size::Wide))
                }
            };
            cx_free.extend(path);
        }
        for (path, size) in cx_free {
            cover_art::backdrop_ready(&path, size);
        }
    }

    /// Follows the focus with the picture behind the screen, when it rests.
    fn step_behind(&mut self, item: &Option<Item>, f: Frame, cx: &mut Context<Self>) {
        let art = match self.console.screen() {
            Screen::Game(id) | Screen::GameSettings(id) | Screen::GameCat(id, _) => {
                Some(Art::Game(id))
            }
            Screen::ShopGame(shop, id) => Some(Art::Shop(shop, id)),
            _ => match item.as_ref().map(|i| &i.look) {
                Some(Look::Cover(art)) => Some(art.clone()),
                // Over buttons and lines the last picture stays.
                _ => None,
            },
        };
        let now = Instant::now();
        if let Some(art) = art
            && self
                .console
                .behind_art
                .as_ref()
                .is_none_or(|(a, _)| *a != art)
        {
            self.console.behind_art = Some((art, now));
        }
        let Some((art, at)) = self.console.behind_art.clone() else {
            return;
        };
        // Resting on a store game: its store answer is asked for already.
        if let Art::Shop(shop, id) = &art
            && at.elapsed() > Duration::from_millis(600)
        {
            self.shop_prefetch_info(*shop, id, cx);
        }
        if at.elapsed() < REST && self.console.behind.is_some() && !f.still {
            self.console.unsettled = true;
            return;
        }
        let wanted = self.console_behind(&art, cx);
        if wanted == self.console.behind {
            return;
        }
        // One fade at a time, and only to what can be drawn.
        if let Some(b) = &wanted
            && !cover_art::backdrop_ready(&b.path, b.size)
        {
            self.console.unsettled = true;
            return;
        }
        if since(self.console.behind_at, FADE_TIME) < 1. && !f.still {
            return;
        }
        self.console.behind_before = self.console.behind.take();
        self.console.behind = wanted;
        self.console.behind_at = Some(now);
    }

    fn render_behind(&self, f: Frame, spotlight: bool) -> Div {
        let t = if f.still {
            1.
        } else {
            since(self.console.behind_at, FADE_TIME)
        };
        let strength = if spotlight { 1. } else { 0.45 };
        let picture = |b: &super::console::Behind, opacity: f32| {
            let (h, o) = match b.size {
                Size::Blur => (f.vh, 0.5),
                _ => (f.vh * 0.74, 0.85),
            };
            // No wide picture: the cover stands at the right like a poster, its colours
            // softly filling the room around it.
            if b.size == Size::Cover {
                let ph = f.vh * 0.6;
                let pw = ph / 1.5;
                return div()
                    .absolute()
                    .inset_0()
                    .opacity((opacity * strength).max(0.001))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .opacity(0.35)
                            .child(cover_art::backdrop(b.path.clone(), Size::Blur, 0.).size_full()),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(f.vh * 0.1))
                            .right(px(f.margin))
                            .w(px(pw))
                            .h(px(ph))
                            .opacity(0.9)
                            .rounded(px(22. * f.u))
                            .shadow(vec![BoxShadow {
                                color: gpui::black().opacity(0.6),
                                offset: point(px(0.), px(30. * f.u)),
                                blur_radius: px(90. * f.u),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .child(cover_art::cover(b.path.clone(), 22. * f.u, false).size_full()),
                    );
            }
            div()
                .absolute()
                .top_0()
                .left_0()
                .w(px(f.vw))
                .h(px(h))
                .opacity((opacity * o * strength).max(0.001))
                .child(cover_art::backdrop(b.path.clone(), b.size, 0.).size_full())
        };
        let mut out = div().absolute().inset_0().bg(pal::bg());
        if self.console.behind.is_none() {
            out = out.child(super::backdrop::glow(
                0.78,
                -0.06,
                1100. * f.u,
                420. * f.u,
                260. * f.u,
                pal::accent().opacity(0.28),
            ));
        }
        if t < 1.
            && let Some(b) = &self.console.behind_before
        {
            out = out.child(picture(b, 1. - t));
        }
        if let Some(b) = &self.console.behind {
            out = out.child(picture(b, t));
        }
        // A wide picture ends in the background, not at an edge.
        let wide = [&self.console.behind, &self.console.behind_before]
            .into_iter()
            .flatten()
            .any(|b| b.size == Size::Wide);
        if wide {
            out = out.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(f.vh * 0.24))
                    .h(px(f.vh * 0.5 + 2.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(pal::bg().opacity(0.), 0.),
                        linear_color_stop(pal::bg(), 1.),
                    )),
            );
        }
        // Text stays readable: darker to the left and at the top.
        out.child(div().absolute().inset_0().bg(linear_gradient(
            90.,
            linear_color_stop(pal::bg().opacity(0.82), 0.),
            linear_color_stop(pal::bg().opacity(0.), 0.75),
        )))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(170. * f.u))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(pal::bg().opacity(0.75), 0.),
                    linear_color_stop(pal::bg().opacity(0.), 1.),
                )),
        )
    }

    // ---- The top and the bottom ----

    /// The middle of the logo at the top (the tabs beside it are a little taller).
    fn logo_y(&self, u: f32) -> f32 {
        40. * u + 10. * u + 25. * u * 1.618 / 2.
    }

    fn console_top(&self, f: Frame, cx: &mut Context<Self>) -> Div {
        let u = f.u;
        let tab = self.console.tab();
        let mut tabs = div().flex().items_center().gap(px(10. * u));
        for (ix, screen) in Screen::TABS.into_iter().enumerate() {
            let label = match screen {
                Screen::Home => self.tr("Home", "Ana sayfa"),
                Screen::Library => self.tr("Library", "Kütüphane"),
                Screen::Stores => self.tr("Stores", "Mağazalar"),
                Screen::Downloads => self.tr("Downloads", "İndirilenler"),
                _ => self.tr("Settings", "Ayarlar"),
            };
            let on = tab == screen;
            tabs = tabs.child(
                div()
                    .id(("console-tab", ix))
                    .px(px(20. * u))
                    .py(px(10. * u))
                    .rounded_full()
                    .text_size(px(25. * u))
                    .font_weight(if on {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .cursor_pointer()
                    .when(on, |d| d.bg(pal::text()).text_color(pal::bg()))
                    .when(!on, |d| {
                        d.text_color(pal::text_soft())
                            .hover(|h| h.bg(pal::text().opacity(0.1)))
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shops.game = None;
                        this.console_open(screen.clone(), cx)
                    })),
            );
        }
        let mut right = div()
            .flex()
            .items_center()
            .gap(px(26. * u))
            .text_size(px(23. * u))
            .text_color(pal::text_soft());
        // What is downloading.
        let active = arkor_launch::progress::list();
        if let Some(e) = active.iter().find(|e| e.fraction().is_some()) {
            right = right.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8. * u))
                    .child(style::icon("download", 22. * u, pal::accent_light()))
                    .child(self.percent(e.fraction().unwrap_or(0.))),
            );
        }
        if let Some(pad) = gamepad::pad()
            && let Some((percent, charging)) = pad.battery
        {
            let icon = match (charging, percent) {
                (true, _) => "battery-charging",
                (_, 70..) => "battery-full",
                (_, 30..) => "battery-medium",
                _ => "battery-low",
            };
            let color = if percent < 20 && !charging {
                pal::warning()
            } else {
                pal::text_soft()
            };
            right = right.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8. * u))
                    .child(style::icon("gamepad-2", 22. * u, color))
                    .child(style::icon(icon, 24. * u, color)),
            );
        }
        right = right.child(
            div()
                .text_color(pal::text())
                .font_weight(FontWeight::MEDIUM)
                .child(clock()),
        );
        let hint = |pad: &'static str, key: &'static str| {
            self.console_glyph(if self.console.pad { pad } else { key }, f)
        };
        div()
            .absolute()
            .top(px(40. * u))
            .left(px(f.margin))
            .right(px(f.margin))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(22. * u))
                    .child(
                        // The start-up animation brings it.
                        div()
                            .opacity(if self.console.intro.is_some() { 0. } else { 1. })
                            .child(style::logo(48. * u)),
                    )
                    .child(hint("LB", "⇧Tab"))
                    .child(tabs)
                    .child(hint("RB", "Tab")),
            )
            .child(right)
    }

    /// A button or key as drawn in hints.
    fn console_glyph(&self, text: &'static str, f: Frame) -> Div {
        let u = f.u;
        div()
            .h(px(32. * u))
            .min_w(px(32. * u))
            .px(px(8. * u))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(if text.chars().count() == 1 { 16. } else { 9. } * u))
            .border_1()
            .border_color(pal::text().opacity(0.35))
            .bg(pal::bg().opacity(0.5))
            .text_size(px(17. * u))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(pal::text_soft())
            .child(text)
    }

    fn console_footer(&self, item: &Option<Item>, f: Frame, cx: &mut Context<Self>) -> Div {
        let u = f.u;
        let pad = self.console.pad;
        let mut hints: Vec<(&'static str, &'static str, Nav)> = Vec::new();
        let keyboard = self.console.overlay == Some(Overlay::Keyboard);
        let login = self.shops.login.is_some();
        hints.push((
            if pad { "A" } else { "↵" },
            self.tr("Choose", "Seç"),
            Nav::Accept,
        ));
        if self.console.overlay.is_none()
            && !login
            && let Some(Item {
                look:
                    Look::Line {
                        value: Value::Choice(_),
                        ..
                    },
                ..
            }) = item
        {
            hints.push(("◀ ▶", self.tr("Change", "Değiştir"), Nav::Right));
        }
        let folder = matches!(self.console.overlay, Some(Overlay::Folder(..)));
        if folder {
            hints.push((
                if pad { "X" } else { "Del" },
                self.tr("Choose this folder", "Bu klasörü seç"),
                Nav::Extra,
            ));
        }
        if keyboard {
            hints.push((
                if pad { "X" } else { "⌫" },
                self.tr("Delete", "Sil"),
                Nav::Extra,
            ));
            if pad {
                hints.push(("Y", self.tr("Space", "Boşluk"), Nav::Search));
            }
        } else if self.console.overlay.is_none() && !login {
            let extra = item.as_ref().and_then(|i| self.extra(i)).map(|a| match a {
                Act::Stop(_) => self.tr("Stop", "Durdur"),
                Act::CancelDownload(_) => self.tr("Cancel", "İptal et"),
                _ => self.tr("Play", "Oyna"),
            });
            if let Some(text) = extra {
                hints.push((if pad { "X" } else { "Del" }, text, Nav::Extra));
            }
            hints.push((
                if pad { "Y" } else { "Ctrl F" },
                self.tr("Search", "Ara"),
                Nav::Search,
            ));
        }
        hints.push((
            if pad { "B" } else { "Esc" },
            if folder {
                self.tr("Up", "Üst klasör")
            } else {
                self.tr("Back", "Geri")
            },
            Nav::Back,
        ));
        if !login {
            hints.push((
                if pad { "☰" } else { "F10" },
                if keyboard {
                    self.tr("Done", "Tamam")
                } else if folder {
                    self.tr("Cancel", "Vazgeç")
                } else {
                    self.tr("Menu", "Menü")
                },
                Nav::Menu,
            ));
        }
        let mut right = div().flex().items_center().gap(px(30. * u));
        for (ix, (glyph, text, nav)) in hints.into_iter().enumerate() {
            right = right.child(
                div()
                    .id(("console-hint", ix))
                    .flex()
                    .items_center()
                    .gap(px(10. * u))
                    .cursor_pointer()
                    .child(self.console_glyph(glyph, f))
                    .child(div().text_color(pal::text_soft()).child(text))
                    .on_click(cx.listener(move |this, _, w, cx| this.nav(nav, w, cx))),
            );
        }
        div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(78. * u))
            .px(px(f.margin))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(40. * u))
            .text_size(px(20. * u))
            .bg(linear_gradient(
                180.,
                linear_color_stop(pal::bg().opacity(0.), 0.),
                linear_color_stop(pal::bg().opacity(0.95), 0.45),
            ))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(pal::muted())
                    .children(self.status.clone()),
            )
            .child(right.flex_none())
    }

    // ---- The focused game, large ----

    fn spotlight(&self, item: &Option<Item>, f: Frame) -> Option<Div> {
        let u = f.u;
        let (title, mut meta): (String, Vec<String>) = match self.console.screen() {
            Screen::Home => match item.as_ref().map(|i| &i.act)? {
                Act::Screen(Screen::Game(id)) => self.spotlight_game(id)?,
                Act::Screen(Screen::ShopGame(shop, _)) => {
                    (item.as_ref()?.label.clone(), vec![shop.name().to_string()])
                }
                _ => ("Arkor".into(), Vec::new()),
            },
            Screen::Game(id) => self.spotlight_game(&id)?,
            Screen::GameSettings(id) => {
                let (title, _) = self.spotlight_game(&id)?;
                (title, vec![self.tr("Settings", "Ayarlar").to_string()])
            }
            Screen::GameCat(id, c) => {
                let (title, _) = self.spotlight_game(&id)?;
                (
                    title,
                    vec![
                        self.tr("Settings", "Ayarlar").to_string(),
                        self.game_cat_name(c).to_string(),
                    ],
                )
            }
            Screen::ShopGame(shop, id) => {
                let title = self
                    .shops
                    .find(shop, &id)
                    .map(|g| g.title.clone())
                    .or_else(|| self.shops.installed(shop, &id).map(|g| g.title.clone()))
                    .unwrap_or_default();
                let mut meta = vec![shop.name().to_string()];
                if let Some(g) = self.shops.installed(shop, &id) {
                    meta.push(self.tr("Installed", "Kurulu").into());
                    if let Some(s) = g.size {
                        meta.push(arkor_launch::progress::human_bytes(s));
                    }
                }
                (title, meta)
            }
            _ => return None,
        };
        meta.retain(|m| !m.is_empty());
        let long = title.chars().count() > 34;
        Some(
            div()
                .absolute()
                .left(px(f.margin))
                .top(px(150. * u))
                .w(px((1180. * u).min(f.width)))
                .h(px(286. * u))
                .flex()
                .flex_col()
                .justify_end()
                .gap(px(14. * u))
                .child(
                    div()
                        .text_size(px(if long { 54. } else { 72. } * u))
                        .line_height(relative(1.24))
                        .font_weight(FontWeight::BOLD)
                        .line_clamp(2)
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(14. * u))
                        .text_size(px(24. * u))
                        .text_color(pal::text_soft())
                        .child(meta.join("  ·  ")),
                ),
        )
    }

    fn spotlight_game(&self, id: &str) -> Option<(String, Vec<String>)> {
        let g = self.library.get(id)?;
        let gid = g.game_id();
        let (played, last) = self.playtime(&gid);
        let mut meta = vec![self.kind_label(g).to_string()];
        if self.running.contains_key(&gid.to_string()) {
            meta.push(self.tr("Running", "Çalışıyor").into());
        } else if let Some(last) = last {
            meta.push(super::game_page::ago(
                arkor_core::stats::now().saturating_sub(last),
                self.global.language,
            ));
        }
        if played > 0 {
            meta.push(arkor_core::stats::format_duration(played));
        }
        Some((g.name.clone(), meta))
    }

    // ---- The rows ----

    fn console_body(
        &mut self,
        rows: &[Row],
        top: f32,
        f: Frame,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let u = f.u;
        // The rows begin a little higher than they show: what glows is not cut off.
        let above = 44. * u;
        let view_h = (f.vh - top - 78. * u).max(100.);
        if rows.is_empty() {
            let text = match self.console.screen() {
                Screen::Downloads => self.tr("Nothing is downloading", "Şu an bir şey inmiyor"),
                _ => self.tr("Nothing here", "Burada bir şey yok"),
            };
            return div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(top))
                .h(px(view_h))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(28. * u))
                .text_color(pal::muted())
                .child(text)
                .into_any_element();
        }
        let cols = self.console.cols;
        let mut ys = Vec::with_capacity(rows.len());
        let mut y = 12. * u;
        for r in rows {
            ys.push(y);
            y += f.row_height(r, cols);
        }
        let total = y + 20. * u;
        let mut focus = self.console.focus();
        let row = focus.row.min(rows.len() - 1);
        // A shelf comes to the top; everything else is kept around the middle.
        let h = f.row_height(&rows[row], cols);
        let target = match rows[row].kind {
            Kind::Shelf => ys[row],
            _ => ys[row] + h / 2. - view_h * 0.46,
        }
        .clamp(0., (total - view_h).max(0.));
        let unsettled = ease_to(&mut focus.scroll, target, f.ease);
        let mut moving = unsettled;
        // Shelves follow their own column.
        for (ix, r) in rows.iter().enumerate() {
            if r.kind != Kind::Shelf {
                continue;
            }
            let (w, _) = f.shelf_tile();
            let step = w + 24. * u;
            let col = if ix == row { focus.col() } else { continue };
            let all = r.items.len() as f32 * step;
            let target = (col as f32 * step - f.width * 0.36).clamp(0., (all - f.width).max(0.));
            let at = focus.shelves.entry(ix).or_insert(0.);
            moving |= ease_to(at, target, f.ease);
        }
        *self.console.focus_mut() = focus.clone();
        self.console.unsettled |= moving;

        let over = self.console.overlay.is_some();
        let grown = if f.still {
            1.
        } else {
            ease_out(since(self.console.moved, FOCUS_TIME))
        };
        let mut out = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(top - above))
            // On under the footer, which fades it out.
            .h(px(view_h + above + 78. * u))
            .overflow_hidden();
        for (ix, r) in rows.iter().enumerate() {
            let ry = ys[ix] - focus.scroll;
            let rh = f.row_height(r, cols);
            // A little more than what is in view, for what grows over its edge.
            if ry > view_h + 78. * u {
                continue;
            }
            // A row leaving at the top fades out instead of peeking under the title.
            let fade = if ry < -2. * u {
                (1. + ry / (rh * 0.3)).clamp(0., 1.)
            } else {
                1.
            };
            if fade <= 0.01 {
                continue;
            }
            let glow = |col: usize| -> f32 {
                let at = (row, focus.col());
                if over {
                    // Dimmed below what is over the screen.
                    return if at == (ix, col) { 0.35 } else { 0. };
                }
                if at == (ix, col) {
                    grown
                } else if self.console.left == Some((ix, col)) {
                    1. - grown
                } else {
                    0.
                }
            };
            let glows: Vec<f32> = (0..r.items.len()).map(glow).collect();
            let shelf = focus.shelves.get(&ix).copied().unwrap_or(0.);
            out = out.child(
                self.console_row(r, ix, (ry + above, rh), shelf, &glows, f, cx)
                    .opacity(fade),
            );
        }
        out.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn console_row(
        &mut self,
        row: &Row,
        ix: usize,
        (y, h): (f32, f32),
        shelf: f32,
        glows: &[f32],
        f: Frame,
        cx: &mut Context<Self>,
    ) -> Div {
        let u = f.u;
        let mut out = div().absolute().left_0().w(px(f.vw)).top(px(y)).h(px(h));
        let mut top = 0.;
        if let Some(title) = &row.title {
            out = out.child(
                div()
                    .absolute()
                    .left(px(f.margin))
                    .top(px(8. * u))
                    .text_size(px(26. * u))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if glows.iter().any(|g| *g > 0.) {
                        pal::text()
                    } else {
                        pal::muted()
                    })
                    .child(title.clone()),
            );
            top = 56. * u;
        }
        match row.kind {
            Kind::Shelf => {
                let (w, th) = f.shelf_tile();
                let step = w + 24. * u;
                for (col, item) in row.items.iter().enumerate() {
                    let x = f.margin + col as f32 * step - shelf;
                    if x > f.vw || x + w < 0. {
                        continue;
                    }
                    out = out.child(self.console_tile(
                        item,
                        (ix, col),
                        (x, top + 10. * u, w, th),
                        glows[col],
                        f,
                        cx,
                    ));
                }
            }
            Kind::Grid => {
                let (w, gap) = f.tile(self.console.cols);
                for (col, item) in row.items.iter().enumerate() {
                    let x = f.margin + col as f32 * (w + gap);
                    out = out.child(self.console_tile(
                        item,
                        (ix, col),
                        (x, top + 10. * u, w, w * 1.5),
                        glows[col],
                        f,
                        cx,
                    ));
                }
            }
            Kind::Pills => {
                let mut line = div()
                    .absolute()
                    .left(px(f.margin))
                    .top(px(top + 8. * u))
                    .flex()
                    .gap(px(14. * u));
                for (col, item) in row.items.iter().enumerate() {
                    line =
                        line.child(self.console_pill(item, ("pill", ix, col), glows[col], f, cx));
                }
                out = out.child(line);
            }
            Kind::Line => {
                if let Some(item) = row.items.first() {
                    let w = (1240. * u).min(f.width);
                    out = out.child(
                        div()
                            .absolute()
                            .left(px(f.margin))
                            .top(px(top))
                            .w(px(w))
                            .h(px(h - top - 12. * u))
                            .child(self.console_line(item, ("line", ix), glows[0], f, cx)),
                    );
                }
            }
            Kind::Keys => {}
        }
        out
    }

    /// The cover of an item.
    fn console_art(
        &mut self,
        art: &Art,
        label: &str,
        (w, h, radius): (f32, f32, f32),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match art {
            Art::Game(id) => match self.covers.get(id).cloned().flatten() {
                Some(path) => cover_art::cover(path, radius, self.moving())
                    .w(px(w))
                    .h(px(h))
                    .flex_none()
                    .into_any_element(),
                None => div()
                    .w(px(w))
                    .h(px(h))
                    .flex_none()
                    .rounded(px(radius))
                    .bg(style::accent_gradient())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .p(px(w * 0.08))
                    .text_size(px((w * 0.13).max(9.)))
                    .font_weight(FontWeight::BOLD)
                    .text_color(pal::on_accent())
                    .child(label.to_string())
                    .into_any_element(),
            },
            Art::Shop(shop, id) => self.shop_art(*shop, id, label, (w, h, radius), cx),
        }
    }

    fn console_tile(
        &mut self,
        item: &Item,
        (row, col): (usize, usize),
        (x, y, w, h): (f32, f32, f32, f32),
        glow: f32,
        f: Frame,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let u = f.u;
        let Look::Cover(art) = &item.look else {
            return div().into_any_element();
        };
        let k = 1. + GROW * glow;
        let (bw, bh) = (w * k, h * k);
        let radius = 14. * u;
        let picture = self.console_art(art, &item.label, (bw, bh, radius), cx);
        let act = item.act.clone();
        div()
            .id(("console-tile", row * 1000 + col))
            .absolute()
            .left(px(x - (bw - w) / 2.))
            .top(px(y - (bh - h) / 2.))
            .w(px(bw))
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .w(px(bw))
                    .h(px(bh))
                    .rounded(px(radius))
                    .when(glow > 0.01, |d| {
                        d.shadow(vec![
                            BoxShadow {
                                color: pal::accent().opacity(0.95 * glow),
                                offset: point(px(0.), px(0.)),
                                blur_radius: px(0.),
                                spread_radius: px(4. * u),
                                inset: false,
                            },
                            BoxShadow {
                                color: pal::accent().opacity(0.45 * glow),
                                offset: point(px(0.), px(0.)),
                                blur_radius: px(44. * u),
                                spread_radius: px(2. * u),
                                inset: false,
                            },
                        ])
                    })
                    .child(picture)
                    .children(item.mark.map(|icon| {
                        div()
                            .absolute()
                            .top(px(10. * u))
                            .left(px(10. * u))
                            .size(px(34. * u))
                            .rounded_full()
                            .bg(pal::bg().opacity(0.85))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(style::icon(icon, 18. * u, pal::success()))
                    }))
                    .children(item.progress.map(|p| {
                        div()
                            .absolute()
                            .left(px(10. * u))
                            .right(px(10. * u))
                            .bottom(px(10. * u))
                            .child(downloads_page::progress_bar(Some(p)))
                    })),
            )
            .child(
                div()
                    .pt(px(12. * u))
                    .w(px(bw))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(21. * u))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if glow > 0.5 {
                                pal::text()
                            } else {
                                pal::text_soft()
                            })
                            .child(item.label.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(17. * u))
                            .text_color(pal::muted())
                            .child(item.sub.clone()),
                    ),
            )
            .on_click(cx.listener(move |this, _, w, cx| {
                this.console.pad = false;
                this.console.focus_mut().set(row, col);
                this.console_do(act.clone(), w, cx);
            }))
            .into_any_element()
    }

    /// Takes the focus by mouse, then acts (`over`: on what is over the screen).
    fn console_click(
        &mut self,
        over: bool,
        (row, col): (usize, usize),
        act: Act,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.console.pad = false;
        if over {
            self.console.over_focus.set(row, col);
        } else {
            self.console.focus_mut().set(row, col);
        }
        self.console_do(act, window, cx);
    }

    fn console_pill(
        &self,
        item: &Item,
        id: (&'static str, usize, usize),
        glow: f32,
        f: Frame,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let u = f.u;
        let (icon, on, main) = match &item.look {
            Look::Pill { icon, on, main } => (*icon, *on, *main),
            _ => ("circle", false, false),
        };
        let focused = glow > 0.5;
        let color = if focused {
            pal::bg()
        } else if main {
            pal::on_accent()
        } else if on {
            pal::accent_light()
        } else {
            pal::text_soft()
        };
        let act = item.act.clone();
        let over = id.0.starts_with("over");
        div()
            .id((id.0, id.1 * 1000 + id.2))
            .flex_none()
            .h(px(60. * u))
            .px(px(if main { 38. } else { 26. } * u))
            .flex()
            .items_center()
            .gap(px(12. * u))
            .rounded_full()
            .text_size(px(23. * u))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(color)
            .cursor_pointer()
            .border_1()
            .border_color(if focused || main {
                gpui::transparent_black()
            } else if on {
                pal::accent().opacity(0.7)
            } else {
                pal::text().opacity(0.14)
            })
            .map(|d| {
                if focused {
                    d.bg(pal::text())
                } else if main {
                    d.bg(style::accent_gradient())
                } else if on {
                    d.bg(pal::bg().blend(pal::accent().opacity(0.2)).opacity(0.9))
                } else {
                    d.bg(pal::bg().opacity(0.6))
                        .hover(|h| h.bg(pal::text().opacity(0.18)))
                }
            })
            .child(style::icon(icon, 23. * u, color))
            .child(item.label.clone())
            .on_click(cx.listener(move |this, _, w, cx| {
                this.console_click(over, (id.1, id.2), act.clone(), w, cx)
            }))
            .into_any_element()
    }

    fn console_line(
        &mut self,
        item: &Item,
        id: (&'static str, usize),
        glow: f32,
        f: Frame,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let u = f.u;
        let Look::Line { icon, value, art } = &item.look else {
            return div().into_any_element();
        };
        let focused = glow > 0.5;
        let (fg, soft) = if focused {
            (pal::bg(), pal::bg().opacity(0.65))
        } else {
            (pal::text(), pal::muted())
        };
        let picture = art.as_ref().map(|a| {
            let (w, h) = (60. * u, 80. * u);
            self.console_art(a, "", (w, h, 8. * u), cx)
        });
        let value = match value {
            Value::None => None,
            Value::Text(t) => Some(
                div()
                    .flex_none()
                    .max_w(px(620. * u))
                    .truncate()
                    .text_size(px(22. * u))
                    .text_color(soft)
                    .child(t.clone())
                    .into_any_element(),
            ),
            Value::Choice(t) => Some(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10. * u))
                    .text_size(px(22. * u))
                    .text_color(if focused { fg } else { soft })
                    .when(focused, |d| {
                        d.child(style::icon("chevron-left", 22. * u, soft))
                    })
                    .child(div().max_w(px(520. * u)).truncate().child(t.clone()))
                    .when(focused, |d| {
                        d.child(style::icon("chevron-right", 22. * u, soft))
                    })
                    .into_any_element(),
            ),
            Value::Switch(on) => Some(
                div()
                    .flex_none()
                    .w(px(64. * u))
                    .h(px(36. * u))
                    .p(px(4. * u))
                    .rounded_full()
                    .flex()
                    .when(*on, |d| d.justify_end())
                    .bg(if *on {
                        pal::accent()
                    } else if focused {
                        pal::bg().opacity(0.25)
                    } else {
                        pal::text().opacity(0.2)
                    })
                    .child(div().size(px(28. * u)).rounded_full().bg(gpui::white()))
                    .into_any_element(),
            ),
        };
        let act = item.act.clone();
        let over = id.0.starts_with("over");
        let (row, col) = (id.1 % LINE_COL, id.1 / LINE_COL);
        div()
            .id(id)
            .relative()
            .size_full()
            .px(px(24. * u))
            .flex()
            .items_center()
            .gap(px(22. * u))
            .rounded(px(18. * u))
            .overflow_hidden()
            .cursor_pointer()
            .map(|d| {
                if focused {
                    d.bg(pal::text())
                } else {
                    d.bg(pal::bg().opacity(0.6))
                        .border_1()
                        .border_color(pal::text().opacity(0.1))
                        .hover(|h| h.bg(pal::text().opacity(0.12)))
                }
            })
            .child(match picture {
                Some(p) => p,
                None => style::icon(icon, 28. * u, soft).into_any_element(),
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(25. * u))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(item.label.clone()),
                    )
                    .when(!item.sub.is_empty(), |d| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(px(19. * u))
                                .text_color(soft)
                                .child(item.sub.clone()),
                        )
                    }),
            )
            .children(value)
            .children(item.progress.map(|p| {
                div()
                    .absolute()
                    .left(px(24. * u))
                    .right(px(24. * u))
                    .bottom(px(6. * u))
                    .h(px(5. * u))
                    .rounded_full()
                    .bg(if focused {
                        pal::bg().opacity(0.12)
                    } else {
                        pal::text().opacity(0.12)
                    })
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .w(relative(p.max(0.005)))
                            .bg(style::accent_gradient()),
                    )
            }))
            .on_click(cx.listener(move |this, _, w, cx| {
                this.console_click(over, (row, col), act.clone(), w, cx)
            }))
            .into_any_element()
    }

    // ---- Over the screen ----

    fn console_overlay_view(&mut self, f: Frame, cx: &mut Context<Self>) -> Option<AnyElement> {
        let u = f.u;
        let overlay = self.console.overlay.clone()?;
        let rows = self.overlay_rows(&overlay);
        self.console.over_focus.step(&rows, 0, 0);
        let (row, col) = (self.console.over_focus.row, self.console.over_focus.col());
        let back = div()
            .id("console-over")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .justify_center();
        if overlay == Overlay::Keyboard {
            return Some(
                back.items_end()
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(pal::bg().opacity(0.1), 0.2),
                        linear_color_stop(pal::bg().opacity(0.9), 0.8),
                    ))
                    .child(self.console_keyboard(&rows, (row, col), f, cx))
                    .into_any_element(),
            );
        }
        let title = match &overlay {
            Overlay::Menu => self.tr("Menu", "Menü").to_string(),
            Overlay::Ask(text, _, _) | Overlay::Pick(text, _) => text.clone(),
            Overlay::Keyboard => String::new(),
            Overlay::Folder(what, _) => match what {
                super::console::FolderFor::Prefix => {
                    self.tr("Choose a Wine prefix", "Wine prefix seç")
                }
                _ => self.tr("Choose a folder", "Klasör seç"),
            }
            .to_string(),
            Overlay::Failed(_, name, _) => {
                format!("{name} {}", self.tr("did not start", "açılamadı"))
            }
        };
        let failed = matches!(overlay, Overlay::Failed(..) | Overlay::Folder(..));
        let line_h = 84. * u;
        let mut body = div().flex().flex_col().gap(px(22. * u));
        if let Overlay::Failed(_, _, tail) = &overlay {
            body = body
                .child(
                    div()
                        .text_size(px(22. * u))
                        .text_color(pal::text_soft())
                        .child(self.tr(
                            "It ended right after starting. The end of its log:",
                            "Başladıktan hemen sonra kapandı. Günlüğünün sonu:",
                        )),
                )
                .child(
                    div()
                        .p(px(20. * u))
                        .rounded(px(16. * u))
                        .bg(gpui::black().opacity(0.45))
                        .border_1()
                        .border_color(pal::text().opacity(0.1))
                        .flex()
                        .flex_col()
                        .gap(px(4. * u))
                        .font_family("monospace")
                        .text_size(px(17. * u))
                        .text_color(pal::text_soft())
                        .children(tail.iter().map(|l| div().truncate().child(l.clone()))),
                );
        }
        if let Overlay::Folder(_, dir) = &overlay {
            // Where it is: each step of the path.
            let mut crumbs = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8. * u))
                .text_size(px(20. * u))
                .text_color(pal::muted())
                .child(style::icon("hard-drive", 20. * u, pal::muted()));
            let parts: Vec<String> = dir
                .components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect();
            let n = parts.len();
            for (i, p) in parts.into_iter().enumerate() {
                crumbs = crumbs
                    .child(style::icon("chevron-right", 16. * u, pal::faint()))
                    .child(
                        div()
                            .when(i + 1 == n, |d| {
                                d.text_color(pal::text()).font_weight(FontWeight::SEMIBOLD)
                            })
                            .child(p),
                    );
            }
            let mut pills = div().flex().gap(px(12. * u));
            for (c, item) in rows[0].items.iter().enumerate() {
                let glow = if row == 0 && c == col { 1. } else { 0. };
                pills = pills.child(self.console_pill(item, ("over-pill", 0, c), glow, f, cx));
            }
            // The places: three beside each other, each line with its own focus.
            let head = rows.iter().take_while(|r| r.kind == Kind::Pills).count();
            let mut places = div().flex().flex_col().gap(px(10. * u));
            for (r, places_row) in rows.iter().enumerate().take(head).skip(1) {
                let mut line = div().flex().gap(px(10. * u));
                for c in 0..3 {
                    let cell = div().flex_1().min_w_0().h(px(78. * u));
                    line = line.child(match places_row.items.get(c) {
                        Some(item) => {
                            let glow = if row == r && c == col { 1. } else { 0. };
                            cell.child(self.console_line(
                                item,
                                ("over-place", r + c * LINE_COL),
                                glow,
                                f,
                                cx,
                            ))
                        }
                        None => cell,
                    });
                }
                places = places.child(line);
            }
            let items = &rows[head..];
            let line_h = 76. * u;
            let view = f.vh * (0.46 - 0.08 * (head as f32 - 1.)).max(0.2);
            let total = items.len() as f32 * line_h;
            let focus_row = row.saturating_sub(head) as f32;
            let target =
                (focus_row * line_h + line_h / 2. - view / 2.).clamp(0., (total - view).max(0.));
            let mut at = self.console.over_focus.scroll;
            self.console.unsettled |= ease_to(&mut at, target, f.ease);
            self.console.over_focus.scroll = at;
            let mut list = div().relative().h(px(view)).overflow_hidden();
            if items.is_empty() {
                list = list.child(
                    div()
                        .pt(px(20. * u))
                        .text_color(pal::muted())
                        .child(self.tr("No folders in here", "Burada klasör yok")),
                );
            }
            for (r, line) in items.iter().enumerate() {
                let y = r as f32 * line_h - at;
                if y > view || y + line_h < 0. {
                    continue;
                }
                let glow = if r + head == row { 1. } else { 0. };
                list = list.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(y))
                        .h(px(line_h - 8. * u))
                        .child(self.console_line(
                            &line.items[0],
                            ("over-line", r + head),
                            glow,
                            f,
                            cx,
                        )),
                );
            }
            body = body.child(crumbs).child(pills).child(places).child(list);
        } else if matches!(overlay, Overlay::Ask(..) | Overlay::Failed(..)) {
            let mut line = div().flex().justify_end().gap(px(14. * u));
            for (c, item) in rows[0].items.iter().enumerate() {
                let glow = if c == col { 1. } else { 0. };
                line = line.child(self.console_pill(item, ("over-pill", 0, c), glow, f, cx));
            }
            body = body.child(line);
        } else {
            // A long list moves with the focus.
            let view = (rows.len() as f32 * line_h).min(f.vh * 0.6);
            let total = rows.len() as f32 * line_h;
            let target =
                (row as f32 * line_h + line_h / 2. - view / 2.).clamp(0., (total - view).max(0.));
            let mut at = self.console.over_focus.scroll;
            self.console.unsettled |= ease_to(&mut at, target, f.ease);
            self.console.over_focus.scroll = at;
            let mut list = div().relative().h(px(view)).overflow_hidden();
            for (r, line) in rows.iter().enumerate() {
                let y = r as f32 * line_h - at;
                if y > view || y + line_h < 0. {
                    continue;
                }
                let glow = if r == row { 1. } else { 0. };
                list = list.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(y))
                        .h(px(line_h - 10. * u))
                        .child(self.console_line(&line.items[0], ("over-line", r), glow, f, cx)),
                );
            }
            body = body.child(list);
        }
        Some(
            back.items_center()
                .bg(gpui::black().opacity(0.62))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.console.overlay = None;
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .w(px(((if failed { 1180. } else { 720. }) * u).min(f.vw * 0.9)))
                        .p(px(36. * u))
                        .flex()
                        .flex_col()
                        .gap(px(26. * u))
                        .rounded(px(28. * u))
                        .bg(pal::bg())
                        .border_1()
                        .border_color(pal::border_strong())
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(16. * u))
                                .when(matches!(overlay, Overlay::Failed(..)), |d| {
                                    d.child(
                                        div()
                                            .size(px(52. * u))
                                            .rounded_full()
                                            .bg(pal::danger().opacity(0.16))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(style::icon(
                                                "triangle-alert",
                                                28. * u,
                                                pal::danger(),
                                            )),
                                    )
                                })
                                .child(
                                    div()
                                        .text_size(px(30. * u))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(title),
                                ),
                        )
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn console_keyboard(
        &mut self,
        rows: &[Row],
        (row, col): (usize, usize),
        f: Frame,
        cx: &mut Context<Self>,
    ) -> Div {
        let u = f.u;
        let mut keys = div().flex().flex_col().gap(px(10. * u));
        for (r, line) in rows.iter().enumerate() {
            let mut out = div().flex().justify_center().gap(px(10. * u));
            for (c, item) in line.items.iter().enumerate() {
                let wide = match item.look {
                    Look::Key(w) => w,
                    _ => 1.,
                };
                let focused = (r, c) == (row, col);
                let act = item.act.clone();
                out = out.child(
                    div()
                        .id(("console-key", r * 100 + c))
                        .w(px((66. * wide + 10. * (wide - 1.)) * u))
                        .h(px(62. * u))
                        .rounded(px(12. * u))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(25. * u))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .map(|d| {
                            if focused {
                                d.bg(pal::text()).text_color(pal::bg())
                            } else {
                                d.bg(pal::text().opacity(0.1))
                                    .hover(|h| h.bg(pal::text().opacity(0.2)))
                            }
                        })
                        .child(item.label.clone())
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.console_click(true, (r, c), act.clone(), w, cx)
                        })),
                );
            }
            keys = keys.child(out);
        }
        let typed = self
            .console
            .edit
            .clone()
            .map(|(_, t)| t)
            .unwrap_or_else(|| self.console.query.clone());
        // A password shows as dots.
        let secret = self.console.secret.is_some();
        let typed = match &self.console.secret {
            Some((_, pw)) => "•".repeat(pw.chars().count()),
            None => typed,
        };
        let empty = typed.is_empty();
        div()
            .mb(px(104. * u))
            .p(px(30. * u))
            .flex()
            .flex_col()
            .gap(px(22. * u))
            .rounded(px(28. * u))
            .bg(pal::bg().opacity(0.96))
            .border_1()
            .border_color(pal::border_strong())
            .child(
                div()
                    .h(px(64. * u))
                    .px(px(24. * u))
                    .flex()
                    .items_center()
                    .gap(px(16. * u))
                    .rounded(px(16. * u))
                    .bg(pal::text().opacity(0.08))
                    .text_size(px(27. * u))
                    .child(style::icon(
                        if secret { "lock" } else { "search" },
                        26. * u,
                        pal::muted(),
                    ))
                    .child(if empty {
                        div().text_color(pal::muted()).child(if secret {
                            self.tr("Administrator password", "Yönetici şifresi")
                        } else {
                            self.tr("Search", "Ara")
                        })
                    } else {
                        div().child(typed)
                    })
                    .child(div().w(px(3. * u)).h(px(30. * u)).bg(pal::accent_light())),
            )
            .child(keys)
    }

    /// A game is starting: black, as the game's own screen will be, with its cover and the
    /// loading light, until its window covers it. Back in Arkor while it runs: what is
    /// playing, and the way to stop it.
    fn console_launching(
        &mut self,
        f: Frame,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let l = self.console.launching.clone()?;
        if l.dismissed {
            return None;
        }
        let u = f.u;
        // Arkor was left: the game's window came.
        if !l.playing
            && !window.is_window_active()
            && l.at.elapsed() > Duration::from_millis(600)
            && let Some(l) = self.console.launching.as_mut()
        {
            l.playing = true;
        }
        let g = self.library.get(&l.id)?.clone();
        let cover: Option<PathBuf> = self.covers.get(&l.id).cloned().flatten();
        let t = if f.still {
            1.
        } else {
            ease_out((l.at.elapsed().as_secs_f32() / 0.3).min(1.))
        };
        if t < 1. {
            self.console.unsettled = true;
        }
        let (w, h) = (240. * u, 360. * u);
        let mut screen = div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(gpui::black().opacity(t))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(28. * u))
            .children(cover.map(|c| {
                div()
                    .opacity(t.max(0.02))
                    .rounded(px(18. * u))
                    .shadow(vec![BoxShadow {
                        color: pal::accent().opacity(0.35),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(80. * u),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .child(cover_art::cover(c, 18. * u, l.playing).w(px(w)).h(px(h)))
            }))
            .child(
                div()
                    .text_size(px(40. * u))
                    .font_weight(FontWeight::BOLD)
                    .text_color(gpui::white())
                    .child(g.name.clone()),
            );
        if !l.playing {
            screen = screen.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14. * u))
                    .text_size(px(24. * u))
                    .text_color(gpui::white().opacity(0.7))
                    .child(super::loader::spinner(40. * u, f.still))
                    .child(self.t().launching),
            );
            return Some(screen);
        }
        let mins = l.at.elapsed().as_secs() / 60;
        let hint = |glyph: &'static str, text: &'static str, nav: Nav| (glyph, text, nav);
        let pad = self.console.pad;
        let hints = [
            hint(
                if pad { "B" } else { "Esc" },
                self.tr("Back to Arkor", "Arkor'a dön"),
                Nav::Back,
            ),
            hint(
                if pad { "X" } else { "Del" },
                self.tr("Stop the game", "Oyunu durdur"),
                Nav::Extra,
            ),
        ];
        let mut row = div().flex().items_center().gap(px(34. * u));
        for (ix, (glyph, text, nav)) in hints.into_iter().enumerate() {
            row = row.child(
                div()
                    .id(("playing-hint", ix))
                    .flex()
                    .items_center()
                    .gap(px(10. * u))
                    .cursor_pointer()
                    .text_size(px(22. * u))
                    .text_color(gpui::white().opacity(0.8))
                    .child(self.console_glyph(glyph, f))
                    .child(text)
                    .on_click(cx.listener(move |this, _, w, cx| this.nav(nav, w, cx))),
            );
        }
        Some(
            screen
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10. * u))
                        .text_size(px(24. * u))
                        .text_color(pal::success())
                        .child(div().size(px(12. * u)).rounded_full().bg(pal::success()))
                        .child(format!(
                            "{} · {}",
                            self.tr("Playing", "Oynanıyor"),
                            arkor_core::stats::format_duration(mins * 60)
                        )),
                )
                .child(row.mt(px(30. * u))),
        )
    }
}

/// Moves `at` towards `target`; `true` while it is not there yet.
fn ease_to(at: &mut f32, target: f32, ease: f32) -> bool {
    *at += (target - *at) * ease;
    if (target - *at).abs() < 0.5 {
        *at = target;
        return false;
    }
    true
}

/// The local time, "21:05".
fn clock() -> String {
    // SAFETY: localtime_r fills the zeroed struct from a valid time.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_arrives() {
        let mut at = 0.;
        let mut n = 0;
        while ease_to(&mut at, 300., 0.2) {
            n += 1;
            assert!(n < 100);
        }
        assert_eq!(at, 300.);
        assert!(
            !ease_to(&mut at, 0., 1.),
            "without motion it is there at once"
        );
        assert_eq!(at, 0.);
    }
}
