//! Library window: "Global" + game list on the left, settings for the selection on the right.
//! Every change is saved immediately; `arkor run` picks it up on the next launch.

mod hud_editor;
mod i18n;
mod widgets;

use arkor_core::mangohud::HudSettings;
use arkor_core::{Game, Global, HudMode, Language, Profile};
use gpui::{
    AnyElement, App, Application, Bounds, Context, Div, SharedString, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size, uniform_list,
};

use i18n::Strings;
use widgets::theme::*;

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    Global,
    Game(usize),
}

pub struct Library {
    games: Vec<Game>,
    warnings: Vec<String>,
    selection: Option<Selection>,
    /// Profile of the selected game (only meaningful when a game is selected).
    profile: Profile,
    global: Global,
    status: Option<SharedString>,
}

#[derive(Clone, Copy)]
enum Toggle {
    GameMode,
    Gamescope,
}

impl Library {
    fn new() -> Self {
        let (games, errors) = arkor_stores::steam::installed_games();
        let mut warnings: Vec<String> = errors.iter().map(|e| format!("{e:#}")).collect();
        let global = Global::load_or_import().unwrap_or_else(|e| {
            // The language setting itself could not be read, so this one is in the default language.
            warnings.push(format!(
                "{}: {e:#}",
                i18n::strings(Language::default()).global_load_failed
            ));
            Global::default()
        });
        Self {
            games,
            warnings,
            selection: None,
            profile: Profile::default(),
            global,
            status: None,
        }
    }

    /// Strings in the user's chosen language.
    pub(crate) fn t(&self) -> &'static Strings {
        i18n::strings(self.global.language)
    }

    fn open(&mut self, target: Option<&str>) {
        let selection = match target {
            None => return,
            Some("global") => Selection::Global,
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

    fn set_selection(&mut self, selection: Selection) {
        self.selection = Some(selection);
        self.status = None;
        if let Selection::Game(ix) = selection {
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
            None => return,
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
            None => return,
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

    fn toggle(&mut self, which: Toggle, cx: &mut Context<Self>) {
        let flag = match which {
            Toggle::GameMode => &mut self.profile.gamemode,
            Toggle::Gamescope => &mut self.profile.gamescope.enabled,
        };
        *flag = !*flag;
        self.save();
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        let global_selected = self.selection == Some(Selection::Global);
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
                    div()
                        .id("global")
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .cursor_pointer()
                        .when(global_selected, |d| d.bg(rgb(SELECTED)))
                        .when(!global_selected, |d| d.hover(|s| s.bg(rgb(ROW_HOVER))))
                        .on_click(cx.listener(|this, _, _, cx| this.select(Selection::Global, cx)))
                        .child(div().child(t.global_settings))
                        .child(widgets::label(format!(
                            "MangoHud {}",
                            if self.global.mangohud.enabled {
                                t.on
                            } else {
                                t.off
                            }
                        ))),
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
                    .first()
                    .map(|w| div().px_4().pb_2().child(widgets::label(format!("⚠ {w}")))),
            )
            .child(div().flex_1().px_2().child(self.render_list(cx)))
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
                            .child(div().text_color(rgb(TEXT)).child(game.name.clone()))
                            .child(widgets::label(game.id.to_string()))
                    })
                    .collect()
            }),
        )
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

    fn header(title: impl Into<SharedString>, subtitle: impl Into<SharedString>) -> Div {
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

    fn render_game(&self, game: &Game, cx: &mut Context<Self>) -> Div {
        let size = game
            .size_on_disk
            .map(|b| format!("  ·  {:.1} GB", b as f64 / 1e9))
            .unwrap_or_default();
        let mode = self.profile.mangohud.mode;
        let t = self.t();

        let mode_chip = |ix: usize, m: HudMode, text: &'static str, cx: &mut Context<Self>| {
            widgets::chip(("hud-mode", ix), text, mode == m)
                .on_click(cx.listener(move |this, _, _, cx| this.set_hud_mode(m, cx)))
        };
        let mut hud_card = widgets::section("MangoHud").child(
            div()
                .flex()
                .gap_2()
                .child(mode_chip(0, HudMode::Global, t.mode_global, cx))
                .child(mode_chip(1, HudMode::Custom, t.mode_custom, cx))
                .child(mode_chip(2, HudMode::Off, t.mode_off, cx)),
        );
        hud_card = match mode {
            HudMode::Global => hud_card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(widgets::label(if self.global.mangohud.enabled {
                        t.mode_global_on_note
                    } else {
                        t.mode_global_off_note
                    }))
                    .child(widgets::chip("goto-global", t.edit_global, false).on_click(
                        cx.listener(|this, _, _, cx| this.select(Selection::Global, cx)),
                    )),
            ),
            HudMode::Custom => hud_card.child(widgets::label(t.mode_custom_note)),
            HudMode::Off => hud_card.child(widgets::label(t.mode_off_note)),
        };

        let custom_editor = match (mode, &self.profile.mangohud.settings) {
            (HudMode::Custom, Some(hud)) => Some(self.render_hud_editor(&hud.clone(), cx)),
            _ => None,
        };

        div()
            .child(Self::header(
                game.name.clone(),
                format!("{}{size}", game.id),
            ))
            .child(hud_card)
            .children(custom_editor)
            .child(
                widgets::section(t.launch_section)
                    .child(
                        widgets::switch_row(
                            "gamemode",
                            "GameMode",
                            t.gamemode_hint,
                            self.profile.gamemode,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle(Toggle::GameMode, cx))),
                    )
                    .child(
                        widgets::switch_row(
                            "gamescope",
                            "Gamescope",
                            t.gamescope_hint,
                            self.profile.gamescope.enabled,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle(Toggle::Gamescope, cx))),
                    )
                    .child(widgets::label(t.steam_launch_option))
                    .child(
                        div()
                            .p_2()
                            .rounded_md()
                            .bg(rgb(BG))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_color(rgb(ACCENT))
                            .child(format!("arkor run {} -- %command%", game.id)),
                    ),
            )
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

/// `open`: `"global"` or a game id (`steam:730`) to show that page on start.
pub fn run(open: Option<String>) -> anyhow::Result<()> {
    // GPUI renders with Vulkan, so a global MANGOHUD=1 would draw the overlay on Arkor itself.
    // SAFETY: called before any threads are spawned. Must be removed again for any game
    // Arkor spawns directly from the UI (the `arkor run` path never sets it).
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
