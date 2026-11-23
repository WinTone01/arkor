//! "Add game": add a game by hand (like Faugus), pick one Arkor found in other launchers,
//! import one from an existing Wine prefix, or install a third-party launcher.

use std::path::PathBuf;

use arkor_core::library::{GameKind, NewGame};
use arkor_core::{GameId, Store};
use arkor_launch::launchers::{self, LAUNCHERS};
use arkor_launch::start::{self, PrefixTool};
use arkor_stores::Detected;
use arkor_stores::prefix::{self, Candidate};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, PathPromptOptions, Window, div, prelude::*, px};

use super::Library;
use super::widgets::{self, theme::*};

/// Sections of the Add game page, most used first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum AddTab {
    #[default]
    Found,
    Manual,
    Prefix,
    Launcher,
}

impl AddTab {
    const ALL: [AddTab; 4] = [
        AddTab::Found,
        AddTab::Manual,
        AddTab::Prefix,
        AddTab::Launcher,
    ];
}

#[derive(Default)]
pub(super) struct AddState {
    pub(super) tab: AddTab,
    kind_linux: bool,
    exe: Option<PathBuf>,
    prefix: Option<PathBuf>,
    detected: Option<Vec<Detected>>,
    detect_errors: Vec<String>,
    detecting: bool,
    import_prefix: Option<PathBuf>,
    candidates: Vec<Candidate>,
    scanning: bool,
    error: Option<String>,
}

impl Library {
    pub(super) fn prompt_path(
        &mut self,
        directories: bool,
        title: &'static str,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, PathBuf, &mut Context<Self>) + 'static,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: Some(title.into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                then(this, path, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Adds `new` to the library, saves, and opens the new game's page.
    pub(super) fn add_to_library(
        &mut self,
        new: NewGame,
        cx: &mut Context<Self>,
    ) -> Option<GameId> {
        let id = self.library.add(new);
        let result = self.library.save();
        if let Err(e) = result {
            self.add.error = Some(format!("{e:#}"));
            cx.notify();
            return None;
        }
        self.add = AddState {
            detected: self.add.detected.take(),
            ..AddState::default()
        };
        let id = GameId::new(Store::Local, id);
        // New games start with the defaults from Settings → Game defaults.
        if self.global.defaults != Default::default() {
            let mut profile = arkor_core::Profile::load(&id).unwrap_or_default();
            self.global.defaults.apply_to(&mut profile);
            if let Err(e) = profile.save(&id) {
                self.add.error = Some(format!("{e:#}"));
            }
        }
        self.reload_games();
        self.select_game(&id, cx);
        // A cover from SteamGridDB when it knows the game by exactly this name.
        self.auto_cover(&id.id, cx);
        Some(id)
    }

    fn add_manual(&mut self, cx: &mut Context<Self>) {
        let name = self.field_text("add-name", cx).unwrap_or_default();
        let args = self.field_text("add-args", cx).unwrap_or_default();
        let Some(exe) = self.add.exe.clone() else {
            return;
        };
        let name = if name.trim().is_empty() {
            prefix::suggested_name(&exe)
        } else {
            name
        };
        let mut new = NewGame::windows(name, exe);
        if self.add.kind_linux {
            new.kind = GameKind::Linux;
        } else {
            new.prefix = self.add.prefix.clone();
        }
        new.args = arkor_core::profile::shell_words(&args);
        self.add_to_library(new, cx);
    }

    fn detect_games(&mut self, cx: &mut Context<Self>) {
        self.add.detecting = true;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let (found, errors) = bg.spawn(async move { arkor_stores::detect() }).await;
            let _ = this.update(cx, |this, cx| {
                this.add.detecting = false;
                this.add.detected = Some(found);
                this.add.detect_errors = errors.iter().map(|e| format!("{e:#}")).collect();
                cx.notify();
            });
        })
        .detach();
    }

    fn scan_prefix(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.add.import_prefix = Some(root.clone());
        self.add.candidates.clear();
        self.add.scanning = true;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let scan_root = root.clone();
            let candidates = bg
                .spawn(async move { prefix::find_executables(&scan_root) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.add.import_prefix.as_ref() == Some(&root) {
                    this.add.candidates = candidates;
                    this.add.scanning = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn install_launcher(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(launcher) = launchers::find(id) else {
            return;
        };
        let prefixes = self.global.prefixes_dir();
        let mut new = NewGame::windows(launcher.name, PathBuf::new());
        // The library assigns the id; the exe lives in that game's default prefix.
        let game_id = self.library.add(new.clone());
        self.library.remove(&game_id);
        let prefix = prefixes.join(&game_id);
        new.exe = prefix.join(launcher.exe);
        let Some(gid) = self.add_to_library(new, cx) else {
            return;
        };
        self.status =
            Some(format!("{} {}…", self.tr("Installing", "Kuruluyor:"), launcher.name).into());
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let installer = launchers::fetch_installer(launcher)?;
                    start::run_prefix_tool(&gid, &PrefixTool::Run(installer))?;
                    anyhow::Ok(())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok(()) => format!("{} ✓", launcher.name).into(),
                    Err(e) => format!("{}: {e:#}", launcher.name).into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_add(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        if self.add.detected.is_none() && !self.add.detecting {
            self.detect_games(cx);
        }
        let name = self.field(
            "add-name",
            |_| String::new(),
            self.tr(
                "Name (optional: taken from the file)",
                "Ad (isteğe bağlı: dosyadan alınır)",
            ),
            cx,
            |_, _| {},
        );
        let args = self.field(
            "add-args",
            |_| String::new(),
            self.tr(
                "Arguments, e.g. -dx12 -skipintro",
                "Argümanlar, ör. -dx12 -skipintro",
            ),
            cx,
            |_, _| {},
        );
        use super::style::{self, pal};
        let state = &self.add;

        // By hand
        let kind_chip = |ix: usize, linux: bool, text: &'static str, cx: &mut Context<Self>| {
            widgets::chip(("add-kind", ix), text, state.kind_linux == linux).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.add.kind_linux = linux;
                    cx.notify();
                },
            ))
        };
        let exe_text = state
            .exe
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| {
                self.tr("No file chosen yet", "Henüz dosya seçilmedi")
                    .into()
            });
        let prefix_text = match &state.prefix {
            Some(p) => p.display().to_string(),
            None => self
                .tr(
                    "A new prefix just for this game (default)",
                    "Sadece bu oyun için yeni bir prefix (varsayılan)",
                )
                .into(),
        };
        let mut manual = widgets::section(self.tr("Add by hand", "Elle ekle"))
            .child(widgets::setting_row(
                self.tr("1. Type", "1. Tür"),
                self.tr(
                    "Windows games run through Proton.",
                    "Windows oyunları Proton ile çalışır.",
                ),
                div()
                    .flex()
                    .gap_2()
                    .child(kind_chip(
                        0,
                        false,
                        self.tr("Windows game", "Windows oyunu"),
                        cx,
                    ))
                    .child(kind_chip(1, true, self.tr("Linux game", "Linux oyunu"), cx)),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("2. Executable", "2. Çalıştırılabilir dosya"),
                exe_text,
                style::button("add-exe", self.tr("Choose…", "Seç…"), state.exe.is_none())
                    .child(style::icon(
                        "folder-open",
                        13.,
                        if state.exe.is_none() {
                            pal::on_accent()
                        } else {
                            pal::muted()
                        },
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let title = this.tr(
                            "Choose the game's executable",
                            "Oyunun çalıştırılabilir dosyasını seç",
                        );
                        this.prompt_path(false, title, cx, |this, path, _| {
                            // A program inside a prefix (e.g. another launcher's) runs there.
                            if this.add.prefix.is_none() {
                                this.add.prefix = arkor_core::library::prefix_containing(&path);
                            }
                            this.add.exe = Some(path)
                        });
                    })),
            ));
        if !state.kind_linux {
            manual = manual.child(widgets::divider()).child(widgets::setting_row(
                self.tr("Wine prefix", "Wine prefix"),
                prefix_text,
                div()
                    .flex()
                    .gap_2()
                    .child(
                        style::button(
                            "add-prefix",
                            self.tr("Use existing…", "Mevcut olanı kullan…"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            let title = this.tr("Choose a Wine prefix", "Bir Wine prefix seç");
                            this.prompt_path(true, title, cx, |this, path, _| {
                                this.add.prefix = Some(prefix::resolve(&path).unwrap_or(path))
                            });
                        })),
                    )
                    .when(state.prefix.is_some(), |d| {
                        d.child(
                            style::button(
                                "add-prefix-default",
                                self.tr("Default", "Varsayılan"),
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.add.prefix = None;
                                cx.notify();
                            })),
                        )
                    }),
            ));
        }
        manual = manual
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("Name", "Ad"),
                self.tr(
                    "Taken from the file when empty.",
                    "Boş bırakılırsa dosyadan alınır.",
                ),
                div().w(px(300.)).child(name),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("Arguments", "Argümanlar"),
                self.tr(
                    "Passed to the game, optional.",
                    "Oyuna verilir, isteğe bağlı.",
                ),
                div().w(px(300.)).child(args),
            ))
            .child(div().flex().pt_1().child(if state.exe.is_some() {
                widgets::primary_button(
                    "add-confirm",
                    self.tr("Add to library", "Kütüphaneye ekle"),
                )
                .child(style::icon("plus", 14., pal::on_accent()))
                .on_click(cx.listener(|this, _, _, cx| this.add_manual(cx)))
            } else {
                widgets::primary_button(
                    "add-confirm",
                    self.tr("Choose the executable first", "Önce dosyayı seç"),
                )
                .opacity(0.45)
                .cursor_default()
            }));

        // 2. Found in other launchers
        let mut found =
            widgets::section(self.tr("Found on this computer", "Bu bilgisayarda bulunanlar"));
        if state.detecting {
            found = found.child(widgets::label(self.tr("Looking…", "Aranıyor…")));
        }
        let mut any = false;
        let mut found_count = 0usize;
        for (ix, d) in state.detected.iter().flatten().enumerate() {
            let origin = d.game.id.to_string();
            if self.library.has_origin(&origin) {
                continue;
            }
            any = true;
            found_count += 1;
            let source = format!("{} · {}", d.game.id.store.as_str(), d.game.id.id);
            let row = div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(super::style::pal::raised().opacity(0.45))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(div().truncate().child(d.game.name.clone()))
                        .child(widgets::label(source)),
                );
            found = found.child(match &d.new {
                Ok(new) => {
                    let new = new.clone();
                    row.child(
                        style::button(("found-add", ix), self.tr("Add", "Ekle"), false)
                            .child(style::icon("plus", 13., pal::muted()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.add_to_library(new.clone(), cx);
                            })),
                    )
                }
                Err(why) => row.child(
                    div()
                        .max_w(gpui::px(300.))
                        .truncate()
                        .child(widgets::label(why.clone())),
                ),
            });
        }
        if state.detected.is_some() && !any {
            found = found.child(widgets::label(
                self.tr("Nothing new found.", "Yeni bir şey bulunamadı."),
            ));
        }
        for e in &state.detect_errors {
            found = found.child(widgets::label(format!("⚠ {e}")));
        }
        found = found.child(
            div().flex().child(
                widgets::chip(
                    "detect-again",
                    self.tr("Search again", "Yeniden ara"),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| this.detect_games(cx))),
            ),
        );

        // 3. From a prefix
        let mut import =
            widgets::section(self.tr("Import from a Wine prefix", "Wine prefix'ten içe aktar"))
                .child(widgets::label(self.t().import_intro))
                .child(div().flex().child(
                    widgets::chip("import-choose", self.t().choose_prefix, false).on_click(
                        cx.listener(|this, _, _, cx| {
                            let title = this.t().choose_prefix;
                            this.prompt_path(
                                true,
                                title,
                                cx,
                                |this, path, cx| match prefix::resolve(&path) {
                                    Some(root) => this.scan_prefix(root, cx),
                                    None => {
                                        this.add.error = Some(format!(
                                            "{}: {}",
                                            this.t().not_a_prefix,
                                            path.display()
                                        ))
                                    }
                                },
                            );
                        }),
                    ),
                ));
        if let Some(root) = state.import_prefix.clone() {
            import = import.child(widgets::label(format!(
                "{}: {}",
                self.t().prefix_label,
                root.display()
            )));
            if state.scanning {
                import = import.child(widgets::label(self.t().scanning));
            } else if state.candidates.is_empty() {
                import = import.child(widgets::label(self.t().no_exes_found));
            }
            for (ix, c) in state.candidates.iter().enumerate() {
                let rel = c
                    .path
                    .strip_prefix(root.join("drive_c"))
                    .unwrap_or(&c.path)
                    .display()
                    .to_string();
                let size = if c.size > 0 {
                    prefix::human_size(c.size)
                } else {
                    String::new()
                };
                let (exe, pfx) = (c.path.clone(), root.clone());
                import = import.child(
                    div()
                        .id(("candidate", ix))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(super::style::pal::raised().opacity(0.45))
                        .cursor_pointer()
                        .hover(|s| s.bg(ROW_HOVER()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let mut new =
                                NewGame::windows(prefix::suggested_name(&exe), exe.clone());
                            new.prefix = Some(pfx.clone());
                            this.add_to_library(new, cx);
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(div().truncate().child(prefix::suggested_name(&c.path)))
                                .child(div().truncate().child(widgets::label(rel))),
                        )
                        .child(div().flex_none().child(widgets::label(size))),
                );
            }
        }

        // 4. Launchers
        let mut launchers_row = div().flex().flex_wrap().gap_2();
        for (ix, l) in LAUNCHERS.iter().enumerate() {
            let id = l.id;
            launchers_row =
                launchers_row
                    .child(widgets::chip(("launcher", ix), l.name, false).on_click(
                        cx.listener(move |this, _, _, cx| this.install_launcher(id, cx)),
                    ));
        }
        let launchers_card =
            widgets::section(self.tr("Install a launcher", "Başlatıcı kur")).child(launchers_row);

        let tab = self.add.tab;
        let mut entries = vec![
            style::NavEntry::new(
                "radar",
                self.tr("Found on this computer", "Bu bilgisayarda bulunanlar"),
            ),
            style::NavEntry::new("file-plus", self.tr("Add by hand", "Elle ekle")),
            style::NavEntry::new("wine", self.tr("From a Wine prefix", "Wine prefix'ten")),
            style::NavEntry::new("download", self.tr("Install a launcher", "Başlatıcı kur")),
        ];
        if found_count > 0 {
            entries[0].badge = Some(found_count.to_string());
        }
        let nav = style::side_nav(
            "add-tab",
            entries,
            AddTab::ALL.iter().position(|t| *t == tab),
            cx,
            |this, i, cx| {
                this.add.tab = AddTab::ALL[i];
                this.add.error = None;
                cx.notify();
            },
        );
        let mut content = div().flex().flex_col().gap_4();
        if let Some(e) = &self.add.error {
            content = content.child(div().text_sm().text_color(pal::warning()).child(e.clone()));
        }
        content = content.child(match tab {
            AddTab::Found => found,
            AddTab::Manual => manual,
            AddTab::Prefix => import,
            AddTab::Launcher => launchers_card,
        });
        div()
            .child(
                div().flex().child(
                    style::back_link("add-back", self.tr("Library", "Kütüphane"))
                        .on_click(cx.listener(|this, _, _, cx| this.go(super::shell::Nav::Library, cx))),
                ),
            )
            .child(Self::header(
                self.tr("Add game", "Oyun ekle"),
                self.tr(
                    "Arkor only lists the games you add. Pick one it found, or add one yourself.",
                    "Arkor sadece senin eklediğin oyunları listeler. Bulduklarından birini seç ya da kendin ekle.",
                ),
            ))
            .child(style::with_side_nav(nav, content))
    }
}
