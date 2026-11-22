//! "Proton versions": installed builds and a downloader for GE-Proton, Proton-CachyOS,
//! DW-Proton and Proton-Wineland.

use std::collections::HashMap;
use std::path::PathBuf;

use arkor_launch::proton_dl::{self, Release, Variant};
use arkor_launch::{components, tools};
use arkor_stores::prefix::human_size;
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*};

use super::Library;
use super::widgets;

/// Sections of the Proton page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ProtonTab {
    #[default]
    Auto,
    Installed,
    Download,
    AntiCheat,
}

/// Something the "Auto-updated" section can install or update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Component {
    Umu,
    AntiCheat,
    Proton(components::Latest),
}

pub(super) struct ProtonsState {
    pub(super) tab: ProtonTab,
    /// Auto-updated component being installed or checked.
    updating: Option<Component>,
    anticheat_busy: bool,
    variant: Variant,
    releases: HashMap<&'static str, Result<Vec<Release>, String>>,
    loading: bool,
    installing: Option<String>,
}

impl Default for ProtonsState {
    fn default() -> Self {
        Self {
            tab: ProtonTab::Auto,
            updating: None,
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

    /// Installs or updates one auto-updated component now (also what happens before a game).
    fn update_component(&mut self, c: Component, cx: &mut Context<Self>) {
        self.protons.updating = Some(c);
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    match c {
                        Component::Umu => components::ensure_umu(true).map(|_| ()),
                        Component::AntiCheat => components::ensure_anticheat(true).map(|_| ()),
                        Component::Proton(l) => components::ensure_latest(l, true).map(|_| ()),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.protons.updating = None;
                this.report(result);
                this.tools = super::enhance::GameTools::scan();
                cx.notify();
            });
        })
        .detach();
    }

    /// One component: name, what it is and its version, and an install / update button.
    fn component_row(
        &self,
        c: Component,
        ix: usize,
        name: &'static str,
        hint: &'static str,
        version: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        use super::style::{self, pal};
        let busy = self.protons.updating;
        let installed = version.is_some();
        let state = match version {
            Some(v) => format!("{} {v}", self.tr("Installed:", "Kurulu:")),
            None => self
                .tr("Not installed yet", "Henüz kurulu değil")
                .to_string(),
        };
        let control = if busy == Some(c) {
            widgets::label(self.tr("Working…", "Çalışıyor…")).into_any_element()
        } else {
            style::button(
                ("auto-update", ix),
                if installed {
                    self.tr("Check for update", "Güncellemeyi denetle")
                } else {
                    self.tr("Install", "Kur")
                },
                !installed,
            )
            .child(style::icon(
                if installed { "refresh-cw" } else { "download" },
                13.,
                if installed {
                    pal::muted()
                } else {
                    pal::on_accent()
                },
            ))
            .when(busy.is_some(), |b| b.opacity(0.4))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.protons.updating.is_none() {
                    this.update_component(c, cx);
                }
            }))
            .into_any_element()
        };
        widgets::setting_row(name, format!("{hint} · {state}"), control)
    }

    fn render_auto(&mut self, cx: &mut Context<Self>) -> Div {
        use components::Latest;
        let tools_card = widgets::section(self.tr("Launch components", "Başlatma bileşenleri"))
            .child(widgets::label(self.tr(
                "What Windows games start with, kept up to date the way Faugus Launcher does (its builds, from github.com/Faugus).",
                "Windows oyunlarının başlatıldığı bileşenler, Faugus Launcher'ın yaptığı gibi güncel tutulur (onun derlemeleri, github.com/Faugus).",
            )))
            .child(self.component_row(
                Component::Umu,
                0,
                "umu-launcher",
                self.tr("Runs Proton outside Steam", "Proton'u Steam dışında çalıştırır"),
                components::umu_version(),
                cx,
            ))
            .child(widgets::divider())
            .child(self.component_row(
                Component::AntiCheat,
                1,
                "EasyAntiCheat & BattlEye",
                self.tr("For games that allow them on Linux", "Linux'ta izin veren oyunlar için"),
                components::anticheat_version(),
                cx,
            ));
        let mut protons_card = widgets::section(self.tr("Always-newest Proton", "Hep en yeni Proton"))
            .child(widgets::label(self.tr(
                "Pick one on a game's page (Runtime). It is downloaded on first use and replaced in place when a new release is out; Steam lists it too.",
                "Bir oyunun sayfasında seç (Çalışma ortamı). İlk kullanımda indirilir, yeni sürüm çıkınca yerinde değiştirilir; Steam de listeler.",
            )));
        for (i, l) in Latest::ALL.into_iter().enumerate() {
            let hint = match l {
                Latest::CachyOs => self.tr(
                    "CachyOS' build, tuned for newer CPUs",
                    "CachyOS derlemesi, yeni işlemciler için ayarlı",
                ),
                Latest::Ge => self.tr(
                    "GloriousEggroll's, with the most game fixes",
                    "GloriousEggroll'un, en çok oyun düzeltmesiyle",
                ),
                Latest::Dw => self.tr("Dawn Winery's", "Dawn Winery'nin"),
                Latest::Wineland => self.tr(
                    "CachyOS with Wayland-first patches",
                    "Wayland öncelikli yamalarla CachyOS",
                ),
            };
            if i > 0 {
                protons_card = protons_card.child(widgets::divider());
            }
            protons_card = protons_card.child(self.component_row(
                Component::Proton(l),
                2 + i,
                l.dir_name(),
                hint,
                l.installed_version(),
                cx,
            ));
        }
        let off = self.global.no_auto_updates;
        let switch_card = widgets::section(self.tr("Automatic updates", "Otomatik güncelleme")).child(
            widgets::setting_row(
                self.tr("Check before every game start", "Her oyun başlamadan önce denetle"),
                self.tr(
                    "A quick check of each release; nothing is downloaded when it is current. Off: only missing components are downloaded.",
                    "Her sürüm için kısa bir kontrol; güncelse hiçbir şey indirilmez. Kapalı: yalnızca eksik bileşenler indirilir.",
                ),
                widgets::switch(!off)
                    .id("auto-updates")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.global.no_auto_updates = !this.global.no_auto_updates;
                        let saved = this.global.save();
                        this.report(saved);
                        cx.notify();
                    })),
            ),
        );
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(switch_card)
            .child(tools_card)
            .child(protons_card)
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
        anticheat = anticheat.child(widgets::label(self.tr(
            "When Steam's runtimes aren't installed, Arkor downloads Faugus Launcher's copy (github.com/Faugus/components) and keeps it updated.",
            "Steam'in runtime'ları kurulu değilse Arkor, Faugus Launcher'ın kopyasını (github.com/Faugus/components) indirir ve güncel tutar.",
        )));
        if ac.eac.is_none() || ac.battleye.is_none() {
            anticheat = anticheat.child(div().flex().child(if self.protons.anticheat_busy {
                widgets::label(self.tr("Downloading…", "İndiriliyor…")).into_any_element()
            } else {
                super::style::button("anticheat-dl", self.tr("Download", "İndir"), true)
                    .on_click(cx.listener(|this, _, _, cx| this.download_anticheat(cx)))
                    .into_any_element()
            }));
        }

        use super::style;
        let tab = self.protons.tab;
        let mut entries = vec![
            style::NavEntry::new(
                "refresh-cw",
                self.tr("Auto-updated", "Otomatik güncellenen"),
            ),
            style::NavEntry::new("hard-drive", self.tr("Installed", "Kurulu")),
            style::NavEntry::new("download", self.tr("Download", "İndir")),
            style::NavEntry::new("shield", self.tr("Anti-cheat", "Anti-cheat")),
        ];
        entries[1].badge = Some(installed.len().to_string()).filter(|n| n != "0");
        let tabs = [
            ProtonTab::Auto,
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
            ProtonTab::Auto => self.render_auto(cx),
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
