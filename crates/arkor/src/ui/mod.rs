//! Arkor's window: navigation and the game library on the left, the selected page on the
//! right. Every change is saved immediately; `arkor run` picks it up on the next launch.

mod add_game;
mod bench_page;
mod big_screen;
mod enhance;
mod fields;
mod game_page;
mod global_page;
mod grid;
mod hud_editor;
mod i18n;
mod protons;
mod sounds;
mod splash;
mod system_page;
mod text_input;
mod tray;
mod widgets;

use std::collections::HashMap;

use arkor_core::library::{Library as GameLibrary, LibraryGame};
use arkor_core::mangohud::HudSettings;
use arkor_core::{Game, GameId, Global, HudMode, Language, Profile};
use gpui::{
    AnyElement, App, Application, Bounds, Context, Div, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
    uniform_list,
};

use add_game::AddState;
use enhance::GameTools;
use fields::Fields;
use game_page::{GameTab, GameUi};
use i18n::Strings;
use protons::ProtonsState;
use widgets::theme::*;

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    Grid,
    Bench,
    Global,
    System,
    Add,
    Protons,
    Game(usize),
}

gpui::actions!(arkor, [ToggleHidden]);

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Sort {
    #[default]
    Name,
    Recent,
    Playtime,
}

/// How the sidebar list is filtered and sorted.
#[derive(Default)]
struct ListFilter {
    sort: Sort,
    category: Option<String>,
    show_hidden: bool,
}

pub struct Library {
    /// The library as display rows (sorted by name) and the raw entries.
    games: Vec<Game>,
    library: GameLibrary,
    list_scroll: UniformListScrollHandle,
    warnings: Vec<String>,
    selection: Option<Selection>,
    /// Profile of the selected game (only meaningful when a game is selected).
    profile: Profile,
    global: Global,
    add: AddState,
    tools: GameTools,
    protons: ProtonsState,
    game_ui: GameUi,
    game_tab: GameTab,
    /// "More MangoHud settings" expanded.
    hud_advanced: bool,
    /// Global settings page shows its MangoHud tab.
    global_hud_tab: bool,
    fields: Fields,
    system: system_page::SystemState,
    bench: bench_page::BenchState,
    /// Games started from this window: game id -> process group id.
    running: HashMap<String, u32>,
    status: Option<SharedString>,
    stats: arkor_core::stats::Stats,
    filter: ListFilter,
    /// Rows of the sidebar list: indices into `games` after search/category/hidden/sort.
    visible: Vec<usize>,
    search: Option<(gpui::Entity<text_input::TextInput>, gpui::Subscription)>,
    search_text: String,
    focus: Option<gpui::FocusHandle>,
    backup_prefixes: bool,
    big_screen: bool,
    big_focus: usize,
    big_cols: usize,
    restore_pending: Option<std::path::PathBuf>,
    /// Cover image per game id (looked up once per library load).
    covers: HashMap<String, Option<std::path::PathBuf>>,
}

impl Library {
    fn new() -> Self {
        let mut warnings = Vec::new();
        let global = Global::load_or_import().unwrap_or_else(|e| {
            // The language setting itself could not be read, so this one is in the default language.
            warnings.push(format!(
                "{}: {e:#}",
                i18n::strings(Language::default()).global_load_failed
            ));
            Global::default()
        });
        let mut lib = Self {
            games: Vec::new(),
            library: GameLibrary::default(),
            list_scroll: UniformListScrollHandle::new(),
            warnings,
            selection: None,
            profile: Profile::default(),
            global,
            add: AddState::default(),
            tools: GameTools::scan(),
            protons: ProtonsState::default(),
            game_ui: GameUi::default(),
            game_tab: GameTab::default(),
            hud_advanced: false,
            global_hud_tab: false,
            fields: Fields::default(),
            system: Default::default(),
            bench: Default::default(),
            running: HashMap::new(),
            status: None,
            stats: arkor_core::stats::Stats::load(),
            filter: ListFilter::default(),
            visible: Vec::new(),
            search: None,
            search_text: String::new(),
            focus: None,
            covers: HashMap::new(),
            backup_prefixes: false,
            big_screen: false,
            big_focus: 0,
            big_cols: 1,
            restore_pending: None,
        };
        lib.reload_games();
        lib
    }

    /// Re-reads the library, keeping the selected game selected if it still exists.
    fn reload_games(&mut self) {
        let selected = self.selected_id();
        match GameLibrary::load() {
            Ok(library) => self.library = library,
            Err(e) => self.warnings.push(format!("{e:#}")),
        }
        self.games = self.library.games();
        self.games.sort_by_key(|g| g.name.to_lowercase());
        self.stats = arkor_core::stats::Stats::load();
        self.covers = self
            .library
            .games
            .iter()
            .map(|g| (g.id.clone(), arkor_launch::covers::cover_for(g)))
            .collect();
        self.apply_filter();
        if let Some(id) = selected {
            self.selection = self
                .games
                .iter()
                .position(|g| g.id == id)
                .map(Selection::Game);
        }
    }

    /// Play time (seconds) and last-played time, Arkor's or Steam's (whichever knows more).
    pub(crate) fn playtime(&self, id: &GameId) -> (u64, Option<u64>) {
        let ours = self.stats.get(id);
        let steam = match self.library.get(&id.id).map(|g| &g.kind) {
            Some(arkor_core::library::GameKind::Steam { appid }) => {
                arkor_stores::steam::playtime(appid)
            }
            _ => None,
        };
        let (steam_secs, steam_last) = steam
            .map(|(m, l)| (m * 60, Some(l).filter(|l| *l > 0)))
            .unwrap_or((0, None));
        (
            ours.playtime_secs.max(steam_secs),
            ours.last_played.max(steam_last),
        )
    }

    pub(crate) fn apply_filter(&mut self) {
        let needle: String = self.search_text.to_lowercase();
        let mut rows: Vec<usize> = (0..self.games.len())
            .filter(|&ix| {
                let game = &self.games[ix];
                let entry = self.library.get(&game.id.id);
                let hidden = entry.is_some_and(|e| e.hidden);
                let category_ok = match &self.filter.category {
                    Some(c) => entry.and_then(|e| e.category.as_ref()) == Some(c),
                    None => true,
                };
                (self.filter.show_hidden || !hidden)
                    && category_ok
                    && (needle.is_empty() || game.name.to_lowercase().contains(&needle))
            })
            .collect();
        match self.filter.sort {
            Sort::Name => {}
            Sort::Recent => rows.sort_by_key(|&ix| {
                std::cmp::Reverse(self.playtime(&self.games[ix].id).1.unwrap_or(0))
            }),
            Sort::Playtime => {
                rows.sort_by_key(|&ix| std::cmp::Reverse(self.playtime(&self.games[ix].id).0))
            }
        }
        self.visible = rows;
    }

    fn categories(&self) -> Vec<String> {
        let mut c: Vec<String> = self
            .library
            .games
            .iter()
            .filter_map(|g| g.category.clone())
            .collect();
        c.sort_by_key(|c| c.to_lowercase());
        c.dedup();
        c
    }

    fn selected_id(&self) -> Option<GameId> {
        match self.selection {
            Some(Selection::Game(ix)) => self.games.get(ix).map(|g| g.id.clone()),
            _ => None,
        }
    }

    /// The selected game's library entry.
    pub(crate) fn entry(&self) -> Option<&LibraryGame> {
        let id = self.selected_id()?;
        self.library.get(&id.id)
    }

    pub(crate) fn entry_mut(&mut self) -> Option<&mut LibraryGame> {
        let id = self.selected_id()?;
        self.library.get_mut(&id.id)
    }

    /// Strings in the user's chosen language.
    pub(crate) fn t(&self) -> &'static Strings {
        i18n::strings(self.global.language)
    }

    /// Inline translation: both languages are written where the text is used.
    pub(crate) fn tr(&self, en: &'static str, tr: &'static str) -> &'static str {
        match self.global.language {
            Language::En => en,
            Language::Tr => tr,
        }
    }

    fn open(&mut self, target: Option<&str>) {
        // `local:far-cry-6#mods` opens a game page on a tab.
        let (target, tab) = match target.and_then(|t| t.split_once('#')) {
            Some((t, tab)) => (Some(t), Some(tab)),
            None => (target, None),
        };
        self.hud_advanced = tab == Some("advanced");
        self.global_hud_tab = matches!(tab, Some("advanced" | "mangohud"));
        self.game_tab = match tab {
            Some("launch") => GameTab::Launch,
            Some("wine") => GameTab::Wine,
            Some("performance") => GameTab::Performance,
            Some("mods") => GameTab::Mods,
            Some("logs") => GameTab::Logs,
            _ => GameTab::General,
        };
        let selection = match target {
            None => return,
            Some("global") => Selection::Global,
            Some("add") | Some("import") => Selection::Add,
            Some("protons") => Selection::Protons,
            Some("system") => Selection::System,
            Some("library") => Selection::Grid,
            Some("benchmarks") => Selection::Bench,
            Some("big") => {
                self.big_screen = true;
                Selection::Grid
            }
            Some(id) => match self.games.iter().position(|g| g.id.to_string() == id) {
                Some(ix) => Selection::Game(ix),
                None => {
                    self.warnings
                        .insert(0, format!("`{id}` {}", self.t().not_found));
                    return;
                }
            },
        };
        if selection == Selection::Bench && tab == Some("all") {
            self.rescan_bench_all();
        }
        self.set_selection(selection);
    }

    fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.set_selection(selection);
        cx.notify();
    }

    pub(crate) fn select_game(&mut self, id: &GameId, cx: &mut Context<Self>) {
        if let Some(ix) = self.games.iter().position(|g| &g.id == id) {
            self.select(Selection::Game(ix), cx);
        }
    }

    fn set_selection(&mut self, selection: Selection) {
        self.selection = Some(selection);
        self.status = None;
        self.fields.clear();
        self.game_ui = GameUi::default();
        if let Selection::Game(ix) = selection {
            if let Some(row) = self.visible.iter().position(|&v| v == ix) {
                self.list_scroll.scroll_to_item(row, ScrollStrategy::Center);
            }
            let t = self.t();
            self.profile = Profile::load(&self.games[ix].id).unwrap_or_else(|e| {
                self.status = Some(format!("{}: {e:#}", t.profile_load_failed).into());
                Profile::default()
            });
            self.tools.select(&self.games[ix]);
        }
    }

    /// Saves whatever is currently selected and reports the result in the status line.
    pub(crate) fn save(&mut self) {
        let result = match self.selection {
            Some(Selection::Global) => self.global.save(),
            Some(Selection::Game(ix)) => self.profile.save(&self.games[ix].id),
            _ => return,
        };
        self.report(result);
    }

    /// Saves the library file (game entries).
    pub(crate) fn save_library(&mut self) {
        let result = self.library.save();
        self.report(result);
    }

    pub(crate) fn report(&mut self, result: anyhow::Result<()>) {
        self.status = Some(match result {
            Ok(()) => self.t().saved.into(),
            Err(e) => format!("{}: {e:#}", self.t().save_failed).into(),
        });
    }

    /// Applies `f` to the HUD being edited: the global one, or the selected game's own.
    pub(crate) fn edit_hud(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut HudSettings)) {
        match self.selection {
            Some(Selection::Global) => f(&mut self.global.mangohud.settings),
            Some(Selection::Game(_)) => {
                let global = &self.global.mangohud.settings;
                f(self
                    .profile
                    .mangohud
                    .settings
                    .get_or_insert_with(|| global.clone()));
            }
            _ => return,
        }
        self.save();
        cx.notify();
    }

    fn set_hud_mode(&mut self, mode: HudMode, cx: &mut Context<Self>) {
        if mode == HudMode::Custom && self.profile.mangohud.settings.is_none() {
            // Start from the global layout so "Custom" is a tweak, not a blank slate.
            self.profile.mangohud.settings = Some(self.global.mangohud.settings.clone());
        }
        self.profile.mangohud.mode = mode;
        self.save();
        cx.notify();
    }

    fn search_input(&mut self, cx: &mut Context<Self>) -> gpui::Entity<text_input::TextInput> {
        if let Some((input, _)) = &self.search {
            return input.clone();
        }
        let placeholder = self.tr("Search games", "Oyun ara");
        let input = cx.new(|cx| text_input::TextInput::field("", placeholder, cx));
        let sub = cx.subscribe(&input, |this, _, event: &text_input::InputEvent, cx| {
            if let text_input::InputEvent::Changed(text) = event {
                this.search_text = text.clone();
                this.apply_filter();
                cx.notify();
            }
        });
        self.search = Some((input.clone(), sub));
        input
    }

    fn library_controls(&mut self, cx: &mut Context<Self>) -> Div {
        let search = self.search_input(cx);
        let sort_chip =
            |ix: usize, sort: Sort, label: &'static str, this: &Self, cx: &mut Context<Self>| {
                widgets::chip(("sort", ix), label, this.filter.sort == sort).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.filter.sort = sort;
                        this.apply_filter();
                        cx.notify();
                    },
                ))
            };
        let mut controls = div().px_2().flex().flex_col().gap_2().child(search).child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(sort_chip(0, Sort::Name, "A–Z", self, cx))
                .child(sort_chip(
                    1,
                    Sort::Recent,
                    self.tr("Recent", "Son"),
                    self,
                    cx,
                ))
                .child(sort_chip(
                    2,
                    Sort::Playtime,
                    self.tr("Most played", "En çok"),
                    self,
                    cx,
                )),
        );
        let categories = self.categories();
        if !categories.is_empty() {
            let mut row = div().flex().flex_wrap().gap_1().child(
                widgets::chip(
                    "cat-all",
                    self.tr("All", "Tümü"),
                    self.filter.category.is_none(),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.filter.category = None;
                    this.apply_filter();
                    cx.notify();
                })),
            );
            for (ix, c) in categories.into_iter().enumerate() {
                let active = self.filter.category.as_ref() == Some(&c);
                row = row.child(widgets::chip(("cat", ix), c.clone(), active).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.filter.category = if this.filter.category.as_ref() == Some(&c) {
                            None
                        } else {
                            Some(c.clone())
                        };
                        this.apply_filter();
                        cx.notify();
                    }),
                ));
            }
            controls = controls.child(row);
        }
        if self.filter.show_hidden || self.library.games.iter().any(|g| g.hidden) {
            controls = controls.child(
                div().flex().child(
                    widgets::chip(
                        "show-hidden",
                        self.tr("Show hidden (Ctrl+H)", "Gizlileri göster (Ctrl+H)"),
                        self.filter.show_hidden,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_hidden(cx))),
                ),
            );
        }
        controls
    }

    fn toggle_hidden(&mut self, cx: &mut Context<Self>) {
        self.filter.show_hidden = !self.filter.show_hidden;
        self.apply_filter();
        cx.notify();
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let controls = self.library_controls(cx);
        let t = self.t();
        let nav_item =
            |id: &'static str, selection: Selection, title: &'static str, hint: Option<String>| {
                let selected = self.selection == Some(selection);
                div()
                    .id(id)
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |d| d.bg(rgb(SELECTED())))
                    .when(!selected, |d| d.hover(|s| s.bg(rgb(ROW_HOVER()))))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(selection, cx)))
                    .child(div().child(title))
                    .children(hint.map(widgets::label))
            };
        let hud_state = format!(
            "MangoHud {}",
            if self.global.mangohud.enabled {
                t.on
            } else {
                t.off
            }
        );
        div()
            .flex()
            .flex_col()
            .w(px(300.))
            .h_full()
            .bg(rgb(PANEL()))
            .border_r_1()
            .border_color(rgb(BORDER()))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .text_xl()
                    .text_color(rgb(ACCENT()))
                    .child("Arkor"),
            )
            .child(
                div()
                    .px_2()
                    .flex()
                    .flex_col()
                    .child(nav_item(
                        "nav-library",
                        Selection::Grid,
                        self.tr("Library", "Kütüphane"),
                        None,
                    ))
                    .child(nav_item(
                        "nav-global",
                        Selection::Global,
                        t.global_settings,
                        Some(hud_state),
                    ))
                    .child(nav_item(
                        "nav-add",
                        Selection::Add,
                        self.tr("Add game", "Oyun ekle"),
                        None,
                    ))
                    .child(nav_item(
                        "nav-protons",
                        Selection::Protons,
                        self.tr("Proton versions", "Proton sürümleri"),
                        None,
                    ))
                    .child(nav_item(
                        "nav-bench",
                        Selection::Bench,
                        self.tr("Benchmarks", "Benchmark'lar"),
                        None,
                    ))
                    .child(nav_item(
                        "nav-system",
                        Selection::System,
                        self.tr("System", "Sistem"),
                        None,
                    )),
            )
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .pb_1()
                    .flex()
                    .justify_between()
                    .child(widgets::label(t.games_header))
                    .child(widgets::label(if self.visible.len() == self.games.len() {
                        self.games.len().to_string()
                    } else {
                        format!("{}/{}", self.visible.len(), self.games.len())
                    })),
            )
            .child(controls)
            .children(
                self.warnings
                    .first()
                    .map(|w| div().px_4().pb_2().child(widgets::label(format!("⚠ {w}")))),
            )
            .child(div().flex_1().px_2().child(self.render_list(cx)))
            .when(!self.running.is_empty(), |d| {
                d.child(
                    div().p_2().border_t_1().border_color(rgb(BORDER())).child(
                        widgets::chip(
                            "stop-all",
                            self.tr("Stop all games", "Tüm oyunları durdur"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.stop_all(cx))),
                    ),
                )
            })
    }

    /// Stops every game started from this window (process groups, then their prefixes).
    fn stop_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<(String, u32)> = self.running.iter().map(|(k, v)| (k.clone(), *v)).collect();
        for (_, pgid) in &ids {
            // SAFETY: plain syscall; a negative pid addresses the whole process group.
            unsafe { libc::kill(-(*pgid as i32), libc::SIGTERM) };
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |_, _| {
            bg.spawn(async move {
                for (id, _) in ids {
                    if let Ok(id) = id.parse() {
                        let _ = arkor_launch::start::kill_prefix(&id);
                    }
                }
            })
            .await;
        })
        .detach();
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        uniform_list(
            "games",
            self.visible.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .map(|row| {
                        let ix = this.visible[row];
                        let game = &this.games[ix];
                        let selected = this.selection == Some(Selection::Game(ix));
                        let running = this.running.contains_key(&game.id.to_string());
                        let kind = this
                            .library
                            .get(&game.id.id)
                            .map(|g| this.kind_label(g))
                            .unwrap_or_default();
                        div()
                            .id(ix)
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .cursor_pointer()
                            .when(selected, |d| d.bg(rgb(SELECTED())))
                            .when(!selected, |d| d.hover(|s| s.bg(rgb(ROW_HOVER()))))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select(Selection::Game(ix), cx)
                            }))
                            // One line only: uniform_list needs every row to be the same height.
                            .child(
                                div()
                                    .truncate()
                                    .text_color(rgb(TEXT()))
                                    .child(game.name.clone()),
                            )
                            .child(widgets::label(if running {
                                format!("▶ {}", this.tr("Running", "Çalışıyor"))
                            } else {
                                match this.playtime(&game.id).0 {
                                    0 => kind.to_string(),
                                    secs => format!(
                                        "{kind} · {}",
                                        arkor_core::stats::format_duration(secs)
                                    ),
                                }
                            }))
                    })
                    .collect()
            }),
        )
        .track_scroll(self.list_scroll.clone())
        .h_full()
    }

    pub(crate) fn kind_label(&self, game: &LibraryGame) -> &'static str {
        use arkor_core::library::GameKind;
        match game.kind {
            GameKind::Windows => self.tr("Windows game", "Windows oyunu"),
            GameKind::Linux => self.tr("Linux game", "Linux oyunu"),
            GameKind::Steam { .. } => "Steam",
            GameKind::Epic { .. } => "Epic Games",
        }
    }

    fn render_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let content = match self.selection {
            None if !self.games.is_empty() => self.render_grid(cx),
            None => {
                return div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(MUTED()))
                    .child(if self.games.is_empty() {
                        self.tr(
                            "Your library is empty. Add a game to get started.",
                            "Kütüphanen boş. Başlamak için bir oyun ekle.",
                        )
                    } else {
                        self.t().pick_something
                    })
                    .child(
                        widgets::primary_button("empty-add", self.tr("Add game", "Oyun ekle"))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.select(Selection::Add, cx)),
                            ),
                    )
                    .into_any_element();
            }
            Some(Selection::Global) => self.render_global(window, cx),
            Some(Selection::Add) => self.render_add(window, cx),
            Some(Selection::Protons) => self.render_protons(cx),
            Some(Selection::Grid) => self.render_grid(cx),
            Some(Selection::Bench) => self.render_bench(cx),
            Some(Selection::System) => self.render_system(cx),
            Some(Selection::Game(ix)) => {
                let game = self.games[ix].clone();
                self.render_game(&game, window, cx)
            }
        };
        div()
            .id("detail")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .child(
                content
                    // Never shrink to the viewport: the scroll container needs the real height.
                    .flex_none()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .max_w(px(900.))
                    .children(self.status.clone().map(widgets::label)),
            )
            .into_any_element()
    }

    pub(crate) fn header(title: impl Into<SharedString>, subtitle: impl Into<SharedString>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_2xl().text_color(rgb(TEXT())).child(title.into()))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(MUTED()))
                    .child(subtitle.into()),
            )
    }
}

impl Render for Library {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_theme(window);
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        if window.focused(cx).is_none() {
            window.focus(&focus);
        }
        use big_screen::*;
        if self.big_screen {
            let big = self.render_big(window, cx);
            return div()
                .track_focus(&focus)
                .key_context("BigScreen")
                .on_action(
                    cx.listener(|this, _: &ToggleBigScreen, w, cx| this.toggle_big_screen(w, cx)),
                )
                .on_action(cx.listener(|this, _: &NavUp, w, cx| this.nav(Nav::Up, w, cx)))
                .on_action(cx.listener(|this, _: &NavDown, w, cx| this.nav(Nav::Down, w, cx)))
                .on_action(cx.listener(|this, _: &NavLeft, w, cx| this.nav(Nav::Left, w, cx)))
                .on_action(cx.listener(|this, _: &NavRight, w, cx| this.nav(Nav::Right, w, cx)))
                .on_action(cx.listener(|this, _: &NavAccept, w, cx| this.nav(Nav::Accept, w, cx)))
                .on_action(cx.listener(|this, _: &NavBack, w, cx| this.nav(Nav::Back, w, cx)))
                .on_action(cx.listener(|this, _: &NavDetails, w, cx| this.nav(Nav::Details, w, cx)))
                .size_full()
                .text_color(rgb(TEXT()))
                .child(big);
        }
        let detail = self.render_detail(window, cx);
        let sidebar = self.render_sidebar(cx);
        div()
            .track_focus(&focus)
            .key_context("Arkor")
            .on_action(cx.listener(|this, _: &ToggleHidden, _, cx| this.toggle_hidden(cx)))
            .on_action(
                cx.listener(|this, _: &ToggleBigScreen, w, cx| this.toggle_big_screen(w, cx)),
            )
            .flex()
            .size_full()
            .bg(rgb(BG()))
            .text_color(rgb(TEXT()))
            .child(sidebar)
            .child(div().flex_1().h_full().child(detail))
    }
}

/// `open`: `"global"`, `"add"`, `"protons"` or a game id (`local:far-cry-6#mods`) to show
/// that page. `start_in_tray`: only the tray icon (autostart), no window.
pub fn run(open: Option<String>, start_in_tray: bool) -> anyhow::Result<()> {
    // GPUI renders with Vulkan, so a global MANGOHUD=1 would draw the overlay on Arkor itself.
    // SAFETY: called before any threads are spawned. Games started from the UI get it
    // removed again (see `game_page::spawn_launch`).
    unsafe { std::env::set_var("DISABLE_MANGOHUD", "1") };
    let global = Global::load_or_import().unwrap_or_default();
    Application::new().run(move |cx: &mut App| {
        text_input::bind_keys(cx);
        cx.bind_keys([gpui::KeyBinding::new("ctrl-h", ToggleHidden, None)]);
        big_screen::bind_keys(cx);
        let main: std::rc::Rc<std::cell::RefCell<Option<gpui::WindowHandle<Library>>>> =
            Default::default();

        let tray_rx = (global.tray || start_in_tray)
            .then(|| {
                let games = GameLibrary::load()
                    .map(|l| {
                        l.games
                            .iter()
                            .map(|g| (g.game_id().to_string(), g.name.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                let labels = match global.language {
                    Language::En => ["Open Arkor", "Play", "Quit"],
                    Language::Tr => ["Arkor'u aç", "Oyna", "Çık"],
                };
                tray::start(games, labels)
            })
            .flatten();
        let has_tray = tray_rx.is_some();

        // Automatic backup, off the UI thread.
        let now = arkor_core::stats::now();
        if global.backup.due(now) {
            let backup = global.backup.clone();
            cx.background_executor()
                .spawn(async move {
                    let dest = backup.dest_dir();
                    if arkor_core::backup::create(&dest, &[], "auto").is_ok() {
                        arkor_core::backup::prune(&dest, backup.keep);
                        if let Ok(mut g) = Global::load_or_import() {
                            g.backup.last = Some(now);
                            let _ = g.save();
                        }
                    }
                })
                .detach();
        }

        if !(start_in_tray && has_tray) {
            *main.borrow_mut() = open_main(open.clone(), cx);
        }
        // Without a tray, closing the window ends Arkor; with one, Arkor stays in the tray.
        cx.on_window_closed(move |cx| {
            if !has_tray && cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        // Gamepad: navigation in big-screen mode; Start opens Arkor if its window is closed.
        if let Some(rx) = big_screen::start_gamepad() {
            let executor = cx.background_executor().clone();
            let main = main.clone();
            cx.spawn(async move |cx| {
                loop {
                    executor.timer(std::time::Duration::from_millis(30)).await;
                    while let Ok(nav) = rx.try_recv() {
                        let _ = cx.update(|cx| {
                            let handle = *main.borrow();
                            let alive = handle.is_some_and(|h| {
                                h.update(cx, |lib, window, cx| lib.nav(nav, window, cx))
                                    .is_ok()
                            });
                            if !alive && nav == big_screen::Nav::Toggle {
                                *main.borrow_mut() = open_main(None, cx);
                            }
                        });
                    }
                }
            })
            .detach();
        }

        if let Some(rx) = tray_rx {
            let main = main.clone();
            let executor = cx.background_executor().clone();
            cx.spawn(async move |cx| {
                loop {
                    executor.timer(std::time::Duration::from_millis(250)).await;
                    while let Ok(cmd) = rx.try_recv() {
                        let _ = cx.update(|cx| match cmd {
                            tray::TrayCmd::Show => match cx.windows().into_iter().next() {
                                Some(w) => {
                                    let _ = w.update(cx, |_, window, _| window.activate_window());
                                }
                                None => *main.borrow_mut() = open_main(None, cx),
                            },
                            tray::TrayCmd::Quit => cx.quit(),
                        });
                    }
                }
            })
            .detach();
        }
    });
    Ok(())
}

fn open_main(open: Option<String>, cx: &mut App) -> Option<gpui::WindowHandle<Library>> {
    let bounds = Bounds::centered(None, size(px(1200.), px(820.)), cx);
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            app_id: Some("arkor".into()),
            ..Default::default()
        },
        move |_, cx| {
            cx.new(|_| {
                let mut lib = Library::new();
                lib.open(open.as_deref());
                lib
            })
        },
    );
    match opened {
        Ok(handle) => {
            cx.activate(true);
            Some(handle)
        }
        Err(e) => {
            eprintln!("failed to open window: {e:#}");
            cx.quit();
            None
        }
    }
}
