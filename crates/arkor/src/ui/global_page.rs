//! Global settings page, plus the environment-variable editor shared with game pages.

use std::collections::BTreeMap;

use gpui::{Context, Div, PathPromptOptions, Window, div, prelude::*, rgb};

use super::widgets::{self, theme::*};
use super::{Library, i18n};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum EnvScope {
    Global,
    Game,
}

impl Library {
    fn env_map(&mut self, scope: EnvScope) -> &mut BTreeMap<String, String> {
        match scope {
            EnvScope::Global => &mut self.global.env,
            EnvScope::Game => &mut self.profile.env,
        }
    }

    /// Rows of KEY=VALUE with remove buttons and a field to add more (Enter adds).
    pub(super) fn env_editor(&mut self, scope: EnvScope, cx: &mut Context<Self>) -> Div {
        let placeholder = self.tr("KEY=VALUE, then Enter", "ANAHTAR=DEĞER, sonra Enter");
        let key = match scope {
            EnvScope::Global => "env-new-global",
            EnvScope::Game => "env-new-game",
        };
        let add = self.submit_field(key, placeholder, cx, move |this, text, _| {
            let text = text.trim();
            if let Some((k, v)) = text.split_once('=')
                && !k.trim().is_empty()
            {
                let (k, v) = (k.trim().to_string(), v.trim().to_string());
                this.env_map(scope).insert(k, v);
                this.save();
            }
        });
        let vars: Vec<(String, String)> = match scope {
            EnvScope::Global => self.global.env.clone(),
            EnvScope::Game => self.profile.env.clone(),
        }
        .into_iter()
        .collect();
        let mut rows = div().flex().flex_col().gap_1();
        for (ix, (k, v)) in vars.into_iter().enumerate() {
            let key_to_remove = k.clone();
            rows =
                rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(rgb(BG()))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_sm()
                                .child(format!("{k}={v}")),
                        )
                        .child(widgets::chip(("env-remove", ix), "✕", false).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.env_map(scope).remove(&key_to_remove);
                                this.save();
                                cx.notify();
                            }),
                        )),
                );
        }
        div().flex().flex_col().gap_2().child(rows).child(add)
    }

    fn choose_prefixes_dir(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(
                self.tr("Choose the prefixes folder", "Prefix klasörünü seç")
                    .into(),
            ),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.global.prefixes_dir = paths.into_iter().next();
                this.save();
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_global(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        let hud = self.global.mangohud.settings.clone();
        if self.global_hud_tab {
            return div()
                .child(Self::header(t.global_settings, t.global_subtitle))
                .child(self.global_tabs(cx))
                .child(
                    widgets::switch_row(
                        "global-hud",
                        "MangoHud",
                        t.hud_default_hint,
                        self.global.mangohud.enabled,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.global.mangohud.enabled = !this.global.mangohud.enabled;
                        this.save();
                        cx.notify();
                    })),
                )
                .child(self.render_hud_editor(&hud, cx));
        }
        let mut languages = div().flex().gap_2();
        for (ix, (lang, name)) in i18n::LANGUAGES.into_iter().enumerate() {
            languages = languages.child(
                widgets::chip(("lang", ix), name, self.global.language == lang).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.global.language = lang;
                        this.save();
                        this.fields.clear();
                        cx.notify();
                    }),
                ),
            );
        }

        let prefixes = self.global.prefixes_dir();
        let custom_prefixes = self.global.prefixes_dir.is_some();
        let discord_id = self.field(
            "discord-app-id",
            |this| this.global.discord.app_id.clone(),
            "123456789012345678",
            cx,
            |this, text| {
                this.global.discord.app_id = text.trim().to_string();
                this.save();
            },
        );
        let env = self.env_editor(super::global_page::EnvScope::Global, cx);
        let sgdb = self.field(
            "sgdb-key",
            |this| this.global.steamgriddb_key.clone(),
            self.tr("SteamGridDB API key", "SteamGridDB API anahtarı"),
            cx,
            |this, text| {
                this.global.steamgriddb_key = text.trim().to_string();
                this.save();
            },
        );

        div()
            .child(Self::header(t.global_settings, t.global_subtitle))
            .child(self.global_tabs(cx))
            .child(widgets::section(t.language).child(languages))
            .child(self.render_appearance(cx))
            .child(
                widgets::section(self.tr("Games", "Oyunlar"))
                    .child(widgets::label(self.tr(
                        "New Windows games get their own prefix in this folder:",
                        "Yeni Windows oyunları bu klasörde kendi prefix'lerini alır:",
                    )))
                    .child(div().text_sm().child(prefixes.display().to_string()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                widgets::chip("prefixes-choose", self.tr("Change…", "Değiştir…"), false)
                                    .on_click(cx.listener(|this, _, _, cx| this.choose_prefixes_dir(cx))),
                            )
                            .when(custom_prefixes, |d| {
                                d.child(widgets::chip("prefixes-default", self.tr("Use default", "Varsayılanı kullan"), false).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.global.prefixes_dir = None;
                                        this.save();
                                        cx.notify();
                                    }),
                                ))
                            }),
                    )
                    .child(
                        widgets::switch_row(
                            "tray",
                            self.tr("System tray icon", "Sistem tepsisi simgesi"),
                            self.tr("Closing the window keeps Arkor in the tray (applies on next start)", "Pencereyi kapatınca Arkor tepside kalır (sonraki açılışta geçerli)"),
                            self.global.tray,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.global.tray = !this.global.tray;
                            this.save();
                            cx.notify();
                        })),
                    )
                    .child(
                        widgets::switch_row(
                            "autostart",
                            self.tr("Start with the system", "Sistemle başlat"),
                            self.tr("Starts minimized to the tray", "Tepside küçültülmüş başlar"),
                            self.global.autostart,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.global.autostart = !this.global.autostart;
                            let result = super::tray::set_autostart(this.global.autostart);
                            this.save();
                            if result.is_err() {
                                this.report(result);
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        widgets::switch_row(
                            "splash",
                            self.tr("Splash window while starting", "Başlarken açılış penceresi"),
                            self.tr("A small window for a few seconds", "Birkaç saniyelik küçük bir pencere"),
                            self.global.splash,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.global.splash = !this.global.splash;
                            this.save();
                            cx.notify();
                        })),
                    )
                    .child(
                        widgets::switch_row(
                            "close-on-launch",
                            self.tr("Close Arkor when a game starts", "Oyun açılınca Arkor'u kapat"),
                            self.tr("The game keeps running", "Oyun çalışmaya devam eder"),
                            self.global.close_on_launch,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.global.close_on_launch = !this.global.close_on_launch;
                            this.save();
                            cx.notify();
                        })),
                    ),
            )
            .child(
                widgets::section(self.tr("Environment variables (all games)", "Ortam değişkenleri (tüm oyunlar)"))
                    .child(widgets::label(self.tr(
                        "A game's own variables override these.",
                        "Oyunun kendi değişkenleri bunları ezer.",
                    )))
                    .child(env),
            )
            .child(self.render_backup(cx))
            .child(
                widgets::section(self.tr("Cover art", "Kapak resimleri"))
                    .child(widgets::label(self.tr(
                        "Free key from steamgriddb.com → Preferences → API; used by the SteamGridDB button on game pages.",
                        "steamgriddb.com → Preferences → API'den ücretsiz anahtar; oyun sayfalarındaki SteamGridDB düğmesi kullanır.",
                    )))
                    .child(sgdb),
            )
            .child(
                widgets::section("Discord")
                    .child(
                        widgets::switch_row(
                            "discord",
                            self.tr("Show what you play on Discord", "Ne oynadığını Discord'da göster"),
                            self.tr("Needs the Discord app running", "Discord uygulamasının açık olması gerekir"),
                            self.global.discord.enabled,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.global.discord.enabled = !this.global.discord.enabled;
                            this.save();
                            cx.notify();
                        })),
                    )
                    .child(widgets::label(self.tr(
                        "Application ID (create an application at discord.com/developers; its name is what Discord shows):",
                        "Uygulama kimliği (discord.com/developers'ta bir uygulama oluştur; Discord onun adını gösterir):",
                    )))
                    .child(discord_id),
            )
    }

    /// Applies the theme and accent from the settings (System follows the window's appearance).
    pub(super) fn apply_theme(&self, window: &Window) {
        use arkor_core::profile::Theme;
        let a = &self.global.appearance;
        let light = match a.theme {
            Theme::Dark => false,
            Theme::Light => true,
            Theme::System => matches!(
                window.appearance(),
                gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
            ),
        };
        widgets::theme::set(light, a.accent.unwrap_or(widgets::theme::DEFAULT_ACCENT));
    }

    pub(super) fn sound(&self, sound: super::sounds::Sound) {
        if self.global.appearance.sounds {
            super::sounds::play(sound);
        }
    }

    pub(super) fn grid_size_chips(&self, cx: &mut Context<Self>) -> Div {
        use arkor_core::profile::GridSize;
        let mut row = div().flex().gap_1();
        for (ix, size) in GridSize::ALL.into_iter().enumerate() {
            let label = match size {
                GridSize::Small => self.tr("S", "K"),
                GridSize::Medium => self.tr("M", "O"),
                GridSize::Large => self.tr("L", "B"),
            };
            row = row.child(
                widgets::chip(
                    ("grid-size", ix),
                    label,
                    self.global.appearance.grid_size == size,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.global.appearance.grid_size = size;
                    // Also used on the library page, where `save` would skip the settings.
                    let saved = this.global.save();
                    this.report(saved);
                    cx.notify();
                })),
            );
        }
        row
    }

    fn render_appearance(&mut self, cx: &mut Context<Self>) -> Div {
        use arkor_core::profile::Theme;
        let a = self.global.appearance.clone();
        let mut themes = div().flex().gap_2();
        for (ix, (theme, label)) in [
            (Theme::Dark, self.tr("Dark", "Koyu")),
            (Theme::Light, self.tr("Light", "Açık")),
            (Theme::System, self.tr("Follow the system", "Sisteme uy")),
        ]
        .into_iter()
        .enumerate()
        {
            themes = themes.child(
                widgets::chip(("theme", ix), label, a.theme == theme).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.global.appearance.theme = theme;
                        this.save();
                        cx.notify();
                    },
                )),
            );
        }
        let current = a.accent.unwrap_or(widgets::theme::DEFAULT_ACCENT);
        let mut accents = div().flex().flex_wrap().gap_2();
        for (ix, (color, en, tr)) in widgets::theme::ACCENTS.into_iter().enumerate() {
            accents = accents.child(
                div()
                    .id(("accent", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .border_1()
                    .border_color(rgb(if current == color { color } else { BORDER() }))
                    .child(div().size_3().rounded_full().bg(rgb(color)))
                    .child(div().text_sm().child(self.tr(en, tr)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.global.appearance.accent =
                            (color != widgets::theme::DEFAULT_ACCENT).then_some(color);
                        this.save();
                        cx.notify();
                    })),
            );
        }
        widgets::section(self.tr("Appearance", "Görünüm"))
            .child(widgets::label(self.tr("Theme", "Tema")))
            .child(themes)
            .child(widgets::label(self.tr("Accent colour", "Vurgu rengi")))
            .child(accents)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(widgets::label(
                        self.tr("Library cover size", "Kütüphane kapak boyutu"),
                    ))
                    .child(self.grid_size_chips(cx)),
            )
            .child(
                widgets::switch_row(
                    "sounds",
                    self.tr("Sounds", "Sesler"),
                    self.tr(
                        "When a game starts or stops, a download finishes, or something fails",
                        "Oyun başlayınca ya da kapanınca, indirme bitince ya da bir hata olunca",
                    ),
                    a.sounds,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.global.appearance.sounds = !this.global.appearance.sounds;
                    this.save();
                    if this.global.appearance.sounds {
                        super::sounds::play(super::sounds::Sound::Done);
                    }
                    cx.notify();
                })),
            )
    }

    fn global_tabs(&self, cx: &mut Context<Self>) -> Div {
        let tab = |ix: usize, hud: bool, label: &'static str, cx: &mut Context<Self>| {
            widgets::chip(("global-tab", ix), label, self.global_hud_tab == hud).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.global_hud_tab = hud;
                    cx.notify();
                }),
            )
        };
        div()
            .flex()
            .gap_2()
            .child(tab(0, false, self.tr("General", "Genel"), cx))
            .child(tab(1, true, "MangoHud", cx))
    }
}

impl Library {
    /// Prefixes Arkor created for library games (the only ones a backup may include).
    fn own_prefixes(&self) -> Vec<std::path::PathBuf> {
        let dir = self.global.prefixes_dir();
        self.library
            .games
            .iter()
            .filter(|g| g.needs_prefix())
            .map(|g| g.effective_prefix(&dir))
            .filter(|p| p.starts_with(&dir) && p.exists())
            .collect()
    }

    fn backup_now(&mut self, cx: &mut Context<Self>) {
        let dest = self.global.backup.dest_dir();
        let prefixes = if self.backup_prefixes {
            self.own_prefixes()
        } else {
            Vec::new()
        };
        self.status = Some(self.tr("Backing up…", "Yedekleniyor…").into());
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move { arkor_core::backup::create(&dest, &prefixes, "manual") })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok((file, _)) => format!(
                        "{} {}",
                        this.tr("Backup written:", "Yedek yazıldı:"),
                        file.display()
                    )
                    .into(),
                    Err(e) => format!("{e:#}").into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn render_backup(&mut self, cx: &mut Context<Self>) -> Div {
        let b = self.global.backup.clone();
        let every = |ix: usize, days: u32, label: &'static str, cx: &mut Context<Self>| {
            widgets::chip(("backup-every", ix), label, b.every_days == days).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.global.backup.every_days = days;
                    this.save();
                    cx.notify();
                },
            ))
        };
        let mut card = widgets::section(self.tr("Backup", "Yedekleme"))
            .child(widgets::label(self.tr(
                "Settings, library, profiles, play time and covers in one .tar.gz. Prefixes Arkor created can be included.",
                "Ayarlar, kütüphane, profiller, oynama süreleri ve kapaklar tek bir .tar.gz dosyasında. Arkor'un oluşturduğu prefix'ler de eklenebilir.",
            )))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().truncate().text_sm().child(b.dest_dir().display().to_string()))
                    .child(widgets::chip("backup-dest", self.tr("Change…", "Değiştir…"), false).on_click(cx.listener(|this, _, _, cx| {
                        let title = this.tr("Choose the backup folder", "Yedek klasörünü seç");
                        this.prompt_path(true, title, cx, |this, path, _| {
                            this.global.backup.dest = Some(path);
                            this.save();
                        });
                    }))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(widgets::primary_button("backup-now", self.tr("Back up now", "Şimdi yedekle")).on_click(cx.listener(|this, _, _, cx| this.backup_now(cx))))
                    .child(widgets::chip("backup-prefixes", self.tr("Include Arkor's prefixes", "Arkor'un prefix'lerini dahil et"), self.backup_prefixes).on_click(cx.listener(|this, _, _, cx| {
                        this.backup_prefixes = !this.backup_prefixes;
                        cx.notify();
                    }))),
            )
            .child(widgets::label(self.tr("Automatic backup (when Arkor starts)", "Otomatik yedek (Arkor açılırken)")))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(every(0, 0, self.tr("Off", "Kapalı"), cx))
                    .child(every(1, 1, self.tr("Daily", "Günlük"), cx))
                    .child(every(2, 7, self.tr("Weekly", "Haftalık"), cx))
                    .child(every(3, 30, self.tr("Monthly", "Aylık"), cx)),
            );
        if b.every_days > 0 {
            card = card.child(widgets::stepper(
                self.tr("Keep", "Sakla"),
                b.keep.to_string(),
                widgets::step_button("keep-", "−").on_click(cx.listener(|this, _, _, cx| {
                    this.global.backup.keep = this.global.backup.keep.saturating_sub(1).max(1);
                    this.save();
                    cx.notify();
                })),
                widgets::step_button("keep+", "+").on_click(cx.listener(|this, _, _, cx| {
                    this.global.backup.keep = (this.global.backup.keep + 1).min(50);
                    this.save();
                    cx.notify();
                })),
            ));
        }
        match self.restore_pending.clone() {
            None => card.child(
                div().flex().child(
                    widgets::chip("backup-restore", self.tr("Restore…", "Geri yükle…"), false)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let title = this.tr("Choose an Arkor backup", "Bir Arkor yedeği seç");
                            this.prompt_path(false, title, cx, |this, path, _| {
                                this.restore_pending = Some(path)
                            });
                        })),
                ),
            ),
            Some(file) => card
                .child(div().text_sm().text_color(rgb(ACCENT())).child(format!(
                    "{} {} ",
                    self.tr(
                        "Replace the current settings and library with",
                        "Mevcut ayarları ve kütüphaneyi şununla değiştir:"
                    ),
                    file.display()
                )))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            widgets::chip("restore-yes", self.tr("Restore", "Geri yükle"), true)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let result = arkor_core::backup::restore(&file);
                                    this.restore_pending = None;
                                    if result.is_ok() {
                                        this.global = arkor_core::Global::load_or_import()
                                            .unwrap_or_default();
                                        this.reload_games();
                                    }
                                    this.report(result);
                                    cx.notify();
                                })),
                        )
                        .child(
                            widgets::chip("restore-no", self.tr("Cancel", "Vazgeç"), false)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.restore_pending = None;
                                    cx.notify();
                                })),
                        ),
                ),
        }
    }
}
