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
}
