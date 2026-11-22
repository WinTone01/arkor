//! Library window: "Global" + game list on the left, settings for the selection on the right.
//! Every change is saved immediately; `arkor run` picks it up on the next launch.

mod hud_editor;
mod i18n;
mod widgets;

use arkor_core::mangohud::HudSettings;
use arkor_core::{Game, GameId, Global, HudMode, Language, Profile, Store, paths};
use gpui::{
    AnyElement, App, Application, Bounds, Context, Div, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
    uniform_list,
};

use i18n::Strings;
use widgets::theme::*;

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    Installed,
    All,
}

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    Global,
    Game(usize),
}

pub struct Library {
    games: Vec<Game>,
    filter: Filter,
    /// Indices into `games` that pass `filter`, in display order.
    visible: Vec<usize>,
    list_scroll: UniformListScrollHandle,
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
        let (games, errors) = arkor_stores::all_games();
        let mut warnings: Vec<String> = errors.iter().map(|e| format!("{e:#}")).collect();
        let global = Global::load_or_import().unwrap_or_else(|e| {
            // The language setting itself could not be read, so this one is in the default language.
            warnings.push(format!(
                "{}: {e:#}",
                i18n::strings(Language::default()).global_load_failed
            ));
            Global::default()
        });
        let mut lib = Self {
            games,
            filter: Filter::Installed,
            visible: Vec::new(),
            list_scroll: UniformListScrollHandle::new(),
            warnings,
            selection: None,
            profile: Profile::default(),
            global,
            status: None,
        };
        lib.apply_filter();
        lib
    }

    fn apply_filter(&mut self) {
        self.visible = (0..self.games.len())
            .filter(|&ix| self.filter == Filter::All || self.games[ix].is_installed())
            .collect();
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.apply_filter();
        cx.notify();
    }

    fn store_name(&self, store: Store) -> &'static str {
        match store {
            Store::Steam => "Steam",
            Store::Epic => "Epic Games",
            Store::Gog => "GOG",
            Store::Amazon => "Amazon",
            Store::Local => self.t().store_local,
        }
    }

    /// Starts the selected game in a separate `arkor launch` process (the same path as the
    /// CLI), logging its output, and reports a failure in the status line when it ends badly.
    fn play(&mut self, cx: &mut Context<Self>) {
        let Some(Selection::Game(ix)) = self.selection else {
            return;
        };
        let id = self.games[ix].id.clone();
        let t = self.t();
        let child = spawn_launch(&id);
        let (mut child, log) = match child {
            Ok(c) => c,
            Err(e) => {
                self.status = Some(format!("{}: {e:#}", t.launch_failed).into());
                cx.notify();
                return;
            }
        };
        self.status = Some(t.launching.into());
        cx.notify();

        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let status = bg.spawn(async move { child.wait() }).await;
            let failure = match status {
                Ok(s) if s.success() => None,
                Ok(s) => Some(last_log_line(&log).unwrap_or_else(|| s.to_string())),
                Err(e) => Some(e.to_string()),
            };
            // The window may have been closed meanwhile; then there is nothing to update.
            let _ = this.update(cx, |this, cx| {
                // On a clean exit just drop the "Starting…" message.
                this.status =
                    failure.map(|why| format!("{}: {why}", this.t().launch_failed).into());
                cx.notify();
            });
        })
        .detach();
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
                Some(ix) => {
                    if !self.games[ix].is_installed() {
                        self.filter = Filter::All;
                        self.apply_filter();
                    }
                    Selection::Game(ix)
                }
                None => {
                    self.warnings
                        .insert(0, format!("`{id}` {}", self.t().not_found));
                    return;
                }
            },
        };
        if let Selection::Game(ix) = selection
            && let Some(row) = self.visible.iter().position(|&v| v == ix)
        {
            self.list_scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
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
                    .child(widgets::label(self.visible.len().to_string())),
            )
            .child(
                div()
                    .px_4()
                    .pb_2()
                    .flex()
                    .gap_2()
                    .child(
                        widgets::chip(
                            "filter-installed",
                            t.filter_installed,
                            self.filter == Filter::Installed,
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| this.set_filter(Filter::Installed, cx)),
                        ),
                    )
                    .child(
                        widgets::chip("filter-all", t.filter_all, self.filter == Filter::All)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_filter(Filter::All, cx)),
                            ),
                    ),
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
            self.visible.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .map(|row| {
                        let ix = this.visible[row];
                        let game = &this.games[ix];
                        let installed = game.is_installed();
                        let subtitle = if installed {
                            this.store_name(game.id.store).to_string()
                        } else {
                            format!(
                                "{} · {}",
                                this.store_name(game.id.store),
                                this.t().not_installed
                            )
                        };
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
                                    .text_color(rgb(if installed { TEXT } else { MUTED }))
                                    .child(game.name.clone()),
                            )
                            .child(widgets::label(subtitle))
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

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(Self::header(
                game.name.clone(),
                format!("{}  ·  {}{size}", self.store_name(game.id.store), game.id),
            ))
            .child(if game.is_installed() {
                widgets::primary_button("play", t.play)
                    .on_click(cx.listener(|this, _, _, cx| this.play(cx)))
                    .into_any_element()
            } else {
                widgets::badge(t.not_installed, false).into_any_element()
            });

        let how_it_starts = if !game.is_installed() {
            t.install_elsewhere
        } else if game.id.store == Store::Steam {
            t.steam_launches_note
        } else {
            t.arkor_launches_note
        };
        let steam_option = (game.id.store == Store::Steam).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
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
                )
        });

        div()
            .child(header)
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
                    .child(widgets::label(how_it_starts))
                    .children(steam_option),
            )
    }
}

/// Spawns `arkor launch <id>` with output going to the game's launch log.
fn spawn_launch(id: &GameId) -> anyhow::Result<(std::process::Child, std::path::PathBuf)> {
    let log = paths::launch_log(id)?;
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out = std::fs::File::create(&log)?;
    let child = std::process::Command::new(std::env::current_exe()?)
        .arg("launch")
        .arg(id.to_string())
        // Set for Arkor's own window only (see `run`).
        .env_remove("DISABLE_MANGOHUD")
        .stdin(std::process::Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out)
        .spawn()?;
    Ok((child, log))
}

/// Last non-empty line of a launch log: usually the error that ended it.
fn last_log_line(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
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
