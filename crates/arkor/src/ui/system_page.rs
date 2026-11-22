//! "System": what Arkor sees on this machine, and which tools are present (with the Arch
//! package to install when one is missing).

use arkor_launch::tools::{self, ToolStatus};
use gpui::{Context, Div, div, prelude::*};

use super::Library;
use super::widgets::{self, theme::*};

type SystemInfo = (Vec<(&'static str, String)>, Vec<ToolStatus>);

#[derive(Default)]
pub(super) struct SystemState {
    info: Option<SystemInfo>,
    loading: bool,
    /// Missing packages split into (in the pacman repos, elsewhere such as the AUR).
    split: Option<(Vec<&'static str>, Vec<&'static str>)>,
    installing: bool,
}

impl Library {
    fn scan_system(&mut self, cx: &mut Context<Self>) {
        self.system.loading = true;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let info = bg
                .spawn(async move {
                    let info = (tools::system_info(), tools::tool_status());
                    let missing: Vec<&'static str> = info
                        .1
                        .iter()
                        .filter(|t| t.found.is_none())
                        .map(|t| t.package)
                        .collect();
                    let split = tools::split_repo_packages(&missing);
                    (info, split)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.system.loading = false;
                this.system.info = Some(info.0);
                this.system.split = Some(info.1);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_system(&mut self, cx: &mut Context<Self>) -> Div {
        if self.system.info.is_none() && !self.system.loading {
            self.scan_system(cx);
        }
        let mut page = div().child(Self::header(
            self.tr("System", "Sistem"),
            self.tr(
                "What Arkor found on this computer.",
                "Arkor'un bu bilgisayarda bulduğu.",
            ),
        ));
        let Some((info, tools)) = &self.system.info else {
            return page.child(widgets::label(self.tr("Looking…", "Bakılıyor…")));
        };
        let row = |label: String, value: Div| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_1()
                .rounded_md()
                .bg(super::style::pal::raised().opacity(0.45))
                .child(
                    div()
                        .w(gpui::px(220.))
                        .text_sm()
                        .text_color(MUTED())
                        .child(label),
                )
                .child(value.flex_1().min_w_0())
        };
        let mut sys = widgets::section(self.tr("Machine", "Makine"));
        for (label, value) in info {
            sys = sys.child(row(
                label.to_string(),
                div().truncate().text_sm().child(value.clone()),
            ));
        }
        let missing = tools.iter().filter(|t| t.found.is_none()).count();
        let mut tool_card = widgets::section(self.tr("Tools", "Araçlar"));
        for t in tools {
            let value = match &t.found {
                Some(v) => div().text_sm().text_color(TEXT()).child(format!("✓ {v}")),
                None => div().text_sm().text_color(ACCENT()).child(format!(
                    "{} · sudo pacman -S {}",
                    self.tr("missing", "eksik"),
                    t.package
                )),
            };
            tool_card = tool_card.child(row(t.name.to_string(), value));
        }
        if missing > 0
            && let Some((repo, other)) = &self.system.split
        {
            if !repo.is_empty() {
                let pkgs = repo.clone();
                tool_card = tool_card
                    .child(widgets::label(self.tr(
                        "Install the missing packages from the Arch repositories:",
                        "Eksik paketleri Arch depolarından kur:",
                    )))
                    .child(
                        div()
                            .p_2()
                            .rounded_md()
                            .bg(super::style::pal::raised().opacity(0.45))
                            .text_color(ACCENT())
                            .text_sm()
                            .child(format!("sudo pacman -S --needed {}", repo.join(" "))),
                    )
                    .child(
                        div().flex().child(
                            widgets::chip(
                                "system-install",
                                if self.system.installing {
                                    self.tr("Installing…", "Kuruluyor…")
                                } else {
                                    self.tr(
                                        "Install now (asks for your password)",
                                        "Şimdi kur (parolanı sorar)",
                                    )
                                },
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if this.system.installing {
                                        return;
                                    }
                                    this.system.installing = true;
                                    cx.notify();
                                    let pkgs = pkgs.clone();
                                    let bg = cx.background_executor().clone();
                                    cx.spawn(async move |this, cx| {
                                        let result = bg
                                            .spawn(async move { tools::install_packages(&pkgs) })
                                            .await;
                                        let _ = this.update(cx, |this, cx| {
                                            this.system.installing = false;
                                            this.status = Some(match result {
                                                Ok(()) => this.tr("Installed.", "Kuruldu.").into(),
                                                Err(e) => format!("{e:#}").into(),
                                            });
                                            this.scan_system(cx);
                                        });
                                    })
                                    .detach();
                                },
                            )),
                        ),
                    );
            }
            if !other.is_empty() {
                tool_card = tool_card.child(widgets::label(format!(
                    "{} {}",
                    self.tr(
                        "Not in the repositories (AUR or another distribution's package):",
                        "Depolarda yok (AUR ya da başka bir dağıtımın paketi):"
                    ),
                    other.join(", ")
                )));
            }
        }
        page = page.child(sys).child(tool_card).child(
            div().flex().child(
                widgets::chip(
                    "system-rescan",
                    self.tr("Check again", "Tekrar kontrol et"),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| this.scan_system(cx))),
            ),
        );
        page
    }
}
