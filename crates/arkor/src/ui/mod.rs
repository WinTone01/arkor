//! Arkor's window: navigation and the game library on the left, the selected page on the
//! right. Every change is saved immediately; `arkor run` picks it up on the next launch.

mod add_game;
mod app_icon;
mod app_update;
mod backdrop;
mod bench_page;
mod clips;
mod console;
mod console_idle;
mod console_intro;
mod console_library;
mod console_pads;
mod console_quick;
mod console_settings;
mod console_sound;
mod console_steam;
mod console_system;
mod console_tools;
mod console_view;
mod cover_art;
mod defaults_page;
mod desk;
mod downloads_page;
mod downloads_queue;
mod enhance;
mod features;
mod fields;
mod game_page;
mod gamepad;
mod global_page;
mod grid;
mod hud_editor;
mod i18n;
mod icons;
mod intro;
mod kwin;
mod loader;
mod mods_page;
mod protons;
mod save_backups;
mod saves;
mod sgdb_picker;
mod shell;
mod shops;
mod shops_page;
mod sounds;
mod splash;
mod steam_card;
mod style;
mod synth;
mod system_page;
mod text_input;
mod tray;
mod trouble;
mod widgets;

#[cfg(test)]
mod ui_tests;

/// Tests that change process-wide XDG variables hold this, so they run one at a time.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

use std::collections::HashMap;

use arkor_core::library::{Library as GameLibrary, LibraryGame};
use arkor_core::mangohud::HudSettings;
use arkor_core::{Game, GameId, Global, HudMode, Language, Profile};
use gpui::{
    AnyElement, App, Bounds, Context, Div, SharedString, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, size,
};

use add_game::AddState;
use enhance::GameTools;
use features::GameTab;
use fields::Fields;
use game_page::GameUi;
use i18n::Strings;
use protons::ProtonsState;
use widgets::theme::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Selection {
    Grid,
    Bench,
    Downloads,
    Global,
    System,
    Add,
    Protons,
    Shops,
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
    /// Open tab of the MangoHud editor.
    hud_tab: hud_editor::HudTab,
    /// Global settings page shows its MangoHud tab.
    global_hud_tab: bool,
    fields: Fields,
    system: system_page::SystemState,
    bench: bench_page::BenchState,
    /// Games started from this window: game id -> process group id.
    running: HashMap<String, u32>,
    /// Games being stopped from here (their end is no failure).
    stopping: std::collections::HashSet<String>,
    /// Window width at the last render (the library grid fits its columns to it).
    viewport_w: f32,
    /// The start-up animation, while it plays (see `intro`).
    intro: Option<intro::Intro>,
    /// The redraw timer for moving covers and backgrounds is running.
    ticking: bool,
    /// Seconds the background has been moving, and when that was counted.
    motion: (f32, Option<std::time::Instant>),
    status: Option<SharedString>,
    stats: arkor_core::stats::Stats,
    filter: ListFilter,
    /// Rows of the sidebar list: indices into `games` after search/category/hidden/sort.
    visible: Vec<usize>,
    search: Option<(gpui::Entity<text_input::TextInput>, gpui::Subscription)>,
    search_text: String,
    focus: Option<gpui::FocusHandle>,
    backup_prefixes: bool,
    /// Console mode (see `console`).
    console: console::Console,
    restore_pending: Option<std::path::PathBuf>,
    /// umu-launcher, anti-cheat and a first Proton, installed in the background at start.
    required: protons::RequiredState,
    settings_tab: global_page::SettingsTab,
    downloads: downloads_page::DownloadsState,
    /// Steam's settings for the selected Steam game.
    steam: steam_card::SteamState,
    /// The selected game's saves and the cloud's.
    saves: saves::SavesUi,
    mods: mods_page::ModsUi,
    /// Why a game did not start.
    trouble: trouble::TroubleUi,
    /// Store accounts and their games.
    shops: shops::ShopsState,
    /// The window was in use when it was last drawn.
    window_active: bool,
    /// The SteamGridDB cover picker on the game page, when open.
    sgdb: Option<sgdb_picker::Picker>,
    sgdb_on_open: bool,
    app_update: app_update::AppUpdate,
    /// The redraw loop for download progress is running.
    downloads_watch: bool,
    /// Move the keyboard focus back to the window on the next render.
    refocus: bool,
    accent_picker: Option<(
        gpui::Entity<gpui::component::color_picker::ColorPickerState>,
        gpui::Subscription,
    )>,
    /// Text in the game page's settings search.
    feature_query: String,
    /// Appearance last pushed to gpui-kit's theme (re-synced when it changes).
    synced_look: Option<(arkor_core::profile::Appearance, bool)>,
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
            warnings,
            selection: None,
            profile: Profile::default(),
            global,
            add: AddState::default(),
            tools: GameTools::scan(),
            protons: ProtonsState::default(),
            game_ui: GameUi::default(),
            game_tab: GameTab::default(),
            hud_tab: Default::default(),
            global_hud_tab: false,
            fields: Fields::default(),
            system: Default::default(),
            bench: Default::default(),
            running: HashMap::new(),
            stopping: Default::default(),
            viewport_w: 1320.,
            intro: None,
            ticking: false,
            motion: (0., None),
            status: None,
            stats: arkor_core::stats::Stats::load(),
            filter: ListFilter::default(),
            visible: Vec::new(),
            search: None,
            search_text: String::new(),
            focus: None,
            covers: HashMap::new(),
            synced_look: None,
            feature_query: String::new(),
            settings_tab: Default::default(),
            downloads: Default::default(),
            steam: Default::default(),
            saves: Default::default(),
            mods: Default::default(),
            trouble: Default::default(),
            shops: shops::ShopsState::load(),
            window_active: true,
            sgdb: None,
            sgdb_on_open: false,
            app_update: Default::default(),
            downloads_watch: false,
            refocus: false,
            accent_picker: None,
            backup_prefixes: false,
            console: Default::default(),
            restore_pending: None,
            required: Default::default(),
        };
        lib.reload_games();
        lib
    }

    /// Redraws about 60 times a second while the background or a cover moves and the window
    /// is in use. (Redrawing with every refresh of a fast screen would cost three times as
    /// much for no visible gain.)
    fn keep_moving(&mut self, window: &Window, cx: &mut Context<Self>) {
        let look = &self.global.appearance;
        let moving = window.is_window_active()
            && !look.reduce_motion
            && (self.console.moving()
                || cover_art::any_playing()
                || (look.backdrop.animated() && !self.console.on));
        let now = std::time::Instant::now();
        if moving && let Some(last) = self.motion.1 {
            // After a pause it goes on where it was.
            self.motion.0 += (now - last).as_secs_f32().min(0.05);
        }
        self.motion.1 = Some(now);
        if !moving || self.ticking {
            return;
        }
        self.ticking = true;
        // The console's focus and lists follow the hand: those are drawn with every refresh.
        let every = if self.console.moving() { 0 } else { 13 };
        Self::tick(cx.entity().downgrade(), now, every, window);
    }

    /// Waits for the screen refresh at which the next of those redraws is due.
    fn tick(this: gpui::WeakEntity<Self>, drawn: std::time::Instant, every: u64, window: &Window) {
        window.on_next_frame(move |window, cx| {
            if drawn.elapsed() < std::time::Duration::from_millis(every) {
                return Self::tick(this, drawn, every, window);
            }
            let _ = this.update(cx, |this, cx| {
                this.ticking = false;
                cx.notify();
            });
        });
    }

    /// Reads what is installed again; the open game page keeps its own state.
    pub(crate) fn rescan_tools(&mut self) {
        self.tools = GameTools::scan();
        if let Some(Selection::Game(ix)) = self.selection
            && let Some(game) = self.games.get(ix)
        {
            self.tools.select(game);
            self.find_game_folders();
        }
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
        if let Some(t) = tab.and_then(hud_editor::HudTab::from_name) {
            self.hud_tab = t;
        }
        self.global_hud_tab = matches!(tab, Some("advanced" | "mangohud"));
        self.game_tab = tab.and_then(GameTab::from_name).unwrap_or_default();
        // `#cover` opens the SteamGridDB cover picker too.
        self.sgdb_on_open = tab == Some("cover");
        // `#search=dlss` opens the game page with its settings search filled in.
        if let Some(q) = tab.and_then(|t| t.strip_prefix("search=")) {
            self.feature_query = q.replace('+', " ");
        }
        let selection = match target {
            None => return,
            Some("global") | Some("settings") => {
                use global_page::SettingsTab;
                self.settings_tab = match tab {
                    Some("appearance") => SettingsTab::Appearance,
                    Some("defaults") => SettingsTab::Defaults,
                    Some("integrations") => SettingsTab::Integrations,
                    Some("backups") => SettingsTab::Backups,
                    Some("shortcuts" | "keys") => SettingsTab::Shortcuts,
                    _ => SettingsTab::General,
                };
                Selection::Global
            }
            Some("overlay") => {
                self.global_hud_tab = true;
                Selection::Global
            }
            Some("add") | Some("import") => {
                self.add.tab = match (target, tab) {
                    (Some("import"), _) | (_, Some("prefix")) => add_game::AddTab::Prefix,
                    (_, Some("manual")) => add_game::AddTab::Manual,
                    (_, Some("launcher")) => add_game::AddTab::Launcher,
                    _ => add_game::AddTab::Found,
                };
                Selection::Add
            }
            Some("protons") => Selection::Protons,
            Some("system") => Selection::System,
            Some("stores") => {
                use arkor_launch::shops::Shop;
                match tab.and_then(|t| t.split_once('/')) {
                    // `stores#gog/1207658924` opens a game, `stores#login/gog` the login.
                    Some((shop, id)) => self.shops.on_open = Some((shop.into(), id.into())),
                    None => self.shops.tab = tab.and_then(Shop::from_key),
                }
                Selection::Shops
            }
            Some("library") => Selection::Grid,
            Some("benchmarks") => Selection::Bench,
            Some("downloads") => {
                if tab == Some("tools") {
                    self.downloads_show_tools();
                }
                Selection::Downloads
            }
            Some("console" | "big") => {
                self.console_start(tab);
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
        // A settings search belongs to the page it was typed on.
        self.feature_query.clear();
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
        self.refocus = true;
        self.status = None;
        self.fields.clear();
        self.game_ui = GameUi::default();
        if let Selection::Game(ix) = selection {
            let t = self.t();
            self.profile = Profile::load(&self.games[ix].id).unwrap_or_else(|e| {
                self.status = Some(format!("{}: {e:#}", t.profile_load_failed).into());
                Profile::default()
            });
            self.tools.select(&self.games[ix]);
            self.steam_refresh(true);
            self.saves_load();
            self.sgdb = None;
            self.find_game_folders();
        }
    }

    /// Saves whatever is currently selected and reports the result in the status line.
    pub(crate) fn save(&mut self) {
        let result = match self.selection {
            Some(Selection::Global) => self.global.save(),
            Some(Selection::Game(ix)) => self.profile.save(&self.games[ix].id),
            _ => return,
        };
        // Something switched on that is not installed yet (a layer, a "Latest" Proton):
        // fetched right away in the download queue.
        if result.is_ok() && matches!(self.selection, Some(Selection::Game(_))) {
            for (key, _) in arkor_launch::downloads::needs(&self.profile) {
                arkor_launch::downloads::enqueue(&key);
            }
            // Steam games: Steam gets the new settings (now, when it is closed).
            self.steam_after_save();
        }
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

    fn toggle_hidden(&mut self, cx: &mut Context<Self>) {
        self.filter.show_hidden = !self.filter.show_hidden;
        self.apply_filter();
        cx.notify();
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

    pub(crate) fn kind_label(&self, game: &LibraryGame) -> &'static str {
        use arkor_core::library::GameKind;
        match game.kind {
            GameKind::Windows => self.tr("Windows game", "Windows oyunu"),
            GameKind::Linux => self.tr("Linux game", "Linux oyunu"),
            GameKind::Steam { .. } => "Steam",
            GameKind::Epic { .. } => "Epic Games",
            GameKind::Amazon { .. } => "Amazon Games",
        }
    }

    fn render_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let content = match self.selection {
            None => self.render_grid(cx),
            Some(Selection::Global) => self.render_global(window, cx),
            Some(Selection::Add) => self.render_add(window, cx),
            Some(Selection::Protons) => self.render_protons(cx),
            Some(Selection::Grid) => self.render_grid(cx),
            Some(Selection::Bench) => self.render_bench(cx),
            Some(Selection::Downloads) => self.render_downloads(cx),
            Some(Selection::System) => self.render_system(cx),
            // Its list of games scrolls by itself.
            Some(Selection::Shops) => return self.render_shops(window, cx),
            Some(Selection::Game(ix)) => {
                let _ = ix;
                self.render_game(window, cx)
            }
        };
        // The library grid uses the full width; other pages keep readable line lengths.
        let wide = matches!(self.selection, None | Some(Selection::Grid));
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
                    .w_full()
                    // Wide windows: a centred column that grows with the window, up to a
                    // comfortable reading width.
                    .when(!wide, |d| d.max_w(px(1280.)).mx_auto()),
            )
            .into_any_element()
    }

    /// The loading animation with what is happening.
    pub(crate) fn loading(&self, text: &'static str) -> Div {
        loader::block(text, None, self.global.appearance.reduce_motion)
    }

    /// A small loading animation in a line.
    pub(crate) fn loading_inline(&self, text: &'static str) -> Div {
        loader::inline(text, self.global.appearance.reduce_motion)
    }

    pub(crate) fn header(title: impl Into<SharedString>, subtitle: impl Into<SharedString>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT())
                    .child(title.into()),
            )
            // A short fact ("12 games") stays; explanations of the page do not.
            .children({
                let subtitle: SharedString = subtitle.into();
                (!subtitle.is_empty() && !widgets::is_explanation(&subtitle))
                    .then(|| div().text_sm().text_color(MUTED()).child(subtitle))
            })
    }
}

/// `open`: `"global"`, `"add"`, `"protons"` or a game id (`local:far-cry-6#mods`) to show
/// that page. `start_in_tray`: only the tray icon (autostart), no window.
pub fn run(open: Option<String>, start_in_tray: bool) -> anyhow::Result<()> {
    // GPUI renders with Vulkan, so a global MANGOHUD=1 would draw the overlay on Arkor itself.
    // SAFETY: called before any threads are spawned. Games started from the UI get it
    // removed again (see `game_page::spawn_launch`).
    unsafe { std::env::set_var("DISABLE_MANGOHUD", "1") };
    // Logins an older Arkor kept as plain files.
    std::thread::spawn(arkor_launch::shops::vault::settle_all);
    let global = Global::load_or_import().unwrap_or_default();
    // Tells `arkor nxm` that this Arkor takes Nexus links (held until it quits).
    let _window_lock = arkor_launch::mods::nexus::hold_window_lock();
    gpui::application()
        .with_assets(icons::ArkorAssets)
        .run(move |cx: &mut App| {
            gpui::init(cx);
            cx.set_cursor_hide_mode(gpui::CursorHideMode::OnTyping);
            style::load_fonts(cx);
            app_icon::install();
            sync_kit_theme(&global.appearance, cx);
            text_input::bind_keys(cx);
            cx.bind_keys([gpui::KeyBinding::new("ctrl-h", ToggleHidden, None)]);
            console::bind_keys(cx);
            shell::bind_keys(cx);
            let main: std::rc::Rc<std::cell::RefCell<Option<MainWindow>>> = Default::default();

            // The tray icon is always there; the setting decides whether closing the window
            // keeps Arkor running in it.
            let tray_rx = true
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
            // Read when the window closes: the setting may have changed since Arkor started.
            let keep_in_tray = move || {
                has_tray && (start_in_tray || Global::load_or_import().is_ok_and(|g| g.tray))
            };

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
                // Asked for, or chosen in the settings.
                let open = match open.clone() {
                    None if global.start_in_console => Some("console".into()),
                    open => open,
                };
                *main.borrow_mut() = open_main(open, cx);
            }
            // Closing the window ends Arkor unless it is set to stay in the tray.
            cx.on_window_closed(move |cx, _| {
                if cx.windows().is_empty() && !keep_in_tray() {
                    cx.quit();
                }
            })
            .detach();

            // Gamepad: console mode; its middle button opens Arkor when its window is closed.
            if let Some(rx) = gamepad::start() {
                let main = main.clone();
                cx.spawn(async move |cx| {
                    while let Ok(nav) = rx.recv().await {
                        cx.update(|cx| {
                            let handle = main.borrow().clone();
                            let alive = handle.is_some_and(|(h, lib)| {
                                h.update(cx, |_, window, cx| {
                                    lib.update(cx, |lib, cx| lib.pad(nav, window, cx))
                                })
                                .is_ok()
                            });
                            if !alive && nav == gamepad::Nav::Toggle {
                                *main.borrow_mut() = open_main(None, cx);
                            }
                        });
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
                        // A Nexus link while only the tray icon is there: the window takes it.
                        if arkor_launch::mods::nexus::has_handed() {
                            cx.update(|cx| {
                                if cx.windows().is_empty() {
                                    *main.borrow_mut() = open_main(None, cx);
                                }
                            });
                        }
                        while let Ok(cmd) = rx.try_recv() {
                            cx.update(|cx| match cmd {
                                tray::TrayCmd::Show => match cx.windows().into_iter().next() {
                                    Some(w) => {
                                        let _ =
                                            w.update(cx, |_, window, _| window.activate_window());
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

/// The main window and its view.
type MainWindow = (gpui::AnyWindowHandle, gpui::Entity<Library>);

/// Applies Arkor's palette to gpui-kit's own components (inputs, menus, sliders).
pub(crate) fn sync_kit_theme(look: &arkor_core::profile::Appearance, cx: &mut App) {
    use gpui::component::Theme;
    use gpui::component::theme::ThemeMode;
    use style::pal;
    let light = match look.theme {
        arkor_core::profile::Theme::Light => true,
        arkor_core::profile::Theme::Dark => false,
        arkor_core::profile::Theme::System => matches!(
            cx.window_appearance(),
            gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
        ),
    };
    pal::set(
        light,
        look.accent.unwrap_or(pal::DEFAULT_ACCENT),
        look.glass,
    );
    Theme::change(
        if light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        },
        None,
        cx,
    );
    let accent = pal::accent();
    let size = look.font_size() as f32;
    Theme::update(cx, |theme| {
        theme.background = pal::bg();
        theme.foreground = pal::text();
        theme.border = pal::border();
        theme.muted = pal::raised();
        theme.muted_foreground = pal::muted();
        theme.secondary = pal::raised();
        theme.secondary_hover = style::shade(pal::raised(), if light { -0.03 } else { 0.03 });
        theme.secondary_foreground = pal::text();
        theme.accent = pal::raised();
        theme.accent_foreground = pal::text();
        theme.input = pal::border_strong();
        theme.popover = pal::card();
        theme.popover_foreground = pal::text();
        theme.primary = accent;
        theme.primary_hover = style::shade(accent, 0.06);
        theme.primary_active = style::shade(accent, -0.06);
        theme.primary_foreground = pal::on_accent();
        theme.ring = accent;
        theme.info = pal::accent_light();
        theme.tokens.primary = accent.into();
        theme.font_size = px(size);
        theme.font_family = style::FONT.into();
    });
}

fn open_main(open: Option<String>, cx: &mut App) -> Option<MainWindow> {
    // `ARKOR_WINDOW_SIZE=2400x1350` opens the window at that size (for checking layouts).
    let (w, h) = std::env::var("ARKOR_WINDOW_SIZE")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))
        })
        .unwrap_or((1320., 860.));
    let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
    let console = open
        .as_deref()
        .is_some_and(|o| o.starts_with("console") || o == "big");
    let full = console && std::env::var_os("ARKOR_WINDOW_SIZE").is_none();
    let opened = gpui::open_window(
        WindowOptions {
            window_bounds: Some(if full {
                WindowBounds::Fullscreen(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            }),
            window_min_size: Some(size(px(1000.), px(640.))),
            app_id: Some("arkor".into()),
            ..Default::default()
        },
        cx,
        move |_, cx| {
            cx.new(|cx| {
                let mut lib = Library::new();
                lib.open(open.as_deref());
                lib.console.opened_full = full;
                // Only the real window installs components and runs the download queue
                // (not the headless tests).
                if !lib.console.on {
                    lib.start_intro();
                }
                // The package version of lsfg-vk takes a moment to look up.
                let bg = cx.background_executor().clone();
                cx.spawn(async move |this, cx| {
                    let info = bg
                        .spawn(async move { arkor_launch::tools::lsfg_layer() })
                        .await;
                    let _ = this.update(cx, |this: &mut Library, _| this.tools.lsfg_info = info);
                })
                .detach();
                arkor_launch::downloads::start();
                // Mods a launch left in a game folder (the computer went off while it ran).
                if !cfg!(test) {
                    cx.background_executor()
                        .spawn(async { arkor_launch::mods::play::leftovers() })
                        .detach();
                }
                if lib.console.on {
                    lib.console_first(cx);
                    lib.start_console_intro();
                }
                lib.watch_progress(cx);
                lib.watch_nxm(cx);
                lib.startup_updates(cx);
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
