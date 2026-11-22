//! "Downloads": every tool Arkor has downloaded (Vulkan layers, OptiScaler, DLSS Enabler,
//! ReShade, Streamline SDK, AMD bundles), each with a check for updates and a remove.
//! Removing a download does not touch games it was installed into: those got their own copy.

use std::path::{Path, PathBuf};

use arkor_launch::bundles::{self, Bundle};
use arkor_launch::{dlss_enabler, layers, optiscaler, reshade, streamline};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*};

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
    /// Kind being updated or removed right now.
    busy: Option<Kind>,
    /// Bumped after each change so the list is read again.
    generation: u64,
}

impl Library {
    fn downloads_job(
        &mut self,
        kind: Kind,
        cx: &mut Context<Self>,
        job: impl FnOnce() -> anyhow::Result<String> + Send + 'static,
    ) {
        if self.downloads.busy.is_some() {
            return;
        }
        self.downloads.busy = Some(kind);
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { job() }).await;
            let _ = this.update(cx, |this, cx| {
                this.downloads.busy = None;
                this.downloads.generation += 1;
                this.status = Some(match result {
                    Ok(msg) => msg.into(),
                    Err(e) => format!("{}: {e:#}", kind.name()).into(),
                });
                // Game pages read download state from here.
                this.tools = super::enhance::GameTools::scan();
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_downloads(&mut self, cx: &mut Context<Self>) -> Div {
        let _ = self.downloads.generation;
        let mut page = div().child(Self::header(
            self.tr("Downloads", "İndirilenler"),
            self.tr(
                "Tools Arkor downloaded for your games. Removing one does not change games it was installed into.",
                "Arkor'un oyunların için indirdiği araçlar. Birini silmek, kurulduğu oyunları değiştirmez.",
            ),
        ));
        let mut any = false;
        for kind in Kind::all() {
            for (ix, (version, dir)) in kind.downloaded().into_iter().enumerate() {
                any = true;
                let busy = self.downloads.busy == Some(kind);
                let key = format!("{kind:?}-{ix}");
                let up_id = gpui::ElementId::Name(format!("dl-update-{key}").into());
                let rm_id = gpui::ElementId::Name(format!("dl-remove-{key}").into());
                let name = kind.name();
                let up_label = self.tr("Check for updates", "Güncellemeleri denetle");
                let done = self.tr("Up to date:", "Güncel:");
                page = page.child(
                    style::card()
                        .p_4()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(
                            div()
                                .size(gpui::px(36.))
                                .flex_none()
                                .rounded(gpui::px(10.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(pal::raised())
                                .child(style::icon(kind.icon(), 18., pal::accent_light())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().font_weight(gpui::FontWeight::MEDIUM).child(
                                    if version.is_empty() {
                                        name.to_string()
                                    } else {
                                        format!("{name} {version}")
                                    },
                                ))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(pal::muted())
                                        .truncate()
                                        .child(dir.display().to_string()),
                                ),
                        )
                        .when(busy, |d| {
                            d.child(widgets::label(self.tr("Working…", "Çalışıyor…")))
                        })
                        .when(!busy, |d| {
                            let dir2 = dir.clone();
                            d.child(
                                style::button(up_id, up_label, false)
                                    .child(style::icon("refresh-cw", 13., pal::muted()))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.downloads_job(kind, cx, move || {
                                            kind.update().map(|v| format!("{done} {name} {v}"))
                                        });
                                    })),
                            )
                            .child(
                                style::button(rm_id, self.tr("Remove", "Sil"), false)
                                    .child(style::icon("trash", 13., pal::danger()))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let dir = dir2.clone();
                                        this.downloads_job(kind, cx, move || {
                                            kind.remove(&dir).map(|_| format!("{name} ✕"))
                                        });
                                    })),
                            )
                        }),
                );
            }
        }
        if !any {
            page = page.child(widgets::label(self.tr(
                "Nothing downloaded yet. Tools you download from a game's page (e.g. OptiScaler, MAKO) appear here.",
                "Henüz bir şey indirilmedi. Bir oyunun sayfasından indirdiğin araçlar (ör. OptiScaler, MAKO) burada görünür.",
            )));
        }
        page
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
