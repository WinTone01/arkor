//! Arkor's own updates in the window: a check at start (when automatic updates are on), a
//! card in Settings → General and a note in the sidebar when a new version is out.

use arkor_launch::self_update::{self, Release};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};

#[derive(Default)]
pub(super) struct AppUpdate {
    pub(super) checking: bool,
    pub(super) installing: bool,
    /// A newer release, once found.
    pub(super) available: Option<Release>,
    /// Result of the last check or install, for the card.
    pub(super) message: Option<String>,
}

impl Library {
    /// Looks for a newer Arkor in the background. `quiet`: no "up to date" message (the
    /// check at start).
    pub(super) fn check_app_update(&mut self, quiet: bool, cx: &mut Context<Self>) {
        if self.app_update.checking || self.app_update.installing {
            return;
        }
        self.app_update.checking = true;
        self.app_update.message = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { self_update::check() }).await;
            let _ = this.update(cx, |this, cx| {
                this.app_update.checking = false;
                match result {
                    Ok(Some(r)) => this.app_update.available = Some(r),
                    Ok(None) => {
                        this.app_update.available = None;
                        if !quiet {
                            this.app_update.message = Some(format!(
                                "{} ({})",
                                this.tr("Arkor is up to date", "Arkor güncel"),
                                self_update::current_version()
                            ));
                        }
                    }
                    Err(e) if !quiet => this.app_update.message = Some(format!("{e:#}")),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Downloads the update, replaces the program and starts it again.
    pub(super) fn install_app_update(&mut self, cx: &mut Context<Self>) {
        let Some(release) = self.app_update.available.clone() else {
            return;
        };
        if self.app_update.installing {
            return;
        }
        self.app_update.installing = true;
        self.app_update.message = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let label = format!("Arkor {}", release.version);
            let result = bg
                .spawn(async move {
                    let _job = arkor_launch::progress::job("arkor-update", &label);
                    self_update::install(&release)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.app_update.installing = false;
                let console = this.console.on;
                match result.and_then(|exe| self_update::restart(&exe, console)) {
                    Ok(()) => cx.quit(),
                    Err(e) => {
                        this.app_update.message = Some(format!("{e:#}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Settings → General: the running version, a check and, when found, the update.
    pub(super) fn render_app_update(&mut self, cx: &mut Context<Self>) -> Div {
        let u = &self.app_update;
        let mut card = widgets::section(self.tr("Arkor updates", "Arkor güncellemeleri")).child(widgets::setting_row(
            format!("Arkor {}", self_update::current_version()),
            self.tr(
                "New versions come from Arkor's GitHub releases. Updating restarts Arkor; running games keep running.",
                "Yeni sürümler Arkor'un GitHub sürümlerinden gelir. Güncellemek Arkor'u yeniden başlatır; açık oyunlar çalışmaya devam eder.",
            ),
            if u.checking {
                self.loading_inline(self.tr("Checking…", "Denetleniyor…")).into_any_element()
            } else {
                style::button("app-update-check", self.tr("Check for updates", "Güncellemeleri denetle"), false)
                    .child(style::icon("refresh-cw", 13., pal::muted()))
                    .on_click(cx.listener(|this, _, _, cx| this.check_app_update(false, cx)))
                    .into_any_element()
            },
        ));
        if let Some(r) = &u.available {
            let size = arkor_launch::progress::human_bytes(r.size);
            card = card.child(
                div()
                    .p_3()
                    .rounded(px(12.))
                    .bg(pal::accent().opacity(0.1))
                    .border_1()
                    .border_color(pal::accent().opacity(0.35))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(style::icon("sparkles", 18., pal::accent_light()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(format!(
                                "{} {}",
                                self.tr("New version:", "Yeni sürüm:"),
                                r.version
                            )))
                            .child(
                                div().text_sm().text_color(pal::muted()).child(
                                    r.notes
                                        .lines()
                                        .find(|l| !l.trim().is_empty())
                                        .unwrap_or("")
                                        .trim_start_matches('#')
                                        .trim()
                                        .to_string(),
                                ),
                            )
                            .child(div().text_xs().text_color(pal::faint()).child(size)),
                    )
                    .child(if u.installing {
                        widgets::label(self.tr("Updating…", "Güncelleniyor…")).into_any_element()
                    } else {
                        style::button(
                            "app-update-install",
                            self.tr("Update and restart", "Güncelle ve yeniden başlat"),
                            true,
                        )
                        .child(style::icon("download", 13., pal::on_accent()))
                        .on_click(cx.listener(|this, _, _, cx| this.install_app_update(cx)))
                        .into_any_element()
                    }),
            );
        }
        card.when_some(u.message.clone(), |c, m| c.child(widgets::label(m)))
    }

    /// A small note in the sidebar while a newer Arkor is available.
    pub(super) fn render_update_note(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<Div>> {
        let r = self.app_update.available.as_ref()?;
        Some(
            div()
                .id("app-update-note")
                .mb_2()
                .px_3()
                .py_2()
                .rounded(px(10.))
                .bg(pal::accent().opacity(0.1))
                .border_1()
                .border_color(pal::accent().opacity(0.3))
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(|s| s.bg(pal::accent().opacity(0.16)))
                .child(style::icon("sparkles", 14., pal::accent_light()))
                .child(div().text_sm().child(format!(
                    "{} {}",
                    self.tr("Update to", "Güncelle:"),
                    r.version
                )))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings_tab = super::global_page::SettingsTab::General;
                    this.go(super::shell::Nav::Settings, cx);
                })),
        )
    }
}
