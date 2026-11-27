//! "System": a short overview of this computer (OS, kernel, CPU, memory, GPU, driver,
//! session, scheduler) with a copy button for bug reports. The optional tools Arkor works
//! with are on the Downloads page (`render_system_tools`), next to everything else that
//! gets installed.

use arkor_launch::tools::{self, ToolStatus};
use gpui::prelude::FluentBuilder;
use gpui::{ClipboardItem, Context, Div, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};

pub(super) type SystemInfo = (Vec<(&'static str, String)>, Vec<ToolStatus>);

#[derive(Default)]
pub(super) struct SystemState {
    pub(super) info: Option<SystemInfo>,
    pub(super) loading: bool,
    /// Missing packages split into (in the pacman repos, elsewhere such as the AUR).
    pub(super) split: Option<(Vec<&'static str>, Vec<&'static str>)>,
    pub(super) installing: bool,
}

/// Shorter names for the tiles: "NVIDIA GeForce RTX 3060" instead of lspci's
/// "NVIDIA Corporation GA106 [GeForce RTX 3060]", no (R)/(TM) in CPU names.
pub(super) fn tidy(key: &str, value: &str) -> String {
    match key {
        // The model is in the last [...] (AMD names have "[AMD/ATI]" first).
        "GPU" => match (value.rfind('['), value.rfind(']')) {
            (Some(a), Some(b)) if b > a => {
                let maker = value.split_whitespace().next().unwrap_or("");
                let maker = if maker.eq_ignore_ascii_case("Advanced") {
                    "AMD"
                } else {
                    maker
                };
                format!("{maker} {}", &value[a + 1..b])
            }
            _ => value.to_string(),
        },
        "CPU" => value
            .replace("(R)", "")
            .replace("(TM)", "")
            .replace("12th Gen ", "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        _ => value.to_string(),
    }
}

/// Icon and caption for a line of `tools::system_info`.
pub(super) fn info_look(key: &str, lib: &Library) -> (&'static str, &'static str) {
    match key {
        "OS" => ("monitor", lib.tr("Operating system", "İşletim sistemi")),
        "Kernel" => ("terminal", lib.tr("Kernel", "Çekirdek")),
        "CPU" => ("cpu", lib.tr("Processor", "İşlemci")),
        "RAM" => ("memory-stick", lib.tr("Memory", "Bellek")),
        "GPU" => ("gpu", lib.tr("Graphics card", "Ekran kartı")),
        "Driver" => (
            "microchip",
            lib.tr("Graphics driver", "Ekran kartı sürücüsü"),
        ),
        "Session" => ("app-window", lib.tr("Desktop session", "Masaüstü oturumu")),
        _ => (
            "monitor-cog",
            lib.tr("CPU scheduler (sched_ext)", "CPU zamanlayıcı (sched_ext)"),
        ),
    }
}

impl Library {
    pub(super) fn scan_system(&mut self, cx: &mut Context<Self>) {
        if self.system.loading {
            return;
        }
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
        let page = div().child(Self::header(
            self.tr("System", "Sistem"),
            self.tr(
                "What Arkor sees on this computer: graphics cards, drivers and the tools games use. Useful when you ask someone for help.",
                "Arkor'un bu bilgisayarda gördükleri: ekran kartları, sürücüler ve oyunların kullandığı araçlar. Birinden yardım isterken işe yarar.",
            ),
        ));
        let Some((info, _)) = self.system.info.clone() else {
            return page.child(self.loading(self.tr("Looking…", "Bakılıyor…")));
        };
        let value = |key: &str| {
            info.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| tidy(key, v))
                .unwrap_or_default()
        };

        // The headline: the distribution and the graphics card, the two that matter most.
        let hero = style::card()
            .p_5()
            .flex()
            .items_center()
            .gap_5()
            .child(
                div()
                    .size(px(64.))
                    .flex_none()
                    .rounded(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(style::accent_gradient())
                    .child(style::icon("monitor", 30., pal::on_accent())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .truncate()
                            .child(value("OS")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(pal::muted())
                            .truncate()
                            .child(format!(
                                "{} · {} {}",
                                value("Session"),
                                self.tr("kernel", "çekirdek"),
                                value("Kernel")
                            )),
                    ),
            )
            .child(
                style::button(
                    "copy-system",
                    self.tr("Copy diagnostics", "Teşhis bilgisini kopyala"),
                    false,
                )
                .child(style::icon("copy", 13., pal::muted()))
                .on_click(cx.listener(move |_, _, _, cx| {
                    // The whole report, for an issue: the desktop's tools and what its windows
                    // answered too (it asks pacman, so not here).
                    let bg = cx.background_executor().clone();
                    cx.spawn(async move |this, cx| {
                        let text = bg.spawn(async { tools::diagnostics() }).await;
                        let _ = this.update(cx, |this, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            this.status = Some(
                                this.tr(
                                    "Copied: paste it into the issue.",
                                    "Kopyalandı: hata bildirimine yapıştırabilirsin.",
                                )
                                .into(),
                            );
                            cx.notify();
                        });
                    })
                    .detach();
                })),
            );

        // One tile per fact, two per row.
        let tile = |key: &'static str, val: String| {
            let (icon, caption) = info_look(key, self);
            style::card()
                .flex_1()
                .min_w(px(260.))
                .p_4()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .size(px(40.))
                        .flex_none()
                        .rounded(px(11.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(pal::accent().opacity(0.12))
                        .child(style::icon(icon, 19., pal::accent_light())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(div().text_xs().text_color(pal::faint()).child(caption))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .truncate()
                                .child(if val.is_empty() || val == "-" {
                                    "—".to_string()
                                } else {
                                    val
                                }),
                        ),
                )
        };
        let mut grid = div().flex().flex_wrap().gap_3();
        for key in ["CPU", "RAM", "GPU", "Driver", "Kernel", "sched_ext"] {
            grid = grid.child(tile(key, value(key)));
        }
        let storage = self.render_storage(cx);
        page.child(div().flex().flex_col().gap_4().child(hero).child(grid).child(storage).child(
            div()
                .id("system-to-tools")
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(pal::muted())
                .cursor_pointer()
                .hover(|s| s.text_color(pal::text()))
                .child(style::icon("toolbox", 14., pal::accent_light()))
                .child(self.tr(
                    "Optional tools (MangoHud, Gamescope, winetricks…): Downloads → System tools",
                    "İsteğe bağlı araçlar (MangoHud, Gamescope, winetricks…): İndirilenler → Sistem araçları",
                ))
                .child(style::icon("chevron-right", 14., pal::faint()))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.downloads_show_tools();
                    this.go(super::shell::Nav::Downloads, cx);
                })),
        ))
    }

    /// Installs packages as a download job (shown with its progress in Downloads): repository
    /// ones with pacman, AUR ones with the AUR helper in a terminal.
    fn install_missing(&mut self, pkgs: Vec<&'static str>, aur: bool, cx: &mut Context<Self>) {
        if self.system.installing {
            return;
        }
        self.system.installing = true;
        self.watch_progress(cx);
        cx.notify();
        let label = pkgs.join(", ");
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let _job = arkor_launch::progress::job("system-tools", &label);
                    if aur {
                        tools::install_aur(&pkgs)
                    } else {
                        tools::install_packages(&pkgs)
                    }
                })
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
    }

    /// Downloads → System tools: the optional programs Arkor works with, whether they are
    /// installed (and which version), and the missing ones to install with pacman.
    pub(super) fn render_system_tools(&mut self, cx: &mut Context<Self>) -> Div {
        if self.system.info.is_none() && !self.system.loading {
            self.scan_system(cx);
        }
        let Some((_, list)) = self.system.info.clone() else {
            return div().child(self.loading(self.tr("Looking…", "Bakılıyor…")));
        };
        let (repo, other) = self.system.split.clone().unwrap_or_default();
        let missing = list.iter().filter(|t| t.found.is_none()).count();

        let mut out = div().flex().flex_col().gap_4();
        // What can be installed in one go.
        if missing > 0 {
            let mut card =
                style::card().p_4().flex().flex_col().gap_3().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(style::icon("package-x", 18., pal::warning()))
                        .child(div().flex_1().font_weight(gpui::FontWeight::MEDIUM).child(
                            format!(
                                "{missing} {}",
                                if missing == 1 {
                                    self.tr("tool is not installed", "araç kurulu değil")
                                } else {
                                    self.tr("tools are not installed", "araç kurulu değil")
                                }
                            ),
                        )),
                );
            if !repo.is_empty() {
                let pkgs = repo.clone();
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px_3()
                                .py_2()
                                .rounded(px(9.))
                                .bg(pal::raised().opacity(0.6))
                                .text_xs()
                                .font_family("monospace")
                                .text_color(pal::text_soft())
                                .truncate()
                                .child(arkor_launch::pkg::install_line(&repo).unwrap_or_default()),
                        )
                        .child(if self.system.installing {
                            widgets::label(self.tr("Installing…", "Kuruluyor…")).into_any_element()
                        } else {
                            style::button(
                                "tools-install",
                                self.tr("Install them", "Hepsini kur"),
                                true,
                            )
                            .child(style::icon("download", 13., pal::on_accent()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.install_missing(pkgs.clone(), false, cx)
                            }))
                            .into_any_element()
                        }),
                );
            }
            if !other.is_empty() {
                let pkgs = other.clone();
                let action = match tools::aur_command(&other) {
                    _ if self.system.installing => {
                        widgets::label(self.tr("Installing…", "Kuruluyor…")).into_any_element()
                    }
                    Some(_) => style::button("tools-aur", self.tr("Download", "İndir"), true)
                        .child(style::icon("download", 13., pal::on_accent()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.install_missing(pkgs.clone(), true, cx)
                        }))
                        .into_any_element(),
                    None if !tools::has_aur() => widgets::label(self.tr(
                        "Not in this distribution's packages: install them from their own sites or Flatpak.",
                        "Bu dağıtımın paketlerinde yok: kendi sitelerinden ya da Flatpak ile kur.",
                    ))
                    .into_any_element(),
                    None => widgets::label(self.tr(
                        "Install an AUR helper (paru or yay) to download these here.",
                        "Bunları buradan indirmek için bir AUR yardımcısı (paru ya da yay) kur.",
                    ))
                    .into_any_element(),
                };
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(div().flex_1().min_w_0().child(widgets::label(format!(
                            "{} {}",
                            if tools::has_aur() {
                                self.tr(
                                    "From the AUR (a terminal opens to build it; confirm there):",
                                    "AUR'dan (derlemek için bir terminal açılır; orada onayla):",
                                )
                            } else {
                                self.tr("Not packaged here:", "Bu dağıtımda paketi yok:")
                            },
                            other.join(", ")
                        ))))
                        .child(action),
                );
            }
            out = out.child(card);
        }

        let mut rows = style::card().overflow_hidden().flex().flex_col();
        let n = list.len();
        for (ix, t) in list.iter().enumerate() {
            let ok = t.found.is_some();
            rows = rows.child(
                div()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(ix + 1 < n, |d| {
                        d.border_b_1().border_color(pal::glass_spec().border)
                    })
                    .child(style::icon(
                        if ok { "package-check" } else { "package" },
                        17.,
                        if ok { pal::success() } else { pal::faint() },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(t.name),
                    )
                    .child(match &t.found {
                        Some(v) => div().text_sm().text_color(pal::muted()).child(v.clone()),
                        None if self.system.installing => style::tag(
                            format!("{} · {}", self.tr("missing", "eksik"), t.package),
                            pal::warning(),
                        ),
                        None => {
                            let pkg = t.package;
                            let aur = other.contains(&pkg);
                            div().flex().items_center().gap_2().child(
                                style::button(
                                    ("tool-install", ix),
                                    self.tr("Download", "İndir"),
                                    false,
                                )
                                .child(style::icon("download", 13., pal::muted()))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.install_missing(vec![pkg], aur, cx),
                                )),
                            )
                        }
                    }),
            );
        }
        out.child(rows).child(
            div().flex().child(
                style::button(
                    "tools-rescan",
                    self.tr("Check again", "Tekrar kontrol et"),
                    false,
                )
                .child(style::icon("refresh-cw", 13., pal::muted()))
                .on_click(cx.listener(|this, _, _, cx| this.scan_system(cx))),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tidies_hardware_names() {
        assert_eq!(
            super::tidy(
                "GPU",
                "NVIDIA Corporation GA106 [GeForce RTX 3060 Lite Hash Rate] (rev a1)"
            ),
            "NVIDIA GeForce RTX 3060 Lite Hash Rate"
        );
        assert_eq!(
            super::tidy(
                "GPU",
                "Advanced Micro Devices, Inc. [AMD/ATI] Navi 48 [Radeon RX 9070 XT]"
            ),
            "AMD Radeon RX 9070 XT"
        );
        assert_eq!(
            super::tidy("CPU", "12th Gen Intel(R) Core(TM) i5-12400F (12 threads)"),
            "Intel Core i5-12400F (12 threads)"
        );
    }
}
