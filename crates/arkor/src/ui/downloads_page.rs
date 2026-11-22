//! "Downloads", like a store's download page: what is downloading now (with a progress bar,
//! size, speed and time left), then everything Arkor has downloaded: Proton builds, Vulkan
//! layers and game mods (OptiScaler, DLSS Enabler, ReShade, Streamline SDK, AMD bundles).
//! Things that follow the newest release get "Check for updates"; a fixed version (e.g. a
//! specific Proton build) does not. Removing a download does not touch games it was
//! installed into: those got their own copy.

use std::path::{Path, PathBuf};

use arkor_launch::bundles::{self, Bundle};
use arkor_launch::components::{self, Latest};
use arkor_launch::progress::{self, Phase};
use arkor_launch::{dlss_enabler, layers, optiscaler, proton_dl, reshade, streamline, tools};
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
}

impl Kind {
    fn all() -> Vec<Kind> {
        let mut v: Vec<Kind> = (0..layers::TOOLS.len()).map(Kind::Layer).collect();
        v.extend([
            Kind::OptiScaler,
            Kind::DlssEnabler,
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
            Kind::ReShade => reshade::downloaded(),
            Kind::Streamline => streamline::downloaded(),
            Kind::Bundle(b) => bundles::downloaded(b),
        }
    }

    /// Fetches the newest version (keeping it if already there) and drops older copies.
    /// Returns the newest version. Blocking.
    fn update(self) -> anyhow::Result<String> {
        let (newest, kept): (String, PathBuf) = match self {
            Kind::Layer(ix) => return layers::install(&layers::TOOLS[ix]),
            Kind::OptiScaler => {
                let r = optiscaler::latest_release()?;
                let dir = optiscaler::download(&r)?;
                (r.tag, dir)
            }
            Kind::DlssEnabler => {
                let builds = dlss_enabler::builds()?;
                let b = builds
                    .iter()
                    .find(|b| b.stable)
                    .or(builds.first())
                    .ok_or_else(|| anyhow::anyhow!("no DLSS Enabler build found"))?;
                let dir = dlss_enabler::download(b)?;
                (b.version.clone(), dir)
            }
            Kind::ReShade => {
                let v = reshade::latest_version()?;
                let dir = reshade::download(&v)?;
                (v, dir)
            }
            Kind::Streamline => {
                let v = streamline::latest_version()?;
                let dir = streamline::download(&v)?;
                (v, dir)
            }
            Kind::Bundle(b) => bundles::download(b)?,
        };
        // Older copies are not needed: games keep the files they were given. Compared by
        // folder, so the copy just downloaded is never the one removed.
        let kept = std::fs::canonicalize(&kept).unwrap_or(kept);
        for (_, dir) in self.downloaded() {
            let same = std::fs::canonicalize(&dir).unwrap_or(dir.clone());
            if same != kept && !kept.starts_with(&same) && !same.starts_with(&kept) {
                remove_dir(&dir)?;
            }
        }
        Ok(newest)
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
    anyhow::ensure!(
        dir.starts_with(&root) && dir != root,
        "{} is not one of Arkor's downloads",
        dir.display()
    );
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[derive(Default)]
pub(super) struct DownloadsState {
    filter: Filter,
    /// Bumped after each change so the list is read again.
    generation: u64,
}

type Job = Box<dyn FnOnce() -> anyhow::Result<String> + Send + 'static>;
type Action = std::rc::Rc<dyn Fn() -> Job>;

/// One row of the installed list.
struct Row {
    key: String,
    icon: &'static str,
    title: String,
    detail: String,
    filter: Filter,
    update: Option<Action>,
    remove: Option<Action>,
}

/// "12 MB / 280 MB · 8.4 MB/s · 31 s left".
fn progress_text(e: &progress::Entry, lib: &Library) -> String {
    match e.phase {
        Phase::Preparing => lib.tr("Preparing…", "Hazırlanıyor…").to_string(),
        Phase::Finishing => lib.tr("Installing…", "Kuruluyor…").to_string(),
        Phase::Downloading => {
            let mut s = match e.total {
                Some(t) => format!(
                    "{} / {}",
                    progress::human_bytes(e.done),
                    progress::human_bytes(t)
                ),
                None => progress::human_bytes(e.done),
            };
            if e.speed > 1. {
                s.push_str(&format!(" · {}/s", progress::human_bytes(e.speed as u64)));
            }
            if let Some(eta) = e.eta_secs() {
                let left = if eta >= 60 {
                    format!("{} min", eta / 60)
                } else {
                    format!("{eta} s")
                };
                s.push_str(&format!(" · {left} {}", lib.tr("left", "kaldı")));
            }
            s
        }
    }
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
                let now = progress::generation();
                if now == seen {
                    continue;
                }
                seen = now;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
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
                this.tools = super::enhance::GameTools::scan();
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
        let done = self.tr("Up to date:", "Güncel:");
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
                let update: Option<Action> = latest.map(|l| {
                    let label = name.clone();
                    std::rc::Rc::new(move || -> Job {
                        let label = label.clone();
                        Box::new(move || {
                            components::ensure_latest(l, true).map(|_| {
                                format!(
                                    "{done} {label} {}",
                                    l.installed_version().unwrap_or_default()
                                )
                            })
                        })
                    }) as Action
                });
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
        let done = self.tr("Up to date:", "Güncel:");
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
                    update: Some(std::rc::Rc::new(move || -> Job {
                        Box::new(move || kind.update().map(|v| format!("{done} {name} {v}")))
                    })),
                    remove: Some(std::rc::Rc::new(move || -> Job {
                        let d = d.clone();
                        Box::new(move || kind.remove(&d).map(|_| format!("{name} ✕")))
                    })),
                });
            }
        }
        rows
    }

    pub(super) fn render_downloads(&mut self, cx: &mut Context<Self>) -> Div {
        let _ = self.downloads.generation;
        let mut page = div().child(Self::header(
            self.tr("Downloads", "İndirilenler"),
            self.tr(
                "What Arkor is downloading and everything it has downloaded. Removing a tool does not change games it was installed into.",
                "Arkor'un indirdikleri ve indirmekte oldukları. Bir aracı silmek, kurulduğu oyunları değiştirmez.",
            ),
        ));
        let section = |text: &'static str| {
            div()
                .pt_2()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(pal::faint())
                .child(text)
        };

        // Active downloads.
        let active = progress::list();
        if !active.is_empty() {
            let mut list = div().flex().flex_col().gap_3();
            for e in &active {
                list = list.child(
                    style::card()
                        .p_4()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(
                            div()
                                .size(px(44.))
                                .flex_none()
                                .rounded(px(12.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(pal::accent().opacity(0.15))
                                .child(style::icon("download", 20., pal::accent_light())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .flex()
                                        .justify_between()
                                        .gap_3()
                                        .child(
                                            div()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .truncate()
                                                .child(e.label.clone()),
                                        )
                                        .children(e.fraction().map(|f| {
                                            div()
                                                .text_sm()
                                                .text_color(pal::accent_light())
                                                .child(format!("{:.0}%", f * 100.))
                                        })),
                                )
                                .child(progress_bar(if e.phase == Phase::Downloading {
                                    e.fraction()
                                } else {
                                    None
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(pal::muted())
                                        .child(progress_text(e, self)),
                                ),
                        ),
                );
            }
            page = page
                .child(section(self.tr("DOWNLOADING", "İNDİRİLİYOR")))
                .child(list);
        }

        // Installed, with filters.
        let mut rows = self.proton_rows();
        rows.extend(self.tool_rows());
        let count = |f: Filter| {
            rows.iter()
                .filter(|r| f == Filter::All || r.filter == f)
                .count()
        };
        let mut chips = div().flex().flex_wrap().gap_2();
        for (ix, (f, label)) in [
            (Filter::All, self.tr("All", "Tümü")),
            (Filter::Proton, "Proton"),
            (
                Filter::Layers,
                self.tr("Vulkan layers", "Vulkan katmanları"),
            ),
            (Filter::Mods, self.tr("Game mods", "Oyun modları")),
        ]
        .into_iter()
        .enumerate()
        {
            chips = chips.child(
                widgets::chip(
                    ("dl-filter", ix),
                    format!("{label} · {}", count(f)),
                    self.downloads.filter == f,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.downloads.filter = f;
                    cx.notify();
                })),
            );
        }
        page = page
            .child(section(self.tr("INSTALLED", "KURULU")))
            .child(chips);

        let filter = self.downloads.filter;
        let shown: Vec<Row> = rows
            .into_iter()
            .filter(|r| filter == Filter::All || r.filter == filter)
            .collect();
        if shown.is_empty() {
            page = page.child(widgets::label(self.tr(
                "Nothing here yet. Proton builds and the tools you download from a game's page (e.g. OptiScaler, MAKO) appear here.",
                "Henüz bir şey yok. Proton derlemeleri ve bir oyunun sayfasından indirdiğin araçlar (ör. OptiScaler, MAKO) burada görünür.",
            )));
        }
        let mut list = div().flex().flex_col().gap_2();
        for (ix, row) in shown.into_iter().enumerate() {
            let busy = progress::running(&row.key);
            let Row {
                key,
                icon,
                title,
                detail,
                update,
                remove,
                ..
            } = row;
            let mut actions = div().flex().items_center().gap_2();
            if busy {
                actions = actions.child(widgets::label(self.tr("Working…", "Çalışıyor…")));
            } else {
                if let Some(update) = update {
                    let (k, l) = (key.clone(), title.clone());
                    actions = actions.child(
                        style::button(
                            ("dl-update", ix),
                            self.tr("Check for updates", "Güncellemeleri denetle"),
                            false,
                        )
                        .child(style::icon("refresh-cw", 13., pal::muted()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.downloads_job(k.clone(), l.clone(), update(), cx)
                        })),
                    );
                }
                if let Some(remove) = remove {
                    let (k, l) = (key.clone(), title.clone());
                    actions = actions.child(
                        style::button(("dl-remove", ix), self.tr("Remove", "Sil"), false)
                            .child(style::icon("trash", 13., pal::danger()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.downloads_job(k.clone(), l.clone(), remove(), cx)
                            })),
                    );
                }
            }
            list = list.child(
                style::card()
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .size(px(40.))
                            .flex_none()
                            .rounded(px(10.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(pal::raised())
                            .child(style::icon(icon, 18., pal::accent_light())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
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
                    .when(busy, |d| {
                        d.child(div().w(px(120.)).child(progress_bar(None)))
                    })
                    .child(actions),
            );
        }
        page.child(list)
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
