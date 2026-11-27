//! "Downloads", like a store's download page: what is downloading now (with a progress bar,
//! size, speed and time left), then everything Arkor has downloaded: Proton builds, Vulkan
//! layers and game mods (OptiScaler, DLSS Enabler, ReShade, Streamline SDK, AMD bundles).
//! Things that follow the newest release get "Check for updates"; a fixed version (e.g. a
//! specific Proton build) does not. Removing a download does not touch games it was
//! installed into: those got their own copy.

use std::path::{Path, PathBuf};

use arkor_launch::bundles::{self, Bundle};
use arkor_launch::components::Latest;
use arkor_launch::downloads::{self, Update};
use arkor_launch::progress;
use arkor_launch::{anr, dlss_enabler, layers, optiscaler, proton_dl, reshade, streamline, tools};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};

/// One kind of download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Layer(usize),
    OptiScaler,
    DlssEnabler,
    Anr,
    ReShade,
    Streamline,
    Bundle(Bundle),
}

/// Filter chips at the top of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Filter {
    #[default]
    All,
    Proton,
    Layers,
    Mods,
    /// Optional programs from the distribution (MangoHud, Gamescope, winetricks…).
    Tools,
}

impl Kind {
    fn all() -> Vec<Kind> {
        let mut v: Vec<Kind> = (0..layers::TOOLS.len()).map(Kind::Layer).collect();
        v.extend([
            Kind::OptiScaler,
            Kind::DlssEnabler,
            Kind::Anr,
            Kind::ReShade,
            Kind::Streamline,
        ]);
        v.extend(Bundle::ALL.map(Kind::Bundle));
        v
    }

    fn filter(self) -> Filter {
        match self {
            Kind::Layer(_) => Filter::Layers,
            _ => Filter::Mods,
        }
    }

    /// Progress job id (the same id the game page's Download buttons use).
    fn key(self) -> &'static str {
        match self {
            Kind::Layer(ix) => layers::TOOLS[ix].id,
            Kind::OptiScaler => "optiscaler",
            Kind::DlssEnabler => "dlss-enabler",
            Kind::Anr => "anr",
            Kind::ReShade => "reshade",
            Kind::Streamline => "streamline",
            Kind::Bundle(b) => b.key(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Layer(ix) => layers::TOOLS[ix].name,
            Kind::OptiScaler => "OptiScaler",
            Kind::DlssEnabler => "DLSS Enabler",
            Kind::Anr => "ANR FG",
            Kind::ReShade => "ReShade",
            Kind::Streamline => "Streamline SDK",
            Kind::Bundle(b) => b.name(),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Kind::Layer(_) => "layers",
            Kind::OptiScaler => "scan-eye",
            Kind::DlssEnabler => "cpu",
            Kind::Anr => "brain",
            Kind::ReShade => "brush",
            Kind::Streamline => "refresh-cw",
            Kind::Bundle(_) => "atom",
        }
    }

    /// Downloaded copies: (version, folder). Layers show one entry without a version.
    fn downloaded(self) -> Vec<(String, PathBuf)> {
        match self {
            Kind::Layer(ix) => {
                let tool = &layers::TOOLS[ix];
                if layers::arkor_installed(tool.id) {
                    let dir = dirs::data_dir()
                        .unwrap_or_default()
                        .join("arkor/layers")
                        .join(tool.id);
                    vec![(String::new(), dir)]
                } else {
                    Vec::new()
                }
            }
            Kind::OptiScaler => optiscaler::downloaded(),
            Kind::DlssEnabler => dlss_enabler::downloaded(),
            Kind::Anr => anr::downloaded(),
            Kind::ReShade => reshade::downloaded(),
            Kind::Streamline => streamline::downloaded(),
            Kind::Bundle(b) => bundles::downloaded(b),
        }
    }

    /// Removes the downloaded copy in `dir` (for layers: unregisters and deletes it).
    fn remove(self, dir: &Path) -> anyhow::Result<()> {
        match self {
            Kind::Layer(ix) => layers::uninstall(layers::TOOLS[ix].id),
            _ => remove_dir(dir),
        }
    }
}

/// Deletes a download folder, only inside Arkor's own data folder.
fn remove_dir(dir: &Path) -> anyhow::Result<()> {
    let root = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("no XDG data dir"))?
        .join("arkor");
    arkor_core::fs_guard::remove_dir_in(dir, &root)
}

#[derive(Default)]
pub(super) struct DownloadsState {
    filter: Filter,
    /// Bumped after each change so the list is read again.
    generation: u64,
    /// Newer versions found by the last check.
    pub(super) updates: Vec<Update>,
    checking: bool,
    checked: bool,
    /// `downloads::finished()` count last seen, to notice queue items ending.
    finished: u64,
    /// Recent download speeds for the graph on the overview.
    pub(super) speed: super::downloads_queue::SpeedGraph,
}

type Job = Box<dyn FnOnce() -> anyhow::Result<String> + Send + 'static>;
type Action = std::rc::Rc<dyn Fn() -> Job>;

/// Something Arkor downloaded, for console mode.
pub(super) struct InstalledThing {
    /// Stable for one thing: its key and folder.
    pub(super) id: String,
    pub(super) icon: &'static str,
    pub(super) title: String,
    pub(super) detail: String,
    pub(super) update: Option<String>,
    pub(super) removable: bool,
    pub(super) proton: bool,
}

/// One row of the installed list.
struct Row {
    key: String,
    icon: &'static str,
    title: String,
    detail: String,
    filter: Filter,
    /// Queue key that brings it to its newest release (none for a fixed version).
    update: Option<String>,
    remove: Option<Action>,
}

/// A thin progress bar; without a known size a short stripe shows it is working.
pub(super) fn progress_bar(fraction: Option<f32>) -> Div {
    let track = div()
        .h(px(6.))
        .w_full()
        .rounded_full()
        .bg(pal::raised())
        .overflow_hidden();
    match fraction {
        Some(f) => track.child(
            div()
                .h_full()
                .w(gpui::relative(f.max(0.02)))
                .rounded_full()
                .bg(style::accent_gradient()),
        ),
        None => track.child(
            div()
                .h_full()
                .w(gpui::relative(0.3))
                .rounded_full()
                .bg(pal::accent().opacity(0.6)),
        ),
    }
}

impl Library {
    /// Opens Downloads on its System tools section (from the System page).
    pub(super) fn downloads_show_tools(&mut self) {
        self.downloads.filter = Filter::Tools;
    }

    /// Queues every update found by the last check.
    pub(super) fn downloads_update_all(&mut self, cx: &mut Context<Self>) {
        for u in self.downloads.updates.clone() {
            downloads::enqueue(&u.key);
        }
        self.watch_progress(cx);
        cx.notify();
    }

    #[cfg(test)]
    pub(super) fn downloads_set_updates(&mut self, updates: Vec<Update>) {
        self.downloads.updates = updates;
        self.downloads.checked = true;
    }

    /// Puts `key` in the download queue (see `arkor_launch::downloads`).
    pub(super) fn enqueue_download(&mut self, key: &str, cx: &mut Context<Self>) {
        downloads::enqueue(key);
        self.watch_progress(cx);
        cx.notify();
    }

    /// At start: when an update check is due (daily or weekly), brings the components to
    /// their newest release and looks for a newer Arkor and newer tools; otherwise only
    /// installs what is missing.
    pub(super) fn startup_updates(&mut self, cx: &mut Context<Self>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let due = self.global.updates_due(now);
        self.ensure_required(due, cx);
        if due {
            self.global.last_update_check = Some(now);
            if let Err(e) = self.global.save() {
                eprintln!("arkor: saving the update time failed: {e:#}");
            }
            self.check_app_update(true, cx);
            self.check_tool_updates(cx);
        }
    }

    /// Looks for newer versions of what is downloaded, in the background.
    pub(super) fn check_tool_updates(&mut self, cx: &mut Context<Self>) {
        if self.downloads.checking {
            return;
        }
        self.downloads.checking = true;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let found = bg.spawn(async move { downloads::check_updates() }).await;
            let _ = this.update(cx, |this, cx| {
                this.downloads.checking = false;
                this.downloads.checked = true;
                this.downloads.updates = found;
                cx.notify();
            });
        })
        .detach();
    }

    /// Redraws while downloads move (one loop for the window's life).
    pub(super) fn watch_progress(&mut self, cx: &mut Context<Self>) {
        if self.downloads_watch {
            return;
        }
        self.downloads_watch = true;
        let timer = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let mut seen = progress::generation();
            loop {
                timer.timer(std::time::Duration::from_millis(300)).await;
                // The speed graph moves every tick while something downloads.
                let graph_moved = this
                    .update(cx, |this, cx| {
                        this.steam_tick(cx);
                        let moved = this.downloads.speed.sample();
                        if moved {
                            cx.notify();
                        }
                        moved
                    })
                    .unwrap_or(false);
                let now = progress::generation();
                if now == seen && !graph_moved {
                    continue;
                }
                seen = now;
                let alive = this.update(cx, |this, cx| {
                    let (count, last) = downloads::finished();
                    if count != this.downloads.finished {
                        // A queued download ended: games read what is installed again.
                        this.downloads.finished = count;
                        this.downloads.generation += 1;
                        if let Some(last) = last {
                            if last.ok {
                                this.downloads.updates.retain(|u| u.key != last.key);
                            }
                            // A store game says only its name.
                            let game = arkor_launch::shops::parse_key(&last.key).is_some();
                            this.status = Some(if game && last.ok {
                                format!("{}: {}", this.tr("Ready", "Hazır"), last.report).into()
                            } else {
                                last.report.into()
                            });
                        }
                        this.rescan_tools();
                        this.shops_after_download(cx);
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    fn downloads_job(&mut self, key: String, label: String, job: Job, cx: &mut Context<Self>) {
        if progress::running(&key) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let _job = progress::job(&key, &label);
                    job()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.downloads.generation += 1;
                this.status = Some(match result {
                    Ok(msg) => msg.into(),
                    Err(e) => format!("{e:#}").into(),
                });
                // Game pages read download state from here.
                this.rescan_tools();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Proton builds: "Latest" ones follow the newest release (Check for updates); a fixed
    /// version does not. Arkor's own installs can be removed.
    fn proton_rows(&self) -> Vec<Row> {
        let user_root = proton_dl::install_root().ok();
        tools::installed_protons()
            .into_iter()
            .map(|(name, path)| {
                let latest = Latest::from_value(&name);
                let removable = user_root.as_ref().is_some_and(|r| path.starts_with(r));
                let detail = match latest.and_then(|l| l.installed_version()) {
                    Some(v) => format!(
                        "{} · {v}",
                        self.tr("Always the newest release", "Hep en yeni sürüm")
                    ),
                    None => path.display().to_string(),
                };
                let update = latest.map(|l| format!("latest:{}", l.dir_name()));
                let remove: Option<Action> = removable.then(|| {
                    let (p, label) = (path.clone(), name.clone());
                    std::rc::Rc::new(move || -> Job {
                        let (p, label) = (p.clone(), label.clone());
                        Box::new(move || proton_dl::uninstall(&p).map(|_| format!("{label} ✕")))
                    }) as Action
                });
                Row {
                    key: format!("proton:{name}"),
                    icon: "wine",
                    title: name,
                    detail,
                    filter: Filter::Proton,
                    update,
                    remove,
                }
            })
            .collect()
    }

    fn tool_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for kind in Kind::all() {
            for (version, dir) in kind.downloaded() {
                let name = kind.name();
                let d = dir.clone();
                rows.push(Row {
                    key: kind.key().to_string(),
                    icon: kind.icon(),
                    title: if version.is_empty() {
                        name.to_string()
                    } else {
                        format!("{name} {version}")
                    },
                    detail: dir.display().to_string(),
                    filter: kind.filter(),
                    // These tools always follow their newest release.
                    update: Some(kind.key().to_string()),
                    remove: Some(std::rc::Rc::new(move || -> Job {
                        let d = d.clone();
                        Box::new(move || kind.remove(&d).map(|_| format!("{name} ✕")))
                    })),
                });
            }
        }
        rows
    }

    /// Everything installed, for console mode.
    pub(super) fn installed_things(&self) -> Vec<InstalledThing> {
        self.proton_rows()
            .into_iter()
            .chain(self.tool_rows())
            .map(|r| InstalledThing {
                id: format!("{}|{}", r.key, r.detail),
                icon: r.icon,
                proton: r.filter == Filter::Proton,
                title: r.title,
                detail: r.detail,
                update: r.update,
                removable: r.remove.is_some(),
            })
            .collect()
    }

    /// Removes one of `installed_things` (in the background).
    pub(super) fn remove_installed(&mut self, id: &str, cx: &mut Context<Self>) {
        let row = self
            .proton_rows()
            .into_iter()
            .chain(self.tool_rows())
            .find(|r| format!("{}|{}", r.key, r.detail) == id);
        if let Some(Row {
            key,
            title,
            remove: Some(remove),
            ..
        }) = row
        {
            self.downloads_job(key, title, remove(), cx);
        }
    }

    pub(super) fn downloads_checking(&self) -> bool {
        self.downloads.checking
    }

    /// "N updates available · Update all", with each update's versions.
    fn updates_card(&mut self, cx: &mut Context<Self>) -> Div {
        let st = &self.downloads;
        let shown: Vec<Update> = st.updates.clone();
        let waiting = shown.iter().filter(|u| !downloads::pending(&u.key)).count();
        let (icon, color, title) = if st.checking {
            (
                "refresh-cw",
                pal::muted(),
                self.tr("Looking for updates…", "Güncellemeler aranıyor…")
                    .to_string(),
            )
        } else if !shown.is_empty() {
            let n = shown.len();
            (
                "sparkles",
                pal::accent_light(),
                match (n, self.global.language == super::Language::Tr) {
                    (_, true) => format!("{n} güncelleme var"),
                    (1, false) => "1 update available".to_string(),
                    (_, false) => format!("{n} updates available"),
                },
            )
        } else if st.checked {
            (
                "circle-check",
                pal::success(),
                self.tr("Everything is up to date", "Her şey güncel")
                    .to_string(),
            )
        } else {
            (
                "refresh-cw",
                pal::muted(),
                self.tr("Updates", "Güncellemeler").to_string(),
            )
        };
        let mut head = div()
            .p_4()
            .flex()
            .items_center()
            .gap_3()
            .child(if st.checking {
                super::loader::spinner(22., self.global.appearance.reduce_motion)
            } else {
                style::icon(icon, 18., color).into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(title),
            );
        if waiting > 0 {
            head = head.child(
                style::button(
                    "updates-all",
                    self.tr("Update all", "Tümünü güncelle"),
                    true,
                )
                .child(style::icon("download", 13., pal::on_accent()))
                .on_click(cx.listener(|this, _, _, cx| this.downloads_update_all(cx))),
            );
        }
        if !st.checking {
            head = head.child(
                style::button(
                    "updates-check",
                    self.tr("Check now", "Şimdi denetle"),
                    false,
                )
                .child(style::icon("refresh-cw", 13., pal::muted()))
                .on_click(cx.listener(|this, _, _, cx| this.check_tool_updates(cx))),
            );
        }
        let mut card = style::card()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(head);
        for (ix, u) in shown.into_iter().enumerate() {
            let busy = downloads::pending(&u.key);
            let key = u.key.clone();
            card = card.child(
                div()
                    .px_4()
                    .py_2p5()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_t_1()
                    .border_color(pal::glass_spec().border)
                    .child(div().flex_1().min_w_0().truncate().child(u.label))
                    // Games have no version numbers worth showing.
                    .children((!u.latest.is_empty()).then(|| {
                        div()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(format!("{} → {}", u.installed, u.latest))
                    }))
                    .child(if busy {
                        div()
                            .w(px(110.))
                            .child(widgets::label(self.tr("In queue", "Sırada")))
                    } else {
                        div().child(
                            style::button(("update-one", ix), self.tr("Update", "Güncelle"), false)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.enqueue_download(&key, cx)
                                })),
                        )
                    }),
            );
        }
        card
    }

    /// Installed things as one framed list with a divider between rows.
    fn installed_list(&mut self, rows: Vec<Row>, cx: &mut Context<Self>) -> Div {
        let mut list = style::card().overflow_hidden().flex().flex_col();
        let n = rows.len();
        for (ix, row) in rows.into_iter().enumerate() {
            let busy = progress::running(&row.key)
                || row.update.as_deref().is_some_and(downloads::pending);
            let Row {
                key,
                icon,
                title,
                detail,
                update,
                remove,
                ..
            } = row;
            let mut actions = div().flex().items_center().gap_2().flex_none();
            if busy {
                actions = actions.child(
                    div()
                        .w(px(140.))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(widgets::label(self.tr("Working…", "Çalışıyor…")))
                        .child(progress_bar(None)),
                );
            } else {
                if let Some(update) = update {
                    actions = actions.child(
                        style::button(
                            ("dl-update", ix),
                            self.tr("Check for updates", "Güncellemeleri denetle"),
                            false,
                        )
                        .child(style::icon("refresh-cw", 13., pal::muted()))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.enqueue_download(&update, cx)),
                        ),
                    );
                }
                if let Some(remove) = remove {
                    let (k, l) = (key.clone(), title.clone());
                    actions = actions.child(
                        div()
                            .id(("dl-remove", ix))
                            .size(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(9.))
                            .border_1()
                            .border_color(pal::border_strong())
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(pal::danger().opacity(0.15))
                                    .border_color(pal::danger().opacity(0.5))
                            })
                            .child(style::icon("trash", 14., pal::danger()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.downloads_job(k.clone(), l.clone(), remove(), cx)
                            })),
                    );
                }
            }
            list = list.child(
                div()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_4()
                    .hover(|s| s.bg(pal::raised().opacity(0.3)))
                    .when(ix + 1 < n, |d| {
                        d.border_b_1().border_color(pal::glass_spec().border)
                    })
                    .child(
                        div()
                            .size(px(42.))
                            .flex_none()
                            .rounded(px(11.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(pal::accent().opacity(0.12))
                            .border_1()
                            .border_color(pal::accent().opacity(0.25))
                            .child(style::icon(icon, 19., pal::accent_light())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .truncate()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(pal::muted())
                                    .truncate()
                                    .child(detail),
                            ),
                    )
                    .child(actions),
            );
        }
        list
    }

    pub(super) fn render_downloads(&mut self, cx: &mut Context<Self>) -> Div {
        let _ = self.downloads.generation;
        let section = |text: &'static str| {
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(pal::faint())
                .child(text)
        };
        let mut rows = self.proton_rows();
        rows.extend(self.tool_rows());
        let count = |f: Filter| {
            rows.iter()
                .filter(|r| f == Filter::All || r.filter == f)
                .count()
        };
        let active = progress::list();

        // Left menu, like a store's download page: an overview and one entry per kind.
        let filters = [
            Filter::All,
            Filter::Proton,
            Filter::Layers,
            Filter::Mods,
            Filter::Tools,
        ];
        let mut entries = vec![
            style::NavEntry::new("layout-dashboard", self.tr("Downloads", "İndirmeler")),
            style::NavEntry::new("wine", "Proton"),
            style::NavEntry::new("layers", self.tr("Vulkan layers", "Vulkan katmanları")),
            style::NavEntry::new("puzzle", self.tr("Game mods", "Oyun modları")),
            style::NavEntry::new("toolbox", self.tr("System tools", "Sistem araçları")),
        ];
        entries[0].badge = Some(active.len().to_string()).filter(|n| n != "0");
        for (i, f) in filters.iter().enumerate().skip(1).take(3) {
            entries[i].badge = Some(count(*f).to_string());
        }
        let current = self.downloads.filter;
        let nav = style::side_nav(
            "downloads-nav",
            entries,
            filters.iter().position(|f| *f == current),
            cx,
            move |this, i, cx| {
                this.downloads.filter = filters[i];
                cx.notify();
            },
        );

        if current == Filter::Tools {
            let tools = self.render_system_tools(cx);
            return div()
                .child(Self::header(
                    self.tr("Downloads", "İndirilenler"),
                    self.tr(
                        "Optional programs Arkor works with, from your distribution's packages.",
                        "Arkor'un birlikte çalıştığı, dağıtımının paketlerinden gelen isteğe bağlı programlar.",
                    ),
                ))
                .child(style::with_side_nav(nav, tools));
        }
        let mut content = div().flex().flex_col().gap_4();
        if current == Filter::All {
            content = content
                .child(self.downloads_overview(cx))
                .child(self.updates_card(cx));
            let protons = tools::installed_protons().len();
            content = content.child(self.render_required(protons, cx));
            return div()
                .child(Self::header(self.tr("Downloads", "İndirilenler"), ""))
                .child(style::with_side_nav(nav, content));
        }
        let shown: Vec<Row> = rows
            .into_iter()
            .filter(|r| current == Filter::All || r.filter == current)
            .collect();
        content = content.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(section(self.tr("INSTALLED", "KURULU")))
                .child(widgets::label(format!(
                    "{} {}",
                    shown.len(),
                    if shown.len() == 1 {
                        self.tr("item", "öğe")
                    } else {
                        self.tr("items", "öğe")
                    }
                ))),
        );
        if shown.is_empty() {
            content = content.child(
                style::card().p_6().flex().flex_col().items_center().gap_2().child(style::icon("hard-drive-download", 28., pal::faint())).child(widgets::label(self.tr(
                    "Nothing here yet. Proton builds and the tools you download from a game's page (e.g. OptiScaler, MAKO) appear here.",
                    "Henüz bir şey yok. Proton derlemeleri ve bir oyunun sayfasından indirdiğin araçlar (ör. OptiScaler, MAKO) burada görünür.",
                ))),
            );
        } else {
            content = content.child(self.installed_list(shown, cx));
        }

        div()
            .child(Self::header(
                self.tr("Downloads", "İndirilenler"),
                self.tr(
                    "What Arkor is downloading, what games need, and everything it has downloaded. Removing a tool does not change games it was installed into.",
                    "Arkor'un indirdikleri, oyunların ihtiyaç duydukları ve indirilmiş her şey. Bir aracı silmek, kurulduğu oyunları değiştirmez.",
                ),
            ))
            .child(style::with_side_nav(nav, content))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn removes_only_inside_arkors_data_folder() {
        let _lock = crate::ui::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests that change XDG variables hold TEST_ENV_LOCK.
        unsafe { std::env::set_var("XDG_DATA_HOME", tmp.path()) };
        let inside = tmp.path().join("arkor/tools/reshade/6.3");
        let outside = tmp.path().join("other/keep");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        assert!(super::remove_dir(&outside).is_err());
        assert!(
            super::remove_dir(&tmp.path().join("arkor")).is_err(),
            "never the whole folder"
        );
        super::remove_dir(&inside).unwrap();
        assert!(!inside.exists() && outside.exists());
    }
}
