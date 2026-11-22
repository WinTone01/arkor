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
    Installed,
    Download,
}

/// Progress of the required components (see `components::ensure_required`).
#[derive(Default)]
pub(super) struct RequiredState {
    /// What is being installed right now.
    pub(super) step: Option<String>,
    pub(super) error: Option<String>,
    pub(super) running: bool,
}

/// A component the page can install or update on request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Component {
    Proton(components::Latest),
}

pub(super) struct ProtonsState {
    pub(super) tab: ProtonTab,
    /// Auto-updated component being installed or checked.
    updating: Option<Component>,
    variant: Variant,
    releases: HashMap<&'static str, Result<Vec<Release>, String>>,
    loading: bool,
    installing: Option<String>,
}

impl Default for ProtonsState {
    fn default() -> Self {
        Self {
            tab: ProtonTab::Installed,
            updating: None,
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

    /// Installs what every Windows game needs (umu-launcher, anti-cheat, a first Proton)
    /// in the background; updates them unless automatic updates are off.
    pub(super) fn ensure_required(&mut self, cx: &mut Context<Self>) {
        if self.required.running {
            return;
        }
        self.required.running = true;
        self.required.error = None;
        let update = !self.global.no_auto_updates;
        let default = self.global.defaults.proton.clone();
        let step = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let bg = cx.background_executor().clone();
        // Shows the current step while the job runs.
        let (tick_step, tick_done, timer) = (step.clone(), done.clone(), bg.clone());
        cx.spawn(async move |this, cx| {
            while !tick_done.load(std::sync::atomic::Ordering::Relaxed) {
                let now = tick_step.lock().ok().and_then(|s| s.clone());
                let alive = this.update(cx, |l, cx| {
                    if l.required.running && l.required.step != now {
                        l.required.step = now;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
                timer.timer(std::time::Duration::from_millis(400)).await;
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let r = components::ensure_required(update, default.as_deref(), &|s| {
                        if let Ok(mut g) = step.lock() {
                            *g = Some(s.to_string());
                        }
                    });
                    done.store(true, std::sync::atomic::Ordering::Relaxed);
                    r
                })
                .await;
            let _ = this.update(cx, |l, cx| {
                l.required.running = false;
                l.required.step = None;
                l.required.error = result.err().map(|e| format!("{e:#}"));
                l.tools = super::enhance::GameTools::scan();
                cx.notify();
            });
        })
        .detach();
    }

    /// Installs or updates one component now (also what happens before a game).
    fn update_component(&mut self, c: Component, cx: &mut Context<Self>) {
        self.protons.updating = Some(c);
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let Component::Proton(l) = c;
                    components::ensure_latest(l, true).map(|_| ())
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

    /// The "always newest" build of `latest`: install, or its version and an update check.
    fn latest_control(
        &self,
        latest: components::Latest,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use super::style::{self, pal};
        let c = Component::Proton(latest);
        if self.protons.updating == Some(c) {
            return widgets::label(self.tr("Working…", "Çalışıyor…")).into_any_element();
        }
        let installed = latest.installed_version().is_some();
        style::button(
            ("latest-action", ix),
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
        .when(self.protons.updating.is_some(), |b| b.opacity(0.4))
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.protons.updating.is_none() {
                this.update_component(c, cx);
            }
        }))
        .into_any_element()
    }

    /// umu-launcher, anti-cheat and a Proton: their state, installed by Arkor itself.
    fn render_required(&self, protons: usize, cx: &mut Context<Self>) -> Div {
        use super::style::{self, pal};
        let ac = tools::anticheat();
        let ours = tools::components_dir().ok();
        let ac_text = match (&ac.eac, &ac.battleye) {
            (Some(e), Some(_)) if ours.as_ref().is_some_and(|d| e.starts_with(d)) => format!(
                "{} {}",
                self.tr("Faugus' copy", "Faugus'un kopyası"),
                components::anticheat_version().unwrap_or_default()
            ),
            (Some(_), Some(_)) => self.tr("Steam's runtimes", "Steam'in runtime'ları").into(),
            _ => String::new(),
        };
        let umu_text = match components::umu_version() {
            Some(v) => format!("{} {v}", self.tr("Faugus' build", "Faugus derlemesi")),
            None if which_umu() => self.tr("System's umu-run", "Sistemin umu-run'ı").into(),
            None => String::new(),
        };
        let proton_text = if protons > 0 {
            format!("{protons} {}", self.tr("installed", "kurulu"))
        } else {
            String::new()
        };
        let step = self.required.step.clone();
        let row = |name: &'static str, ok_text: String, ix: usize| {
            let ok = !ok_text.is_empty();
            let busy = step.as_deref() == Some(name)
                || (ix == 2 && step.as_deref().is_some_and(|s| s.contains("Latest")));
            div()
                .flex()
                .items_center()
                .gap_3()
                .py_1()
                .child(if busy {
                    style::icon("loader", 16., pal::accent_light())
                } else if ok {
                    style::icon("circle-check", 16., pal::success())
                } else {
                    style::icon("circle-alert", 16., pal::warning())
                })
                .child(
                    div()
                        .flex_1()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(name),
                )
                .child(widgets::label(if busy {
                    self.tr("Installing…", "Kuruluyor…").to_string()
                } else if ok {
                    ok_text
                } else {
                    self.tr("Missing", "Eksik").to_string()
                }))
        };
        let mut card = widgets::section(self.tr("Required for Windows games", "Windows oyunları için gerekli"))
            .child(widgets::label(self.tr(
                "Arkor installs these by itself when it starts and keeps them updated, as Faugus Launcher does.",
                "Arkor bunları açılırken kendisi kurar ve Faugus Launcher gibi güncel tutar.",
            )))
            .child(row("umu-launcher", umu_text, 0))
            .child(widgets::divider())
            .child(row("EasyAntiCheat & BattlEye", ac_text, 1))
            .child(widgets::divider())
            .child(row("Proton", proton_text, 2));
        if let Some(e) = &self.required.error {
            card = card
                .child(
                    div()
                        .text_sm()
                        .text_color(pal::warning())
                        .child(format!("⚠ {e}")),
                )
                .child(
                    div().flex().child(
                        style::button("required-retry", self.tr("Try again", "Yeniden dene"), true)
                            .on_click(cx.listener(|this, _, _, cx| this.ensure_required(cx))),
                    ),
                );
        }
        card
    }

    fn remove_proton(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let result = proton_dl::uninstall(&path);
        self.report(result);
        self.tools = super::enhance::GameTools::scan();
        cx.notify();
    }

    pub(super) fn render_protons(&mut self, cx: &mut Context<Self>) -> Div {
        use super::style::{self, pal};
        use components::Latest;
        let variant = self.protons.variant;
        let tab = self.protons.tab;
        if tab == ProtonTab::Download
            && !self.protons.releases.contains_key(variant.label())
            && !self.protons.loading
        {
            self.load_releases(variant, cx);
        }
        let user_root = proton_dl::install_root().ok();
        let installed = tools::installed_protons();

        let content = match tab {
            ProtonTab::Installed => {
                let mut list = widgets::section(self.tr("Installed builds", "Kurulu derlemeler"));
                if installed.is_empty() {
                    list = list.child(widgets::label(self.tr(
                        "None yet. One is installed by itself; others in Download.",
                        "Henüz yok. Biri kendiliğinden kurulur; diğerleri İndir bölümünde.",
                    )));
                }
                for (ix, (name, path)) in installed.iter().enumerate() {
                    let latest = Latest::from_value(name);
                    let removable = user_root.as_ref().is_some_and(|r| path.starts_with(r));
                    let p = path.clone();
                    let hint = match latest {
                        Some(l) => format!(
                            "{} · {}",
                            self.tr(
                                "Always the newest, updated before each start",
                                "Hep en yeni, her başlatmadan önce güncellenir"
                            ),
                            l.installed_version().unwrap_or_default()
                        ),
                        None => path.display().to_string(),
                    };
                    let mut controls = div().flex().gap_2();
                    if let Some(l) = latest {
                        controls = controls.child(self.latest_control(l, 10 + ix, cx));
                    }
                    controls =
                        controls.child(if removable {
                            style::button(("proton-remove", ix), self.tr("Remove", "Kaldır"), false)
                                .child(style::icon("trash-2", 13., pal::danger()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_proton(p.clone(), cx)
                                }))
                                .into_any_element()
                        } else {
                            widgets::label(source_label(path, self)).into_any_element()
                        });
                    if ix > 0 {
                        list = list.child(widgets::divider());
                    }
                    list = list.child(widgets::setting_row(name.clone(), hint, controls));
                }
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(self.render_required(installed.len(), cx))
                    .child(list)
            }
            ProtonTab::Download => {
                let mut chips = div().flex().flex_wrap().gap_2();
                for (ix, v) in Variant::ALL.into_iter().enumerate() {
                    chips = chips.child(
                        widgets::chip(("variant", ix), v.label(), v == variant).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.protons.variant = v;
                                cx.notify();
                            }),
                        ),
                    );
                }
                let latest = Latest::ALL
                    .into_iter()
                    .find(|l| l.variant() == variant)
                    .unwrap_or(Latest::Ge);
                let preset = div()
                    .p_3()
                    .rounded(gpui::px(12.))
                    .border_1()
                    .border_color(pal::accent().opacity(0.4))
                    .bg(pal::accent().opacity(0.08))
                    .child(widgets::setting_row(
                        format!("{} ✦", latest.dir_name()),
                        match latest.installed_version() {
                            Some(v) => format!(
                                "{} · {} {v}",
                                self.tr("Recommended: always the newest release, updated before each game start", "Önerilen: hep en yeni sürüm, her oyundan önce güncellenir"),
                                self.tr("installed", "kurulu"),
                            ),
                            None => self
                                .tr(
                                    "Recommended: always the newest release, updated before each game start",
                                    "Önerilen: hep en yeni sürüm, her oyundan önce güncellenir",
                                )
                                .to_string(),
                        },
                        self.latest_control(latest, 0, cx),
                    ));
                let mut versions = div().flex().flex_col().gap_1();
                if self.protons.loading {
                    versions = versions.child(widgets::label(
                        self.tr("Loading releases…", "Sürümler yükleniyor…"),
                    ));
                }
                match self.protons.releases.get(variant.label()) {
                    Some(Err(e)) => versions = versions.child(widgets::label(format!("⚠ {e}"))),
                    Some(Ok(list)) if list.is_empty() => {
                        versions =
                            versions.child(widgets::label(self.tr("No releases.", "Sürüm yok.")))
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
                                widgets::label(self.tr("Installing…", "Kuruluyor…"))
                                    .into_any_element()
                            } else if have {
                                widgets::label(self.tr("Installed", "Kurulu")).into_any_element()
                            } else if self.protons.installing.is_some() {
                                div().into_any_element()
                            } else {
                                style::button(
                                    ("proton-install", ix),
                                    self.tr("Install", "Kur"),
                                    false,
                                )
                                .child(style::icon("download", 13., pal::muted()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.install_proton(release.clone(), cx)
                                }))
                                .into_any_element()
                            };
                            if ix > 0 {
                                versions = versions.child(widgets::divider());
                            }
                            versions = versions.child(widgets::setting_row(
                                r.tag.clone(),
                                format!(
                                    "{}{}",
                                    human_size(r.size),
                                    if ix == 0 {
                                        self.tr(" · newest", " · en yeni")
                                    } else {
                                        ""
                                    }
                                ),
                                action,
                            ));
                        }
                    }
                    None => {}
                }
                div().flex().flex_col().gap_4().child(
                    widgets::section(self.tr("Download a Proton build", "Proton derlemesi indir"))
                        .child(widgets::setting_row(
                            self.tr("Build", "Derleme"),
                            self.tr("GE-Proton suits most games; CachyOS is tuned for newer CPUs.", "GE-Proton çoğu oyuna uyar; CachyOS yeni işlemciler için ayarlıdır."),
                            chips,
                        ))
                        .child(preset)
                        .child(widgets::subheading(self.tr("A specific version (never changes)", "Belirli bir sürüm (hiç değişmez)")))
                        .child(versions)
                        .child(widgets::label(self.tr(
                            "Installed into ~/.local/share/Steam/compatibilitytools.d, so Steam can use them too. Checked against the release checksum.",
                            "~/.local/share/Steam/compatibilitytools.d içine kurulur, böylece Steam de kullanabilir. Sürümün sağlama toplamıyla doğrulanır.",
                        ))),
                )
            }
        };

        let mut entries = vec![
            style::NavEntry::new("hard-drive", self.tr("Installed", "Kurulu")),
            style::NavEntry::new("download", self.tr("Download", "İndir")),
        ];
        entries[0].badge = Some(installed.len().to_string()).filter(|n| n != "0");
        let tabs = [ProtonTab::Installed, ProtonTab::Download];
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
        div()
            .child(Self::header(
                self.tr("Proton & Wine", "Proton ve Wine"),
                self.tr(
                    "Builds Windows games run with. Pick one per game (game page → Compatibility) or a default for all (Settings → Game defaults).",
                    "Windows oyunlarının çalıştığı derlemeler. Oyun başına seç (oyun sayfası → Uyumluluk) ya da hepsi için varsayılan belirle (Ayarlar → Oyun varsayılanları).",
                ),
            ))
            .child(style::with_side_nav(nav, content))
    }
}

/// Whether the system has its own umu-run.
fn which_umu() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("umu-run").is_file()))
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
