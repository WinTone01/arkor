//! A game's page: Play/Stop, its entry (name, executable, arguments, prefix), launch settings,
//! environment, Proton and protonfixes, prefix tools, logs, MangoHud and the extras.

use std::path::PathBuf;

use arkor_core::library::GameKind;
use arkor_core::{GameId, HudMode, paths};
use arkor_launch::start::{self, PrefixTool};
use arkor_launch::umu_db;
use gpui::{Context, Div, div, prelude::*};

use super::global_page::EnvScope;
use super::widgets::{self, theme::*};
use super::{Library, Selection};

/// Page-local state of the game page (reset when another page is selected).
#[derive(Default)]
pub(super) struct GameUi {
    confirm_delete: bool,
    delete_prefix: bool,
    umu_hits: Option<Result<Vec<umu_db::Entry>, String>>,
    umu_loading: bool,
    tool_busy: Option<String>,
    show_log: bool,
    /// Programs that appeared in the prefix after running an installer.
    new_exes: Vec<PathBuf>,
}

const RUNTIMES: [(&str, &str); 4] = [
    ("umu-sniper", "Sniper"),
    ("umu-soldier", "Soldier"),
    ("umu-scout", "Scout"),
    ("umu-steamrt4", "Steam Runtime 4"),
];

impl Library {
    /// Starts the selected game in a separate `arkor launch` process (the same path as the
    /// CLI) in its own process group, logging its output; reports a failure when it ends badly.
    pub(super) fn play(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        let (mut child, log) = match spawn_launch(&id) {
            Ok(c) => c,
            Err(e) => {
                self.status = Some(format!("{}: {e:#}", self.t().launch_failed).into());
                self.sound(super::sounds::Sound::Error);
                cx.notify();
                return;
            }
        };
        self.sound(super::sounds::Sound::GameStart);
        self.running.insert(id.to_string(), child.id());
        self.status = Some(self.t().launching.into());
        cx.notify();
        let close = self.global.close_on_launch;
        if self.global.splash {
            let name = self.entry().map(|g| g.name.clone()).unwrap_or_default();
            let subtitle = self.t().launching;
            super::splash::show(name, subtitle, cx, move |cx| {
                if close {
                    cx.quit();
                }
            });
        } else if close {
            cx.quit();
            return;
        }
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
                this.running.remove(&id.to_string());
                this.sound(if failure.is_some() {
                    super::sounds::Sound::Error
                } else {
                    super::sounds::Sound::GameExit
                });
                // On a clean exit just drop the "Starting…" message.
                this.status =
                    failure.map(|why| format!("{}: {why}", this.t().launch_failed).into());
                cx.notify();
            });
        })
        .detach();
    }

    /// Stops a game started here: SIGTERM to its process group, then `wineserver -k`.
    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        if let Some(pgid) = self.running.get(&id.to_string()) {
            // SAFETY: plain syscall; a negative pid addresses the whole process group.
            unsafe { libc::kill(-(*pgid as i32), libc::SIGTERM) };
        }
        if self.entry().is_some_and(|g| g.needs_prefix()) {
            let bg = cx.background_executor().clone();
            cx.spawn(async move |_, _| {
                let _ = bg.spawn(async move { start::kill_prefix(&id) }).await;
            })
            .detach();
        }
    }

    fn run_tool(&mut self, tool: PrefixTool, label: String, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        // Installers add programs to the prefix; compare before/after to offer them.
        let watch = matches!(tool, PrefixTool::Run(_))
            .then(|| {
                self.entry()
                    .map(|g| g.effective_prefix(&self.global.prefixes_dir()))
            })
            .flatten();
        self.game_ui.tool_busy = Some(label.clone());
        self.game_ui.new_exes.clear();
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let (result, new_exes) = bg
                .spawn(async move {
                    let scan = |p: &std::path::Path| {
                        arkor_stores::prefix::find_executables(p)
                            .into_iter()
                            .map(|c| c.path)
                            .collect::<Vec<_>>()
                    };
                    let before = watch.as_deref().map(scan).unwrap_or_default();
                    let result = start::run_prefix_tool(&id, &tool);
                    let after = watch.as_deref().map(scan).unwrap_or_default();
                    (
                        result,
                        after
                            .into_iter()
                            .filter(|p| !before.contains(p))
                            .collect::<Vec<_>>(),
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.game_ui.tool_busy = None;
                this.game_ui.new_exes = new_exes;
                this.status = Some(match result {
                    Ok(s) if s.success() => format!("{label} ✓").into(),
                    Ok(s) => format!("{label}: {s}").into(),
                    Err(e) => format!("{label}: {e:#}").into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn find_umu(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.entry().map(|g| g.name.clone()) else {
            return;
        };
        self.game_ui.umu_loading = true;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let db = umu_db::load()?;
                    anyhow::Ok(
                        umu_db::search(&db, &name)
                            .into_iter()
                            .cloned()
                            .collect::<Vec<_>>(),
                    )
                })
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = this.update(cx, |this, cx| {
                this.game_ui.umu_loading = false;
                this.game_ui.umu_hits = Some(result);
                cx.notify();
            });
        })
        .detach();
    }

    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        let suffix = self.tr("(copy)", "(kopya)");
        let result = self.library.duplicate(&id.id, suffix).and_then(|new_id| {
            self.library.save()?;
            let new_gid = GameId::new(arkor_core::Store::Local, new_id);
            self.profile.save(&new_gid)?;
            Ok(new_gid)
        });
        match result {
            Ok(new_id) => {
                self.reload_games();
                self.select_game(&new_id, cx);
            }
            Err(e) => self.report(Err(e)),
        }
        cx.notify();
    }

    /// Prefix that may be deleted with the game: only one Arkor created (inside its
    /// prefixes folder) and no other game uses.
    fn deletable_prefix(&self) -> Option<PathBuf> {
        let game = self.entry()?;
        if !game.needs_prefix() {
            return None;
        }
        let dir = self.global.prefixes_dir();
        let prefix = game.effective_prefix(&dir);
        let ours = prefix.starts_with(&dir) && prefix != dir;
        (ours && self.library.prefix_shared_with(&game.id, &dir).is_empty() && prefix.is_dir())
            .then_some(prefix)
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        let prefix = if self.game_ui.delete_prefix {
            self.deletable_prefix()
        } else {
            None
        };
        self.library.remove(&id.id);
        let mut result = self.library.save();
        if result.is_ok()
            && let Ok(path) = arkor_core::Profile::path_for(&id)
        {
            let _ = std::fs::remove_file(path);
        }
        if result.is_ok()
            && let Some(prefix) = prefix
        {
            result = std::fs::remove_dir_all(&prefix)
                .map_err(|e| anyhow::anyhow!("{}: {e}", prefix.display()));
        }
        self.selection = None;
        self.reload_games();
        self.report(result);
        cx.notify();
    }

    fn edit_entry(&mut self, f: impl FnOnce(&mut arkor_core::library::LibraryGame)) {
        if let Some(entry) = self.entry_mut() {
            f(entry);
            self.save_library();
        }
    }

    /// Name, category, visibility, cover, duplicate and delete.
    pub(super) fn render_details_card(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.t();
        let entry = self.entry().cloned()?;
        let name = self.field(
            "game-name",
            |this| this.entry().map(|g| g.name.clone()).unwrap_or_default(),
            "",
            cx,
            |this, text| {
                if !text.trim().is_empty() {
                    let text = text.trim().to_string();
                    this.edit_entry(|g| g.name = text);
                    let id = this.selected_id();
                    this.reload_games();
                    if let Some(id) = id {
                        this.selection = this
                            .games
                            .iter()
                            .position(|g| g.id == id)
                            .map(Selection::Game);
                    }
                }
            },
        );
        let mut entry_card = widgets::section(self.tr("Details", "Ayrıntılar"))
            .child(widgets::label(t.name_label))
            .child(name);
        let category = self.field(
            "game-category",
            |this| {
                this.entry()
                    .and_then(|g| g.category.clone())
                    .unwrap_or_default()
            },
            self.tr(
                "Category, e.g. Shooters (optional)",
                "Kategori, ör. FPS (isteğe bağlı)",
            ),
            cx,
            |this, text| {
                let value = Some(text.trim().to_string()).filter(|t| !t.is_empty());
                this.edit_entry(|g| g.category = value);
                this.apply_filter();
            },
        );
        let hidden = entry.hidden;
        entry_card = entry_card
            .child(widgets::label(self.tr("Category", "Kategori")))
            .child(category)
            .child(
                div().flex().child(
                    widgets::chip(
                        "hide-game",
                        self.tr("Hidden in the library", "Kütüphanede gizli"),
                        hidden,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.edit_entry(|g| g.hidden = !g.hidden);
                        this.apply_filter();
                        cx.notify();
                    })),
                ),
            );
        entry_card = entry_card.child(self.render_cover_controls(&entry.id, &entry.name, cx));
        Some(entry_card.child(self.manage_row(&entry.name, cx)))
    }

    /// What runs: the executable and its arguments, the prefix, or the store's id.
    pub(super) fn render_exec_card(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.t();
        let entry = self.entry().cloned()?;
        let has_prefix = entry.needs_prefix();
        let mut entry_card = widgets::section(self.tr("Executable", "Çalıştırılabilir dosya"));
        match &entry.kind {
            GameKind::Windows | GameKind::Linux => {
                let args = self.field(
                    "game-args",
                    |this| {
                        this.entry()
                            .map(|g| join_words(&g.args))
                            .unwrap_or_default()
                    },
                    self.tr("Arguments, e.g. -dx12", "Argümanlar, ör. -dx12"),
                    cx,
                    |this, text| {
                        let words = arkor_core::profile::shell_words(text);
                        this.edit_entry(|g| g.args = words);
                    },
                );
                entry_card = entry_card
                    .child(widgets::label(
                        self.tr("Executable", "Çalıştırılabilir dosya"),
                    ))
                    .child(path_row(
                        entry.exe.display().to_string(),
                        "pick-exe",
                        self.tr("Change…", "Değiştir…"),
                        cx,
                        |this, cx| {
                            let title = this.tr(
                                "Choose the game's executable",
                                "Oyunun çalıştırılabilir dosyasını seç",
                            );
                            this.prompt_path(false, title, cx, |this, path, _| {
                                this.edit_entry(|g| g.exe = path)
                            });
                        },
                    ))
                    .child(widgets::label(self.tr("Arguments", "Argümanlar")))
                    .child(args);
                if has_prefix {
                    let prefix = entry.effective_prefix(&self.global.prefixes_dir());
                    let shared = self
                        .library
                        .prefix_shared_with(&entry.id, &self.global.prefixes_dir())
                        .len();
                    entry_card = entry_card
                        .child(widgets::label(t.prefix_label))
                        .child(path_row(
                            prefix.display().to_string(),
                            "pick-prefix",
                            self.tr("Change…", "Değiştir…"),
                            cx,
                            |this, cx| {
                                let title = this.tr("Choose a Wine prefix", "Bir Wine prefix seç");
                                this.prompt_path(true, title, cx, |this, path, _| {
                                    let root = arkor_stores::prefix::resolve(&path).unwrap_or(path);
                                    this.edit_entry(|g| g.prefix = Some(root));
                                });
                            },
                        ))
                        .when(entry.prefix.is_some(), |d| {
                            d.child(
                                div().flex().child(
                                    widgets::chip(
                                        "prefix-default",
                                        self.tr(
                                            "Use own default prefix",
                                            "Kendi varsayılan prefix'ini kullan",
                                        ),
                                        false,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.edit_entry(|g| g.prefix = None);
                                            cx.notify();
                                        },
                                    )),
                                ),
                            )
                        })
                        .when(shared > 0, |d| {
                            d.child(widgets::label(format!(
                                "{} {shared}",
                                self.tr(
                                    "Other games using this prefix:",
                                    "Bu prefix'i kullanan diğer oyunlar:"
                                )
                            )))
                        });
                }
            }
            GameKind::Steam { appid } => {
                entry_card = entry_card.child(widgets::label(format!("Steam App ID: {appid}")));
                if which("protontricks") {
                    let appid = appid.clone();
                    entry_card = entry_card.child(div().flex().child(
                        widgets::chip("protontricks", "protontricks", false).on_click(cx.listener(
                            move |_, _, _, _| {
                                let _ = std::process::Command::new("protontricks")
                                    .arg(&appid)
                                    .arg("--gui")
                                    .spawn();
                            },
                        )),
                    ));
                }
            }
            GameKind::Epic { app_name } => {
                entry_card = entry_card.child(widgets::label(format!("Epic: {app_name}")))
            }
        }
        let _ = t;
        Some(entry_card)
    }

    /// MangoHud for this game: global, its own layout (with the editor) or off.
    pub(super) fn render_mangohud_card(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        // MangoHud mode.
        let mode = self.profile.mangohud.mode;
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
                        cx.listener(|this, _, _, cx| this.go(super::shell::Nav::Overlay, cx)),
                    )),
            ),
            HudMode::Custom => hud_card.child(widgets::label(t.mode_custom_note)),
            HudMode::Off => hud_card.child(widgets::label(t.mode_off_note)),
        };
        let custom_editor = match (mode, self.profile.mangohud.settings.clone()) {
            (HudMode::Custom, Some(hud)) => Some(self.render_hud_editor(&hud, cx)),
            _ => None,
        };

        let mut out = div().flex().flex_col().gap_4().child(hud_card);
        if let Some(editor) = custom_editor {
            out = out.child(editor);
        }
        out
    }

    /// Environment variables of this game.
    pub(super) fn render_env_card(&mut self, cx: &mut Context<Self>) -> Div {
        widgets::section(self.tr("Environment variables", "Ortam değişkenleri"))
            .child(widgets::label(self.tr(
                "Override global ones. Also settable in launch options (KEY=VALUE %command%).",
                "Global olanları ezer. Başlatma seçeneklerinde de yazılabilir (ANAHTAR=DEĞER %command%).",
            )))
            .child(self.env_editor(EnvScope::Game, cx))
    }

    /// Steam picks Proton itself.
    pub(super) fn render_steam_runtime_note(&mut self, _cx: &mut Context<Self>) -> Div {
        widgets::section("Proton").child(widgets::label(self.tr(
            "Steam runs this game with its own Proton; pick it in the game's Properties in Steam. Launch options, environment and the other tabs still apply.",
            "Bu oyunu Steam kendi Proton'uyla çalıştırır; Steam'de oyunun Özellikler bölümünden seç. Başlatma seçenekleri, ortam ve diğer sekmeler yine geçerli.",
        )))
    }

    /// Starts or stops the selected game.
    pub(super) fn toggle_play(&mut self, cx: &mut Context<Self>) {
        let running = self
            .selected_id()
            .is_some_and(|id| self.running.contains_key(&id.to_string()));
        if running {
            self.stop(cx)
        } else {
            self.play(cx)
        }
    }

    /// Duplicate / delete (with confirmation and the optional prefix removal).
    fn manage_row(&self, name: &str, cx: &mut Context<Self>) -> Div {
        if !self.game_ui.confirm_delete {
            return div()
                .flex()
                .gap_2()
                .pt_2()
                .child(
                    widgets::chip("duplicate", self.tr("Duplicate", "Kopyala"), false)
                        .on_click(cx.listener(|this, _, _, cx| this.duplicate(cx))),
                )
                .child(
                    widgets::chip("delete", self.tr("Delete…", "Sil…"), false).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.game_ui.confirm_delete = true;
                            cx.notify();
                        }),
                    ),
                );
        }
        let deletable = self.deletable_prefix();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_2()
            .child(div().text_sm().text_color(ACCENT()).child(format!(
                "{} “{name}”? {}",
                self.tr("Remove", "Kaldır:"),
                self.tr(
                    "Game files are not touched.",
                    "Oyun dosyalarına dokunulmaz."
                )
            )))
            .when_some(deletable, |d, prefix| {
                d.child(
                    widgets::switch_row(
                        "delete-prefix",
                        self.tr("Also delete its prefix", "Prefix'ini de sil"),
                        self.tr(
                            "Created by Arkor, used by no other game",
                            "Arkor oluşturdu, başka oyun kullanmıyor",
                        ),
                        self.game_ui.delete_prefix,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.game_ui.delete_prefix = !this.game_ui.delete_prefix;
                        cx.notify();
                    })),
                )
                .child(widgets::label(prefix.display().to_string()))
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        widgets::chip("delete-confirm", self.tr("Delete", "Sil"), true)
                            .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
                    )
                    .child(
                        widgets::chip("delete-cancel", self.tr("Cancel", "Vazgeç"), false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.game_ui.confirm_delete = false;
                                cx.notify();
                            })),
                    ),
            )
    }

    pub(super) fn render_launch(
        &mut self,
        game_id: &str,
        is_steam: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.t();
        let options = self.field(
            "launch-options",
            |this| this.profile.launch_options.clone(),
            "VAR=1 strace %command% -arg",
            cx,
            |this, text| {
                this.profile.launch_options = text.to_string();
                this.save();
            },
        );
        let pre = self.field(
            "pre-launch",
            |this| this.profile.pre_launch.clone(),
            self.tr("Command before the game", "Oyundan önce çalışacak komut"),
            cx,
            |this, text| {
                this.profile.pre_launch = text.to_string();
                this.save();
            },
        );
        let post = self.field(
            "post-launch",
            |this| this.profile.post_launch.clone(),
            self.tr("Command after the game", "Oyundan sonra çalışacak komut"),
            cx,
            |this, text| {
                this.profile.post_launch = text.to_string();
                this.save();
            },
        );
        let extra = self.profile.extra_app.clone();
        let switch = |id: &'static str,
                      title: &'static str,
                      hint: &'static str,
                      on: bool,
                      f: fn(&mut arkor_core::Profile),
                      cx: &mut Context<Self>| {
            widgets::switch_row(id, title, hint, on).on_click(cx.listener(move |this, _, _, cx| {
                f(&mut this.profile);
                this.save();
                cx.notify();
            }))
        };
        let mut card = widgets::section(t.launch_section)
            .child(switch(
                "gamemode",
                "GameMode",
                t.gamemode_hint,
                self.profile.gamemode,
                |p| p.gamemode = !p.gamemode,
                cx,
            ))
            .child(switch(
                "no-sleep",
                self.tr("Keep the system awake", "Sistemi uyanık tut"),
                self.tr(
                    "No sleep or screen blanking while playing",
                    "Oynarken uyku ya da ekran kararması yok",
                ),
                self.profile.no_sleep,
                |p| p.no_sleep = !p.no_sleep,
                cx,
            ))
            .child(switch(
                "proton-log",
                self.tr("Write Proton logs", "Proton log'u yaz"),
                self.tr(
                    "For troubleshooting; makes the game slower",
                    "Sorun gidermek için; oyunu yavaşlatır",
                ),
                self.profile.proton_log,
                |p| p.proton_log = !p.proton_log,
                cx,
            ))
            .child(widgets::label(
                self.tr("Launch options", "Başlatma seçenekleri"),
            ))
            .child(options)
            .child(widgets::label(self.tr("Before launch", "Başlatmadan önce")))
            .child(pre)
            .child(widgets::label(self.tr("After exit", "Çıkıştan sonra")))
            .child(post)
            .child(widgets::label(self.tr(
                "Extra program started with the game",
                "Oyunla birlikte başlayan ek program",
            )));
        card = match &extra.path {
            Some(path) => card
                .child(path_row(
                    path.display().to_string(),
                    "extra-remove",
                    self.tr("Remove", "Kaldır"),
                    cx,
                    |this, cx| {
                        this.profile.extra_app.path = None;
                        this.save();
                        cx.notify();
                    },
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_6()
                        .items_end()
                        .child(widgets::stepper(
                            self.tr("Delay", "Gecikme"),
                            format!("{} s", extra.delay_secs),
                            widgets::step_button("extra-delay-", "−").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.profile.extra_app.delay_secs =
                                        this.profile.extra_app.delay_secs.saturating_sub(1);
                                    this.save();
                                    cx.notify();
                                },
                            )),
                            widgets::step_button("extra-delay+", "+").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.profile.extra_app.delay_secs =
                                        (this.profile.extra_app.delay_secs + 1).min(300);
                                    this.save();
                                    cx.notify();
                                },
                            )),
                        ))
                        .child(
                            widgets::chip(
                                "extra-before",
                                self.tr("Start it before the game", "Oyundan önce başlat"),
                                extra.before_game,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.profile.extra_app.before_game =
                                    !this.profile.extra_app.before_game;
                                this.save();
                                cx.notify();
                            })),
                        ),
                ),
            None => card.child(div().flex().child(
                widgets::chip("extra-pick", self.tr("Choose…", "Seç…"), false).on_click(
                    cx.listener(|this, _, _, cx| {
                        let title = this.tr("Choose a program", "Bir program seç");
                        this.prompt_path(false, title, cx, |this, path, _| {
                            this.profile.extra_app.path = Some(path);
                            this.save();
                        });
                    }),
                ),
            )),
        };
        if is_steam {
            card = card
                .child(widgets::label(t.steam_launches_note))
                .child(widgets::label(t.steam_launch_option))
                .child(code_box(format!("arkor run local:{game_id} -- %command%")));
        }
        card
    }

    pub(super) fn render_protonfix(&mut self, cx: &mut Context<Self>) -> Div {
        let umu = self.field(
            "umu-id",
            |this| this.profile.umu_id.clone().unwrap_or_default(),
            "umu-1086940",
            cx,
            |this, text| {
                let text = text.trim();
                this.profile.umu_id = (!text.is_empty()).then(|| text.to_string());
                this.save();
            },
        );
        let mut card = widgets::section(self.tr("Protonfixes", "Protonfix'ler"))
            .child(widgets::label(self.tr(
                "Proton applies known fixes for a game when it knows its umu id. Empty = none.",
                "Proton, umu kimliğini bildiği oyunlara bilinen düzeltmeleri uygular. Boş = yok.",
            )))
            .child(umu)
            .child(
                div().flex().child(
                    widgets::chip(
                        "umu-find",
                        self.tr("Find in umu-database", "umu-database'de ara"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.find_umu(cx))),
                ),
            );
        if self.game_ui.umu_loading {
            card = card.child(widgets::label(self.tr("Searching…", "Aranıyor…")));
        }
        match &self.game_ui.umu_hits {
            Some(Ok(hits)) if hits.is_empty() => {
                card = card.child(widgets::label(self.tr("No match.", "Eşleşme yok.")))
            }
            Some(Ok(hits)) => {
                let mut row = div().flex().flex_wrap().gap_2();
                for (ix, hit) in hits.iter().enumerate() {
                    let id = hit.umu_id.clone();
                    let text =
                        format!("{} · {}", hit.title.clone().unwrap_or_default(), hit.umu_id);
                    row = row.child(
                        widgets::chip(
                            ("umu-hit", ix),
                            text,
                            self.profile.umu_id.as_deref() == Some(&hit.umu_id),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.profile.umu_id = Some(id.clone());
                            this.fields.remove("umu-id");
                            this.save();
                            cx.notify();
                        })),
                    );
                }
                card = card.child(row);
            }
            Some(Err(e)) => card = card.child(widgets::label(format!("⚠ {e}"))),
            None => {}
        }
        card
    }

    pub(super) fn render_runtime(&mut self, cx: &mut Context<Self>) -> Div {
        let current = self.profile.linux_runtime.clone();
        let mut row = div().flex().flex_wrap().gap_2().child(
            widgets::chip(
                "runtime-none",
                self.tr("Run directly", "Doğrudan çalıştır"),
                current.is_none(),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.profile.linux_runtime = None;
                this.save();
                cx.notify();
            })),
        );
        for (ix, (id, label)) in RUNTIMES.into_iter().enumerate() {
            row = row.child(
                widgets::chip(("runtime", ix), label, current.as_deref() == Some(id)).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.profile.linux_runtime = Some(id.to_string());
                        this.save();
                        cx.notify();
                    }),
                ),
            );
        }
        widgets::section("Steam Linux Runtime")
            .child(widgets::label(self.tr(
                "Run the game inside a Steam runtime container (through umu) for better library compatibility.",
                "Daha iyi kütüphane uyumluluğu için oyunu bir Steam runtime konteynerinde (umu ile) çalıştır.",
            )))
            .child(row)
    }

    pub(super) fn render_prefix_tools(&mut self, cx: &mut Context<Self>) -> Div {
        let verbs = self.field(
            "winetricks-verbs",
            |_| String::new(),
            self.tr(
                "winetricks verbs, e.g. vcrun2022 d3dcompiler_47",
                "winetricks fiilleri, ör. vcrun2022 d3dcompiler_47",
            ),
            cx,
            |_, _| {},
        );
        let card = widgets::section(self.tr("Prefix tools", "Prefix araçları"));
        if let Some(b) = self.game_ui.tool_busy.clone() {
            return card.child(widgets::label(format!("{b}…")));
        }
        card.child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    widgets::chip("tool-winecfg", "winecfg", false).on_click(cx.listener(
                        |this, _, _, cx| this.run_tool(PrefixTool::Winecfg, "winecfg".into(), cx),
                    )),
                )
                .child(
                    widgets::chip(
                        "tool-run",
                        self.tr("Run a file in the prefix…", "Prefix'te dosya çalıştır…"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        let title = this.tr(
                            "Choose an .exe, .msi, .bat or .reg file",
                            "Bir .exe, .msi, .bat ya da .reg dosyası seç",
                        );
                        this.prompt_path(false, title, cx, |this, path, cx| {
                            let label = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            this.run_tool(PrefixTool::Run(path), label, cx);
                        });
                    })),
                )
                .child(
                    widgets::chip(
                        "tool-open",
                        self.tr("Open prefix folder", "Prefix klasörünü aç"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, _| {
                        if let Some(g) = this.entry() {
                            let p = g.effective_prefix(&this.global.prefixes_dir());
                            let _ = std::process::Command::new("xdg-open").arg(p).spawn();
                        }
                    })),
                )
                .child(
                    widgets::chip(
                        "tool-kill",
                        self.tr(
                            "Stop everything in the prefix",
                            "Prefix'teki her şeyi durdur",
                        ),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                ),
        )
        .child(widgets::label("winetricks"))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().child(verbs))
                .child(
                    widgets::chip("tool-winetricks", self.tr("Run", "Çalıştır"), false).on_click(
                        cx.listener(|this, _, _, cx| {
                            let verbs = arkor_core::profile::shell_words(
                                &this.field_text("winetricks-verbs", cx).unwrap_or_default(),
                            );
                            this.run_tool(PrefixTool::Winetricks(verbs), "winetricks".into(), cx);
                        }),
                    ),
                ),
        )
        .child(widgets::label(self.tr(
            "No verbs opens the winetricks window.",
            "Fiil yazmazsan winetricks penceresi açılır.",
        )))
        .children((!self.game_ui.new_exes.is_empty()).then(|| self.render_new_exes(cx)))
    }

    /// After an installer ran: offer what it installed as new library games (same prefix).
    fn render_new_exes(&self, cx: &mut Context<Self>) -> Div {
        let prefix = self
            .entry()
            .map(|g| g.effective_prefix(&self.global.prefixes_dir()));
        let mut list = div()
            .flex()
            .flex_col()
            .gap_1()
            .pt_2()
            .child(widgets::label(self.tr(
                "New programs in the prefix; add any of them as a game:",
                "Prefix'te yeni programlar var; istediğini oyun olarak ekle:",
            )));
        for (ix, exe) in self.game_ui.new_exes.iter().enumerate() {
            let (exe, pfx) = (exe.clone(), prefix.clone());
            let name = arkor_stores::prefix::suggested_name(&exe);
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(super::style::pal::raised().opacity(0.45))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(div().child(name))
                            .child(
                                div()
                                    .truncate()
                                    .child(widgets::label(exe.display().to_string())),
                            ),
                    )
                    .child(
                        widgets::chip(("new-exe", ix), self.tr("Add", "Ekle"), false).on_click(
                            cx.listener(move |this, _, _, cx| {
                                let mut new = arkor_core::library::NewGame::windows(
                                    arkor_stores::prefix::suggested_name(&exe),
                                    exe.clone(),
                                );
                                new.prefix = pfx.clone();
                                this.add_to_library(new, cx);
                            }),
                        ),
                    ),
            );
        }
        list
    }

    fn log_dir(&self) -> Option<PathBuf> {
        self.selected_id()
            .and_then(|id| paths::launch_log(&id).ok())
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    }

    pub(super) fn render_logs(&mut self, id: &GameId, cx: &mut Context<Self>) -> Div {
        let show_label = if self.game_ui.show_log {
            self.tr("Hide last launch log", "Son başlatma log'unu gizle")
        } else {
            self.tr("Show last launch log", "Son başlatma log'unu göster")
        };
        let mut card = widgets::section(self.tr("Logs", "Log'lar")).child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    widgets::chip("log-show", show_label, self.game_ui.show_log).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.game_ui.show_log = !this.game_ui.show_log;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    widgets::chip(
                        "log-open",
                        self.tr("Open log folder", "Log klasörünü aç"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, _| {
                        if let Some(dir) = this.log_dir() {
                            let _ = std::fs::create_dir_all(&dir);
                            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                        }
                    })),
                )
                .child(
                    widgets::chip(
                        "log-clear",
                        self.tr("Clear logs", "Log'ları temizle"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let (Some(dir), Some(id)) = (this.log_dir(), this.selected_id()) {
                            clear_logs(&dir, &id.file_stem());
                        }
                        this.status = Some(this.tr("Logs cleared", "Log'lar temizlendi").into());
                        cx.notify();
                    })),
                ),
        );
        if self.game_ui.show_log {
            let text = paths::launch_log(id)
                .ok()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default();
            let tail: Vec<&str> = text
                .lines()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let mut lines = div()
                .flex()
                .flex_col()
                .p_2()
                .rounded_md()
                .bg(super::style::pal::raised().opacity(0.45))
                .text_xs()
                .font_family("monospace");
            if tail.is_empty() {
                lines = lines.child(widgets::label(self.tr("No log yet.", "Henüz log yok.")));
            }
            for line in tail {
                lines = lines.child(div().truncate().child(line.to_string()));
            }
            card = card.child(lines);
        }
        card
    }
}

fn join_words(words: &[String]) -> String {
    words
        .iter()
        .map(|w| {
            if w.is_empty() || w.contains(char::is_whitespace) {
                format!("\"{w}\"")
            } else {
                w.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Removes one game's launch log, and Proton logs (`steam-*.log`) in the log folder.
fn clear_logs(dir: &std::path::Path, stem: &str) {
    let _ = std::fs::remove_file(dir.join(format!("{stem}.log")));
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("steam-") && name.ends_with(".log") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

fn path_row(
    text: String,
    id: &'static str,
    button: &'static str,
    cx: &mut Context<Library>,
    on_click: impl Fn(&mut Library, &mut Context<Library>) + 'static,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(div().flex_1().min_w_0().truncate().text_sm().child(text))
        .child(
            widgets::chip(id, button, false)
                .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx))),
        )
}

fn code_box(text: String) -> Div {
    div()
        .p_2()
        .rounded_md()
        .bg(super::style::pal::raised().opacity(0.45))
        .border_1()
        .border_color(BORDER())
        .text_color(ACCENT())
        .child(text)
}

/// Spawns `arkor launch <id>` in its own process group, output going to the game's log.
fn spawn_launch(id: &GameId) -> anyhow::Result<(std::process::Child, std::path::PathBuf)> {
    use std::os::unix::process::CommandExt;
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
        // Own group: Stop can signal the whole tree, and closing Arkor doesn't end the game.
        .process_group(0)
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

fn which(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// Starts a library game outside the window (tray menu): own process group, output to its log.
pub(super) fn launch_detached(id: &str) {
    if let Ok(id) = id.parse::<GameId>() {
        let _ = spawn_launch(&id);
    }
}

/// "3 days ago" / "3 gün önce".
pub(super) fn ago(secs: u64, lang: arkor_core::Language) -> String {
    let (n, en, tr) = match secs {
        s if s < 3600 => (s / 60, "min", "dk"),
        s if s < 86_400 => (s / 3600, "h", "sa"),
        s if s < 30 * 86_400 => (s / 86_400, "days", "gün"),
        s => (s / (30 * 86_400), "months", "ay"),
    };
    match lang {
        arkor_core::Language::En => format!("{n} {en} ago"),
        arkor_core::Language::Tr => format!("{n} {tr} önce"),
    }
}

impl Library {
    fn refresh_cover(&mut self, id: &str) {
        if let Some(g) = self.library.get(id) {
            self.covers
                .insert(id.to_string(), arkor_launch::covers::cover_for(g));
        }
    }

    fn cover_job(
        &mut self,
        id: String,
        job: impl FnOnce() -> anyhow::Result<PathBuf> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.status = Some(self.tr("Getting the cover…", "Kapak alınıyor…").into());
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { job() }).await;
            let _ = this.update(cx, |this, cx| {
                this.refresh_cover(&id);
                this.report(result.map(|_| ()));
                cx.notify();
            });
        })
        .detach();
    }

    fn render_cover_controls(&mut self, id: &str, name: &str, cx: &mut Context<Self>) -> Div {
        let id_owned = id.to_string();
        let url = self.submit_field(
            "cover-url",
            self.tr("Image URL, then Enter", "Resim adresi, sonra Enter"),
            cx,
            move |this, text, cx| {
                let (id, url) = (id_owned.clone(), text.trim().to_string());
                this.cover_job(
                    id.clone(),
                    move || arkor_launch::covers::set_from_url(&id, &url),
                    cx,
                );
            },
        );
        let (id_file, id_sgdb, id_reset, name_sgdb) = (
            id.to_string(),
            id.to_string(),
            id.to_string(),
            name.to_string(),
        );
        div()
            .flex()
            .gap_4()
            .pt_2()
            .child(self.cover_tile(id, name, 90., 135.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(widgets::label(self.tr("Cover", "Kapak")))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                widgets::chip(
                                    "cover-file",
                                    self.tr("Choose image…", "Resim seç…"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        let id = id_file.clone();
                                        let title =
                                            this.tr("Choose a cover image", "Bir kapak resmi seç");
                                        this.prompt_path(false, title, cx, move |this, path, _| {
                                            let r = arkor_launch::covers::set_from_file(&id, &path)
                                                .map(|_| ());
                                            this.refresh_cover(&id);
                                            this.report(r);
                                        });
                                    },
                                )),
                            )
                            .child(widgets::chip("cover-sgdb", "SteamGridDB", false).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    let (id, name, key) = (
                                        id_sgdb.clone(),
                                        name_sgdb.clone(),
                                        this.global.steamgriddb_key.clone(),
                                    );
                                    this.cover_job(
                                        id.clone(),
                                        move || {
                                            arkor_launch::covers::set_from_steamgriddb(
                                                &id, &name, &key,
                                            )
                                        },
                                        cx,
                                    );
                                }),
                            ))
                            .child(
                                widgets::chip("cover-reset", self.tr("Reset", "Sıfırla"), false)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        arkor_launch::covers::reset(&id_reset);
                                        this.refresh_cover(&id_reset);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(url),
            )
    }

    /// App-menu, desktop and Steam shortcuts, and the command line for other launchers.
    pub(super) fn render_shortcuts(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let game = self.selected_game()?.clone();
        let id = game.id.clone();
        let command = arkor_launch::shortcuts::launch_command(&id);
        let in_menu = arkor_launch::shortcuts::menu_entry_exists(&id);
        let mut buttons = div().flex().flex_wrap().gap_2();
        let shortcut = move |this: &mut Self, desktop: bool, cx: &mut Context<Self>| {
            let Some(game) = this.selected_game().cloned() else {
                return;
            };
            let icon = this.covers.get(&game.id.id).cloned().flatten();
            this.status = Some(
                match arkor_launch::shortcuts::add_menu_entry(
                    &game.id,
                    &game.name,
                    icon.as_deref(),
                    desktop,
                ) {
                    Ok(_) if desktop => this
                        .tr(
                            "Added to the app menu and the desktop.",
                            "Uygulama menüsüne ve masaüstüne eklendi.",
                        )
                        .into(),
                    Ok(_) => this
                        .tr("Added to the app menu.", "Uygulama menüsüne eklendi.")
                        .into(),
                    Err(e) => format!("{e:#}").into(),
                },
            );
            cx.notify();
        };
        buttons = buttons
            .child(
                widgets::chip(
                    "sc-menu",
                    self.tr("Add to app menu", "Uygulama menüsüne ekle"),
                    in_menu,
                )
                .on_click(cx.listener(move |this, _, _, cx| shortcut(this, false, cx))),
            )
            .child(
                widgets::chip(
                    "sc-desktop",
                    self.tr("Add to desktop", "Masaüstüne ekle"),
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| shortcut(this, true, cx))),
            );
        if in_menu {
            buttons = buttons.child(
                widgets::chip(
                    "sc-remove",
                    self.tr("Remove shortcuts", "Kısayolları kaldır"),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(game) = this.selected_game() {
                        arkor_launch::shortcuts::remove_menu_entry(&game.id);
                    }
                    cx.notify();
                })),
            );
        }
        if game.id.store != arkor_core::game::Store::Steam {
            buttons = buttons.child(
                widgets::chip("sc-steam", self.tr("Add to Steam", "Steam'e ekle"), false).on_click(
                    cx.listener(|this, _, _, cx| {
                        let Some(game) = this.selected_game().cloned() else {
                            return;
                        };
                        let icon = this.covers.get(&game.id.id).cloned().flatten();
                        this.status = Some(
                            match arkor_launch::shortcuts::add_to_steam(
                                &game.id,
                                &game.name,
                                icon.as_deref(),
                            ) {
                                Ok(n) => format!(
                                    "{} ({n})",
                                    this.tr(
                                        "Added to Steam as a non-Steam game",
                                        "Steam'e Steam dışı oyun olarak eklendi"
                                    )
                                )
                                .into(),
                                Err(e) => format!("{e:#}").into(),
                            },
                        );
                        cx.notify();
                    }),
                ),
            );
        }
        let copy = command.clone();
        Some(
            widgets::section(self.tr("Shortcuts", "Kısayollar"))
                .child(buttons)
                .child(widgets::label(self.tr(
                    "Launch command, for Heroic, Lutris or scripts (Arkor applies this game's settings):",
                    "Başlatma komutu; Heroic, Lutris ya da betikler için (Arkor bu oyunun ayarlarını uygular):",
                )))
                .child(
                    div().flex().items_center().gap_3().child(div().flex_1().text_sm().font_family("monospace").child(command)).child(
                        widgets::chip("sc-copy", self.tr("Copy", "Kopyala"), false).on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                            this.status = Some(this.tr("Copied.", "Kopyalandı.").into());
                            cx.notify();
                        })),
                    ),
                ),
        )
    }
}
