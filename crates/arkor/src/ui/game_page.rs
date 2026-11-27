//! A game's page: Play/Stop, its entry (name, executable, arguments, prefix), launch settings,
//! environment, Proton and protonfixes, prefix tools, logs, MangoHud and the extras.

use std::path::PathBuf;

use arkor_core::library::GameKind;
use arkor_core::{GameId, HudMode, paths};
use arkor_launch::start::{self, PrefixTool};
use arkor_launch::umu_db;
use gpui::{Context, Div, div, prelude::*, px};

use super::global_page::EnvScope;
use super::widgets::{self, theme::*};
use super::{Library, Selection};

/// Page-local state of the game page (reset when another page is selected).
#[derive(Default)]
pub(super) struct GameUi {
    /// Sub-tab of "Upscaling & frame gen" (index into `features::UPSCALE_GROUPS`).
    pub(super) upscale_group: usize,
    confirm_delete: bool,
    delete_prefix: bool,
    umu_hits: Option<Result<Vec<umu_db::Entry>, String>>,
    umu_loading: bool,
    tool_busy: Option<String>,
    /// Programs that appeared in the prefix after running an installer.
    new_exes: Vec<PathBuf>,
    /// Settings → Game defaults: "Apply to every game" asked for confirmation.
    pub(super) confirm_apply_all: bool,
    /// Feature panels opened or closed by hand (others follow whether they are on).
    pub(super) expanded: std::collections::HashMap<&'static str, bool>,
}

/// Other runtimes a Linux game can use instead of the default Steam Runtime 4.
const RUNTIMES: [(&str, &str); 3] = [
    ("umu-sniper", "Sniper"),
    ("umu-soldier", "Soldier"),
    ("umu-scout", "Scout"),
];

impl Library {
    /// Starts the selected game in a separate `arkor launch` process (the same path as the
    /// CLI) in its own process group, logging its output; reports a failure when it ends badly.
    pub(super) fn play(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        // On a disk that is not mounted: the desktop mounts it (asking what it needs to).
        // Console mode did that before.
        if !self.console.on
            && let Some(drive) = self.game_disk(&id).filter(|d| d.mount.is_none())
        {
            self.status = Some(
                self.tr("Mounting the game's disk…", "Oyunun diski bağlanıyor…")
                    .into(),
            );
            cx.notify();
            let bg = cx.background_executor().clone();
            cx.spawn(async move |this, cx| {
                let result = bg
                    .spawn(async move { arkor_launch::admin::mount_asking(&drive.disk) })
                    .await;
                let _ = this.update(cx, |this, cx| match result {
                    Ok(()) => {
                        this.select_game(&id, cx);
                        // Once: a disk that still is not there ends here.
                        if this.game_disk(&id).is_none() {
                            this.play(cx);
                        }
                    }
                    Err(e) => {
                        this.status = Some(format!("{e:#}").into());
                        this.sound(super::sounds::Sound::Error);
                        cx.notify();
                    }
                });
            })
            .detach();
            return;
        }
        // An emulator game whose emulator is missing: downloaded first, then it starts.
        if self.emu_before_play(&id, cx) {
            return;
        }
        // Newer saves from the cloud first; it starts by itself then.
        if self.saves_first(&id, cx) {
            return;
        }
        let screen = self.console.on.then_some(self.console.screen_px);
        let trial = self.trouble_trial(&id);
        let (mut child, log) = match spawn_launch(&id, screen, trial) {
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
        // Console mode shows the start itself and is there again after the game.
        let close = self.global.close_on_launch && !self.console.on;
        if self.global.splash && !self.console.on {
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
        let started = std::time::Instant::now();
        let name = self.entry().map(|g| g.name.clone()).unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let status = bg.spawn(async move { child.wait() }).await;
            let ended = arkor_launch::trouble::Ended {
                // A signal as shells count it.
                code: status.as_ref().ok().and_then(|s| {
                    use std::os::unix::process::ExitStatusExt;
                    s.code().or(s.signal().map(|n| 128 + n))
                }),
                secs: started.elapsed().as_secs(),
            };
            let failure = match status {
                Ok(s) if s.success() => None,
                // Played for a while: closing it is no failure, whatever the game's exit code
                // (many end with one, or are ended by a signal when closed).
                Ok(_) if started.elapsed() > std::time::Duration::from_secs(20) => None,
                Ok(s) => Some(error_line(&log).unwrap_or_else(|| s.to_string())),
                Err(e) => Some(e.to_string()),
            };
            // The window may have been closed meanwhile; then there is nothing to update.
            let _ = this.update(cx, |this, cx| {
                // Stopped from here: not a failure either.
                let failure = failure.filter(|_| !this.stopping.remove(&id.to_string()));
                this.running.remove(&id.to_string());
                this.saves_after(&id, cx);
                this.trouble_ended(&id, ended, cx);
                this.quick_over(&id.id, cx);
                // Console mode comes back to the front when the game ends.
                this.console.come_back = this.console.on;
                if let Some(why) = &failure
                    && this.console.on
                {
                    this.console_failed(&id, &name, why, &log);
                }
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
        self.stopping.insert(id.to_string());
        if let Some(pgid) = self.running.get(&id.to_string()) {
            // SAFETY: plain syscall; a negative pid addresses the whole process group.
            unsafe { libc::kill(-(*pgid as i32), libc::SIGTERM) };
        }
        // A Steam game runs under Steam, not under Arkor's process: end it there.
        if let Some(arkor_core::library::GameKind::Steam { appid }) =
            self.entry().map(|g| g.kind.clone())
        {
            let bg = cx.background_executor().clone();
            cx.spawn(async move |_, _| {
                let _ = bg
                    .spawn(async move { arkor_launch::steam_link::stop_game(&appid) })
                    .await;
            })
            .detach();
        }
        if self.entry().is_some_and(|g| g.needs_prefix()) {
            let bg = cx.background_executor().clone();
            cx.spawn(async move |_, _| {
                let _ = bg.spawn(async move { start::kill_prefix(&id) }).await;
            })
            .detach();
        }
    }

    pub(super) fn run_tool(&mut self, tool: PrefixTool, label: String, cx: &mut Context<Self>) {
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

    /// Looks `name` up in umu-database (the game's own name when `None`).
    fn find_umu(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        let Some(name) = name
            .filter(|n| !n.trim().is_empty())
            .or_else(|| self.entry().map(|g| g.name.clone()))
        else {
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
            // It may hold saves: from the trash it can be put back.
            result = arkor_core::fs_guard::trash(&prefix);
        }
        self.selection = None;
        self.reload_games();
        self.report(result);
        cx.notify();
    }

    pub(super) fn edit_entry(&mut self, f: impl FnOnce(&mut arkor_core::library::LibraryGame)) {
        if let Some(entry) = self.entry_mut() {
            f(entry);
            self.save_library();
        }
    }

    pub(super) fn render_details_card(&mut self, cx: &mut Context<Self>) -> Option<Div> {
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
        let category = self.field(
            "game-category",
            |this| {
                this.entry()
                    .and_then(|g| g.category.clone())
                    .unwrap_or_default()
            },
            self.tr("e.g. Shooters", "ör. FPS"),
            cx,
            |this, text| {
                let value = Some(text.trim().to_string()).filter(|t| !t.is_empty());
                this.edit_entry(|g| g.category = value);
                this.apply_filter();
            },
        );
        let card = widgets::section(self.tr("Name, category and cover", "Ad, kategori ve kapak"))
            .child(widgets::setting_row(
                self.tr("Name", "Ad"),
                self.tr(
                    "Shown in the library and shortcuts.",
                    "Kütüphanede ve kısayollarda görünür.",
                ),
                div().w(px(300.)).child(name),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("Category", "Kategori"),
                self.tr(
                    "Optional; the library can filter by it.",
                    "İsteğe bağlı; kütüphane buna göre süzebilir.",
                ),
                div().w(px(300.)).child(category),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("Hidden", "Gizli"),
                self.tr(
                    "Not shown in the library (Ctrl+H shows hidden games).",
                    "Kütüphanede görünmez (Ctrl+H gizlileri gösterir).",
                ),
                widgets::switch(entry.hidden)
                    .id("hide-game")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.edit_entry(|g| g.hidden = !g.hidden);
                        this.apply_filter();
                        cx.notify();
                    })),
            ))
            .child(widgets::divider())
            .child(self.render_cover_controls(&entry.id, &entry.name, cx))
            .child(widgets::divider())
            .child(self.manage_row(&entry.name, cx));
        Some(card)
    }

    /// What runs: the executable and its arguments, the prefix, or the store's id.
    pub(super) fn render_exec_card(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        use super::style::{self, pal};
        let entry = self.entry().cloned()?;
        if matches!(entry.kind, GameKind::Emulated { .. }) {
            return Some(self.render_emulated_card(&entry, cx));
        }
        let mut card = widgets::section(self.tr("Game files", "Oyun dosyaları"));
        match &entry.kind {
            GameKind::Windows | GameKind::Linux => {
                let args = self.field(
                    "game-args",
                    |this| {
                        this.entry()
                            .map(|g| join_words(&g.args))
                            .unwrap_or_default()
                    },
                    "-dx12 -skipintro",
                    cx,
                    |this, text| {
                        let words = arkor_core::profile::shell_words(text);
                        this.edit_entry(|g| g.args = words);
                    },
                );
                card = card
                    .child(widgets::setting_row(
                        self.tr("Executable", "Çalıştırılabilir dosya"),
                        entry.exe.display().to_string(),
                        style::button("pick-exe", self.tr("Change…", "Değiştir…"), false)
                            .child(style::icon("folder-open", 13., pal::muted()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let title = this.tr(
                                    "Choose the game's executable",
                                    "Oyunun çalıştırılabilir dosyasını seç",
                                );
                                this.prompt_path(false, title, cx, |this, path, _| {
                                    this.edit_entry(|g| g.exe = path)
                                });
                            })),
                    ))
                    .child(widgets::divider())
                    .child(widgets::setting_row(
                        self.tr("Arguments", "Argümanlar"),
                        self.tr("Passed to the game itself.", "Oyunun kendisine verilir."),
                        div().w(px(300.)).child(args),
                    ));
                if entry.needs_prefix() {
                    let prefix = entry.effective_prefix(&self.global.prefixes_dir());
                    let shared = self
                        .library
                        .prefix_shared_with(&entry.id, &self.global.prefixes_dir())
                        .len();
                    let mut hint = prefix.display().to_string();
                    if shared > 0 {
                        hint = format!(
                            "{hint} · {} {shared}",
                            self.tr("also used by", "şunlar da kullanıyor:")
                        );
                    }
                    card = card.child(widgets::divider()).child(widgets::setting_row(
                        self.tr("Wine prefix", "Wine prefix"),
                        hint,
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                style::button(
                                    "pick-prefix",
                                    self.tr("Change…", "Değiştir…"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        let title =
                                            this.tr("Choose a Wine prefix", "Bir Wine prefix seç");
                                        this.prompt_path(true, title, cx, |this, path, _| {
                                            let root = arkor_stores::prefix::resolve(&path)
                                                .unwrap_or(path);
                                            this.edit_entry(|g| g.prefix = Some(root));
                                        });
                                    },
                                )),
                            )
                            .when(entry.prefix.is_some(), |d| {
                                d.child(
                                    style::button(
                                        "prefix-default",
                                        self.tr("Own default", "Kendi varsayılanı"),
                                        false,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.edit_entry(|g| g.prefix = None);
                                            cx.notify();
                                        },
                                    )),
                                )
                            }),
                    ));
                }
            }
            // Drawn by `render_emulated_card` (returned above).
            GameKind::Emulated { .. } => {}
            GameKind::Steam { appid } => {
                let appid = appid.clone();
                card = card.child(widgets::setting_row(
                    "Steam App ID",
                    appid.clone(),
                    div().when(which("protontricks"), |d| {
                        d.child(
                            style::button("protontricks", "protontricks", false).on_click(
                                cx.listener(move |_, _, _, _| {
                                    let _ = std::process::Command::new("protontricks")
                                        .arg(&appid)
                                        .arg("--gui")
                                        .spawn();
                                }),
                            ),
                        )
                    }),
                ));
            }
            GameKind::Epic { app_name } => {
                card = card.child(widgets::setting_row("Epic", app_name.clone(), div()));
            }
            GameKind::Amazon { id } => {
                card = card.child(widgets::setting_row("Amazon", id.clone(), div()));
            }
        }
        // Installed by Arkor: its files can be checked or removed.
        if let Some((shop, id)) = self.entry_shop(&entry) {
            card = card.child(widgets::divider()).child(widgets::setting_row(
                shop.name(),
                "",
                style::button("store-manage", self.tr("Manage", "Yönet"), false)
                    .child(style::icon("store", 13., pal::muted()))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.shop_open_game(shop, &id, cx)),
                    ),
            ));
        }
        Some(card)
    }

    /// MangoHud for this game: global, its own layout (with the editor) or off.
    pub(super) fn render_mangohud_card(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        // MangoHud mode.
        let mode = self.profile.mangohud.mode;
        let global_on = self.global.mangohud.enabled;
        let card = |ix: usize,
                    m: HudMode,
                    icon: &'static str,
                    title: &'static str,
                    hint: &'static str,
                    cx: &mut Context<Self>| {
            widgets::choice_card(("hud-mode", ix), icon, title, hint, mode == m)
                .on_click(cx.listener(move |this, _, _, cx| this.set_hud_mode(m, cx)))
        };
        let mut hud_card = widgets::section("MangoHud").child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(card(
                    0,
                    HudMode::Global,
                    "copy",
                    t.mode_global,
                    if global_on {
                        t.mode_global_on_note
                    } else {
                        t.mode_global_off_note
                    },
                    cx,
                ))
                .child(card(
                    1,
                    HudMode::Custom,
                    "pencil",
                    t.mode_custom,
                    t.mode_custom_note,
                    cx,
                ))
                .child(card(
                    2,
                    HudMode::Off,
                    "eye-off",
                    t.mode_off,
                    t.mode_off_note,
                    cx,
                )),
        );
        if mode == HudMode::Global {
            hud_card = hud_card.child(
                div().flex().child(
                    super::style::button("goto-global", t.edit_global, false)
                        .child(super::style::icon(
                            "arrow-right",
                            13.,
                            super::style::pal::muted(),
                        ))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.go(super::shell::Nav::Overlay, cx)),
                        ),
                ),
            );
        }
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
            .child(self.env_editor(EnvScope::Game, cx))
    }

    /// Steam picks Proton itself.
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

    fn manage_row(&self, name: &str, cx: &mut Context<Self>) -> Div {
        use super::style::{self, pal};
        if !self.game_ui.confirm_delete {
            return widgets::setting_row(
                self.tr("Manage", "Yönet"),
                self.tr(
                    "A copy keeps all settings; deleting never touches game files.",
                    "Kopya tüm ayarları korur; silmek oyun dosyalarına dokunmaz.",
                ),
                div()
                    .flex()
                    .gap_2()
                    .child(
                        style::button("duplicate", self.tr("Duplicate", "Kopyala"), false)
                            .child(style::icon("copy", 13., pal::muted()))
                            .on_click(cx.listener(|this, _, _, cx| this.duplicate(cx))),
                    )
                    .child(
                        style::button("delete", self.tr("Delete…", "Sil…"), false)
                            .child(style::icon("trash", 13., pal::danger()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.game_ui.confirm_delete = true;
                                cx.notify();
                            })),
                    ),
            );
        }
        let deletable = self.deletable_prefix();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(px(12.))
            .border_1()
            .border_color(pal::danger().opacity(0.4))
            .bg(pal::danger().opacity(0.06))
            .child(div().font_weight(gpui::FontWeight::MEDIUM).child(format!(
                "{} “{name}”?",
                self.tr("Remove", "Kaldırılsın mı:")
            )))
            .child(widgets::label(self.tr(
                "It leaves the library; game files are not touched.",
                "Kütüphaneden çıkar; oyun dosyalarına dokunulmaz.",
            )))
            .when_some(deletable, |d, prefix| {
                d.child(widgets::setting_row(
                    self.tr(
                        "Also move its prefix to the trash",
                        "Prefix'ini de çöpe taşı",
                    ),
                    format!(
                        "{} · {}",
                        self.tr(
                            "Created by Arkor, used by no other game",
                            "Arkor oluşturdu, başka oyun kullanmıyor"
                        ),
                        prefix.display()
                    ),
                    widgets::switch(self.game_ui.delete_prefix)
                        .id("delete-prefix")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.game_ui.delete_prefix = !this.game_ui.delete_prefix;
                            cx.notify();
                        })),
                ))
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        style::button("delete-confirm", self.tr("Delete", "Sil"), true)
                            .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
                    )
                    .child(
                        style::button("delete-cancel", self.tr("Cancel", "Vazgeç"), false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.game_ui.confirm_delete = false;
                                cx.notify();
                            })),
                    ),
            )
    }

    pub(super) fn render_launch(&mut self, is_steam: bool, cx: &mut Context<Self>) -> Div {
        use super::style::{self, pal};
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
            "notify-send 'Starting'",
            cx,
            |this, text| {
                this.profile.pre_launch = text.to_string();
                this.save();
            },
        );
        let post = self.field(
            "post-launch",
            |this| this.profile.post_launch.clone(),
            "notify-send 'Done'",
            cx,
            |this, text| {
                this.profile.post_launch = text.to_string();
                this.save();
            },
        );
        let extra = self.profile.extra_app.clone();
        let mut card = widgets::section(self.tr("Launch options and commands", "Başlatma seçenekleri ve komutlar"))
            .child(widgets::setting_row(
                self.tr("Launch options", "Başlatma seçenekleri"),
                self.tr(
                    "As in Steam: KEY=VALUE words become variables; %command% is the game, words around it wrap it.",
                    "Steam'deki gibi: ANAHTAR=DEĞER kelimeleri değişken olur; %command% oyundur, çevresindekiler onu sarar.",
                ),
                div().w(px(320.)).child(options),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("Before the game", "Oyundan önce"),
                self.tr("A shell command; the game waits for it.", "Bir kabuk komutu; oyun onu bekler."),
                div().w(px(320.)).child(pre),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("After the game", "Oyundan sonra"),
                self.tr("Runs when the game has closed.", "Oyun kapanınca çalışır."),
                div().w(px(320.)).child(post),
            ))
            .child(widgets::divider());
        card = match &extra.path {
            Some(path) => card
                .child(widgets::setting_row(
                    self.tr("Second program", "İkinci program"),
                    path.display().to_string(),
                    style::button("extra-remove", self.tr("Remove", "Kaldır"), false)
                        .child(style::icon("x", 13., pal::muted()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.profile.extra_app.path = None;
                            this.save();
                            cx.notify();
                        })),
                ))
                .child(widgets::setting_row(
                    self.tr("Delay", "Gecikme"),
                    self.tr("Seconds between the two.", "İkisi arasındaki saniye."),
                    widgets::stepper_control(
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
                    ),
                ))
                .child(widgets::setting_row(
                    self.tr("Start it first", "Önce onu başlat"),
                    self.tr(
                        "Otherwise it starts after the game.",
                        "Yoksa oyundan sonra başlar.",
                    ),
                    widgets::switch(extra.before_game)
                        .id("extra-before")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.profile.extra_app.before_game =
                                !this.profile.extra_app.before_game;
                            this.save();
                            cx.notify();
                        })),
                )),
            None => card.child(widgets::setting_row(
                self.tr("Second program", "İkinci program"),
                self.tr(
                    "Started with the game, e.g. a trainer or mod manager; closed with it.",
                    "Oyunla başlar, ör. bir trainer ya da mod yöneticisi; onunla kapanır.",
                ),
                style::button("extra-pick", self.tr("Choose…", "Seç…"), false)
                    .child(style::icon("folder-open", 13., pal::muted()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let title = this.tr("Choose a program", "Bir program seç");
                        this.prompt_path(false, title, cx, |this, path, _| {
                            this.profile.extra_app.path = Some(path);
                            this.save();
                        });
                    })),
            )),
        };
        if is_steam {
            card = card
                .child(widgets::divider())
                .child(widgets::label(t.steam_launches_note));
        }
        card
    }

    /// Keeping the system awake while playing.
    pub(super) fn render_sleep(&mut self, cx: &mut Context<Self>) -> Div {
        widgets::section(self.tr("Keep awake", "Uyku engelle")).child(widgets::setting_row(
            self.tr("Keep the system awake", "Sistemi uyanık tut"),
            self.tr(
                "No sleep or screen blanking while playing.",
                "Oynarken uyku ya da ekran kararması yok.",
            ),
            widgets::switch(self.profile.no_sleep)
                .id("no-sleep")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.profile.no_sleep = !this.profile.no_sleep;
                    this.save();
                    cx.notify();
                })),
        ))
    }

    pub(super) fn render_protonfix(&mut self, cx: &mut Context<Self>) -> Div {
        use super::style::{self, pal};
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
        let search = self.submit_field(
            "umu-search",
            self.tr(
                "Game name, e.g. Warframe, then Enter",
                "Oyun adı, ör. Warframe, sonra Enter",
            ),
            cx,
            |this, text, cx| this.find_umu(Some(text.to_string()), cx),
        );
        let mut card = widgets::section(self.tr("Protonfixes", "Protonfix'ler"))
            .child(widgets::setting_row(
                self.tr("umu id", "umu kimliği"),
                self.tr(
                    "Proton applies the known fixes for this id. Empty = none.",
                    "Proton bu kimlik için bilinen düzeltmeleri uygular. Boş = yok.",
                ),
                div().w(px(200.)).child(umu),
            ))
            .child(widgets::setting_row(
                self.tr("Find the id by name", "Kimliği adla bul"),
                self.tr(
                    "Type the game's name; pick it from the matches below.",
                    "Oyunun adını yaz; aşağıdaki eşleşmelerden seç.",
                ),
                div()
                    .flex()
                    .gap_2()
                    .child(div().w(px(260.)).child(search))
                    .child(
                        style::button("umu-find", self.tr("Search", "Ara"), false)
                            .child(style::icon("search", 13., pal::muted()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let text = this
                                    .fields
                                    .inputs
                                    .get("umu-search")
                                    .map(|(f, _)| f.read(cx).text());
                                this.find_umu(text, cx)
                            })),
                    ),
            ));
        if self.game_ui.umu_loading {
            card =
                card.child(self.loading_inline(
                    self.tr("Searching umu-database…", "umu-database'de aranıyor…"),
                ));
        }
        match &self.game_ui.umu_hits {
            Some(Ok(hits)) if hits.is_empty() => {
                card = card.child(widgets::label(
                    self.tr("No match in umu-database.", "umu-database'de eşleşme yok."),
                ))
            }
            Some(Ok(hits)) => {
                let mut list = div()
                    .flex()
                    .flex_col()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(pal::border_strong())
                    .overflow_hidden();
                let n = hits.len();
                for (ix, hit) in hits.iter().enumerate() {
                    let id = hit.umu_id.clone();
                    let chosen = self.profile.umu_id.as_deref() == Some(&hit.umu_id);
                    let store = hit.store.clone().filter(|s| !s.is_empty() && s != "none");
                    list = list.child(
                        div()
                            .id(("umu-hit", ix))
                            .px_3()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_3()
                            .cursor_pointer()
                            .when(ix + 1 < n, |d| {
                                d.border_b_1().border_color(pal::glass_spec().border)
                            })
                            .when(chosen, |d| d.bg(pal::accent().opacity(0.12)))
                            .hover(|d| d.bg(pal::raised().opacity(0.5)))
                            .child(style::icon(
                                if chosen { "circle-check" } else { "circle" },
                                14.,
                                if chosen {
                                    pal::accent_light()
                                } else {
                                    pal::faint()
                                },
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(hit.title.clone().unwrap_or_default()),
                            )
                            .children(store.map(|s| style::tag(s, pal::muted())))
                            .child(
                                div()
                                    .text_sm()
                                    .font_family("monospace")
                                    .text_color(pal::muted())
                                    .child(hit.umu_id.clone()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.profile.umu_id = Some(id.clone());
                                this.fields.remove("umu-id");
                                this.save();
                                cx.notify();
                            })),
                    );
                }
                card = card
                    .child(widgets::subheading(
                        self.tr("Matches: pick one", "Eşleşmeler: birini seç"),
                    ))
                    .child(list);
            }
            Some(Err(e)) => card = card.child(widgets::label(format!("⚠ {e}"))),
            None => {}
        }
        card
    }

    pub(super) fn render_runtime(&mut self, cx: &mut Context<Self>) -> Div {
        use arkor_core::profile::NO_RUNTIME;
        let current = self.profile.linux_runtime.clone().filter(|r| !r.is_empty());
        let chip = |id: (&'static str, usize),
                    label: String,
                    value: Option<&'static str>,
                    current: &Option<String>,
                    cx: &mut Context<Self>| {
            widgets::chip(id, label, current.as_deref() == value).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.profile.linux_runtime = value.map(str::to_string);
                    this.save();
                    cx.notify();
                },
            ))
        };
        let mut row = div().flex().flex_wrap().gap_2().child(chip(
            ("runtime", 0),
            format!("Steam Runtime 4 ({})", self.tr("default", "varsayılan")),
            None,
            &current,
            cx,
        ));
        for (ix, (id, label)) in RUNTIMES.into_iter().enumerate() {
            row = row.child(chip(
                ("runtime", ix + 1),
                label.to_string(),
                Some(id),
                &current,
                cx,
            ));
        }
        row = row.child(chip(
            ("runtime", 9),
            self.tr("Run directly", "Doğrudan çalıştır").to_string(),
            Some(NO_RUNTIME),
            &current,
            cx,
        ));
        widgets::section("Steam Linux Runtime").child(widgets::setting_row(
            self.tr("Container", "Konteyner"),
            self.tr(
                "Like Faugus Launcher, Linux games run inside Steam Runtime 4 by default; older games may need Soldier or Scout.",
                "Faugus Launcher gibi, Linux oyunları varsayılan olarak Steam Runtime 4 içinde çalışır; eski oyunlar Soldier ya da Scout isteyebilir.",
            ),
            row,
        ))
    }

    pub(super) fn render_prefix_tools(&mut self, cx: &mut Context<Self>) -> Div {
        use super::style::{self, pal};
        let verbs = self.field(
            "winetricks-verbs",
            |_| String::new(),
            "vcrun2022 d3dcompiler_47",
            cx,
            |_, _| {},
        );
        let mut card = widgets::section(self.tr("Wine prefix tools", "Wine prefix araçları"));
        if let Some(b) = self.game_ui.tool_busy.clone() {
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(style::icon("loader", 14., pal::accent_light()))
                    .child(widgets::label(format!("{b}…"))),
            );
        }
        let busy = self.game_ui.tool_busy.is_some();
        let btn = |id: &'static str, label: &'static str, icon: &'static str| {
            style::button(id, label, false)
                .child(style::icon(icon, 13., pal::muted()))
                .when(busy, |b| b.opacity(0.4))
        };
        card.child(widgets::setting_row(
            self.tr("Wine settings", "Wine ayarları"),
            self.tr(
                "winecfg: Windows version, drives, audio, DLL overrides.",
                "winecfg: Windows sürümü, sürücüler, ses, DLL ayarları.",
            ),
            btn("tool-winecfg", "winecfg", "settings").on_click(cx.listener(|this, _, _, cx| {
                if this.game_ui.tool_busy.is_none() {
                    this.run_tool(PrefixTool::Winecfg, "winecfg".into(), cx)
                }
            })),
        ))
        .child(widgets::divider())
        .child(widgets::setting_row(
            "winetricks",
            self.tr(
                "Install runtimes by name; empty opens the winetricks window.",
                "Bileşenleri adıyla kur; boş bırakırsan winetricks penceresi açılır.",
            ),
            div()
                .flex()
                .gap_2()
                .child(div().w(px(220.)).child(verbs))
                .child(
                    btn("tool-winetricks", self.tr("Run", "Çalıştır"), "play").on_click(
                        cx.listener(|this, _, _, cx| {
                            if this.game_ui.tool_busy.is_some() {
                                return;
                            }
                            let verbs = arkor_core::profile::shell_words(
                                &this.field_text("winetricks-verbs", cx).unwrap_or_default(),
                            );
                            this.run_tool(PrefixTool::Winetricks(verbs), "winetricks".into(), cx);
                        }),
                    ),
                ),
        ))
        .child(widgets::divider())
        .child(widgets::setting_row(
            self.tr("Run a file in the prefix", "Prefix'te dosya çalıştır"),
            self.tr(
                "An installer, patch or .reg file (.exe, .msi, .bat, .reg).",
                "Bir kurulum, yama ya da .reg dosyası (.exe, .msi, .bat, .reg).",
            ),
            btn("tool-run", self.tr("Choose…", "Seç…"), "folder-open").on_click(cx.listener(
                |this, _, _, cx| {
                    if this.game_ui.tool_busy.is_some() {
                        return;
                    }
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
                },
            )),
        ))
        .child(widgets::divider())
        .child(widgets::setting_row(
            self.tr("Prefix folder", "Prefix klasörü"),
            self.tr("Its C: drive is in drive_c.", "C: sürücüsü drive_c içinde."),
            btn("tool-open", self.tr("Open", "Aç"), "folder-open").on_click(cx.listener(
                |this, _, _, _| {
                    if let Some(g) = this.entry() {
                        let p = g.effective_prefix(&this.global.prefixes_dir());
                        let _ = std::process::Command::new("xdg-open").arg(p).spawn();
                    }
                },
            )),
        ))
        .child(widgets::divider())
        .child(widgets::setting_row(
            self.tr("Stop everything", "Her şeyi durdur"),
            self.tr(
                "Ends every program in the prefix (wineserver -k), e.g. a stuck launcher.",
                "Prefix'teki her programı kapatır (wineserver -k), ör. takılan bir başlatıcı.",
            ),
            style::button("tool-kill", self.tr("Stop", "Durdur"), false)
                .child(style::icon("square", 12., pal::danger()))
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
        ))
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
        use super::style::{self, pal};
        let log = paths::launch_log(id).ok();
        let text = log
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        let tail: Vec<&str> = {
            let mut v: Vec<&str> = text.lines().rev().take(60).collect();
            v.reverse();
            v
        };
        let when = log
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|d| ago(d.as_secs(), self.global.language));
        let mut lines = div()
            .id("log-lines")
            .flex()
            .flex_col()
            .max_h(px(420.))
            .overflow_y_scroll()
            .p_3()
            .rounded(px(10.))
            .bg(pal::bg().opacity(0.6))
            .border_1()
            .border_color(pal::glass_spec().border)
            .text_xs()
            .font_family("monospace");
        if tail.is_empty() {
            lines = lines.child(widgets::label(self.tr(
                "No log yet: start the game once.",
                "Henüz kayıt yok: oyunu bir kez başlat.",
            )));
        }
        for line in tail {
            let lower = line.to_lowercase();
            let color =
                if lower.contains("error") || lower.contains("err:") || lower.contains("failed") {
                    pal::danger()
                } else if lower.contains("warn") {
                    pal::warning()
                } else {
                    pal::text_soft()
                };
            lines = lines.child(div().text_color(color).child(line.to_string()));
        }
        widgets::section(self.tr("Launch log", "Başlatma kaydı"))
            .child(widgets::setting_row(
                self.tr("Proton's own log", "Proton'un kendi kaydı"),
                self.tr(
                    "Much more detail (PROTON_LOG) for hard problems; makes the game slower, so turn it off again.",
                    "Zor sorunlar için çok daha fazla ayrıntı (PROTON_LOG); oyunu yavaşlatır, sonra kapat.",
                ),
                widgets::switch(self.profile.proton_log)
                    .id("proton-log")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.profile.proton_log = !this.profile.proton_log;
                        this.save();
                        cx.notify();
                    })),
            ))
            .child(widgets::divider())
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div().flex_1().child(widgets::label(match when {
                            Some(w) => format!("{} {w}", self.tr("Last start:", "Son başlatma:")),
                            None => self.tr("Last 60 lines of the last start.", "Son başlatmanın son 60 satırı.").to_string(),
                        })),
                    )
                    .child(
                        style::button("log-refresh", self.tr("Refresh", "Yenile"), false)
                            .child(style::icon("refresh-cw", 13., pal::muted()))
                            .on_click(cx.listener(|_, _, _, cx| cx.notify())),
                    )
                    .child(
                        style::button("log-open", self.tr("Open folder", "Klasörü aç"), false)
                            .child(style::icon("folder-open", 13., pal::muted()))
                            .on_click(cx.listener(|this, _, _, _| {
                                if let Some(dir) = this.log_dir() {
                                    let _ = std::fs::create_dir_all(&dir);
                                    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                                }
                            })),
                    )
                    .child(
                        style::button("log-clear", self.tr("Clear", "Temizle"), false)
                            .child(style::icon("trash", 13., pal::muted()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let (Some(dir), Some(id)) = (this.log_dir(), this.selected_id()) {
                                    clear_logs(&dir, &id.file_stem());
                                }
                                this.status = Some(this.tr("Logs cleared", "Kayıtlar temizlendi").into());
                                cx.notify();
                            })),
                    ),
            )
            .child(lines)
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
fn spawn_launch(
    id: &GameId,
    console: Option<(u32, u32)>,
    trial: arkor_launch::trouble::Trial,
) -> anyhow::Result<(std::process::Child, std::path::PathBuf)> {
    use std::os::unix::process::CommandExt;
    let log = paths::launch_log(id)?;
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out = std::fs::File::create(&log)?;
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    if let Some((w, h)) = console.filter(|(w, h)| *w > 0 && *h > 0) {
        // Started from console mode: kept full screen (see `arkor_launch::run`).
        cmd.env("ARKOR_CONSOLE_SIZE", format!("{w}x{h}"));
        arkor_launch::note_console_launch(id, (w, h));
    }
    if let Some(text) = trial.text() {
        cmd.env(arkor_launch::trouble::TRIAL_ENV, text);
    }
    let child = cmd
        .arg("launch")
        .arg(id.to_string())
        // They were looked at here (see `saves_first`).
        .env("ARKOR_SAVES_DONE", "1")
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

/// The line of a launch log that says what went wrong: the last error, else the last line.
pub(super) fn error_line(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines
        .iter()
        .rev()
        .find(|l| {
            let l = l.to_lowercase();
            l.starts_with("arkor:") || l.contains("error") || l.contains("failed")
        })
        .or(lines.last())
        .map(|l| l.to_string())
}

/// The last lines of a launch log.
pub(super) fn log_tail(path: &std::path::Path, n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

fn which(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// Starts a library game outside the window (tray menu): own process group, output to its log.
pub(super) fn launch_detached(id: &str) {
    if let Ok(id) = id.parse::<GameId>() {
        let _ = spawn_launch(&id, None, Default::default());
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
    pub(super) fn refresh_cover(&mut self, id: &str) {
        if let Some(g) = self.library.get(id) {
            let path = arkor_launch::covers::cover_for(g);
            // A new picture can have the same file name as the old one.
            for p in [path.as_ref(), self.covers.get(id).and_then(Option::as_ref)]
                .into_iter()
                .flatten()
            {
                super::cover_art::forget(p);
            }
            self.covers.insert(id.to_string(), path);
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
        if std::mem::take(&mut self.sgdb_on_open) {
            self.sgdb_open(id.to_string(), name.to_string(), cx);
        }
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
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(self.tr("Cover", "Kapak")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                super::style::button(
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
                            .child(
                                super::style::button("cover-sgdb", "SteamGridDB", false)
                                    .child(super::style::icon(
                                        "images",
                                        13.,
                                        super::style::pal::muted(),
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.sgdb_open(id_sgdb.clone(), name_sgdb.clone(), cx)
                                    })),
                            )
                            .child(
                                super::style::button(
                                    "cover-reset",
                                    self.tr("Reset", "Sıfırla"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        arkor_launch::covers::reset(&id_reset);
                                        this.refresh_cover(&id_reset);
                                        cx.notify();
                                    },
                                )),
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
                super::style::button(
                    "sc-menu",
                    self.tr("Add to app menu", "Uygulama menüsüne ekle"),
                    in_menu,
                )
                .on_click(cx.listener(move |this, _, _, cx| shortcut(this, false, cx))),
            )
            .child(
                super::style::button(
                    "sc-desktop",
                    self.tr("Add to desktop", "Masaüstüne ekle"),
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| shortcut(this, true, cx))),
            );
        if in_menu {
            buttons = buttons.child(
                super::style::button(
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
                super::style::button("sc-steam", self.tr("Add to Steam", "Steam'e ekle"), false)
                    .on_click(cx.listener(|this, _, _, cx| {
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
                    })),
            );
        }
        let copy = command.clone();
        Some(
            widgets::section(self.tr("Shortcuts", "Kısayollar"))
                .child(widgets::setting_row(
                    self.tr("Start it from outside Arkor", "Arkor dışından başlat"),
                    if in_menu {
                        self.tr("It is in the app menu.", "Uygulama menüsünde var.")
                    } else {
                        self.tr(
                            "App menu, desktop or Steam (as a non-Steam game).",
                            "Uygulama menüsü, masaüstü ya da Steam (Steam dışı oyun olarak).",
                        )
                    },
                    buttons,
                ))
                .child(widgets::divider())
                .child(widgets::setting_row(
                    self.tr("Launch command", "Başlatma komutu"),
                    self.tr(
                        "For Heroic, Lutris or scripts; Arkor applies this game's settings.",
                        "Heroic, Lutris ya da betikler için; Arkor bu oyunun ayarlarını uygular.",
                    ),
                    super::style::button("sc-copy", self.tr("Copy", "Kopyala"), false)
                        .child(super::style::icon("copy", 13., super::style::pal::muted()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                            this.status = Some(this.tr("Copied.", "Kopyalandı.").into());
                            cx.notify();
                        })),
                ))
                .child(code_box(command)),
        )
    }
}
