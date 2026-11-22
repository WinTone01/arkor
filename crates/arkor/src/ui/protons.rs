//! "Proton versions": installed builds and a downloader for GE-Proton, Proton-CachyOS,
//! DW-Proton and Proton-Wineland.

use std::collections::HashMap;
use std::path::PathBuf;

use arkor_launch::proton_dl::{self, Release, Variant};
use arkor_launch::tools;
use arkor_stores::prefix::human_size;
use gpui::{Context, Div, div, prelude::*};

use super::Library;
use super::widgets;

/// Sections of the Proton page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ProtonTab {
    #[default]
    Installed,
    Download,
    AntiCheat,
}

pub(super) struct ProtonsState {
    pub(super) tab: ProtonTab,
    anticheat_busy: bool,
    variant: Variant,
    releases: HashMap<&'static str, Result<Vec<Release>, String>>,
    loading: bool,
    installing: Option<String>,
}

impl Default for ProtonsState {
    fn default() -> Self {
        Self {
            tab: ProtonTab::Installed,
            anticheat_busy: false,
            variant: Variant::Ge,
            releases: HashMap::new(),
            loading: false,
            installing: None,
        }
    }
}

impl Library {
    fn load_releases(&mut self, variant: Variant, cx: &mut Context<Self>) {
        self.protons.loading = true;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move { proton_dl::list(variant) })
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = this.update(cx, |this, cx| {
                this.protons.loading = false;
                this.protons.releases.insert(variant.label(), result);
                cx.notify();
            });
        })
        .detach();
    }

    fn install_proton(&mut self, release: Release, cx: &mut Context<Self>) {
        self.protons.installing = Some(release.tag.clone());
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let tag = release.tag.clone();
            let result = bg.spawn(async move { proton_dl::install(&release) }).await;
            let _ = this.update(cx, |this, cx| {
                this.protons.installing = None;
                this.status = Some(match result {
                    Ok(dir) => format!("{tag} ✓ {}", dir.display()).into(),
                    Err(e) => format!("{tag}: {e:#}").into(),
                });
                this.tools = super::enhance::GameTools::scan();
                cx.notify();
            });
        })
        .detach();
    }

    fn download_anticheat(&mut self, cx: &mut Context<Self>) {
        self.protons.anticheat_busy = true;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { tools::download_anticheat() }).await;
            let _ = this.update(cx, |this, cx| {
                this.protons.anticheat_busy = false;
                this.report(result.map(|_| ()));
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_proton(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let result = proton_dl::uninstall(&path);
        self.report(result);
        self.tools = super::enhance::GameTools::scan();
        cx.notify();
    }

    pub(super) fn render_protons(&mut self, cx: &mut Context<Self>) -> Div {
        let variant = self.protons.variant;
        if !self.protons.releases.contains_key(variant.label()) && !self.protons.loading {
            self.load_releases(variant, cx);
        }
        let user_root = proton_dl::install_root().ok();
        let installed = tools::installed_protons();

        let mut installed_card = widgets::section(self.tr("Installed", "Kurulu"));
        if installed.is_empty() {
            installed_card =
                installed_card.child(widgets::label(self.tr("None yet.", "Henüz yok.")));
        }
        for (ix, (name, path)) in installed.iter().enumerate() {
            let removable = user_root.as_ref().is_some_and(|r| path.starts_with(r));
            let p = path.clone();
            installed_card = installed_card.child(
                div()
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
                            .child(div().child(name.clone()))
                            .child(
                                div()
                                    .truncate()
                                    .child(widgets::label(path.display().to_string())),
                            ),
                    )
                    .child(if removable {
                        super::style::button(
                            ("proton-remove", ix),
                            self.tr("Remove", "Kaldır"),
                            false,
                        )
                        .child(super::style::icon(
                            "trash-2",
                            13.,
                            super::style::pal::danger(),
                        ))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.remove_proton(p.clone(), cx)),
                        )
                        .into_any_element()
                    } else {
                        widgets::label(source_label(path, self)).into_any_element()
                    }),
            );
        }

        let mut tabs = div().flex().flex_wrap().gap_2();
        for (ix, v) in Variant::ALL.into_iter().enumerate() {
            tabs = tabs.child(
                widgets::chip(("variant", ix), v.label(), v == variant).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.protons.variant = v;
                        cx.notify();
                    },
                )),
            );
        }
        let mut available = widgets::section(self.tr("Download", "İndir")).child(widgets::setting_row(
            self.tr("Build", "Derleme"),
            self.tr("GE-Proton suits most games; CachyOS is tuned for newer CPUs.", "GE-Proton çoğu oyuna uyar; CachyOS yeni işlemciler için ayarlıdır."),
            tabs,
        )).child(widgets::label(self.tr(
            "Installed into ~/.local/share/Steam/compatibilitytools.d, so Steam can use them too. Checked against the release checksum.",
            "~/.local/share/Steam/compatibilitytools.d içine kurulur, böylece Steam de kullanabilir. Sürümün sağlama toplamıyla doğrulanır.",
        )));
        if self.protons.loading {
            available = available.child(widgets::label(
                self.tr("Loading releases…", "Sürümler yükleniyor…"),
            ));
        }
        match self.protons.releases.get(variant.label()) {
            Some(Err(e)) => available = available.child(widgets::label(format!("⚠ {e}"))),
            Some(Ok(list)) if list.is_empty() => {
                available = available.child(widgets::label(self.tr("No releases.", "Sürüm yok.")))
            }
            Some(Ok(list)) => {
                for (ix, r) in list.iter().enumerate() {
                    let have = installed.iter().any(|(name, _)| {
                        let n = name.to_lowercase();
                        let t = r.tag.to_lowercase();
                        n == t
                            || n.ends_with(&t)
                            || n.contains(
                                t.trim_start_matches("cachyos-")
                                    .trim_start_matches("dwproton-"),
                            )
                    });
                    let busy = self.protons.installing.as_deref() == Some(r.tag.as_str());
                    let release = r.clone();
                    let action = if busy {
                        widgets::label(self.tr("Installing…", "Kuruluyor…")).into_any_element()
                    } else if have {
                        widgets::label(self.tr("Installed", "Kurulu")).into_any_element()
                    } else if self.protons.installing.is_some() {
                        div().into_any_element()
                    } else {
                        super::style::button(
                            ("proton-install", ix),
                            self.tr("Install", "Kur"),
                            false,
                        )
                        .child(super::style::icon(
                            "download",
                            13.,
                            super::style::pal::muted(),
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.install_proton(release.clone(), cx)
                        }))
                        .into_any_element()
                    };
                    available = available.child(
                        div()
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
                                    .gap_3()
                                    .child(div().child(r.tag.clone()))
                                    .child(widgets::label(human_size(r.size))),
                            )
                            .child(action),
                    );
                }
            }
            None => {}
        }

        let ac = tools::anticheat();
        let ac_row = |name: &'static str, path: &Option<PathBuf>| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(super::style::pal::raised().opacity(0.45))
                .child(div().child(name))
                .child(widgets::label(match path {
                    Some(p) => p.display().to_string(),
                    None => self.tr("not installed", "kurulu değil").to_string(),
                }))
        };
        let mut anticheat = widgets::section(self.tr("Anti-cheat runtimes", "Anti-cheat runtime'ları"))
            .child(widgets::label(self.tr(
                "Games with EasyAntiCheat or BattlEye need these (if the developer allows Linux). Steam's own are used when installed.",
                "EasyAntiCheat ya da BattlEye kullanan oyunlar bunlara ihtiyaç duyar (geliştirici Linux'a izin veriyorsa). Kuruluysa Steam'inkiler kullanılır.",
            )))
            .child(ac_row("EasyAntiCheat", &ac.eac))
            .child(ac_row("BattlEye", &ac.battleye));
        if ac.eac.is_none() || ac.battleye.is_none() {
            anticheat = anticheat.child(div().flex().child(if self.protons.anticheat_busy {
                widgets::label(self.tr("Downloading…", "İndiriliyor…")).into_any_element()
            } else {
                super::style::button(
                    "anticheat-dl",
                    self.tr(
                        "Download (from Faugus components)",
                        "İndir (Faugus bileşenlerinden)",
                    ),
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| this.download_anticheat(cx)))
                .into_any_element()
            }));
        }

        use super::style;
        let tab = self.protons.tab;
        let mut entries = vec![
            style::NavEntry::new("hard-drive", self.tr("Installed", "Kurulu")),
            style::NavEntry::new("download", self.tr("Download", "İndir")),
            style::NavEntry::new("shield", self.tr("Anti-cheat", "Anti-cheat")),
        ];
        entries[0].badge = Some(installed.len().to_string()).filter(|n| n != "0");
        let tabs = [
            ProtonTab::Installed,
            ProtonTab::Download,
            ProtonTab::AntiCheat,
        ];
        let nav = style::side_nav(
            "proton-tab",
            entries,
            tabs.iter().position(|t| *t == tab),
            cx,
            move |this, i, cx| {
                this.protons.tab = tabs[i];
                cx.notify();
            },
        );
        let content = match tab {
            ProtonTab::Installed => installed_card,
            ProtonTab::Download => available,
            ProtonTab::AntiCheat => anticheat,
        };
        div()
            .child(Self::header(
                self.tr("Proton & Wine", "Proton ve Wine"),
                self.tr(
                    "Builds Windows games run with. Pick one per game on the game's page; Automatic uses the best installed.",
                    "Windows oyunlarının çalıştığı derlemeler. Oyun sayfasında oyun başına seçilir; Otomatik en iyi kurulu sürümü kullanır.",
                ),
            ))
            .child(style::with_side_nav(nav, content))
    }
}

/// Who provides an installed build that Arkor won't remove.
fn source_label(path: &std::path::Path, lib: &Library) -> String {
    let p = path.display().to_string();
    if p.starts_with("/usr/") {
        lib.tr("system package", "sistem paketi").into()
    } else if p.contains("/heroic/") {
        "Heroic".into()
    } else if p.contains("/Steam/") || p.contains("/.steam/") {
        "Steam".into()
    } else {
        path.parent()
            .map(|d| d.display().to_string())
            .unwrap_or_default()
    }
}
