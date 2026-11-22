//! A game's page: Play, MangoHud mode/editor, launch options, and removal for imported games.

use arkor_core::imported::Imported;
use arkor_core::{Game, GameId, HudMode, Store, paths};
use gpui::{Context, Div, div, prelude::*, rgb};

use super::widgets::{self, theme::*};
use super::{Library, Selection};

#[derive(Clone, Copy)]
pub(super) enum Toggle {
    GameMode,
    Gamescope,
}

impl Library {
    fn toggle(&mut self, which: Toggle, cx: &mut Context<Self>) {
        let flag = match which {
            Toggle::GameMode => &mut self.profile.gamemode,
            Toggle::Gamescope => &mut self.profile.gamescope.enabled,
        };
        *flag = !*flag;
        self.save();
        cx.notify();
    }

    /// Starts the selected game in a separate `arkor launch` process (the same path as the
    /// CLI), logging its output, and reports a failure in the status line when it ends badly.
    fn play(&mut self, cx: &mut Context<Self>) {
        let Some(Selection::Game(ix)) = self.selection else {
            return;
        };
        let id = self.games[ix].id.clone();
        let t = self.t();
        let (mut child, log) = match spawn_launch(&id) {
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

    fn remove_imported(&mut self, id: GameId, cx: &mut Context<Self>) {
        let result = Imported::load().and_then(|mut imported| {
            imported.remove(&id.id);
            imported.save()
        });
        match result {
            Ok(()) => {
                self.selection = None;
                self.reload_games();
            }
            Err(e) => self.status = Some(format!("{}: {e:#}", self.t().save_failed).into()),
        }
        cx.notify();
    }

    pub(super) fn render_game(&self, game: &Game, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        let size = game
            .size_on_disk
            .map(|b| format!("  ·  {:.1} GB", b as f64 / 1e9))
            .unwrap_or_default();
        let mode = self.profile.mangohud.mode;

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(Self::header(
                game.name.clone(),
                format!("{}  ·  {}{size}", self.store_name(game.id.store), game.id),
            ))
            .child(
                widgets::primary_button("play", t.play)
                    .on_click(cx.listener(|this, _, _, cx| this.play(cx))),
            );

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

        let steam = game.id.store == Store::Steam;
        let steam_option = steam.then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(widgets::label(t.steam_launch_option))
                .child(code_box(format!("arkor run {} -- %command%", game.id)))
        });

        let imported = (game.id.store == Store::Local).then(|| {
            let prefix = Imported::load()
                .ok()
                .and_then(|i| i.get(&game.id.id).map(|g| g.prefix.display().to_string()))
                .unwrap_or_default();
            let id = game.id.clone();
            widgets::section(t.store_local)
                .child(widgets::label(t.prefix_label))
                .child(code_box(prefix))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            widgets::chip("remove-imported", t.remove_imported, false).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.remove_imported(id.clone(), cx)
                                }),
                            ),
                        )
                        .child(widgets::label(t.remove_imported_note)),
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
                    .child(widgets::label(if steam {
                        t.steam_launches_note
                    } else {
                        t.arkor_launches_note
                    }))
                    .children(steam_option),
            )
            .children(imported)
    }
}

fn code_box(text: String) -> Div {
    div()
        .p_2()
        .rounded_md()
        .bg(rgb(BG))
        .border_1()
        .border_color(rgb(BORDER))
        .text_color(rgb(ACCENT))
        .child(text)
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
        // Set for Arkor's own window only (see `ui::run`).
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
