//! Library window: "Global" + installed games on the left, settings for the selection on the
//! right. Every change is saved immediately; `arkor run` picks it up on the next launch.

mod game_page;
mod hud_editor;
mod i18n;
mod import;
mod widgets;

use arkor_core::mangohud::HudSettings;
use arkor_core::{Game, GameId, Global, HudMode, Language, Profile, Store};
use gpui::{
    AnyElement, App, Application, Bounds, Context, Div, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
    uniform_list,
};

use i18n::Strings;
use import::ImportState;
use widgets::theme::*;

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    Global,
    Import,
    Game(usize),
}

pub struct Library {
    games: Vec<Game>,
    list_scroll: UniformListScrollHandle,
    /// Problems not tied to a source (e.g. unreadable settings).
    warnings: Vec<String>,
    /// Sources that could not be read on the last scan.
    source_errors: Vec<String>,
    selection: Option<Selection>,
    /// Profile of the selected game (only meaningful when a game is selected).
    profile: Profile,
    global: Global,
    import: ImportState,
    status: Option<SharedString>,
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
            list_scroll: UniformListScrollHandle::new(),
            warnings,
            source_errors: Vec::new(),
            selection: None,
            profile: Profile::default(),
            global,
            import: ImportState::default(),
            status: None,
        };
        lib.reload_games();
        lib
    }

    /// Re-scans every source, keeping the selected game selected if it still exists.
    fn reload_games(&mut self) {
        let selected = match self.selection {
            Some(Selection::Game(ix)) => self.games.get(ix).map(|g| g.id.clone()),
            _ => None,
        };
        let (games, errors) = arkor_stores::all_games();
        self.games = games;
        self.source_errors = errors.iter().map(|e| format!("{e:#}")).collect();
        if let Some(id) = selected {
            match self.games.iter().position(|g| g.id == id) {
                Some(ix) => self.selection = Some(Selection::Game(ix)),
                None => self.selection = None,
            }
        }
    }

    /// Strings in the user's chosen language.
    pub(crate) fn t(&self) -> &'static Strings {
        i18n::strings(self.global.language)
    }

    pub(crate) fn store_name(&self, store: Store) -> &'static str {
        match store {
            Store::Steam => "Steam",
            Store::Epic => "Epic Games",
            Store::Gog => "GOG",
            Store::Heroic => "Heroic",
            Store::Faugus => "Faugus",
            Store::Local => self.t().store_local,
        }
    }

    fn open(&mut self, target: Option<&str>) {
        let selection = match target {
            None => return,
            Some("global") => Selection::Global,
            Some("import") => Selection::Import,
            Some(id) => match self.games.iter().position(|g| g.id.to_string() == id) {
                Some(ix) => Selection::Game(ix),
                None => {
                    self.warnings
                        .insert(0, format!("`{id}` {}", self.t().not_found));
                    return;
                }
            },
        };
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
        if let Selection::Game(ix) = selection {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Center);
            let t = self.t();
            self.profile = Profile::load(&self.games[ix].id).unwrap_or_else(|e| {
                self.status = Some(format!("{}: {e:#}", t.profile_load_failed).into());
                Profile::default()
            });
        }
    }

    /// Saves whatever is currently selected and reports the result in the status line.
    fn save(&mut self) {
        let result = match self.selection {
            Some(Selection::Global) => self.global.save(),
            Some(Selection::Game(ix)) => self.profile.save(&self.games[ix].id),
            Some(Selection::Import) | None => return,
        };
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
            Some(Selection::Import) | None => return,
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

    fn set_language(&mut self, language: Language, cx: &mut Context<Self>) {
        self.global.language = language;
        self.save();
        cx.notify();
    }

    fn toggle_global_hud(&mut self, cx: &mut Context<Self>) {
        self.global.mangohud.enabled = !self.global.mangohud.enabled;
        self.save();
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
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
                    .when(selected, |d| d.bg(rgb(SELECTED)))
                    .when(!selected, |d| d.hover(|s| s.bg(rgb(ROW_HOVER))))
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
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .text_xl()
                    .text_color(rgb(ACCENT))
                    .child("Arkor"),
            )
            .child(
                div().px_2().child(
                    nav_item(
                        "global",
                        Selection::Global,
                        t.global_settings,
                        Some(hud_state),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.select(Selection::Global, cx))),
                ),
            )
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .pb_1()
                    .flex()
                    .justify_between()
                    .child(widgets::label(t.games_header))
                    .child(widgets::label(self.games.len().to_string())),
            )
            .children(
                self.warnings
                    .iter()
                    .chain(&self.source_errors)
                    .next()
                    .map(|w| div().px_4().pb_2().child(widgets::label(format!("⚠ {w}")))),
            )
            .child(div().flex_1().px_2().child(self.render_list(cx)))
            .child(
                div().p_2().border_t_1().border_color(rgb(BORDER)).child(
                    nav_item("import", Selection::Import, t.import_prefix, None)
                        .on_click(cx.listener(|this, _, _, cx| this.select(Selection::Import, cx))),
                ),
            )
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        uniform_list(
            "games",
            self.games.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .map(|ix| {
                        let game = &this.games[ix];
                        let selected = this.selection == Some(Selection::Game(ix));
                        div()
                            .id(ix)
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .cursor_pointer()
                            .when(selected, |d| d.bg(rgb(SELECTED)))
                            .when(!selected, |d| d.hover(|s| s.bg(rgb(ROW_HOVER))))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select(Selection::Game(ix), cx)
                            }))
                            // One line only: uniform_list needs every row to be the same height.
                            .child(
                                div()
                                    .truncate()
                                    .text_color(rgb(TEXT))
                                    .child(game.name.clone()),
                            )
                            .child(widgets::label(this.store_name(game.id.store)))
                    })
                    .collect()
            }),
        )
        .track_scroll(self.list_scroll.clone())
        .h_full()
    }

    fn render_detail(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = match self.selection {
            None => {
                return div()
                    .flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(MUTED))
                    .child(self.t().pick_something)
                    .into_any_element();
            }
            Some(Selection::Global) => self.render_global(cx),
            Some(Selection::Import) => self.render_import(cx),
            Some(Selection::Game(ix)) => self.render_game(&self.games[ix], cx),
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
            .child(div().text_2xl().text_color(rgb(TEXT)).child(title.into()))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child(subtitle.into()),
            )
    }

    fn render_global(&self, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        let hud = self.global.mangohud.settings.clone();
        let mut languages = div().flex().gap_2();
        for (ix, (lang, name)) in i18n::LANGUAGES.into_iter().enumerate() {
            languages = languages.child(
                widgets::chip(("lang", ix), name, self.global.language == lang)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_language(lang, cx))),
            );
        }
        div()
            .child(Self::header(t.global_settings, t.global_subtitle))
            .child(widgets::section(t.language).child(languages))
            .child(
                widgets::switch_row(
                    "global-hud",
                    "MangoHud",
                    t.hud_default_hint,
                    self.global.mangohud.enabled,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_global_hud(cx))),
            )
            .child(self.render_hud_editor(&hud, cx))
    }
}

impl Render for Library {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(self.render_sidebar(cx))
            .child(div().flex_1().h_full().child(self.render_detail(cx)))
    }
}

/// `open`: `"global"`, `"import"` or a game id (`steam:730`) to show that page on start.
pub fn run(open: Option<String>) -> anyhow::Result<()> {
    // GPUI renders with Vulkan, so a global MANGOHUD=1 would draw the overlay on Arkor itself.
    // SAFETY: called before any threads are spawned. Games started from the UI get it
    // removed again (see `game_page::spawn_launch`).
    unsafe { std::env::set_var("DISABLE_MANGOHUD", "1") };
    Application::new().run(|cx: &mut App| {
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
        if let Err(e) = opened {
            eprintln!("failed to open window: {e:#}");
            cx.quit();
        }
        cx.activate(true);
    });
    Ok(())
}
