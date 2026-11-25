//! The Stores page: the account's games on Epic, GOG and Amazon, and the windows for
//! logging in and for one game (installing it, or what can be done with it).

use arkor_launch::progress::human_bytes;
use arkor_launch::shops::Shop;
use arkor_launch::shops::vault::Kept;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, FontWeight, Window, div, prelude::*, px, relative, uniform_list,
};

use super::cover_art::Size;
use super::shops::GameUi;
use super::style::{self, pal};
use super::{Library, cover_art, downloads_page, loader, widgets};

const GAP: f32 = 18.;
/// Below a cover: the title and the state.
const CAPTION: f32 = 52.;

/// The store's mark as an icon name (see `icons`).
pub(super) fn shop_icon(shop: Shop) -> &'static str {
    match shop {
        Shop::Epic => "store-epic",
        Shop::Gog => "store-gog",
        Shop::Amazon => "store-amazon",
        Shop::Steam => "store-steam",
    }
}

/// The store's own colour, behind its mark.
pub(super) fn shop_color(shop: Shop) -> gpui::Hsla {
    style::hex(match shop {
        Shop::Epic => 0x2a2a2e,
        Shop::Gog => 0x86328a,
        Shop::Amazon => 0xff9900,
        Shop::Steam => 0x1b2838,
    })
}

/// The store's mark on a tile of its colour.
pub(super) fn shop_mark(shop: Shop, size: f32) -> Div {
    let ink = if shop == Shop::Amazon {
        style::hex(0x141414)
    } else {
        gpui::white()
    };
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(size * 0.28))
        .bg(shop_color(shop))
        .border_1()
        .border_color(gpui::white().opacity(0.12))
        .flex()
        .items_center()
        .justify_center()
        .child(style::icon(shop_icon(shop), size * 0.6, ink))
}

/// "Türkçe" for `tr-TR`: the languages stores offer most, else the code.
pub(super) fn language_name(code: &str) -> String {
    let name = match code.split('-').next().unwrap_or(code) {
        "en" => "English",
        "tr" => "Türkçe",
        "de" => "Deutsch",
        "fr" => "Français",
        "es" => "Español",
        "it" => "Italiano",
        "pl" => "Polski",
        "ru" => "Русский",
        "pt" => "Português",
        "ja" => "日本語",
        "ko" => "한국어",
        "zh" => "中文",
        "cs" => "Čeština",
        "hu" => "Magyar",
        "nl" => "Nederlands",
        "ar" => "العربية",
        "uk" => "Українська",
        _ => return code.to_string(),
    };
    match code {
        "pt-BR" => "Português (BR)".into(),
        "es-MX" => "Español (MX)".into(),
        "zh-Hant" | "zh-TW" => "繁體中文".into(),
        _ => name.to_string(),
    }
}

impl Library {
    pub(super) fn render_shops(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        super::shops::trim_covers(window);
        // A refresh also when the page was opened another way than by its button.
        self.shops_opened(cx);
        let tab = self.shops.tab;
        let shown = self.shops.shown(tab);
        let any_login = Shop::ALL.into_iter().any(|s| self.shops.logged_in(s));
        let mut page = div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .child(Self::header(
                self.tr("Stores", "Mağazalar"),
                if any_login {
                    format!("{} {}", shown.len(), self.tr("games", "oyun"))
                } else {
                    String::new()
                },
            ))
            .child(self.shops_toolbar(cx));
        match tab {
            Some(shop) => page = page.child(self.account_bar(shop, cx)),
            None => {
                let mut row = div().flex().gap_3();
                for shop in Shop::ALL {
                    row = row.child(self.account_card(shop, cx));
                }
                page = page.child(row);
            }
        }
        if let Some((done, total)) = self.shops_preparing() {
            let text = self.tr("Getting your games ready", "Oyunların hazırlanıyor");
            return page
                .child(div().flex_1().flex().items_center().justify_center().child(
                    super::loader::block(
                        text,
                        Some(format!("{done} / {total}")),
                        self.global.appearance.reduce_motion,
                    ),
                ))
                .into_any_element();
        }
        if shown.is_empty() && !self.shops.refreshing.is_empty() {
            let text = self.tr("Reading your games…", "Oyunların okunuyor…");
            return page
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(self.loading(text)),
                )
                .into_any_element();
        }
        if shown.is_empty() {
            return page
                .child(div().flex_1().child(self.shops_empty(tab)))
                .into_any_element();
        }
        // As on the library page: columns of about the chosen cover size, filling the width.
        let scale = (self.viewport_w / 1320.).clamp(1., 1.45);
        let preferred = self.global.appearance.grid_size.cover_width() * 0.92 * scale;
        let around = 244. + 6. * self.global.appearance.font_size() as f32;
        let available = (self.viewport_w - around).max(preferred);
        let cols = (((available + GAP) / (preferred + GAP)).floor() as usize).max(2);
        let tile_w = ((available - GAP * (cols - 1) as f32) / cols as f32).floor();
        let cover_h = (tile_w * 4. / 3.).floor();
        let row_h = cover_h + CAPTION + GAP;
        let rows = shown.len().div_ceil(cols);
        let list = uniform_list(
            "shop-games",
            rows,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|row| {
                        let mut line = div().h(px(row_h)).flex().gap(px(GAP));
                        for &(shop, ix) in shown.iter().skip(row * cols).take(cols) {
                            line = line.child(this.shop_tile(shop, ix, tile_w, cover_h, cx));
                        }
                        line
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .flex_1()
        .min_h_0();
        page.child(list).into_any_element()
    }

    /// Which store, the search and the filter.
    fn shops_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let mut tabs = vec![("layout-grid", self.tr("All", "Tümü"))];
        tabs.extend(Shop::ALL.into_iter().map(|s| (shop_icon(s), s.name())));
        let selected = match self.shops.tab {
            None => 0,
            Some(s) => 1 + Shop::ALL.iter().position(|x| *x == s).unwrap_or(0),
        };
        let strip = style::tab_strip("shop-tab", &tabs, selected, cx, |this, i, cx| {
            this.shops.tab = (i > 0).then(|| Shop::ALL[i - 1]);
            cx.notify();
        });
        let placeholder = self.tr("Search your games", "Oyunlarında ara");
        let search = self.field(
            "shop-search",
            |this| this.shops.query.clone(),
            placeholder,
            cx,
            |this, text| this.shops.query = text.to_string(),
        );
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .child(strip)
            .child(div().flex_1())
            .child(
                widgets::chip(
                    "shop-installed",
                    self.tr("Installed", "Kurulu"),
                    self.shops.only_installed,
                )
                .child(style::icon("hard-drive", 13., pal::muted()))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.shops.only_installed = !this.shops.only_installed;
                    cx.notify();
                })),
            )
            .child(div().w(px(260.)).child(search))
    }

    /// One store on the All page: who is logged in, or the way to log in.
    fn account_card(&mut self, shop: Shop, cx: &mut Context<Self>) -> Div {
        let ended = self
            .shops
            .errors
            .get(&shop)
            .is_some_and(|e| super::shops::login_ended(e));
        let logged_in = self.shops.logged_in(shop) && !ended;
        style::card()
            .flex_1()
            .min_w_0()
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .gap_3()
            .child(shop_mark(shop, 34.))
            .child(self.account_text(shop))
            .child(if logged_in {
                self.refresh_button(shop, cx).into_any_element()
            } else {
                style::button(
                    ("shop-login", shop as usize),
                    self.tr("Log in", "Giriş yap"),
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.shop_login(shop, cx)))
                .into_any_element()
            })
    }

    fn account_text(&self, shop: Shop) -> Div {
        let error = self.shops.errors.get(&shop);
        let line = match error {
            Some(e) => self.shop_error(e),
            None if self.shops.refreshing.contains(&shop) => self
                .tr("Reading your games…", "Oyunların okunuyor…")
                .to_string(),
            None if self.shops.logged_in(shop) => {
                let n = self.shops.owned(shop).len();
                let games = format!("{n} {}", self.tr("games", "oyun"));
                match self.shops.account(shop) {
                    Some(name) => format!("{name} · {games}"),
                    None => games,
                }
            }
            None => self.tr("Not logged in", "Giriş yapılmadı").to_string(),
        };
        // How the login is kept, where Arkor keeps it.
        let kept = match self.shops.kept(shop) {
            Kept::Sealed => Some(self.tr(
                "The login is encrypted on this computer.",
                "Giriş bu bilgisayarda şifreli duruyor.",
            )),
            Kept::Plain => Some(self.tr(
                "The login is a file only you can read (systemd cannot encrypt it here).",
                "Giriş yalnızca senin okuyabildiğin bir dosyada (systemd burada şifreleyemiyor).",
            )),
            Kept::Nothing => None,
        };
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(shop.name()),
                    )
                    .when_some(kept, |d, text| {
                        let icon = match self.shops.kept(shop) {
                            Kept::Sealed => "lock-keyhole",
                            _ => "lock-open",
                        };
                        d.child(
                            div()
                                .id(("login-kept", shop as usize))
                                .flex_none()
                                .child(style::icon(icon, 11., pal::faint()))
                                .tooltip(move |_, cx| widgets::tip(text.into(), cx)),
                        )
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(if error.is_some() {
                        pal::warning()
                    } else {
                        pal::muted()
                    })
                    .child(line),
            )
    }

    fn refresh_button(&self, shop: Shop, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let busy = self.shops.refreshing.contains(&shop);
        style::icon_button(("shop-refresh", shop as usize), "refresh-cw", false)
            .when(busy, |d| d.opacity(0.4))
            .on_click(cx.listener(move |this, _, _, cx| this.shops_refresh(shop, cx)))
    }

    /// The account of the one store shown.
    fn account_bar(&mut self, shop: Shop, cx: &mut Context<Self>) -> Div {
        let card = self.account_card(shop, cx).flex_none();
        // Steam's account is Steam's own.
        if !self.shops.logged_in(shop) || shop == Shop::Steam {
            return card;
        }
        card.child(
            style::button("shop-logout", self.tr("Log out", "Çıkış yap"), false)
                .child(style::icon("log-out", 13., pal::muted()))
                .on_click(cx.listener(move |this, _, _, cx| this.shop_logout(shop, cx))),
        )
    }

    fn shops_empty(&self, tab: Option<Shop>) -> Div {
        let logged_in = Shop::ALL
            .into_iter()
            .any(|s| tab.is_none_or(|t| t == s) && self.shops.logged_in(s));
        let reading = !self.shops.refreshing.is_empty();
        let text = if !logged_in {
            self.tr(
                "Log in to a store to install your games from it.",
                "Oyunlarını kurmak için bir mağazaya giriş yap.",
            )
        } else if reading {
            self.tr("Reading your games…", "Oyunların okunuyor…")
        } else {
            self.tr("No games here.", "Burada oyun yok.")
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .py(px(70.))
            .child(
                div()
                    .size(px(64.))
                    .rounded(px(18.))
                    .bg(pal::raised())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(style::icon("store", 30., pal::accent_light())),
            )
            .child(div().text_color(pal::muted()).child(text))
    }

    /// The portrait picture of a store game, or a tile with its initial.
    pub(super) fn shop_art(
        &mut self,
        shop: Shop,
        id: &str,
        title: &str,
        (w, h, radius): (f32, f32, f32),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.shop_cover(shop, id, cx) {
            Some(path) => cover_art::cover(path, radius, false)
                .w(px(w))
                .h(px(h))
                .flex_none()
                .into_any_element(),
            None => div()
                .w(px(w))
                .h(px(h))
                .flex_none()
                .rounded(px(radius))
                .bg(pal::raised())
                .flex()
                .items_center()
                .justify_center()
                .p_3()
                .text_center()
                .text_size(px((w * 0.1).clamp(11., 26.)))
                .text_color(pal::muted())
                .child(title.to_string())
                .into_any_element(),
        }
    }

    fn shop_tile(
        &mut self,
        shop: Shop,
        ix: usize,
        w: f32,
        h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(game) = self.shops.owned(shop).get(ix).cloned() else {
            return div().into_any_element();
        };
        let installed = self.shops.is_installed(shop, &game.id);
        let job = self.shop_job(shop, &game.id);
        let art = self.shop_art(shop, &game.id, &game.title, (w - 6., h - 6., 10.), cx);
        let state: AnyElement = match &job {
            Some((_, Some(f))) => div()
                .pt_1()
                .child(downloads_page::progress_bar(Some(*f)))
                .into_any_element(),
            Some((_, None)) => div()
                .text_xs()
                .text_color(pal::accent_light())
                .child(self.tr("In queue", "Sırada"))
                .into_any_element(),
            None if installed => div()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(pal::success())
                .child(style::icon("check", 12., pal::success()))
                .child(self.tr("Installed", "Kurulu"))
                .into_any_element(),
            None => div()
                .text_xs()
                .text_color(pal::muted())
                .child(shop.name())
                .into_any_element(),
        };
        let id = game.id.clone();
        let hover_id = game.id.clone();
        div()
            .id((shop.key(), ix))
            .w(px(w))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1p5()
            .cursor_pointer()
            .group("shop-tile")
            .child(
                div()
                    .rounded(px(13.))
                    .p(px(2.))
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .group_hover("shop-tile", |s| s.border_color(pal::accent().opacity(0.7)))
                    .child(art),
            )
            .child(
                div()
                    .px_1()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(game.title.clone()),
            )
            .child(div().px_1().child(state))
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                if *over {
                    this.shop_prefetch_info(shop, &hover_id, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.shop_open_game(shop, &id, cx)))
            .into_any_element()
    }

    // ---- Windows over the page ----

    /// The login window or the window of one game, over everything.
    pub(super) fn render_shop_windows(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let window = if self.shops.login.is_some() {
            self.login_window(cx)
        } else if self.shops.game.is_some() {
            self.shop_game_window(cx)?
        } else {
            return None;
        };
        Some(
            div()
                .id("shop-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .bg(gpui::black().opacity(0.55))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.close_shop_windows(cx);
                    }),
                )
                .child(window)
                .into_any_element(),
        )
    }

    /// Closes the store window on top; `false` when none was open.
    pub(super) fn close_shop_windows(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(l) = &self.shops.login
            && let super::shops::LoginMode::Window(pid) = l.mode
        {
            // SAFETY: plain syscall on the login window's own process.
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
        let open = self.shops.login.take().is_some() || self.shops.game.take().is_some();
        if open {
            self.refocus = true;
            cx.notify();
        }
        open
    }

    /// The login: in Arkor's own login window (just log in there), or, without one, in the
    /// browser with the code copied back.
    fn login_window(&mut self, cx: &mut Context<Self>) -> Div {
        use super::shops::LoginMode;
        let Some(l) = self.shops.login.as_ref() else {
            return div();
        };
        if l.mode != LoginMode::Browser {
            return self.login_simple(cx);
        }
        self.login_browser(cx)
    }

    fn login_simple(&mut self, cx: &mut Context<Self>) -> Div {
        use super::shops::LoginMode;
        let Some(l) = self.shops.login.as_ref() else {
            return div();
        };
        let (shop, mode, busy, error) = (l.shop, l.mode, l.busy, l.error.clone());
        let still = self.global.appearance.reduce_motion;
        let (title, text): (&str, &str) = match (mode, busy) {
            (LoginMode::Closed, false) => (
                self.tr("The login window was closed", "Giriş penceresi kapatıldı"),
                self.tr("Open it again to log in.", "Giriş yapmak için yeniden aç."),
            ),
            (_, true) if l.login.is_some() => (
                self.tr("Logging in…", "Giriş yapılıyor…"),
                self.tr("Almost there.", "Neredeyse bitti."),
            ),
            (LoginMode::Window(_), _) => (
                self.tr(
                    "Log in in the window that opened",
                    "Açılan pencerede giriş yap",
                ),
                self.tr(
                    "Arkor takes over as soon as you are logged in; nothing to copy.",
                    "Giriş yapar yapmaz Arkor devralır; hiçbir şey kopyalaman gerekmez.",
                ),
            ),
            _ => (
                self.tr("Opening the login window…", "Giriş penceresi açılıyor…"),
                "",
            ),
        };
        let waiting = !(mode == LoginMode::Closed && !busy) && error.is_none();
        let mut actions = div().flex().items_center().justify_center().gap_2();
        if mode == LoginMode::Closed && !busy {
            actions = actions.child(
                style::button("login-again", self.tr("Open it again", "Yeniden aç"), true)
                    .on_click(cx.listener(|this, _, _, cx| this.shop_login_window(cx))),
            );
        }
        actions = actions.child(
            style::button(
                "login-browser",
                self.tr("Use my browser instead", "Bunun yerine tarayıcımı kullan"),
                false,
            )
            .child(style::icon("external-link", 13., pal::muted()))
            .on_click(cx.listener(|this, _, _, cx| this.shop_login_browser(cx))),
        );
        div()
            .w(px(520.))
            .rounded(px(22.))
            .overflow_hidden()
            .border_1()
            .border_color(pal::border_strong())
            .bg(pal::bg())
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .relative()
                    .h(px(170.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::linear_gradient(
                        160.,
                        gpui::linear_color_stop(shop_color(shop).opacity(0.7), 0.),
                        gpui::linear_color_stop(pal::bg(), 1.),
                    ))
                    .child(
                        div()
                            .relative()
                            .size(px(124.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(waiting, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .child(loader::spinner(124., still)),
                                )
                            })
                            .child(shop_mark(shop, 64.)),
                    )
                    .child(
                        div().absolute().top_3().right_3().child(
                            style::icon_button("shop-login-close", "x", false)
                                .bg(pal::bg().opacity(0.7))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.close_shop_windows(cx);
                                })),
                        ),
                    ),
            )
            .child(
                div()
                    .px_8()
                    .pt_5()
                    .pb_7()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .text_center()
                    .child(div().text_xl().font_weight(FontWeight::BOLD).child(title))
                    .when(!text.is_empty(), |d| {
                        d.child(div().text_sm().text_color(pal::muted()).child(text))
                    })
                    .children(error.map(|e| {
                        div()
                            .text_sm()
                            .text_color(pal::warning())
                            .child(format!("⚠ {}", self.shop_error(&e)))
                    }))
                    .child(actions.mt_2()),
            )
    }

    fn login_browser(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(l) = self.shops.login.as_ref() else {
            return div();
        };
        let (shop, busy, error, paste) = (l.shop, l.busy, l.error.clone(), l.paste);
        let url = l.login.as_ref().map(|l| l.url.clone());
        let ready = url.is_some();
        let still = self.global.appearance.reduce_motion;
        // Where the user is: preparing, in the browser, or being logged in.
        let stage = match (ready, busy) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2,
        };
        let steps = [
            self.tr("Log in", "Giriş yap"),
            self.tr("Copy", "Kopyala"),
            self.tr("Done", "Bitti"),
        ];
        let mut track = div().flex().items_center().gap_2();
        for (ix, label) in steps.into_iter().enumerate() {
            let reached = stage >= ix.max(1);
            let current = (stage == 1 && ix < 2) || (stage == 2 && ix == 2);
            if ix > 0 {
                track = track.child(div().flex_1().h(px(2.)).rounded_full().bg(
                    if reached && stage > ix {
                        pal::accent()
                    } else {
                        pal::text().opacity(0.12)
                    },
                ));
            }
            track = track.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(26.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .when(current, |d| {
                                d.bg(pal::accent()).text_color(pal::on_accent())
                            })
                            .when(!current, |d| {
                                d.bg(pal::text().opacity(0.08)).text_color(pal::muted())
                            })
                            .child((ix + 1).to_string()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if current { pal::text() } else { pal::muted() })
                            .child(label),
                    ),
            );
        }
        // What the page in the browser ends on, and what to copy from it.
        let (what, keys, sample) = match shop {
            Shop::Epic => (
                self.tr(
                    "After logging in, Epic shows a page of text like this. Copy all of it:",
                    "Giriş yapınca Epic böyle bir yazı sayfası gösterir. Hepsini kopyala:",
                ),
                "Ctrl+A  ·  Ctrl+C",
                r#"{"redirectUrl": "…", "authorizationCode": "a1b2c3…", "sid": null}"#,
            ),
            Shop::Gog => (
                self.tr(
                    "After logging in, GOG opens an almost empty page. Copy its address:",
                    "Giriş yapınca GOG neredeyse boş bir sayfa açar. Adresini kopyala:",
                ),
                "Ctrl+L  ·  Ctrl+C",
                "https://embed.gog.com/on_login_success?origin=client&code=…",
            ),
            Shop::Amazon => (
                self.tr(
                    "After logging in, Amazon opens its home page. Copy its address:",
                    "Giriş yapınca Amazon ana sayfasını açar. Adresini kopyala:",
                ),
                "Ctrl+L  ·  Ctrl+C",
                "https://www.amazon.com/?openid.oa2.authorization_code=…",
            ),
            Shop::Steam => ("", "", ""),
        };
        let mono = |text: &'static str| {
            div()
                .px_3()
                .py_2p5()
                .rounded(px(10.))
                .bg(pal::text().opacity(0.06))
                .border_1()
                .border_color(pal::text().opacity(0.1))
                .text_xs()
                .font_family("monospace")
                .text_color(pal::text_soft())
                .truncate()
                .child(text)
        };
        let status: Div = match stage {
            0 => self.loading_inline(self.tr("Opening the login page…", "Giriş sayfası açılıyor…")),
            2 => self.loading_inline(self.tr("Logging in…", "Giriş yapılıyor…")),
            _ => loader::inline(
                self.tr(
                    "Waiting for you: come back to Arkor after copying",
                    "Seni bekliyor: kopyaladıktan sonra Arkor'a dön",
                ),
                still,
            ),
        };
        let placeholder = self.tr("Paste it here, then Enter", "Buraya yapıştır, sonra Enter");
        let field = paste.then(|| {
            self.submit_field("shop-code", placeholder, cx, |this, text, cx| {
                this.shop_login_text(text, cx)
            })
        });
        div()
            .w(px(580.))
            .rounded(px(20.))
            .overflow_hidden()
            .border_1()
            .border_color(pal::border_strong())
            .bg(pal::bg())
            .flex()
            .flex_col()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .relative()
                    .px_6()
                    .py_5()
                    .flex()
                    .items_center()
                    .gap_4()
                    .bg(gpui::linear_gradient(
                        120.,
                        gpui::linear_color_stop(shop_color(shop).opacity(0.55), 0.),
                        gpui::linear_color_stop(pal::bg(), 1.),
                    ))
                    .child(shop_mark(shop, 52.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::BOLD)
                                    .child(shop.name()),
                            )
                            .child(div().text_sm().text_color(pal::text_soft()).child(self.tr(
                                "Link your account to install your games",
                                "Oyunlarını kurmak için hesabını bağla",
                            ))),
                    )
                    .child(style::icon_button("shop-login-close", "x", false).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.close_shop_windows(cx);
                        }),
                    )),
            )
            .child(
                div()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(track)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_sm().child(self.tr(
                                "1. Log in on the page that opened in your browser.",
                                "1. Tarayıcında açılan sayfada giriş yap.",
                            )))
                            .child(div().text_sm().child(format!("2. {what}"))),
                    )
                    .child(mono(sample))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(style::icon("keyboard", 15., pal::muted()))
                            .child(keys),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded(px(12.))
                            .bg(pal::accent().opacity(0.08))
                            .border_1()
                            .border_color(pal::accent().opacity(0.25))
                            .child(status),
                    )
                    .children(field)
                    .children(error.map(|e| {
                        div()
                            .text_sm()
                            .text_color(pal::warning())
                            .child(format!("⚠ {}", self.shop_error(&e)))
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .id("shop-login-paste")
                                    .text_sm()
                                    .text_color(pal::muted())
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(pal::text()))
                                    .when(!paste, |d| {
                                        d.child(self.tr(
                                            "Paste it by hand instead",
                                            "Elle yapıştırmak istiyorum",
                                        ))
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(l) = this.shops.login.as_mut() {
                                            l.paste = true;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .children(url.map(|url| {
                                style::button(
                                    "shop-login-open",
                                    self.tr("Open the page again", "Sayfayı yeniden aç"),
                                    false,
                                )
                                .child(style::icon("external-link", 13., pal::muted()))
                                .on_click(move |_, _, cx| cx.open_url(&url))
                            })),
                    ),
            )
    }

    /// The wide picture of a store game; until it is there, its cover blurred (never a
    /// cut-out of the cover, which looks zoomed in) with the loading light on it.
    pub(super) fn shop_banner(
        &mut self,
        shop: Shop,
        id: &str,
        (w, h, radius): (f32, f32, f32),
        cx: &mut Context<Self>,
    ) -> Div {
        let wide = self.shop_wide(shop, id, cx);
        let pending = wide.is_none() && self.shop_wide_pending(shop, id);
        let cover = self.shop_cover(shop, id, cx);
        let ready = wide
            .as_ref()
            .is_some_and(|p| cover_art::backdrop_ready(p, Size::Wide));
        let mut out =
            div()
                .relative()
                .w(px(w))
                .h(px(h))
                .overflow_hidden()
                .bg(gpui::linear_gradient(
                    135.,
                    gpui::linear_color_stop(shop_color(shop).opacity(0.6), 0.),
                    gpui::linear_color_stop(pal::bg(), 1.),
                ));
        match (wide, cover) {
            (Some(p), _) if ready => {
                out = out.child(cover_art::backdrop(p, Size::Wide, radius).size_full());
            }
            (_, Some(c)) => {
                out = out.child(cover_art::backdrop(c, Size::Blur, radius).size_full());
            }
            _ => {}
        }
        if pending || (!ready && self.shop_wide(shop, id, cx).is_some()) {
            out = out.child(
                div()
                    .absolute()
                    .top_3()
                    .left_3()
                    .child(loader::spinner(26., self.global.appearance.reduce_motion)),
            );
        }
        out
    }

    fn shop_game_window(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let g = self.shops.game.as_ref()?;
        let (shop, id, title) = (g.shop, g.game.id.clone(), g.game.title.clone());
        let installed = self.shops.installed(shop, &id).cloned();
        let job = self.shop_job(shop, &id);
        const W: f32 = 820.;
        const H: f32 = 300.;
        let banner = self
            .shop_banner(shop, &id, (W, H, 0.), cx)
            .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(pal::bg().opacity(0.), 0.4),
                gpui::linear_color_stop(pal::bg(), 1.),
            )))
            .child(
                div().absolute().top_3().right_3().child(
                    style::icon_button("shop-game-close", "x", false)
                        .bg(pal::bg().opacity(0.8))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_shop_windows(cx);
                        })),
                ),
            );
        let cover = self.shop_art(shop, &id, &title, (126., 168., 12.), cx);
        let status = match (&installed, &job) {
            (_, Some(_)) => self.tr("Downloading", "İndiriliyor"),
            (Some(_), None) => self.tr("Installed", "Kurulu"),
            (None, None) => self.tr("In your library", "Hesabında"),
        };
        let head = div()
            .px_7()
            .mt(px(-96.))
            .relative()
            .flex()
            .items_end()
            .gap_5()
            .child(
                div()
                    .flex_none()
                    .rounded(px(14.))
                    .border_2()
                    .border_color(pal::bg())
                    .shadow_lg()
                    .child(cover),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pb_1()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::BOLD)
                            .line_clamp(2)
                            .child(title),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .text_color(pal::text_soft())
                            .child(shop_mark(shop, 20.))
                            .child(shop.name())
                            .child("·")
                            .child(status),
                    ),
            );
        let body = if let Some((key, fraction)) = job {
            self.shop_window_job(&key, fraction, cx)
        } else if installed.is_some() {
            self.shop_window_installed(shop, &id, cx)
        } else {
            self.shop_window_install(cx)
        };
        Some(
            div()
                .w(px(W))
                .max_h(relative(0.92))
                .rounded(px(20.))
                .overflow_hidden()
                .border_1()
                .border_color(pal::border_strong())
                .bg(pal::bg())
                .flex()
                .flex_col()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(banner)
                .child(head)
                .child(
                    div()
                        .id("shop-window-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px_7()
                        .pt_5()
                        .pb_7()
                        .child(body),
                ),
        )
    }

    /// A download or repair is under way.
    fn shop_window_job(&mut self, key: &str, fraction: Option<f32>, cx: &mut Context<Self>) -> Div {
        let entry = arkor_launch::progress::list()
            .into_iter()
            .find(|e| e.key == key);
        let detail = match &entry {
            Some(e) => match e.total {
                Some(t) => format!(
                    "{} / {}  ·  {}/s",
                    human_bytes(e.done),
                    human_bytes(t),
                    human_bytes(e.speed as u64)
                ),
                None => self.tr("Preparing", "Hazırlanıyor").to_string(),
            },
            None => self
                .tr("Waiting in the queue", "Sırada bekliyor")
                .to_string(),
        };
        let key = key.to_string();
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .p_5()
                    .rounded(px(16.))
                    .bg(pal::text().opacity(0.04))
                    .border_1()
                    .border_color(pal::text().opacity(0.08))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .justify_between()
                            .child(
                                div()
                                    .text_3xl()
                                    .font_weight(FontWeight::BOLD)
                                    .child(fraction.map_or("—".into(), |f| self.percent(f))),
                            )
                            .child(div().text_sm().text_color(pal::muted()).child(detail)),
                    )
                    .child(downloads_page::progress_bar(Some(fraction.unwrap_or(0.)))),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        style::button("shop-cancel", self.tr("Cancel", "İptal et"), false)
                            .child(style::icon("x", 13., pal::muted()))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                arkor_launch::downloads::cancel(&key);
                                cx.notify();
                            })),
                    )
                    .child(
                        style::button("shop-downloads", self.tr("Downloads", "İndirilenler"), true)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shops.game = None;
                                this.go(super::shell::Nav::Downloads, cx);
                            })),
                    ),
            )
    }

    fn shop_window_installed(&mut self, shop: Shop, id: &str, cx: &mut Context<Self>) -> Div {
        let Some(g) = self.shops.game.as_ref() else {
            return div();
        };
        if self.shops.removing.contains(&g.key()) {
            return self.loading(self.tr("Uninstalling…", "Kaldırılıyor…"));
        }
        let game = self.shops.installed(shop, id).cloned();
        if g.confirm_remove {
            let id = id.to_string();
            let size = game.as_ref().and_then(|g| g.size);
            return div()
                .p_5()
                .rounded(px(16.))
                .bg(pal::danger().opacity(0.08))
                .border_1()
                .border_color(pal::danger().opacity(0.3))
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(style::icon("trash", 20., pal::danger()))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(div().font_weight(FontWeight::SEMIBOLD).child(self.tr(
                                    "Delete the game's files?",
                                    "Oyunun dosyaları silinsin mi?",
                                )))
                                .child(div().text_sm().text_color(pal::muted()).child(self.tr(
                                    "Saves in its prefix stay.",
                                    "Prefix'indeki kayıtlar kalır.",
                                ))),
                        )
                        .children(
                            size.map(|s| div().text_color(pal::muted()).child(human_bytes(s))),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            style::button("shop-keep", self.tr("Keep", "Vazgeç"), false).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(g) = this.shops.game.as_mut() {
                                        g.confirm_remove = false;
                                    }
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            style::button("shop-remove", self.tr("Uninstall", "Kaldır"), true)
                                .bg(pal::danger())
                                .border_color(pal::danger())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.shop_uninstall(shop, &id, cx)
                                })),
                        ),
                );
        }
        let ids = [id.to_string(), id.to_string(), id.to_string()];
        let [play, page, repair] = ids;
        let fact = |label: &'static str, value: String| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_xs().text_color(pal::faint()).child(label))
                .child(div().text_sm().truncate().child(value))
        };
        let facts = game.map(|g| {
            div()
                .p_4()
                .rounded(px(14.))
                .bg(pal::text().opacity(0.04))
                .border_1()
                .border_color(pal::text().opacity(0.08))
                .flex()
                .gap_5()
                .child(fact(
                    self.tr("Folder", "Klasör"),
                    g.path.display().to_string(),
                ))
                .children(
                    g.size
                        .map(|s| fact(self.tr("On disk", "Diskte"), human_bytes(s)).flex_none()),
                )
                .child(
                    fact(
                        self.tr("Version", "Sürüm"),
                        if g.linux { "Linux" } else { "Windows" }.into(),
                    )
                    .flex_none(),
                )
        });
        let update = self.shop_update(shop, id);
        div().flex().flex_col().gap_4().children(facts).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    style::button("shop-uninstall", self.tr("Uninstall", "Kaldır"), false)
                        .child(style::icon("trash", 13., pal::muted()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(g) = this.shops.game.as_mut() {
                                g.confirm_remove = true;
                            }
                            cx.notify();
                        })),
                )
                .child(
                    style::button(
                        "shop-repair",
                        self.tr("Check files", "Dosyaları denetle"),
                        false,
                    )
                    .child(style::icon("shield-check", 13., pal::muted()))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.shop_repair(shop, &repair, cx)),
                    ),
                )
                .child(div().flex_1())
                .when(update, |d| {
                    let key = arkor_launch::shops::key(shop, id);
                    d.child(
                        style::button("shop-update", self.tr("Update", "Güncelle"), true)
                            .child(style::icon("circle-arrow-up", 13., pal::on_accent()))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.enqueue_download(&key, cx)),
                            ),
                    )
                })
                .child(
                    style::button("shop-page", self.tr("Settings", "Ayarlar"), false)
                        .child(style::icon("sliders-horizontal", 13., pal::muted()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.shop_show_in_library(shop, &page, cx)
                        })),
                )
                .child(
                    widgets::primary_button("shop-play", self.tr("Play", "Oyna"))
                        .child(style::icon("play", 15., pal::on_accent()))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.shop_play(shop, &play, cx)),
                        ),
                ),
        )
    }

    fn shop_window_install(&mut self, cx: &mut Context<Self>) -> Div {
        let t = |en: &'static str, tr: &'static str| self.tr(en, tr);
        let Some(g) = self.shops.game.as_ref() else {
            return div();
        };
        let logged_in = self.shops.logged_in(g.shop);
        let shop = g.shop;
        let heading = |text: &'static str| {
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(pal::faint())
                .child(text.to_uppercase())
        };
        // ---- Left: the choices ----
        let mut choices = div().flex_1().min_w_0().flex().flex_col().gap_5();
        if let Some(c) = &g.compat {
            choices = choices.child(compat_card(self, c));
        }
        if g.game.linux && g.game.windows {
            let seg = |id: &'static str, icon: &'static str, label: &'static str, on: bool| {
                div()
                    .id(id)
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .py_2()
                    .rounded(px(10.))
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(on, |d| d.bg(pal::accent()).text_color(pal::on_accent()))
                    .when(!on, |d| {
                        d.text_color(pal::text_soft())
                            .hover(|h| h.bg(pal::text().opacity(0.06)))
                    })
                    .child(style::icon(
                        icon,
                        15.,
                        if on { pal::on_accent() } else { pal::muted() },
                    ))
                    .child(label)
            };
            choices = choices.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(heading(t("Version", "Sürüm")))
                    .child(
                        div()
                            .p_1()
                            .flex()
                            .gap_1()
                            .rounded(px(12.))
                            .bg(pal::text().opacity(0.05))
                            .child(
                                seg(
                                    "shop-linux",
                                    "monitor",
                                    t("Linux (native)", "Linux (yerel)"),
                                    g.linux,
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.shop_set_platform(true, cx)),
                                ),
                            )
                            .child(
                                seg(
                                    "shop-windows",
                                    "wine",
                                    t("Windows (Proton)", "Windows (Proton)"),
                                    !g.linux,
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.shop_set_platform(false, cx)),
                                ),
                            ),
                    ),
            );
        }
        if let Some(Ok(info)) = &g.info {
            if info.languages.len() > 1 {
                let mut section = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(heading(t("Language", "Dil")))
                    .child(
                        div()
                            .id("shop-lang-open")
                            .px_3()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(10.))
                            .bg(pal::text().opacity(0.05))
                            .border_1()
                            .border_color(pal::text().opacity(0.1))
                            .cursor_pointer()
                            .hover(|s| s.bg(pal::text().opacity(0.09)))
                            .child(style::icon("languages", 15., pal::muted()))
                            .child(div().flex_1().text_sm().child(language_name(&g.language)))
                            .child(style::icon(
                                if g.languages_open {
                                    "chevron-up"
                                } else {
                                    "chevron-down"
                                },
                                15.,
                                pal::muted(),
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(g) = this.shops.game.as_mut() {
                                    g.languages_open = !g.languages_open;
                                }
                                cx.notify();
                            })),
                    );
                if g.languages_open {
                    let mut chips = div().flex().flex_wrap().gap_1p5();
                    for (ix, code) in info.languages.iter().enumerate() {
                        let pick = code.clone();
                        chips = chips.child(
                            widgets::chip(
                                ("shop-lang", ix),
                                language_name(code),
                                g.language == *code,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(g) = this.shops.game.as_mut() {
                                        g.language = pick.clone();
                                        g.languages_open = false;
                                    }
                                    cx.notify();
                                },
                            )),
                        );
                    }
                    section = section.child(chips);
                }
                choices = choices.child(section);
            }
            if !info.dlcs.is_empty() {
                let all = info.dlcs.iter().all(|d| g.dlcs.contains(&d.id));
                let mut list = div()
                    .flex()
                    .flex_col()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(pal::text().opacity(0.1))
                    .overflow_hidden();
                for (ix, d) in info.dlcs.iter().enumerate() {
                    let on = g.dlcs.contains(&d.id);
                    let dlc = d.id.clone();
                    list = list.child(
                        div()
                            .id(("shop-dlc", ix))
                            .px_3()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_3()
                            .cursor_pointer()
                            .when(ix > 0, |s| {
                                s.border_t_1().border_color(pal::text().opacity(0.08))
                            })
                            .hover(|s| s.bg(pal::text().opacity(0.05)))
                            .child(widgets::checkbox(on))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .child(d.title.clone()),
                            )
                            .children(d.size.map(|s| {
                                div()
                                    .text_xs()
                                    .text_color(pal::muted())
                                    .child(human_bytes(s))
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(g) = this.shops.game.as_mut()
                                    && !g.dlcs.remove(&dlc)
                                {
                                    g.dlcs.insert(dlc.clone());
                                }
                                cx.notify();
                            })),
                    );
                }
                let ids: Vec<String> = info.dlcs.iter().map(|d| d.id.clone()).collect();
                choices = choices.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(heading(t("DLC", "DLC")))
                                .child(
                                    div()
                                        .id("shop-dlc-all")
                                        .text_xs()
                                        .text_color(pal::accent_light())
                                        .cursor_pointer()
                                        .child(if all {
                                            t("Choose none", "Hiçbirini seçme")
                                        } else {
                                            t("Choose all", "Hepsini seç")
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(g) = this.shops.game.as_mut() {
                                                if all {
                                                    g.dlcs.clear();
                                                } else {
                                                    g.dlcs.extend(ids.iter().cloned());
                                                }
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(list),
                );
            }
        }
        // How a Windows build runs: its Proton and prefix (as Heroic asks), and shortcuts.
        // Steam chooses the Proton and the folder itself.
        let steam = g.shop == Shop::Steam;
        if !g.linux && !steam {
            let auto = match &self.global.defaults.proton {
                Some(d) => format!("{} ({d})", t("Default", "Varsayılan")),
                None => t("Automatic", "Otomatik").to_string(),
            };
            let current = g.proton.clone().unwrap_or(auto.clone());
            let mut proton = div()
                .flex()
                .flex_col()
                .gap_2()
                .child(heading("Proton"))
                .child(
                    div()
                        .id("shop-proton-open")
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(10.))
                        .bg(pal::text().opacity(0.05))
                        .border_1()
                        .border_color(pal::text().opacity(0.1))
                        .cursor_pointer()
                        .hover(|s| s.bg(pal::text().opacity(0.09)))
                        .child(style::icon("wine", 15., pal::muted()))
                        .child(div().flex_1().text_sm().child(current))
                        .child(style::icon(
                            if g.proton_open {
                                "chevron-up"
                            } else {
                                "chevron-down"
                            },
                            15.,
                            pal::muted(),
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(g) = this.shops.game.as_mut() {
                                g.proton_open = !g.proton_open;
                            }
                            cx.notify();
                        })),
                );
            if g.proton_open {
                let mut names: Vec<(String, Option<String>)> = vec![(auto, None)];
                for l in arkor_launch::components::Latest::ALL {
                    names.push((l.dir_name().into(), Some(l.dir_name().into())));
                }
                for (name, _) in &self.tools.protons {
                    if !names.iter().any(|(n, _)| n == name) {
                        names.push((name.clone(), Some(name.clone())));
                    }
                }
                let mut chips = div().flex().flex_wrap().gap_1p5();
                for (ix, (label, value)) in names.into_iter().enumerate() {
                    chips = chips.child(
                        widgets::chip(("shop-proton", ix), label, g.proton == value).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if let Some(g) = this.shops.game.as_mut() {
                                    g.proton = value.clone();
                                    g.proton_open = false;
                                }
                                cx.notify();
                            }),
                        ),
                    );
                }
                proton = proton.child(chips);
            }
            let prefix_text = match &g.prefix {
                Some(p) => p.display().to_string(),
                None => t(
                    "Its own, in the prefixes folder",
                    "Kendi prefix'i, prefix klasöründe",
                )
                .to_string(),
            };
            choices = choices.child(proton).child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(heading(t("Wine prefix", "Wine prefix")))
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(10.))
                            .bg(pal::text().opacity(0.05))
                            .border_1()
                            .border_color(pal::text().opacity(0.1))
                            .child(style::icon("folder-cog", 15., pal::muted()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .child(prefix_text),
                            )
                            .child(
                                div()
                                    .id("shop-prefix")
                                    .text_sm()
                                    .text_color(pal::accent_light())
                                    .cursor_pointer()
                                    .child(t("Change", "Değiştir"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let title =
                                            this.tr("Choose a Wine prefix", "Bir Wine prefix seç");
                                        this.prompt_path(true, title, cx, |this, path, cx| {
                                            if let Some(g) = this.shops.game.as_mut() {
                                                g.prefix = Some(path);
                                            }
                                            cx.notify();
                                        });
                                    })),
                            ),
                    ),
            );
        }
        let check = |id: &'static str, on: bool, label: &'static str| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .text_sm()
                .child(widgets::checkbox(on))
                .child(label)
        };
        choices = choices.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(heading(t("Shortcuts", "Kısayollar")))
                .child(
                    div()
                        .flex()
                        .gap_5()
                        .child(
                            check(
                                "shop-menu",
                                g.menu,
                                t("In the app menu", "Uygulama menüsünde"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(g) = this.shops.game.as_mut() {
                                    g.menu = !g.menu;
                                }
                                cx.notify();
                            })),
                        )
                        .child(
                            check(
                                "shop-desktop",
                                g.desktop,
                                t("On the desktop", "Masaüstünde"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(g) = this.shops.game.as_mut() {
                                    g.desktop = !g.desktop;
                                }
                                cx.notify();
                            })),
                        ),
                ),
        );
        if steam {
            choices = choices.child(div().text_sm().text_color(pal::muted()).child(t(
                "Steam downloads it into its library and keeps it up to date; its progress shows here.",
                "Steam onu kendi kütüphanesine indirir ve güncel tutar; ilerlemesi burada görünür.",
            )));
        }
        choices = choices.when(!steam, |c| {
            c.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(heading(t("Folder", "Klasör")))
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(10.))
                            .bg(pal::text().opacity(0.05))
                            .border_1()
                            .border_color(pal::text().opacity(0.1))
                            .child(style::icon("folder", 15., pal::muted()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .child(g.dir.display().to_string()),
                            )
                            .child(
                                div()
                                    .id("shop-dir")
                                    .text_sm()
                                    .text_color(pal::accent_light())
                                    .cursor_pointer()
                                    .child(t("Change", "Değiştir"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let title = this.tr(
                                            "Where to install games",
                                            "Oyunların kurulacağı yer",
                                        );
                                        this.prompt_path(true, title, cx, |this, path, cx| {
                                            if let Some(g) = this.shops.game.as_mut() {
                                                g.dir = path;
                                                g.error = None;
                                            }
                                            cx.notify();
                                        });
                                    })),
                            ),
                    ),
            )
        });

        // ---- Right: what it takes, and the button ----
        let free = arkor_launch::shops::free_space(&g.dir);
        let need = g.needed();
        let mut summary = div()
            .w(px(250.))
            .flex_none()
            .p_5()
            .rounded(px(16.))
            .bg(pal::text().opacity(0.04))
            .border_1()
            .border_color(pal::text().opacity(0.08))
            .flex()
            .flex_col()
            .gap_4();
        match &g.info {
            None if logged_in => {
                summary = summary
                    .child(self.loading_inline(t("Working out the size…", "Boyut hesaplanıyor…")))
                    .child(div().text_xs().text_color(pal::muted()).child(t(
                        "No need to wait: install now and Arkor uses the usual choices.",
                        "Beklemene gerek yok: şimdi kurarsan olağan seçimler kullanılır.",
                    )));
            }
            None => {
                summary = summary.child(widgets::label(t(
                    "Log in to install it.",
                    "Kurmak için giriş yap.",
                )));
            }
            Some(Err(e)) => {
                summary = summary.child(
                    div()
                        .text_sm()
                        .text_color(pal::warning())
                        .child(format!("⚠ {}", self.shop_error(e))),
                );
            }
            Some(Ok(info)) => {
                let row = |label: &'static str, value: String| {
                    div()
                        .flex()
                        .justify_between()
                        .text_sm()
                        .child(div().text_color(pal::muted()).child(label))
                        .child(div().font_weight(FontWeight::MEDIUM).child(value))
                };
                if let Some(n) = info.download {
                    summary = summary.child(row(t("Download", "İndirilecek"), human_bytes(n)));
                }
                if let Some(n) = need {
                    summary = summary.child(row(t("On disk", "Diskte"), human_bytes(n)));
                }
                if let (Some(free), Some(need)) = (free, need) {
                    // The disk: what the game takes of the free space.
                    let share = (need as f32 / free.max(1) as f32).min(1.);
                    let fits = need <= free;
                    summary = summary.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(
                                div()
                                    .h(px(8.))
                                    .rounded_full()
                                    .bg(pal::text().opacity(0.1))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(share.max(0.02)))
                                            .rounded_full()
                                            .bg(if fits {
                                                style::accent_gradient()
                                            } else {
                                                pal::danger().into()
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if fits { pal::muted() } else { pal::danger() })
                                    .child(if fits {
                                        format!(
                                            "{} {}",
                                            human_bytes(free - need),
                                            t("free afterwards", "boş kalır")
                                        )
                                    } else {
                                        format!(
                                            "{}: {} {}",
                                            t("Not enough space", "Yeterli yer yok"),
                                            human_bytes(free),
                                            t("free", "boş")
                                        )
                                    }),
                            ),
                    );
                }
            }
        }
        let can = can_install(g);
        summary = summary.children(g.error.clone().map(|e| {
            div()
                .text_sm()
                .text_color(pal::warning())
                .child(format!("⚠ {e}"))
        }));
        summary = summary.child(if logged_in {
            widgets::primary_button(
                "shop-install",
                if g.confirm_anyway {
                    t("Install anyway", "Yine de kur")
                } else {
                    t("Install", "Kur")
                },
            )
            .w_full()
            .py_3()
            .when(g.confirm_anyway, |d| d.bg(pal::danger()))
            .child(style::icon("download", 16., pal::on_accent()))
            .when(!can, |d| d.opacity(0.4))
            .on_click(cx.listener(move |this, _, _, cx| {
                let Some(g) = this.shops.game.as_mut() else {
                    return;
                };
                if !can_install(g) {
                    return;
                }
                // A game that will not work on Linux: asked once more.
                if g.compat.as_ref().is_some_and(|c| c.warns()) && !g.confirm_anyway {
                    g.confirm_anyway = true;
                    cx.notify();
                    return;
                }
                this.shop_install(cx);
            }))
        } else {
            widgets::primary_button("shop-install-login", t("Log in", "Giriş yap"))
                .w_full()
                .py_3()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.shops.game = None;
                    this.shop_login(shop, cx);
                }))
        });
        div()
            .flex()
            .items_start()
            .gap_6()
            .child(choices)
            .child(summary)
    }

    pub(super) fn shop_set_platform(&mut self, linux: bool, cx: &mut Context<Self>) {
        if let Some(g) = self.shops.game.as_mut()
            && g.linux != linux
        {
            g.linux = linux;
            self.shop_ask_info(cx);
        }
    }
}

/// The store answered and there is room.
/// There is room, and the store did not refuse. Its answer need not be in yet: the sizes
/// are only shown, the download checks the space itself.
pub(super) fn can_install(g: &GameUi) -> bool {
    !matches!(g.info, Some(Err(_))) && g.missing_space().is_none()
}

/// What is known about a game on Linux, as badges, with a warning when it will not work.
pub(super) fn compat_card(lib: &Library, c: &arkor_launch::compat::Compat) -> Div {
    use arkor_launch::compat::{AntiCheat, Deck};
    let t = |en: &'static str, tr: &'static str| lib.tr(en, tr);
    let mut badges = div().flex().flex_wrap().gap_2();
    let badge = |icon: &'static str, label: String, color: gpui::Hsla| {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .px_2p5()
            .py_1()
            .rounded_full()
            .bg(color.opacity(0.12))
            .border_1()
            .border_color(color.opacity(0.3))
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(color)
            .child(style::icon(icon, 13., color))
            .child(label)
    };
    if let Some(a) = c.anticheat {
        let (text, color) = match a {
            AntiCheat::Supported => (
                t("Anti-cheat: supported", "Hile koruması: destekleniyor"),
                pal::success(),
            ),
            AntiCheat::Running => (
                t("Anti-cheat: works", "Hile koruması: çalışıyor"),
                pal::success(),
            ),
            AntiCheat::Planned => (
                t("Anti-cheat: planned", "Hile koruması: planlanıyor"),
                pal::warning(),
            ),
            AntiCheat::Broken => (
                t("Anti-cheat: broken", "Hile koruması: bozuk"),
                pal::danger(),
            ),
            AntiCheat::Denied => (
                t(
                    "Anti-cheat: Linux refused",
                    "Hile koruması: Linux reddediliyor",
                ),
                pal::danger(),
            ),
        };
        badges = badges.child(badge("shield-check", text.into(), color));
    }
    if let Some(tier) = &c.protondb {
        let color = match tier.as_str() {
            "platinum" | "native" | "gold" => pal::success(),
            "silver" | "bronze" => pal::warning(),
            _ => pal::danger(),
        };
        let mut name = tier.clone();
        if let Some(f) = name.get_mut(0..1) {
            f.make_ascii_uppercase();
        }
        badges = badges.child(badge("gauge", format!("ProtonDB: {name}"), color));
    }
    if let Some(d) = c.deck {
        let (text, color) = match d {
            Deck::Verified => (
                t("Steam Deck: verified", "Steam Deck: doğrulanmış"),
                pal::success(),
            ),
            Deck::Playable => (
                t("Steam Deck: playable", "Steam Deck: oynanabilir"),
                pal::warning(),
            ),
            Deck::Unsupported => (
                t("Steam Deck: unsupported", "Steam Deck: desteklenmiyor"),
                pal::danger(),
            ),
        };
        badges = badges.child(badge("gamepad-2", text.into(), color));
    }
    let mut out = div().flex().flex_col().gap_2();
    if c.anticheat.is_none() && c.protondb.is_none() && c.deck.is_none() {
        return out;
    }
    out = out.child(
        div()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(pal::faint())
            .child(t("ON LINUX", "LINUX'TA")),
    );
    out = out.child(badges);
    if c.warns() {
        let names = if c.anticheats.is_empty() {
            String::new()
        } else {
            format!(" ({})", c.anticheats.join(", "))
        };
        let text = if matches!(c.anticheat, Some(AntiCheat::Broken | AntiCheat::Denied)) {
            format!(
                "{}{names}",
                t(
                    "Its anti-cheat does not let it run on Linux: online play will not work, and there is a risk of a ban.",
                    "Hile koruması Linux'ta çalışmasına izin vermiyor: çevrim içi oyun çalışmaz ve ban riski var.",
                )
            )
        } else {
            t(
                "ProtonDB players report that it does not run.",
                "ProtonDB oyuncuları çalışmadığını bildiriyor.",
            )
            .to_string()
        };
        out = out.child(
            div()
                .p_3()
                .flex()
                .items_start()
                .gap_2()
                .rounded(px(12.))
                .bg(pal::danger().opacity(0.1))
                .border_1()
                .border_color(pal::danger().opacity(0.3))
                .text_sm()
                .child(style::icon("triangle-alert", 16., pal::danger()))
                .child(div().flex_1().child(text)),
        );
    }
    out
}
