//! The Steam card of a Steam game's page: which Proton Steam uses for it, whether Steam
//! starts it through Arkor (so Arkor's settings apply), and whether Steam already has these
//! settings. Changes are written into Steam's files right away when Steam is closed; while
//! Steam runs they wait for Play (Steam is closed and opened again) or "Apply now".

use arkor_core::library::GameKind;
use arkor_launch::steam_link::{self, Tool};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*};

use super::style::{self, pal};
use super::{Library, widgets};

/// What Steam has for the selected game (read when the game is selected or changed).
#[derive(Default)]
pub(super) struct SteamState {
    appid: Option<String>,
    tools: Vec<Tool>,
    /// Steam's compatibility tool for the game now (`None`: its default).
    current_tool: Option<String>,
    /// What differs from Arkor's settings ("Proton", "launch options").
    out_of_sync: Vec<&'static str>,
    steam_running: bool,
    /// Closing Steam and writing its files.
    applying: bool,
    error: Option<String>,
}

impl Library {
    fn steam_appid(&self) -> Option<String> {
        match &self.entry()?.kind {
            GameKind::Steam { appid } => Some(appid.clone()),
            _ => None,
        }
    }

    /// Reads Steam's settings for the selected game; when it was just selected, takes the
    /// launch options the user had in Steam into Arkor (once).
    pub(super) fn steam_refresh(&mut self, selected: bool) {
        let Some(appid) = self.steam_appid() else {
            self.steam = SteamState::default();
            return;
        };
        let Some(id) = self.selected_id() else { return };
        if selected && steam_link::adopt_launch_options(&appid, &id, &mut self.profile) {
            let saved = self.profile.save(&id);
            self.report(saved);
        }
        if selected || self.steam.tools.is_empty() {
            self.steam.tools = steam_link::tools();
        }
        self.steam.current_tool = steam_link::compat_tool(&appid);
        self.steam.out_of_sync = steam_link::out_of_sync(&appid, &id, &self.profile);
        self.steam.steam_running = steam_link::steam_running();
        self.steam.appid = Some(appid);
    }

    /// After a save: writes the game's settings into Steam when it is closed.
    pub(super) fn steam_after_save(&mut self) {
        let (Some(appid), Some(id)) = (self.steam_appid(), self.selected_id()) else {
            return;
        };
        if steam_link::out_of_sync(&appid, &id, &self.profile).is_empty() {
            self.steam.out_of_sync.clear();
            return;
        }
        if !steam_link::steam_running()
            && let Some(root) = steam_link::root()
            && let Err(e) = steam_link::apply_in(&root, &appid, &id, &self.profile)
        {
            self.steam.error = Some(format!("{e:#}"));
        }
        self.steam_refresh(false);
    }

    /// Closes Steam, writes the settings and opens Steam again.
    fn steam_apply_now(&mut self, cx: &mut Context<Self>) {
        let (Some(appid), Some(id)) = (self.steam_appid(), self.selected_id()) else {
            return;
        };
        let profile = self.profile.clone();
        self.steam.applying = true;
        self.steam.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let was_running = steam_link::steam_running();
                    steam_link::sync(&appid, &id, &profile)?;
                    if was_running {
                        steam_link::open_url("steam://open/games")?;
                    }
                    anyhow::Ok(())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.steam.applying = false;
                this.steam.error = result.err().map(|e| format!("{e:#}"));
                this.steam_refresh(false);
                cx.notify();
            });
        })
        .detach();
    }

    fn steam_set_tool(&mut self, tool: Option<String>, cx: &mut Context<Self>) {
        self.profile.steam.compat_tool = tool;
        self.save();
        cx.notify();
    }

    pub(super) fn render_steam(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(appid) = self.steam.appid.clone() else {
            return div();
        };
        let st = &self.steam;
        // Where things stand.
        let status = if st.applying {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(style::icon("refresh-cw", 14., pal::muted()))
                .child(widgets::label(self.tr(
                    "Closing Steam and applying…",
                    "Steam kapatılıp uygulanıyor…",
                )))
        } else if st.out_of_sync.is_empty() {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(style::icon("circle-check", 14., pal::success()))
                .child(widgets::label(
                    self.tr("Steam has these settings.", "Bu ayarlar Steam'de."),
                ))
        } else {
            let what = st
                .out_of_sync
                .iter()
                .map(|w| match *w {
                    "launch options" => self.tr("launch options", "başlatma seçenekleri"),
                    other => other,
                })
                .collect::<Vec<_>>()
                .join(", ");
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(style::icon("clock", 14., pal::warning()))
                        .child(widgets::label(if st.steam_running {
                            format!(
                                "{} ({what})",
                                self.tr(
                                    "Applied when you press Play",
                                    "Oyna'ya basınca uygulanır",
                                )
                            )
                        } else {
                            format!(
                                "{} ({what})",
                                self.tr("Not in Steam yet", "Henüz Steam'de değil")
                            )
                        })),
                )
                .child(
                    style::button(
                        "steam-apply",
                        if st.steam_running {
                            self.tr(
                                "Apply now (restarts Steam)",
                                "Şimdi uygula (Steam yeniden açılır)",
                            )
                        } else {
                            self.tr("Apply", "Uygula")
                        },
                        true,
                    )
                    .flex_none()
                    .on_click(cx.listener(|this, _, _, cx| this.steam_apply_now(cx))),
                )
        };

        // Proton: Steam's own choice, its default, or one of the tools it can use.
        let chosen = self.profile.steam.compat_tool.clone();
        let current = st.current_tool.as_ref().and_then(|n| {
            st.tools
                .iter()
                .find(|t| &t.name == n)
                .map(|t| t.display.clone())
                .or(Some(n.clone()))
        });
        let keep = match &current {
            Some(name) => format!("{} ({name})", self.tr("As set in Steam", "Steam'deki gibi")),
            None => self.tr("As set in Steam", "Steam'deki gibi").to_string(),
        };
        let mut chips = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                widgets::chip(("steam-tool", 0usize), keep, chosen.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.steam_set_tool(None, cx))),
            )
            .child(
                widgets::chip(
                    ("steam-tool", 1usize),
                    self.tr("Steam's default", "Steam varsayılanı"),
                    chosen.as_deref() == Some(""),
                )
                .on_click(
                    cx.listener(|this, _, _, cx| this.steam_set_tool(Some(String::new()), cx)),
                ),
            );
        for (ix, tool) in st.tools.iter().enumerate() {
            let name = tool.name.clone();
            chips = chips.child(
                widgets::chip(
                    ("steam-tool", 2 + ix),
                    tool.display.clone(),
                    chosen.as_deref() == Some(tool.name.as_str()),
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.steam_set_tool(Some(name.clone()), cx)),
                ),
            );
        }

        let through_arkor = !self.profile.steam.no_arkor;
        let mut card = widgets::section("Steam")
            .child(status)
            .when_some(st.error.clone(), |d, e| {
                d.child(div().text_sm().text_color(pal::warning()).child(format!("⚠ {e}")))
            })
            .child(widgets::divider())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().font_weight(gpui::FontWeight::MEDIUM).child("Proton"))
                    .child(chips),
            )
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("Start through Arkor", "Arkor üzerinden başlat"),
                self.tr(
                    "Steam's launch options become `arkor run … %command%`: MangoHud, lsfg-vk, the environment and the launch options set here apply. Off: Steam gets only the launch options from the Launch tab.",
                    "Steam'in başlatma seçenekleri `arkor run … %command%` olur: MangoHud, lsfg-vk, ortam ve burada yazılan başlatma seçenekleri uygulanır. Kapalı: Steam yalnızca Başlatma sekmesindeki seçenekleri alır.",
                ),
                widgets::switch(through_arkor)
                    .id("steam-through-arkor")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.profile.steam.no_arkor = !this.profile.steam.no_arkor;
                        this.save();
                        cx.notify();
                    })),
            ));
        let (details, validate) = (appid.clone(), appid);
        card = card.child(widgets::divider()).child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    style::button("steam-open", self.tr("Open in Steam", "Steam'de aç"), false)
                        .child(style::icon("external-link", 13., pal::muted()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let r = steam_link::open_url(&format!(
                                "steam://nav/games/details/{details}"
                            ));
                            if let Err(e) = r {
                                this.status = Some(format!("{e:#}").into());
                                cx.notify();
                            }
                        })),
                )
                .child(
                    style::button(
                        "steam-verify",
                        self.tr("Verify game files", "Oyun dosyalarını doğrula"),
                        false,
                    )
                    .child(style::icon("shield-check", 13., pal::muted()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let r = steam_link::open_url(&format!("steam://validate/{validate}"));
                        if let Err(e) = r {
                            this.status = Some(format!("{e:#}").into());
                            cx.notify();
                        }
                    })),
                ),
        );
        card
    }
}
