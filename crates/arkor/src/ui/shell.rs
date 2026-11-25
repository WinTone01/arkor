//! The window frame: background, the sidebar (navigation, recently played, running games),
//! the top bar (where you are, status) and the content panel the pages draw into.

use gpui::prelude::FluentBuilder;
use gpui::*;

use super::console::ToggleConsole;
use super::style::{self, pal};
use super::{Library, Selection};

actions!(
    shell,
    [
        FocusSearch,
        GoLibrary,
        GoSettings,
        GoBack,
        NextTab,
        PrevTab,
        PlayGame
    ]
);

/// Window-wide shortcuts. Text fields keep their own keys (their context is more specific).
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-f", FocusSearch, Some("Arkor")),
        KeyBinding::new("ctrl-l", GoLibrary, Some("Arkor")),
        KeyBinding::new("ctrl-,", GoSettings, Some("Arkor")),
        KeyBinding::new("escape", GoBack, Some("Arkor")),
        KeyBinding::new("ctrl-tab", NextTab, Some("Arkor")),
        KeyBinding::new("ctrl-shift-tab", PrevTab, Some("Arkor")),
        KeyBinding::new("ctrl-enter", PlayGame, Some("Arkor")),
    ]);
}

/// Shortcut list for the Settings page: (keys, English, Turkish).
pub(super) const SHORTCUTS: [(&str, &str, &str); 9] = [
    (
        "Ctrl+F",
        "Search the library, or the open game's settings",
        "Kütüphanede ya da açık oyunun ayarlarında ara",
    ),
    ("Ctrl+L", "Library", "Kütüphane"),
    ("Ctrl+,", "Settings", "Ayarlar"),
    (
        "Esc",
        "Back from a game's page or Add game to the library",
        "Oyun sayfasından ya da Oyun ekle'den kütüphaneye dön",
    ),
    (
        "Ctrl+Tab",
        "Next tab of the game page",
        "Oyun sayfasında sonraki sekme",
    ),
    (
        "Ctrl+Shift+Tab",
        "Previous tab of the game page",
        "Oyun sayfasında önceki sekme",
    ),
    (
        "Ctrl+Enter",
        "Play or stop the open game",
        "Açık oyunu başlat ya da durdur",
    ),
    (
        "Ctrl+H",
        "Show or hide hidden games",
        "Gizli oyunları göster ya da gizle",
    ),
    ("F11", "Console mode (gamepad)", "Konsol modu (oyun kolu)"),
];

/// Sidebar entries. Overlay and Settings share the global settings page (different tabs).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Nav {
    Library,
    Add,
    Shops,
    Protons,
    Overlay,
    Bench,
    Downloads,
    System,
    Settings,
}

impl Library {
    fn nav_active(&self, nav: Nav) -> bool {
        match (nav, self.selection) {
            // Adding a game is part of the library (its button is on the library page).
            (
                Nav::Library,
                None | Some(Selection::Grid) | Some(Selection::Game(_)) | Some(Selection::Add),
            ) => true,
            (Nav::Shops, Some(Selection::Shops)) => true,
            (Nav::Protons, Some(Selection::Protons)) => true,
            (Nav::Bench, Some(Selection::Bench)) => true,
            (Nav::Downloads, Some(Selection::Downloads)) => true,
            (Nav::System, Some(Selection::System)) => true,
            (Nav::Overlay, Some(Selection::Global)) => self.global_hud_tab,
            (Nav::Settings, Some(Selection::Global)) => !self.global_hud_tab,
            _ => false,
        }
    }

    pub(super) fn go(&mut self, nav: Nav, cx: &mut Context<Self>) {
        let selection = match nav {
            Nav::Library => Selection::Grid,
            Nav::Add => Selection::Add,
            Nav::Shops => Selection::Shops,
            Nav::Protons => Selection::Protons,
            Nav::Bench => Selection::Bench,
            Nav::Downloads => Selection::Downloads,
            Nav::System => Selection::System,
            Nav::Overlay | Nav::Settings => Selection::Global,
        };
        self.global_hud_tab = nav == Nav::Overlay;
        self.select(selection, cx);
        if nav == Nav::Shops {
            self.shops_opened(cx);
        }
    }

    fn nav_title(&self, nav: Nav) -> &'static str {
        match nav {
            Nav::Library => self.tr("Library", "Kütüphane"),
            Nav::Add => self.tr("Add game", "Oyun ekle"),
            Nav::Shops => self.tr("Stores", "Mağazalar"),
            Nav::Protons => self.tr("Proton & Wine", "Proton ve Wine"),
            Nav::Overlay => self.tr("MangoHud overlay", "MangoHud overlay"),
            Nav::Bench => self.tr("Benchmarks", "Benchmark'lar"),
            Nav::Downloads => self.tr("Downloads", "İndirilenler"),
            Nav::System => self.tr("System", "Sistem"),
            Nav::Settings => self.tr("Settings", "Ayarlar"),
        }
    }

    fn nav_item(
        &self,
        nav: Nav,
        icon: &'static str,
        badge: Option<String>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let active = self.nav_active(nav);
        div()
            .id(("nav", nav as usize))
            .flex()
            .gap_3()
            .px_3()
            .py_2()
            .rounded(px(12.))
            .items_center()
            .text_sm()
            .cursor_pointer()
            .when(active, |s| {
                s.bg(pal::raised())
                    .text_color(pal::text())
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!active, |s| {
                s.text_color(pal::muted())
                    .hover(|h| h.bg(pal::raised().opacity(0.6)).text_color(pal::text()))
            })
            .child(style::icon(
                icon,
                16.,
                if active {
                    pal::accent_light()
                } else {
                    pal::muted()
                },
            ))
            .child(div().flex_1().child(self.nav_title(nav)))
            .children(badge.map(|b| div().text_xs().text_color(pal::faint()).child(b)))
            .on_click(cx.listener(move |this, _, _, cx| this.go(nav, cx)))
    }

    fn section_label(text: &'static str) -> Div {
        div()
            .px_3()
            .pt_4()
            .pb_1()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(pal::faint())
            .child(text.to_uppercase())
    }

    /// The last played games, for one-click access from any page.
    fn recent_games(&self) -> Vec<usize> {
        let mut played: Vec<(u64, usize)> = self
            .games
            .iter()
            .enumerate()
            .filter_map(|(ix, g)| self.playtime(&g.id).1.map(|last| (last, ix)))
            .collect();
        played.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
        played.into_iter().take(4).map(|(_, ix)| ix).collect()
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let separator = || div().h(px(1.)).mx_2().my_2().bg(pal::glass_spec().border);
        let count = Some(self.games.len().to_string()).filter(|c| c != "0");
        let mut side = style::panel()
            .w(px(240.))
            .h_full()
            .flex_none()
            .p_3()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .px_2()
                    .pt_2()
                    .pb_2()
                    .items_center()
                    .child(self.sidebar_logo())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_sm().font_weight(FontWeight::BOLD).child("Arkor"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(pal::muted())
                                    .child(self.tr("Game launcher", "Oyun başlatıcı")),
                            ),
                    ),
            )
            .child(separator())
            .child(self.nav_item(Nav::Library, "library-big", count, cx))
            .child(self.nav_item(Nav::Shops, "store", None, cx))
            .child(Self::section_label(self.tr("Tools", "Araçlar")))
            .child(self.nav_item(Nav::Protons, "wine", None, cx))
            .child(self.nav_item(Nav::Overlay, "gauge", None, cx))
            .child(self.nav_item(Nav::Bench, "chart-line", None, cx))
            .child(self.nav_item(Nav::Downloads, "hard-drive-download", None, cx));

        let recent = self.recent_games();
        if !recent.is_empty() {
            side = side.child(Self::section_label(
                self.tr("Continue playing", "Oynamaya devam et"),
            ));
            for ix in recent {
                let game = &self.games[ix];
                let active = self.selection == Some(Selection::Game(ix));
                let running = self.running.contains_key(&game.id.to_string());
                side = side.child(
                    div()
                        .id(("recent", ix))
                        .flex()
                        .items_center()
                        .gap_2p5()
                        .px_2()
                        .py_1p5()
                        .rounded(px(10.))
                        .cursor_pointer()
                        .when(active, |s| s.bg(pal::raised()))
                        .when(!active, |s| s.hover(|h| h.bg(pal::raised().opacity(0.6))))
                        .child(self.cover_tile(&game.id.id, &game.name, 22., 30.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .text_color(if active {
                                    pal::text()
                                } else {
                                    pal::text_soft()
                                })
                                .child(game.name.clone()),
                        )
                        .when(running, |s| {
                            s.child(div().size(px(7.)).rounded_full().bg(pal::success()))
                        })
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.select(Selection::Game(ix), cx)),
                        ),
                );
            }
        }

        side = side.child(div().flex_1());
        if let Some(note) = self.render_update_note(cx) {
            side = side.child(note);
        }
        // Downloads in progress, like a store's download bar; opens the Downloads page.
        let active = arkor_launch::progress::list();
        // Jobs still looking up their release are only being checked; once bytes flow (or
        // the download is being unpacked) it is a download.
        let downloading = active
            .iter()
            .filter(|e| e.phase != arkor_launch::progress::Phase::Preparing)
            .count();
        let checking = downloading == 0;
        if !active.is_empty() {
            let mut card = div()
                .id("downloads-progress")
                .mb_2()
                .p_3()
                .rounded(px(12.))
                .bg(pal::accent().opacity(0.08))
                .border_1()
                .border_color(pal::accent().opacity(0.3))
                .flex()
                .flex_col()
                .gap_2()
                .cursor_pointer()
                .hover(|s| s.bg(pal::accent().opacity(0.14)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(pal::muted())
                        .child(style::icon(
                            if checking { "refresh-cw" } else { "download" },
                            13.,
                            pal::accent_light(),
                        ))
                        .child(if checking {
                            self.tr("Checking for updates", "Güncellemeler denetleniyor")
                                .to_string()
                        } else {
                            format!(
                                "{} · {}",
                                self.tr("Downloading", "İndiriliyor"),
                                downloading
                            )
                        }),
                );
            // Downloads first, then what is only being checked.
            let mut shown: Vec<_> = active.iter().collect();
            shown.sort_by_key(|e| e.phase == arkor_launch::progress::Phase::Preparing);
            for e in shown.into_iter().take(2) {
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .gap_2()
                                .text_sm()
                                .child(div().min_w_0().truncate().child(e.label.clone()))
                                .children(e.fraction().map(|f| {
                                    div()
                                        .text_xs()
                                        .text_color(pal::accent_light())
                                        .child(format!("{:.0}%", f * 100.))
                                })),
                        )
                        .when(e.phase != arkor_launch::progress::Phase::Preparing, |d| {
                            d.child(super::downloads_page::progress_bar(e.fraction()))
                        }),
                );
            }
            side = side
                .child(card.on_click(cx.listener(|this, _, _, cx| this.go(Nav::Downloads, cx))));
        }
        if !self.running.is_empty() {
            let n = self.running.len();
            side = side.child(
                style::card()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(8.)).rounded_full().bg(pal::success()))
                            .child(
                                div()
                                    .text_sm()
                                    .child(format!("{n} {}", self.tr("running", "çalışıyor"))),
                            ),
                    )
                    .child(
                        style::button("stop-all", self.tr("Stop all", "Hepsini durdur"), false)
                            .child(style::icon("square", 12., pal::danger()))
                            .on_click(cx.listener(|this, _, _, cx| this.stop_all(cx))),
                    ),
            );
        }
        side.children(self.warnings.first().map(|w| {
            div()
                .mt_2()
                .px_2()
                .text_xs()
                .text_color(pal::warning())
                .child(format!("⚠ {w}"))
        }))
        // App-level pages stay at the bottom, out of the way of the library and tools.
        .child(separator())
        .child(self.nav_item(Nav::System, "cpu", None, cx))
        .child(self.nav_item(Nav::Settings, "settings", None, cx))
    }

    /// Where you are, e.g. "Library › Far Cry 6", and the latest status message.
    fn render_topbar(&self, cx: &mut Context<Self>) -> Div {
        let crumb = |text: SharedString, current: bool| {
            div()
                .text_color(if current { pal::text() } else { pal::faint() })
                .when(current, |d| d.font_weight(FontWeight::MEDIUM))
                .child(text)
        };
        let mut path = div().flex().gap_2().items_center().text_sm();
        let (first, second): (&'static str, Option<SharedString>) = match self.selection {
            Some(Selection::Game(ix)) => (
                self.nav_title(Nav::Library),
                self.games.get(ix).map(|g| g.name.clone().into()),
            ),
            Some(Selection::Add) => (
                self.nav_title(Nav::Library),
                Some(self.nav_title(Nav::Add).into()),
            ),
            Some(Selection::Shops) => (self.nav_title(Nav::Shops), None),
            Some(Selection::Protons) => (self.nav_title(Nav::Protons), None),
            Some(Selection::Bench) => (self.nav_title(Nav::Bench), None),
            Some(Selection::Downloads) => (self.nav_title(Nav::Downloads), None),
            Some(Selection::System) => (self.nav_title(Nav::System), None),
            Some(Selection::Global) if self.global_hud_tab => (self.nav_title(Nav::Overlay), None),
            Some(Selection::Global) => (self.nav_title(Nav::Settings), None),
            _ => (self.nav_title(Nav::Library), None),
        };
        path = path.child(crumb("Arkor".into(), false)).child(style::icon(
            "chevron-right",
            14.,
            pal::faint(),
        ));
        // On a sub-page (a game, Add game) the first part leads back to the library.
        path = if second.is_some() {
            path.child(
                div()
                    .id("crumb-back")
                    .cursor_pointer()
                    .rounded(px(6.))
                    .hover(|s| s.bg(pal::raised()))
                    .px_1()
                    .child(crumb(first.into(), false))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Nav::Library, cx))),
            )
        } else {
            path.child(crumb(first.into(), true))
        };
        if let Some(second) = second {
            path = path
                .child(style::icon("chevron-right", 14., pal::faint()))
                .child(crumb(second, true));
        }
        div()
            .h(px(30.))
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(path)
            .child(div().flex_1())
            .children(self.status.clone().map(|s| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(pal::raised().opacity(0.7))
                    .border_1()
                    .border_color(pal::glass_spec().border)
                    .text_xs()
                    .text_color(pal::text_soft())
                    .child(style::icon("info", 13., pal::accent_light()))
                    .child(div().truncate().child(s))
            }))
            .child({
                let text: SharedString = self.tr("Console mode (F11)", "Konsol modu (F11)").into();
                div()
                    .id("to-console")
                    .flex_none()
                    .px_2()
                    .py_1()
                    .rounded(px(9.))
                    .cursor_pointer()
                    .hover(|s| s.bg(pal::raised()))
                    .child(style::icon("gamepad-2", 18., pal::muted()))
                    .tooltip(move |_, cx| super::widgets::tip(text.clone(), cx))
                    .on_click(cx.listener(|this, _, w, cx| this.toggle_console(w, cx)))
            })
    }
}

impl Library {
    /// Ctrl+F: the game page's settings search, otherwise the library search.
    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = if matches!(self.selection, Some(Selection::Game(_))) {
            let placeholder = self.tr("Search settings…", "Ayarlarda ara…");
            self.field(
                "feature-search",
                |this| this.feature_query.clone(),
                placeholder,
                cx,
                |this, text| {
                    this.feature_query = text.to_string();
                },
            )
        } else {
            if !matches!(self.selection, None | Some(Selection::Grid)) {
                self.go(Nav::Library, cx);
            }
            self.search_input(cx)
        };
        // The page change above must not take the focus back from the field.
        self.refocus = false;
        let handle = input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab on the game page (tabs without anything for the game are skipped).
    fn step_game_tab(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(entry) = self.entry().cloned() else {
            return;
        };
        let tabs: Vec<super::features::GameTab> = super::features::GameTab::ALL
            .into_iter()
            .filter(|t| {
                *t == super::features::GameTab::Overview
                    || super::features::FEATURES
                        .iter()
                        .any(|f| f.tab == *t && (f.applies)(self, &entry))
            })
            .collect();
        let at = tabs.iter().position(|t| *t == self.game_tab).unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(tabs.len() as i32) as usize;
        self.game_tab = tabs[next];
        self.feature_query.clear();
        self.fields.inputs.remove("feature-search");
        cx.notify();
    }
}

impl Render for Library {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.viewport_w = f32::from(window.viewport_size().width);
        self.window_active = window.is_window_active();
        self.apply_theme(window, cx);
        self.watch_progress(cx);
        self.keep_moving(window, cx);
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        // After a page or mode change a focused field may no longer be drawn; keys would
        // then reach nothing, so focus goes back to the window.
        if self.refocus || window.focused(cx).is_none() {
            self.refocus = false;
            window.focus(&focus, cx);
        }
        let look = self.global.appearance.clone();
        let base = div()
            .size_full()
            .capture_key_down(cx.listener(|this, _, _, cx| this.skip_intro(cx)))
            .relative()
            .overflow_hidden()
            .bg(pal::bg())
            .text_color(pal::text())
            .font_family(style::FONT)
            .text_size(px(look.font_size() as f32))
            .child(super::backdrop::render(
                look.backdrop,
                look.reduce_motion,
                look.backdrop_image.as_deref(),
                self.motion.0,
            ));
        // The start-up animation lies over the page, which is drawn underneath all along.
        let page_in = self.step_intro(window);
        let intro = self.render_intro(window, cx);
        if self.console.on {
            use super::console::*;
            let console = self.render_console(window, cx);
            // The keys, as if they were buttons.
            macro_rules! keys {
                ($base:expr, $($action:ident => $nav:ident),*) => {
                    $base$(.on_action(cx.listener(|this, _: &$action, w, cx| {
                        this.console.pad = false;
                        this.nav(Nav::$nav, w, cx)
                    })))*
                };
            }
            let base = base
                .track_focus(&focus)
                .key_context(CONTEXT)
                .on_action(cx.listener(|this, _: &ToggleConsole, w, cx| this.toggle_console(w, cx)))
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                    let k = &e.keystroke;
                    if !k.modifiers.control && !k.modifiers.alt && !k.modifiers.platform {
                        this.console_type(&k.key, k.key_char.as_deref(), cx);
                    }
                }));
            let base = keys!(base,
                NavUp => Up, NavDown => Down, NavLeft => Left, NavRight => Right,
                NavAccept => Accept, NavBack => Back, NavExtra => Extra, NavSearch => Search,
                NavPrevTab => PrevTab, NavNextTab => NextTab, NavPageUp => PageUp,
                NavPageDown => PageDown, NavMenu => Menu);
            return base.child(console).children(intro);
        }
        let detail = self.render_detail(window, cx);
        let sidebar = self.render_sidebar(cx);
        base.track_focus(&focus)
            .key_context("Arkor")
            .on_action(cx.listener(|this, _: &super::ToggleHidden, _, cx| this.toggle_hidden(cx)))
            .on_action(
                cx.listener(|this, _: &FocusSearch, window, cx| this.focus_search(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &GoLibrary, _, cx| this.go(super::shell::Nav::Library, cx)),
            )
            .on_action(
                cx.listener(|this, _: &GoSettings, _, cx| this.go(super::shell::Nav::Settings, cx)),
            )
            .on_action(cx.listener(|this, _: &GoBack, _, cx| {
                if this.close_shop_windows(cx) {
                    return;
                }
                if matches!(this.selection, Some(Selection::Game(_) | Selection::Add)) {
                    this.go(super::shell::Nav::Library, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextTab, _, cx| this.step_game_tab(1, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, _, cx| this.step_game_tab(-1, cx)))
            .on_action(cx.listener(|this, _: &PlayGame, _, cx| {
                if matches!(this.selection, Some(Selection::Game(_))) {
                    this.toggle_play(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleConsole, w, cx| this.toggle_console(w, cx)))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .p_4()
                    .gap_4()
                    .child(
                        div()
                            .h_full()
                            .flex_none()
                            .relative()
                            .when_some(page_in, |d, p| {
                                d.left(px(-56. * (1. - p.sidebar)))
                                    .opacity(p.sidebar.max(0.02))
                            })
                            .child(sidebar),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .gap_3()
                            .relative()
                            .when_some(page_in, |d, p| {
                                d.top(px(34. * (1. - p.panel))).opacity(p.panel.max(0.02))
                            })
                            .child(self.render_topbar(cx))
                            .child(
                                style::panel()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(detail),
                            ),
                    ),
            )
            .children(self.render_sgdb_picker(cx))
            .children(self.render_shop_windows(cx))
            .children(intro)
    }
}
