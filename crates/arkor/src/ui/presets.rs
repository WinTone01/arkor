//! Launch presets on the game page (save this game's settings under a name, give a preset's
//! settings to this game) and for several picked games at once.

use gpui::{Context, Div, div, prelude::*, px};

use super::style::pal;
use super::{Library, widgets};
use arkor_core::profile::presets;

impl Library {
    /// Gives the preset `name` to the open game.
    fn apply_preset_here(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some((_, p)) = presets::list().into_iter().find(|(n, _)| n == name) {
            self.profile.apply_preset(&p);
            self.save();
            self.status =
                Some(format!("{}: {name}", self.tr("Preset applied", "Şablon uygulandı")).into());
        }
        cx.notify();
    }

    /// Gives the preset `name` to every picked game (their own settings files).
    pub(super) fn apply_preset_to_picked(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some((_, p)) = presets::list().into_iter().find(|(n, _)| n == name) else {
            return;
        };
        let mut failed = None;
        for id in self.picked.clone() {
            let gid = arkor_core::GameId::new(arkor_core::Store::Local, id.as_str());
            let mut prof = arkor_core::Profile::load(&gid).unwrap_or_default();
            prof.apply_preset(&p);
            if let Err(e) = prof.save(&gid) {
                failed = Some(e);
            }
        }
        self.status = Some(match failed {
            None => format!(
                "{} {} {}",
                self.picked.len(),
                self.tr("games got the preset", "oyuna şablon uygulandı:"),
                name
            )
            .into(),
            Some(e) => format!("{e:#}").into(),
        });
        cx.notify();
    }

    /// The picked games' row of presets.
    pub(super) fn picked_presets(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let list = presets::list();
        if list.is_empty() {
            return None;
        }
        let mut row = div().flex().flex_wrap().items_center().gap_2().child(
            div()
                .text_sm()
                .text_color(pal::muted())
                .child(self.tr("Apply a preset:", "Şablon uygula:")),
        );
        for (ix, (name, _)) in list.into_iter().enumerate() {
            row = row.child(
                widgets::chip(("pick-preset", ix), name.clone(), false).on_click(
                    cx.listener(move |this, _, _, cx| this.apply_preset_to_picked(&name, cx)),
                ),
            );
        }
        Some(row)
    }

    /// The game page's card.
    pub(super) fn render_presets(&mut self, cx: &mut Context<Self>) -> Div {
        let input = self.submit_field(
            "preset-name",
            self.tr(
                "Name, then Enter (e.g. Old DirectX games)",
                "Ad, sonra Enter (ör. Eski DirectX oyunları)",
            ),
            cx,
            |this, text, cx| {
                let r = presets::save(text, &this.profile);
                this.report(r);
                cx.notify();
            },
        );
        let mut card = widgets::section(self.tr("Presets", "Şablonlar"))
            .child(widgets::label(self.tr(
                "Save this game's settings (Proton, switches, frame generation, overlay, launch options…) under a name, then give them to other games in one click. What only fits this game, like its Steam settings, is not copied.",
                "Bu oyunun ayarlarını (Proton, anahtarlar, kare üretimi, gösterge, başlatma seçenekleri…) bir adla kaydet, sonra tek tıkla başka oyunlara ver. Steam ayarları gibi yalnızca bu oyuna uyanlar kopyalanmaz.",
            )))
            .child(widgets::setting_row(
                self.tr("Save as a preset", "Şablon olarak kaydet"),
                "",
                div().w(px(320.)).child(input),
            ));
        let list = presets::list();
        if list.is_empty() {
            return card;
        }
        card = card.child(widgets::subheading(
            self.tr("Give this game a preset", "Bu oyuna şablon ver"),
        ));
        for (ix, (name, _)) in list.into_iter().enumerate() {
            let (n1, n2) = (name.clone(), name.clone());
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().text_sm().child(name))
                    .child(
                        widgets::chip(("preset-apply", ix), self.tr("Apply", "Uygula"), false)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.apply_preset_here(&n1, cx)),
                            ),
                    )
                    .child(
                        widgets::chip(("preset-del", ix), self.tr("Delete", "Sil"), false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let r = presets::remove(&n2);
                                this.report(r);
                                cx.notify();
                            })),
                    ),
            );
        }
        card
    }

    /// The sandbox card.
    pub(super) fn render_sandbox(&mut self, cx: &mut Context<Self>) -> Div {
        let sb = self.profile.sandbox.clone();
        let home = self
            .selected_id()
            .and_then(|id| arkor_launch::sandbox::home_of(&id).ok());
        let mut card = widgets::section(self.tr("Sandbox", "Yalıtılmış çalıştırma"))
            .child(
                widgets::switch_row(
                    "sandbox-on",
                    self.tr("Run it in a sandbox", "Yalıtılmış çalıştır"),
                    self.tr(
                        "For a game from an unknown source. It cannot reach the internet or see your files: it only sees its own folder, its Wine prefix and what it needs to start. Its saves and settings are kept in a folder of its own.",
                        "Kaynağı belli olmayan bir oyun için. İnternete çıkamaz ve dosyalarını göremez: yalnızca kendi klasörünü, Wine prefix'ini ve başlaması için gerekenleri görür. Kayıtları ve ayarları kendine ait bir klasörde tutulur.",
                    ),
                    sb.enabled,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.profile.sandbox.enabled = !this.profile.sandbox.enabled;
                    this.save();
                    cx.notify();
                })),
            );
        if !sb.enabled {
            return card;
        }
        card = card.child(
            widgets::switch_row(
                "sandbox-net",
                self.tr("Allow the internet", "İnternete izin ver"),
                self.tr(
                    "For online games. Your files stay hidden either way.",
                    "Çevrim içi oyunlar için. Dosyaların her durumda gizli kalır.",
                ),
                sb.network,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.profile.sandbox.network = !this.profile.sandbox.network;
                this.save();
                cx.notify();
            })),
        );
        if arkor_launch::start::find_in_path("bwrap").is_none() {
            card = card.child(widgets::label(self.tr(
                "Bubblewrap (bwrap) is not installed; the game will not start until it is.",
                "Bubblewrap (bwrap) kurulu değil; kurulana kadar oyun başlamaz.",
            )));
        }
        if let Some(home) = home {
            card = card.child(
                div().flex().child(
                    widgets::chip(
                        "sandbox-home",
                        self.tr("Open its own folder", "Kendi klasörünü aç"),
                        false,
                    )
                    .on_click(move |_, _, _| {
                        let _ = std::fs::create_dir_all(&home);
                        let _ = std::process::Command::new("xdg-open").arg(&home).spawn();
                    }),
                ),
            );
        }
        card
    }

    /// Syncs with the other computers in the background, when a place is set.
    pub(super) fn sync_now(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || !self.global.sync.is_on() {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let r = bg.spawn(async { arkor_launch::sync::run() }).await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(Some(rep)) if rep.games + rep.sessions + rep.notes > 0 => {
                        if let Ok(lib) = arkor_core::library::Library::load() {
                            this.library = lib;
                            this.reload_games();
                            this.apply_filter();
                        }
                        this.stats_forget();
                        this.status = Some(
                            format!(
                                "{}: {} {} · {} {}",
                                this.tr("Synced", "Eşitlendi"),
                                rep.games,
                                this.tr("games", "oyun"),
                                rep.sessions,
                                this.tr("sessions", "oturum")
                            )
                            .into(),
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        this.status = Some(format!("{}: {e:#}", this.tr("Sync", "Eşitleme")).into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Settings → Integrations: where the computers meet.
    pub(super) fn render_sync_settings(&mut self, cx: &mut Context<Self>) -> Div {
        let folder = self
            .global
            .sync
            .folder
            .as_ref()
            .map(|f| f.display().to_string())
            .unwrap_or_else(|| self.tr("No folder chosen", "Klasör seçilmedi").to_string());
        let remote = self.field(
            "sync-rclone",
            |this| this.global.sync.rclone.clone().unwrap_or_default(),
            "gdrive:arkor",
            cx,
            |this, text| {
                let t = text.trim();
                this.global.sync.rclone = (!t.is_empty()).then(|| t.to_string());
                let r = this.global.save();
                this.report(r);
            },
        );
        widgets::section(self.tr("Sync between computers", "Bilgisayarlar arası eşitleme"))
            .child(widgets::label(self.tr(
                "Statuses, ratings, favourites, categories, journal notes and play time stay the same on all your computers (or both sides of a dual boot). Each computer writes one file into a shared place and reads the others'. Your games and saves are not copied.",
                "Durumlar, puanlar, favoriler, kategoriler, günlük notları ve oynama süresi bütün bilgisayarlarında aynı kalır (ya da çift önyüklemenin iki tarafında). Her bilgisayar ortak bir yere bir dosya yazar ve diğerlerininkini okur. Oyunların ve kayıtların kopyalanmaz.",
            )))
            .child(widgets::setting_row(
                self.tr("Shared folder", "Ortak klasör"),
                self.tr(
                    "A folder every computer sees: Syncthing, Dropbox, Nextcloud, a network drive.",
                    "Her bilgisayarın gördüğü bir klasör: Syncthing, Dropbox, Nextcloud, ağ sürücüsü.",
                ),
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().text_color(pal::muted()).child(folder))
                    .child(
                        widgets::chip("sync-folder", self.tr("Choose…", "Seç…"), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                let title = this.tr("The shared folder", "Ortak klasör");
                                this.prompt_path(true, title, cx, |this, dir, cx| {
                                    this.global.sync.folder = Some(dir);
                                    let r = this.global.save();
                                    this.report(r);
                                    this.sync_now(cx);
                                    cx.notify();
                                });
                            }),
                        ),
                    )
                    .children(self.global.sync.folder.is_some().then(|| {
                        widgets::chip("sync-folder-off", self.tr("Stop", "Kapat"), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.global.sync.folder = None;
                                let r = this.global.save();
                                this.report(r);
                                cx.notify();
                            }),
                        )
                    })),
            ))
            .child(widgets::setting_row(
                self.tr("Or a cloud (rclone)", "Ya da bir bulut (rclone)"),
                self.tr(
                    "A remote set up with `rclone config` and a folder in it, like gdrive:arkor. Used when no shared folder is chosen.",
                    "`rclone config` ile kurulmuş bir uzak alan ve içinde bir klasör, ör. gdrive:arkor. Ortak klasör seçili değilse kullanılır.",
                ),
                div().w(px(240.)).child(remote),
            ))
            .child(
                div().flex().child(
                    widgets::chip("sync-now", self.tr("Sync now", "Şimdi eşitle"), false)
                        .on_click(cx.listener(|this, _, _, cx| this.sync_now(cx))),
                ),
            )
    }

    /// Hidden games are behind a PIN not typed yet.
    pub(super) fn hidden_locked(&self) -> bool {
        self.global.hidden_pin.is_some() && !self.hidden_unlocked
    }

    /// The PIN asked before hidden games show.
    pub(super) fn pin_card(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.pin_asking {
            return None;
        }
        let input = self.secret_field(
            "pin-ask",
            self.tr("PIN, then Enter", "PIN, sonra Enter"),
            cx,
        );
        let sub_input = input.clone();
        if !self.fields.inputs.contains_key("pin-ask-sub") {
            let sub = cx.subscribe(
                &sub_input,
                |this, input, ev: &super::text_input::InputEvent, cx| {
                    if let super::text_input::InputEvent::Submit(text) = ev {
                        let ok = this
                            .global
                            .hidden_pin
                            .as_deref()
                            .is_some_and(|k| arkor_core::profile::pin::check(k, text));
                        if ok {
                            this.hidden_unlocked = true;
                            this.pin_asking = false;
                            this.filter.show_hidden = true;
                            this.apply_filter();
                        } else {
                            this.status = Some(this.tr("Wrong PIN", "PIN yanlış").into());
                        }
                        input.update(cx, |i, cx| i.set_text("", cx));
                        cx.notify();
                    }
                },
            );
            self.fields
                .inputs
                .insert("pin-ask-sub".into(), (input.clone(), sub));
        }
        Some(
            super::style::card()
                .p_4()
                .flex()
                .items_center()
                .gap_3()
                .child(super::style::icon("lock", 16., pal::accent_light()))
                .child(div().flex_1().text_sm().child(self.tr(
                    "Hidden games are locked. Type the PIN to show them.",
                    "Gizli oyunlar kilitli. Göstermek için PIN'i yaz.",
                )))
                .child(div().w(px(180.)).child(input))
                .child(
                    widgets::chip("pin-cancel", self.tr("Cancel", "Vazgeç"), false).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.pin_asking = false;
                            cx.notify();
                        }),
                    ),
                ),
        )
    }

    /// Settings: set, change or remove the PIN of hidden games.
    pub(super) fn render_pin_settings(&mut self, cx: &mut Context<Self>) -> Div {
        let has = self.global.hidden_pin.is_some();
        let set = self.submit_field(
            "pin-set",
            if has {
                self.tr(
                    "Current PIN to remove it, then Enter",
                    "Kaldırmak için şimdiki PIN, sonra Enter",
                )
            } else {
                self.tr(
                    "New PIN (4+ digits), then Enter",
                    "Yeni PIN (4+ rakam), sonra Enter",
                )
            },
            cx,
            |this, text, cx| {
                match this.global.hidden_pin.clone() {
                    Some(k) => {
                        if arkor_core::profile::pin::check(&k, text) {
                            this.global.hidden_pin = None;
                            let r = this.global.save();
                            this.report(r);
                        } else {
                            this.status = Some(this.tr("Wrong PIN", "PIN yanlış").into());
                        }
                    }
                    None => match arkor_core::profile::pin::make(text) {
                        Ok(k) => {
                            this.global.hidden_pin = Some(k);
                            this.hidden_unlocked = false;
                            this.filter.show_hidden = false;
                            this.apply_filter();
                            let r = this.global.save();
                            this.report(r);
                        }
                        Err(e) => this.status = Some(format!("{e:#}").into()),
                    },
                }
                cx.notify();
            },
        );
        set.update(cx, |i, _| i.set_masked(true));
        widgets::section(self.tr("Hidden games PIN", "Gizli oyunlar için PIN"))
            .child(widgets::label(if has {
                self.tr(
                    "Hidden games show only after the PIN is typed (once per start of Arkor). It keeps them from a quick look, it is not encryption.",
                    "Gizli oyunlar yalnızca PIN yazıldıktan sonra görünür (Arkor her açıldığında bir kez). Göz atanlardan saklar; şifreleme değildir.",
                )
            } else {
                self.tr(
                    "Keep hidden games behind a PIN, for example when others use this computer.",
                    "Gizli oyunları bir PIN arkasında tut; örneğin bu bilgisayarı başkaları da kullanıyorsa.",
                )
            }))
            .child(div().w(px(320.)).child(set))
    }
}
