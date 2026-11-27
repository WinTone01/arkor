//! Global settings page, plus the environment-variable editor shared with game pages.

use std::collections::BTreeMap;

use gpui::{Context, Div, PathPromptOptions, Window, div, prelude::*, px};

use super::widgets::{self, theme::*};
use super::{Library, i18n};

/// Tabs of the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SettingsTab {
    #[default]
    General,
    Appearance,
    Defaults,
    Integrations,
    Backups,
    Shortcuts,
}

impl SettingsTab {
    pub(super) const ALL: [SettingsTab; 6] = [
        SettingsTab::General,
        SettingsTab::Appearance,
        SettingsTab::Defaults,
        SettingsTab::Integrations,
        SettingsTab::Backups,
        SettingsTab::Shortcuts,
    ];
}

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
                        .bg(super::style::pal::raised().opacity(0.45))
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

    pub(super) fn choose_prefixes_dir(&mut self, cx: &mut Context<Self>) {
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
                .child(Self::header(
                    self.tr("FPS overlay (MangoHud)", "FPS göstergesi (MangoHud)"),
                    self.tr(
                        "MangoHud shows FPS, temperatures and usage on top of games. This is the layout every game uses, unless it has its own (game page → Performance overlay).",
                        "MangoHud oyunların üstünde FPS, sıcaklık ve kullanım değerlerini gösterir. Bu, kendi düzeni olmayan her oyunun kullandığı düzendir (oyun sayfası → Performans göstergesi).",
                    ),
                ))
                .child(
                    widgets::switch_row(
                        "global-hud",
                        self.tr("Show MangoHud in games", "Oyunlarda MangoHud'u göster"),
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

        use super::style;
        let tab = self.settings_tab;
        let entries = vec![
            style::NavEntry::new("settings", self.tr("General", "Genel")),
            style::NavEntry::new("palette", self.tr("Appearance", "Görünüm")),
            style::NavEntry::new("gamepad-2", self.tr("Game defaults", "Oyun varsayılanları")),
            style::NavEntry::new("plug", self.tr("Integrations", "Entegrasyonlar")),
            style::NavEntry::new("archive", self.tr("Backups", "Yedekleme")),
            style::NavEntry::new(
                "keyboard",
                self.tr("Keyboard shortcuts", "Klavye kısayolları"),
            ),
        ];
        let nav = style::side_nav(
            "settings-tab",
            entries,
            Some(tab as usize),
            cx,
            |this, i, cx| {
                this.settings_tab = SettingsTab::ALL[i];
                cx.notify();
            },
        );
        let switch = |id: &'static str, on: bool, cx: &mut Context<Self>, f: fn(&mut Self)| {
            widgets::switch(on)
                .id(id)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    f(this);
                    this.save();
                    cx.notify();
                }))
        };
        let content = match tab {
            SettingsTab::General => div()
                .flex()
                .flex_col()
                .gap_4()
                .child(self.render_app_update(cx))
                .child(self.render_pin_settings(cx))
                .child(
                    widgets::section(self.tr("Language", "Dil")).child(widgets::setting_row(
                        self.tr("Interface language", "Arayüz dili"),
                        self.tr("Every page and message of Arkor.", "Arkor'un tüm sayfaları ve mesajları."),
                        languages,
                    )),
                )
                .child(
                    widgets::section(self.tr("Window and start-up", "Pencere ve açılış"))
                        .child(widgets::setting_row(
                            self.tr("Keep running in the tray", "Kapatınca tepside çalışsın"),
                            self.tr("Closing the window leaves Arkor running in the tray (applies on next start).", "Pencere kapanınca Arkor tepside çalışmaya devam eder (sonraki açılışta geçerli)."),
                            switch("tray", self.global.tray, cx, |this| this.global.tray = !this.global.tray),
                        ))
                        .child(widgets::divider())
                        .child(widgets::setting_row(
                            self.tr("Start with the system", "Sistemle başlat"),
                            self.tr("Starts minimized to the tray.", "Tepside küçültülmüş başlar."),
                            widgets::switch(self.global.autostart)
                                .id("autostart")
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.global.autostart = !this.global.autostart;
                                    let result = super::tray::set_autostart(this.global.autostart);
                                    this.save();
                                    if result.is_err() {
                                        this.report(result);
                                    }
                                    cx.notify();
                                })),
                        ))
                        .child(widgets::divider())
                        .child(widgets::setting_row(
                            self.tr("Start in console mode", "Konsol modunda başlat"),
                            self.tr("The full-screen gamepad interface (F11 switches it).", "Tam ekran oyun kolu arayüzü (F11 ile geçilir)."),
                            switch("start-console", self.global.start_in_console, cx, |this| {
                                this.global.start_in_console = !this.global.start_in_console
                            }),
                        ))
                        .child(widgets::divider())
                        .child(widgets::setting_row(
                            self.tr("Splash window while starting", "Başlarken açılış penceresi"),
                            self.tr("A small window for a few seconds.", "Birkaç saniyelik küçük bir pencere."),
                            switch("splash", self.global.splash, cx, |this| this.global.splash = !this.global.splash),
                        ))
                        .child(widgets::divider())
                        .child(widgets::setting_row(
                            self.tr("Close Arkor when a game starts", "Oyun açılınca Arkor'u kapat"),
                            self.tr("The game keeps running.", "Oyun çalışmaya devam eder."),
                            switch("close-on-launch", self.global.close_on_launch, cx, |this| {
                                this.global.close_on_launch = !this.global.close_on_launch
                            }),
                        )),
                )
                .child(self.render_clips(cx))
                                .child(
                    widgets::section("Steam").child(widgets::setting_row(
                        self.tr("Control Steam", "Steam'i yönet"),
                        self.steam_control_text(),
                        {
                            let on = self.global.steam_control;
                            let text = if on {
                                self.tr("Switch off", "Kapat")
                            } else {
                                self.tr("Switch on", "Aç")
                            };
                            style::button("steam-control", text, on).on_click(cx.listener(
                                move |this, _, _, cx| this.set_steam_control(!on, cx),
                            ))
                        },
                    )),
                )
                .child(
                    widgets::section(self.tr("Store games", "Mağaza oyunları"))
                        .child(widgets::setting_row(
                            self.tr("Saves follow the stores' clouds", "Kayıtlar mağaza bulutuyla eşitlensin"),
                            self.tr(
                                "Epic and GOG games: newer saves are fetched before a game starts and uploaded when it ends. Copies are kept; each game can be set on its own.",
                                "Epic ve GOG oyunları: yeni kayıtlar oyun başlamadan alınır, kapanınca yüklenir. Kopyalar saklanır; her oyun ayrıca ayarlanabilir.",
                            ),
                            switch("cloud-saves-all", !self.global.no_cloud_saves, cx, |this| {
                                this.global.no_cloud_saves = !this.global.no_cloud_saves
                            }),
                        ))
                        .child(widgets::divider())
                        .child(
                        widgets::setting_row(
                            self.tr("Games folder", "Oyun klasörü"),
                            self.global.games_dir().display().to_string(),
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    style::button(
                                        "games-dir",
                                        self.tr("Change…", "Değiştir…"),
                                        false,
                                    )
                                    .child(style::icon("folder-open", 13., style::pal::muted()))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let title = this.tr(
                                            "Where to install games",
                                            "Oyunların kurulacağı yer",
                                        );
                                        this.prompt_path(true, title, cx, |this, path, _| {
                                            this.global.games_dir = Some(path);
                                            this.save();
                                        });
                                    })),
                                )
                                .when(self.global.games_dir.is_some(), |d| {
                                    d.child(
                                        style::button(
                                            "games-dir-default",
                                            self.tr("Default", "Varsayılan"),
                                            false,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.global.games_dir = None;
                                                this.save();
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                }),
                        ),
                    ),
                ),
            SettingsTab::Appearance => self.render_appearance(_window, cx),
            SettingsTab::Defaults => self.render_defaults(cx),
            SettingsTab::Integrations => div()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    widgets::section(self.tr("Cover art (SteamGridDB)", "Kapak resimleri (SteamGridDB)"))
                        .child(sgdb),
                )
                .child(self.render_info_settings(cx))
                .child(self.render_sync_settings(cx))
                .child(self.render_sunshine(cx))
                .child(
                    widgets::section("Discord")
                        .child(widgets::setting_row(
                            self.tr("Show what you play on Discord", "Ne oynadığını Discord'da göster"),
                            self.tr("Needs the Discord app running.", "Discord uygulamasının açık olması gerekir."),
                            switch("discord", self.global.discord.enabled, cx, |this| {
                                this.global.discord.enabled = !this.global.discord.enabled
                            }),
                        ))
                        .child(widgets::divider())
                        .child(widgets::label(self.tr(
                            "Application ID (create an application at discord.com/developers; its name is what Discord shows):",
                            "Uygulama kimliği (discord.com/developers'ta bir uygulama oluştur; Discord onun adını gösterir):",
                        )))
                        .child(discord_id),
                ),
            SettingsTab::Backups => self.render_backup(cx),
            SettingsTab::Shortcuts => self.render_shortcut_list(),
        };
        div()
            .child(Self::header(
                self.tr("Settings", "Ayarlar"),
                self.tr(
                    "How Arkor behaves and looks, and what every game gets unless it has its own setting.",
                    "Arkor'un nasıl davrandığı ve göründüğü, ve kendi ayarı olmayan her oyunun ne aldığı.",
                ),
            ))
            .child(style::with_side_nav(nav, content))
    }

    /// Applies the theme and accent from the settings (System follows the window's appearance).
    /// Applies the theme, accent and glass from the settings to Arkor's palette and
    /// gpui-kit's components; only re-syncs when they (or the desktop's light/dark) change.
    pub(super) fn apply_theme(&mut self, window: &Window, cx: &mut gpui::App) {
        let system_light = matches!(
            window.appearance(),
            gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
        );
        let look = self.global.appearance.clone();
        let key = (look.clone(), system_light);
        if self.synced_look.as_ref() != Some(&key) {
            super::sync_kit_theme(&look, cx);
            self.synced_look = Some(key);
        }
    }

    pub(super) fn sound(&self, sound: super::sounds::Sound) {
        use super::console_sound::Cue;
        if self.console.on {
            // Console mode has sounds of its own (a start is heard when it is chosen).
            match sound {
                super::sounds::Sound::GameStart => {}
                super::sounds::Sound::GameExit => self.cue(Cue::Return),
                // Not over a game.
                super::sounds::Sound::Done if !self.running.is_empty() => {}
                super::sounds::Sound::Done => self.cue(Cue::Done),
                super::sounds::Sound::Error => self.cue(Cue::Error),
            }
        } else if self.global.appearance.sounds {
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

    /// The colour picker for a custom accent (created once, needs the window).
    fn make_accent_picker(
        &mut self,
        current: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<gpui::component::color_picker::ColorPickerState> {
        use gpui::component::color_picker::{ColorPickerEvent, ColorPickerState};
        if let Some((picker, _)) = &self.accent_picker {
            return picker.clone();
        }
        let picker =
            cx.new(|cx| ColorPickerState::new(window, cx).default_value(gpui::rgb(current)));
        let sub = cx.subscribe(&picker, |this, _, event: &ColorPickerEvent, cx| {
            let ColorPickerEvent::Change(Some(color)) = event else {
                return;
            };
            let c: gpui::Rgba = (*color).into();
            let hex = ((c.r * 255.).round() as u32) << 16
                | ((c.g * 255.).round() as u32) << 8
                | (c.b * 255.).round() as u32;
            this.edit_look(cx, |l| l.accent = Some(hex));
        });
        self.accent_picker = Some((picker.clone(), sub));
        picker
    }

    fn render_shortcut_list(&self) -> Div {
        use super::style::pal;
        let mut list = div().flex().flex_col().gap_1();
        for (keys, en, tr) in super::shell::SHORTCUTS {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div().w(px(150.)).flex().child(
                            div()
                                .flex_none()
                                .px_2()
                                .py_0p5()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(pal::border_strong())
                                .bg(pal::raised().opacity(0.6))
                                .text_xs()
                                .font_family("monospace")
                                .text_color(pal::text_soft())
                                .child(keys),
                        ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(pal::text_soft())
                            .child(self.tr(en, tr)),
                    ),
            );
        }
        widgets::section(self.tr("Keyboard shortcuts", "Klavye kısayolları")).child(list)
    }

    fn edit_look(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut arkor_core::profile::Appearance),
    ) {
        f(&mut self.global.appearance);
        let saved = self.global.save();
        self.report(saved);
        cx.notify();
    }

    /// A selectable tile with a small preview above its name.
    fn look_tile(
        id: (&'static str, usize),
        name: &'static str,
        active: bool,
        preview: gpui::AnyElement,
    ) -> gpui::Stateful<Div> {
        use super::style::pal;
        div()
            .id(id)
            .w(px(118.))
            .flex()
            .flex_col()
            .gap_2()
            .p_1p5()
            .rounded(px(14.))
            .border_1()
            .border_color(if active {
                pal::accent()
            } else {
                pal::glass_spec().border
            })
            .bg(if active {
                pal::accent().opacity(0.1)
            } else {
                gpui::transparent_black()
            })
            .cursor_pointer()
            .hover(|s| s.border_color(pal::accent().opacity(0.6)))
            .child(
                div()
                    .h(px(64.))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .bg(pal::bg())
                    .relative()
                    .child(preview),
            )
            .child(
                div()
                    .px_1()
                    .pb_0p5()
                    .text_sm()
                    .text_color(if active {
                        pal::accent_light()
                    } else {
                        pal::text_soft()
                    })
                    .child(name),
            )
    }

    fn render_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        use super::style::{self, pal};
        use arkor_core::profile::{Backdrop, Glass, Theme};
        let a = self.global.appearance.clone();
        let row = |title: &'static str, hint: &'static str, control: gpui::AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(div().font_weight(gpui::FontWeight::MEDIUM).child(title))
                        .child(widgets::hint_icon(format!("look-{title}"), hint)),
                )
                .child(control)
        };

        // Theme
        let mut themes = div().flex().flex_wrap().gap_3();
        for (ix, (theme, label, fill, ink)) in [
            (
                Theme::Dark,
                self.tr("Dark", "Koyu"),
                0x0d0d11u32,
                0xf4f4f5u32,
            ),
            (Theme::Light, self.tr("Light", "Açık"), 0xf7f8fa, 0x16171b),
            (
                Theme::System,
                self.tr("Follow the system", "Sisteme uy"),
                0x6b6b75,
                0xffffff,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let preview = div()
                .absolute()
                .inset_0()
                .bg(style::hex(fill))
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .w(px(50.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(style::hex(ink).opacity(0.8)),
                )
                .child(
                    div()
                        .w(px(70.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(style::hex(ink).opacity(0.35)),
                )
                .child(
                    div()
                        .mt_1()
                        .w(px(28.))
                        .h(px(10.))
                        .rounded(px(4.))
                        .bg(pal::accent()),
                )
                .into_any_element();
            themes = themes.child(
                Self::look_tile(("theme", ix), label, a.theme == theme, preview).on_click(
                    cx.listener(move |this, _, _, cx| this.edit_look(cx, |l| l.theme = theme)),
                ),
            );
        }

        // Accent
        let current = a.accent.unwrap_or(pal::DEFAULT_ACCENT);
        let mut accents = div().flex().flex_wrap().gap_2();
        for (ix, (color, en, tr)) in pal::ACCENTS.into_iter().enumerate() {
            let active = current == color;
            accents = accents.child(
                div()
                    .id(("accent", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl_1p5()
                    .pr_3()
                    .py_1p5()
                    .rounded_full()
                    .cursor_pointer()
                    .border_1()
                    .border_color(if active {
                        style::hex(color)
                    } else {
                        pal::glass_spec().border
                    })
                    .bg(if active {
                        style::hex(color).opacity(0.12)
                    } else {
                        pal::raised().opacity(0.4)
                    })
                    .hover(|s| s.border_color(style::hex(color).opacity(0.7)))
                    .child(
                        div()
                            .size(px(18.))
                            .rounded_full()
                            .bg(style::hex(color))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(active, |d| {
                                d.child(style::icon("check", 12., gpui::white()))
                            }),
                    )
                    .child(div().text_sm().child(self.tr(en, tr)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit_look(cx, |l| {
                            l.accent = (color != pal::DEFAULT_ACCENT).then_some(color)
                        })
                    })),
            );
        }

        // Custom colour from a picker (live, saved on change).
        let picker = self.make_accent_picker(current, window, cx);
        let custom = !pal::ACCENTS.iter().any(|(c, _, _)| *c == current);
        accents = accents.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .pl_1()
                .pr_3()
                .py_0p5()
                .rounded_full()
                .border_1()
                .border_color(if custom {
                    pal::accent()
                } else {
                    pal::glass_spec().border
                })
                .bg(pal::raised().opacity(0.4))
                .child(
                    gpui::component::color_picker::ColorPicker::new(&picker).featured_colors(
                        pal::ACCENTS
                            .iter()
                            .map(|(c, _, _)| style::hex(*c))
                            .collect(),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_sm().child(self.tr("Custom", "Özel")))
                        .child(
                            div()
                                .text_xs()
                                .text_color(pal::muted())
                                .child(format!("#{current:06X}")),
                        ),
                ),
        );

        // Glass
        let mut glasses = div().flex().flex_wrap().gap_3();
        for (ix, glass) in Glass::ALL.into_iter().enumerate() {
            let (name, radius, fill, border): (&'static str, f32, f32, f32) = match glass {
                Glass::Acrylic => ("Acrylic", 10., 0.7, 0.12),
                Glass::Crystal => ("Crystal", 5., 0.3, 0.25),
                Glass::Smoked => ("Smoked", 9., 0.95, 0.18),
                Glass::Soft => (self.tr("Soft", "Yumuşak"), 14., 0.06, 0.04),
            };
            let preview = div()
                .absolute()
                .inset_0()
                .child(
                    div()
                        .absolute()
                        .left(px(-10.))
                        .top(px(-14.))
                        .size(px(70.))
                        .rounded_full()
                        .bg(pal::accent().opacity(0.55)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(18.))
                        .top(px(14.))
                        .w(px(74.))
                        .h(px(40.))
                        .rounded(px(radius))
                        .bg(pal::card().opacity(fill))
                        .border_1()
                        .border_color(pal::text().opacity(border)),
                )
                .into_any_element();
            glasses = glasses.child(
                Self::look_tile(("glass", ix), name, a.glass == glass, preview).on_click(
                    cx.listener(move |this, _, _, cx| this.edit_look(cx, |l| l.glass = glass)),
                ),
            );
        }

        // Backdrop
        let mut backdrops = div().flex().flex_wrap().gap_3();
        for (ix, kind) in Backdrop::ALL.into_iter().enumerate() {
            let name = match kind {
                Backdrop::Nebula => self.tr("Nebula", "Bulutsu"),
                Backdrop::Eclipse => self.tr("Eclipse", "Tutulma"),
                Backdrop::Pulse => self.tr("Pulse", "Dalga"),
                Backdrop::Grid => self.tr("Grid", "Izgara"),
                Backdrop::Aurora => self.tr("Aurora ✦", "Kutup ışığı ✦"),
                Backdrop::Tide => self.tr("Tide ✦", "Gelgit ✦"),
                Backdrop::Drift => self.tr("Drift ✦", "Süzülme ✦"),
                Backdrop::Plain => self.tr("Plain", "Düz"),
                Backdrop::Image => self.tr("Your picture…", "Kendi görselin…"),
            };
            // Small previews are always still.
            let preview = div()
                .absolute()
                .inset_0()
                .child(super::backdrop::render(
                    kind,
                    true,
                    a.backdrop_image.as_deref(),
                    0.,
                ))
                .into_any_element();
            backdrops = backdrops.child(
                Self::look_tile(("backdrop", ix), name, a.backdrop == kind, preview).on_click(
                    cx.listener(move |this, _, _, cx| {
                        if kind == Backdrop::Image {
                            let title =
                                this.tr("Choose a background picture", "Bir arka plan görseli seç");
                            this.prompt_path(false, title, cx, |this, path, cx| {
                                this.edit_look(cx, |l| {
                                    l.backdrop = Backdrop::Image;
                                    l.backdrop_image = Some(path);
                                })
                            });
                        } else {
                            this.edit_look(cx, |l| l.backdrop = kind)
                        }
                    }),
                ),
            );
        }

        // Text size
        let mut sizes = div().flex().gap_1p5();
        for (ix, size) in arkor_core::profile::Appearance::FONT_SIZES
            .into_iter()
            .enumerate()
        {
            sizes = sizes.child(
                widgets::chip(
                    ("font-size", ix),
                    format!("{size} px"),
                    a.font_size() == size,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit_look(cx, |l| l.font_size = Some(size))
                })),
            );
        }

        let theme_card = widgets::section(self.tr("Theme and colours", "Tema ve renkler"))
            .gap_5()
            .child(row(
                self.tr("Theme", "Tema"),
                self.tr(
                    "Dark, light, or whatever the desktop uses.",
                    "Koyu, açık ya da masaüstü neyi kullanıyorsa o.",
                ),
                themes.into_any_element(),
            ))
            .child(row(
                self.tr("Accent colour", "Vurgu rengi"),
                self.tr(
                    "Used for highlights, switches and the logo.",
                    "Vurgularda, anahtarlarda ve logoda kullanılır.",
                ),
                accents.into_any_element(),
            ))
            .child(row(
                self.tr("Panel style", "Panel stili"),
                self.tr(
                    "How see-through the panels and cards are, and how round.",
                    "Panellerin ve kartların ne kadar saydam ve yuvarlak olduğu.",
                ),
                glasses.into_any_element(),
            ))
            .child(row(
                self.tr("Background", "Arka plan"),
                self.tr(
                    "Drawn behind the panels. ✦ marks the animated ones.",
                    "Panellerin arkasında çizilir. ✦ işaretliler hareketli.",
                ),
                backdrops.into_any_element(),
            ));
        let toggle = |id: &'static str,
                      on: bool,
                      cx: &mut Context<Self>,
                      f: fn(&mut arkor_core::profile::Appearance)| {
            widgets::switch(on)
                .id(id)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit_look(cx, f);
                    if id == "sounds" && this.global.appearance.sounds {
                        super::sounds::play(super::sounds::Sound::Done);
                    }
                }))
        };
        let layout_card =
            widgets::section(self.tr("Text, size and motion", "Yazı, boyut ve hareket"))
                .child(widgets::setting_row(
                    self.tr("Text size", "Yazı boyutu"),
                    self.tr(
                        "Every text and spacing in the window follows it.",
                        "Penceredeki tüm yazılar ve boşluklar buna göre ölçeklenir.",
                    ),
                    sizes,
                ))
                .child(widgets::divider())
                .child(widgets::setting_row(
                    self.tr("Library cover size", "Kütüphane kapak boyutu"),
                    self.tr(
                        "Small, medium or large covers on the Library page.",
                        "Kütüphane sayfasında küçük, orta ya da büyük kapaklar.",
                    ),
                    self.grid_size_chips(cx),
                ))
                .child(widgets::divider())
                .child(widgets::setting_row(
                    self.tr("Reduce motion", "Hareketi azalt"),
                    self.tr(
                        "Animated backgrounds stand still.",
                        "Hareketli arka planlar durağan kalır.",
                    ),
                    toggle("reduce-motion", a.reduce_motion, cx, |l| {
                        l.reduce_motion = !l.reduce_motion
                    }),
                ))
                .child(widgets::divider())
                .child(widgets::setting_row(
                    self.tr("Sounds", "Sesler"),
                    self.tr(
                        "When a game starts or stops, a download finishes, or something fails.",
                        "Oyun başlayınca ya da kapanınca, indirme bitince ya da bir hata olunca.",
                    ),
                    toggle("sounds", a.sounds, cx, |l| l.sounds = !l.sounds),
                ));
        let console_card = self.render_console_themes(cx);
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(theme_card)
            .child(layout_card)
            .child(console_card)
    }

    /// Console mode's themes: the built-in ones, and a theme file to share or take in.
    fn render_console_themes(&mut self, cx: &mut Context<Self>) -> Div {
        use arkor_core::profile::ConsoleTheme;
        let current = self.console_theme();
        let mut chips = div().flex().flex_wrap().gap_2();
        let mut themes = ConsoleTheme::builtin();
        if !themes.iter().any(|t| t.name == current.name) {
            themes.push(current.clone());
        }
        for (ix, t) in themes.into_iter().enumerate() {
            let on = t.name == current.name;
            let label = self.console_theme_name(&t.name);
            chips = chips.child(
                widgets::chip(("ctheme", ix), label, on).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.global.console_theme =
                            (t != ConsoleTheme::default()).then(|| t.clone());
                        let r = this.global.save();
                        this.report(r);
                        cx.notify();
                    },
                )),
            );
        }
        let export = widgets::chip(
            "ctheme-export",
            self.tr("Save as a file…", "Dosya olarak kaydet…"),
            false,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            let t = this.console_theme();
            let dir = dirs::download_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_default();
            let file = dir.join(format!("{}.arkor-theme.json", t.name.replace('/', "_")));
            this.status = Some(match t.export(&file) {
                Ok(()) => file.display().to_string().into(),
                Err(e) => format!("{e:#}").into(),
            });
            cx.notify();
        }));
        let import = widgets::chip(
            "ctheme-import",
            self.tr("Open a theme file…", "Tema dosyası aç…"),
            false,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            let title = this.tr(
                "Choose a .arkor-theme.json file",
                ".arkor-theme.json dosyası seç",
            );
            this.prompt_path(false, title, cx, |this, file, cx| {
                match ConsoleTheme::import(&file) {
                    Ok(t) => {
                        this.global.console_theme = Some(t);
                        let r = this.global.save();
                        this.report(r);
                    }
                    Err(e) => this.status = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
        widgets::section(self.tr("Console mode", "Konsol modu"))
            .child(widgets::label(self.tr(
                "How console mode looks on the TV: rows of covers or a grid, big or small covers, round or square corners, the focus colour. A theme can be saved as a file and shared.",
                "Konsol modunun televizyonda nasıl göründüğü: kapak sıraları ya da ızgara, büyük ya da küçük kapaklar, yuvarlak ya da köşeli kenarlar, odak rengi. Bir tema dosya olarak kaydedilip paylaşılabilir.",
            )))
            .child(chips)
            .child(div().flex().gap_2().child(export).child(import))
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
        use super::style::{self, pal};
        let now_card = widgets::section(self.tr("Back up now", "Şimdi yedekle"))
            .child(widgets::setting_row(
                self.tr("Backup folder", "Yedek klasörü"),
                b.dest_dir().display().to_string(),
                style::button("backup-dest", self.tr("Change…", "Değiştir…"), false)
                    .child(style::icon("folder-open", 13., pal::muted()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let title = this.tr("Choose the backup folder", "Yedek klasörünü seç");
                        this.prompt_path(true, title, cx, |this, path, _| {
                            this.global.backup.dest = Some(path);
                            this.save();
                        });
                    })),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr(
                    "Include Arkor's prefixes",
                    "Arkor'un prefix'lerini dahil et",
                ),
                self.tr(
                    "Game installs inside them make the file large.",
                    "İçlerindeki oyun kurulumları dosyayı büyütür.",
                ),
                widgets::switch(self.backup_prefixes)
                    .id("backup-prefixes")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.backup_prefixes = !this.backup_prefixes;
                        cx.notify();
                    })),
            ))
            .child(
                div().flex().pt_1().child(
                    widgets::primary_button("backup-now", self.tr("Back up now", "Şimdi yedekle"))
                        .child(style::icon("archive", 14., pal::on_accent()))
                        .on_click(cx.listener(|this, _, _, cx| this.backup_now(cx))),
                ),
            );
        let mut auto = widgets::section(self.tr("Automatic backup", "Otomatik yedek")).child(
            widgets::setting_row(
                self.tr("How often", "Ne sıklıkla"),
                self.tr(
                    "Checked when Arkor starts.",
                    "Arkor açılırken kontrol edilir.",
                ),
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(every(0, 0, self.tr("Off", "Kapalı"), cx))
                    .child(every(1, 1, self.tr("Daily", "Günlük"), cx))
                    .child(every(2, 7, self.tr("Weekly", "Haftalık"), cx))
                    .child(every(3, 30, self.tr("Monthly", "Aylık"), cx)),
            ),
        );
        if b.every_days > 0 {
            auto = auto.child(widgets::divider()).child(widgets::setting_row(
                self.tr("Keep", "Sakla"),
                self.tr(
                    "Older automatic backups are deleted.",
                    "Daha eski otomatik yedekler silinir.",
                ),
                widgets::stepper_control(
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
                ),
            ));
        }
        let card = widgets::section(self.tr("Restore", "Geri yükle"));
        let restore = match self.restore_pending.clone() {
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
                .child(div().text_sm().text_color(ACCENT()).child(format!(
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
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(now_card)
            .child(auto)
            .child(restore)
    }
}
