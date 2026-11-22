//! Settings → Game defaults (after Heroic's "Game Defaults"): the Proton for games left on
//! "Automatic", what new games start with, the prefix folder, shared environment and
//! updates. Also the Proton picker and tweak list that the game page uses too.

use std::collections::BTreeSet;

use arkor_launch::components::Latest;
use gpui::prelude::FluentBuilder;
use gpui::{AnyElement, Context, Div, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};

impl Library {
    /// Automatic, the always-newest builds, then the installed ones. `set` stores the pick.
    pub(super) fn proton_picker(
        &self,
        id: &'static str,
        current: Option<&str>,
        auto_label: &'static str,
        cx: &mut Context<Self>,
        set: fn(&mut Self, Option<String>, &mut Context<Self>),
    ) -> Div {
        let chosen_latest = current.and_then(Latest::from_value);
        let mut first = div().flex().flex_wrap().gap_2().child(
            widgets::chip((id, 0usize), auto_label, current.is_none())
                .on_click(cx.listener(move |this, _, _, cx| set(this, None, cx))),
        );
        for (ix, l) in Latest::ALL.into_iter().enumerate() {
            let name = l.dir_name();
            first = first.child(
                widgets::chip((id, 1 + ix), name, chosen_latest == Some(l))
                    .child(style::icon("refresh-cw", 12., pal::muted()))
                    .on_click(
                        cx.listener(move |this, _, _, cx| set(this, Some(name.to_string()), cx)),
                    ),
            );
        }
        let mut installed = div().flex().flex_wrap().gap_2();
        let mut any = false;
        for (ix, (name, _)) in self.tools.protons.iter().enumerate() {
            if Latest::from_value(name).is_some() {
                continue;
            }
            any = true;
            let value = name.clone();
            installed = installed.child(
                widgets::chip((id, 10 + ix), name.clone(), current == Some(name.as_str()))
                    .on_click(
                        cx.listener(move |this, _, _, cx| set(this, Some(value.clone()), cx)),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(first)
            .child(widgets::label(self.tr(
                "↻ = always the newest release, downloaded on first use and updated before each start.",
                "↻ = hep en yeni sürüm; ilk kullanımda indirilir, her başlatmadan önce güncellenir.",
            )))
            .when(any, |d| {
                d.child(widgets::subheading(self.tr(
                    "Installed versions (never change)",
                    "Kurulu sürümler (hiç değişmez)",
                )))
                .child(installed)
            })
    }

    /// Every tweak as a check box, grouped by what it is about.
    pub(super) fn tweak_list(
        &self,
        id: &'static str,
        selected: &BTreeSet<String>,
        cx: &mut Context<Self>,
        toggle: fn(&mut Self, &'static str, &mut Context<Self>),
    ) -> Div {
        use arkor_core::tweaks::{Category, TWEAKS};
        let lang_tr = self.global.language == arkor_core::Language::Tr;
        let title = |c: Category| match c {
            Category::Upscaling => self.tr("Upscaling", "Ölçekleme"),
            Category::Display => self.tr("Display", "Ekran"),
            Category::Latency => self.tr("Latency", "Gecikme"),
            Category::Compatibility => self.tr("Compatibility", "Uyumluluk"),
            Category::Gpu => "GPU",
            Category::Input => self.tr("Input", "Giriş"),
            Category::Other => self.tr("Other", "Diğer"),
        };
        let mut list = div().flex().flex_col().gap_2();
        for (ci, c) in Category::ALL.into_iter().enumerate() {
            let gpus = self.tools.gpus;
            let tweaks: Vec<_> = TWEAKS
                .iter()
                .enumerate()
                .filter(|(_, t)| t.category == c)
                // Hidden when this computer has no GPU they work with.
                .filter(|(_, t)| match arkor_core::tweaks::gpu_need(t.id) {
                    Some(arkor_core::tweaks::GpuNeed::Amd) => gpus.amd,
                    Some(arkor_core::tweaks::GpuNeed::Nvidia) => gpus.nvidia,
                    None => true,
                })
                .collect();
            if tweaks.is_empty() {
                continue;
            }
            let on = tweaks
                .iter()
                .filter(|(_, t)| selected.contains(t.id))
                .count();
            let mut grid = div().flex().flex_wrap().gap_1();
            for (ix, tw) in tweaks {
                let label = if lang_tr { tw.label_tr } else { tw.label_en };
                let text = match tw.needs {
                    Some(n) => format!("{label} ({n})"),
                    None => label.to_string(),
                };
                let tid = tw.id;
                grid = grid.child(
                    widgets::check_item((id, ix), text, selected.contains(tid))
                        .w(px(330.))
                        .on_click(cx.listener(move |this, _, _, cx| toggle(this, tid, cx))),
                );
            }
            if ci > 0 {
                list = list.child(widgets::divider());
            }
            list = list
                .child(widgets::subheading(format!("{} · {on}", title(c))))
                .child(grid);
        }
        list
    }

    fn edit_defaults(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut arkor_core::profile::GameDefaults),
    ) {
        f(&mut self.global.defaults);
        let saved = self.global.save();
        self.report(saved);
        cx.notify();
    }

    /// Copies the defaults into every game's profile.
    fn apply_defaults_to_all(&mut self, cx: &mut Context<Self>) {
        let defaults = self.global.defaults.clone();
        let mut failed = None;
        for game in &self.library.games {
            let id = game.game_id();
            let mut profile = arkor_core::Profile::load(&id).unwrap_or_default();
            defaults.apply_to(&mut profile);
            if let Err(e) = profile.save(&id) {
                failed = Some(e);
            }
        }
        self.game_ui.confirm_apply_all = false;
        self.report(failed.map_or(Ok(()), Err));
        cx.notify();
    }

    pub(super) fn render_defaults(&mut self, cx: &mut Context<Self>) -> Div {
        let d = self.global.defaults.clone();
        let switch = |id: &'static str,
                      on: bool,
                      cx: &mut Context<Self>,
                      f: fn(&mut arkor_core::profile::GameDefaults)| {
            widgets::switch(on)
                .id(id)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.edit_defaults(cx, f)))
        };
        let launch_options = self.field(
            "default-launch-options",
            |this| this.global.defaults.launch_options.clone(),
            "VAR=1 %command% -dx12",
            cx,
            |this, text| {
                this.global.defaults.launch_options = text.to_string();
                let saved = this.global.save();
                this.report(saved);
            },
        );
        let env = self.env_editor(super::global_page::EnvScope::Global, cx);
        let prefixes = self.global.prefixes_dir();
        let custom_prefixes = self.global.prefixes_dir.is_some();

        let compat = widgets::section(self.tr("Proton and Wine", "Proton ve Wine"))
            .child(widgets::setting_row(
                self.tr("Default Proton", "Varsayılan Proton"),
                self.tr(
                    "For every game whose Proton is “Automatic” and whose launcher didn't pick one.",
                    "Proton'u “Otomatik” olan ve başlatıcısının seçmediği her oyun için.",
                ),
                div(),
            ))
            .child(self.proton_picker(
                "default-proton",
                d.proton.as_deref(),
                self.tr("Pick automatically", "Otomatik seç"),
                cx,
                |this, value, cx| this.edit_defaults(cx, |d| d.proton = value),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("Prefix folder", "Prefix klasörü"),
                format!(
                    "{} {}",
                    self.tr("Each new Windows game gets its own prefix in:", "Her yeni Windows oyunu kendi prefix'ini şurada alır:"),
                    prefixes.display()
                ),
                div()
                    .flex()
                    .gap_2()
                    .child(
                        style::button("prefixes-choose", self.tr("Change…", "Değiştir…"), false)
                            .child(style::icon("folder-open", 13., pal::muted()))
                            .on_click(cx.listener(|this, _, _, cx| this.choose_prefixes_dir(cx))),
                    )
                    .when(custom_prefixes, |d| {
                        d.child(
                            style::button("prefixes-default", self.tr("Use default", "Varsayılanı kullan"), false)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.global.prefixes_dir = None;
                                    this.save();
                                    cx.notify();
                                })),
                        )
                    }),
            ));

        let new_games =
            widgets::section(self.tr("New games start with", "Yeni oyunlar şununla başlar"))
                .child(widgets::label(self.tr(
                    "Copied into a game when it is added; change it on the game's page afterwards.",
                    "Oyun eklenince ona kopyalanır; sonra oyunun sayfasında değiştirilebilir.",
                )))
                .child(widgets::setting_row(
                    "GameMode",
                    self.tr(
                        "Feral's CPU and GPU tuning while the game runs.",
                        "Oyun çalışırken Feral'ın CPU ve GPU ayarları.",
                    ),
                    switch("default-gamemode", d.gamemode, cx, |d| {
                        d.gamemode = !d.gamemode
                    }),
                ))
                .child(widgets::divider())
                .child(widgets::setting_row(
                    self.tr("Keep the system awake", "Sistemi uyanık tut"),
                    self.tr(
                        "No sleep or screen blanking while playing.",
                        "Oynarken uyku ya da ekran kararması yok.",
                    ),
                    switch("default-no-sleep", d.no_sleep, cx, |d| {
                        d.no_sleep = !d.no_sleep
                    }),
                ))
                .child(widgets::divider())
                .child(widgets::setting_row(
                    "MangoHud",
                    self.tr(
                        "Games use the shared layout unless they have their own.",
                        "Kendi düzeni olmayan oyunlar ortak düzeni kullanır.",
                    ),
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            widgets::switch(self.global.mangohud.enabled)
                                .id("default-mangohud")
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.global.mangohud.enabled = !this.global.mangohud.enabled;
                                    let saved = this.global.save();
                                    this.report(saved);
                                    cx.notify();
                                })),
                        )
                        .child(
                            style::button(
                                "default-hud-edit",
                                self.tr("Edit layout", "Düzeni düzenle"),
                                false,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.go(super::shell::Nav::Overlay, cx)
                                }),
                            ),
                        ),
                ))
                .child(widgets::divider())
                .child(widgets::setting_row(
                    self.tr("Launch options", "Başlatma seçenekleri"),
                    self.tr(
                        "Steam style: KEY=VALUE words and %command%.",
                        "Steam biçimi: ANAHTAR=DEĞER kelimeleri ve %command%.",
                    ),
                    div().w(px(300.)).child(launch_options),
                ))
                .child(widgets::divider())
                .child(widgets::subheading(self.tr(
                    "Proton and driver switches",
                    "Proton ve sürücü anahtarları",
                )))
                .child(
                    self.tweak_list("default-tweak", &d.tweaks, cx, |this, id, cx| {
                        this.edit_defaults(cx, |d| {
                            if !d.tweaks.remove(id) {
                                d.tweaks.insert(id.to_string());
                            }
                        })
                    }),
                );

        let apply: AnyElement = if self.game_ui.confirm_apply_all {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(widgets::label(self.tr(
                    "Turn these on in every game in the library? Their other settings stay.",
                    "Bunlar kütüphanedeki her oyunda açılsın mı? Diğer ayarları değişmez.",
                )))
                .child(
                    style::button(
                        "apply-all-yes",
                        self.tr("Apply to all", "Hepsine uygula"),
                        true,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.apply_defaults_to_all(cx))),
                )
                .child(
                    style::button("apply-all-no", self.tr("Cancel", "Vazgeç"), false).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.game_ui.confirm_apply_all = false;
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element()
        } else {
            style::button(
                "apply-all",
                self.tr("Apply to every game now…", "Şimdi her oyuna uygula…"),
                false,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.game_ui.confirm_apply_all = true;
                cx.notify();
            }))
            .into_any_element()
        };
        let new_games = new_games.child(div().flex().pt_1().child(apply));

        let launch = widgets::section(self.tr("Environment (all games)", "Ortam (tüm oyunlar)"))
            .child(widgets::label(self.tr(
                "Variables added to every game at launch. A game's own variables override these.",
                "Her oyuna başlatırken eklenen değişkenler. Oyunun kendi değişkenleri bunları ezer.",
            )))
            .child(env);

        let updates = widgets::section(self.tr("Updates", "Güncellemeler")).child(widgets::setting_row(
            self.tr("Keep launch components up to date", "Başlatma bileşenlerini güncel tut"),
            self.tr(
                "Before a game starts: umu-launcher, the anti-cheat runtimes and its always-newest Proton. Off: only what is missing is downloaded.",
                "Oyun başlamadan önce: umu-launcher, anti-cheat runtime'ları ve hep en yeni Proton'u. Kapalı: yalnızca eksik olan indirilir.",
            ),
            widgets::switch(!self.global.no_auto_updates)
                .id("auto-updates-settings")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.global.no_auto_updates = !this.global.no_auto_updates;
                    let saved = this.global.save();
                    this.report(saved);
                    cx.notify();
                })),
        ));

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(compat)
            .child(new_games)
            .child(launch)
            .child(updates)
    }
}
