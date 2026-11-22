//! Per-game extras on the game page: Proton choice, lsfg-vk frame generation and OptiScaler.

use std::path::PathBuf;

use arkor_core::profile::{LSFG_FLOW_SCALE_RANGE, LSFG_MULTIPLIER_RANGE};
use arkor_core::{Game, LsfgMode, Store};
use arkor_launch::optiscaler::{self, Manifest, PROXY_DLLS};
use arkor_launch::tools;
use gpui::{Context, Div, PathPromptOptions, div, prelude::*, rgb};

use super::widgets::{self, theme::*};
use super::{Library, Selection};

/// Machine facts (scanned once) plus OptiScaler state for the selected game.
pub(super) struct GameTools {
    protons: Vec<(String, PathBuf)>,
    lsfg_layer: bool,
    lossless_dll: bool,
    manifest: Option<Manifest>,
    downloaded: Option<(String, PathBuf)>,
    /// Folders holding the game's likely main executables, best first.
    folders: Vec<PathBuf>,
    target: Option<PathBuf>,
    proxy: &'static str,
    busy: Option<&'static str>,
    error: Option<String>,
}

impl GameTools {
    pub(super) fn scan() -> Self {
        Self {
            protons: tools::installed_protons(),
            lsfg_layer: tools::lsfg_layer_installed(),
            lossless_dll: tools::lossless_dll().is_some(),
            manifest: None,
            downloaded: None,
            folders: Vec::new(),
            target: None,
            proxy: PROXY_DLLS[0],
            busy: None,
            error: None,
        }
    }

    /// Resets the per-game part for `game`.
    pub(super) fn select(&mut self, game: &Game) {
        self.manifest = Manifest::load(&game.id).ok().flatten();
        self.downloaded = optiscaler::downloaded().into_iter().next();
        self.folders.clear();
        if let Some(dir) = &game.install_dir {
            for exe in arkor_stores::prefix::game_executables(dir) {
                if let Some(parent) = exe.path.parent()
                    && !self.folders.iter().any(|f| f == parent)
                {
                    self.folders.push(parent.to_path_buf());
                }
                if self.folders.len() == 4 {
                    break;
                }
            }
        }
        self.target = self.folders.first().cloned();
        self.proxy = PROXY_DLLS[0];
        self.busy = None;
        self.error = None;
    }
}

impl Library {
    fn set_proton(&mut self, proton: Option<String>, cx: &mut Context<Self>) {
        self.profile.proton = proton;
        self.save();
        cx.notify();
    }

    fn edit_lsfg(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut arkor_core::Lsfg)) {
        f(&mut self.profile.lsfg);
        self.save();
        cx.notify();
    }

    fn selected_game(&self) -> Option<&Game> {
        match self.selection {
            Some(Selection::Game(ix)) => self.games.get(ix),
            _ => None,
        }
    }

    fn download_optiscaler(&mut self, cx: &mut Context<Self>) {
        self.tools.busy = Some(self.t().opti_downloading);
        self.tools.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let release = optiscaler::latest_release()?;
                    let dir = optiscaler::download(&release)?;
                    anyhow::Ok((release.tag, dir))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools.busy = None;
                match result {
                    Ok(done) => this.tools.downloaded = Some(done),
                    Err(e) => this.tools.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn choose_opti_folder(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(self.t().opti_choose_folder.into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.tools.target = paths.into_iter().next();
                cx.notify();
            });
        })
        .detach();
    }

    fn install_optiscaler(&mut self, cx: &mut Context<Self>) {
        let (Some(game), Some(target), Some((tag, source))) = (
            self.selected_game(),
            self.tools.target.clone(),
            self.tools.downloaded.clone(),
        ) else {
            return;
        };
        let id = game.id.clone();
        let proxy = self.tools.proxy;
        self.tools.busy = Some(self.t().opti_install);
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move { optiscaler::install(&id, &tag, &source, &target, proxy) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools.busy = None;
                match result {
                    Ok(m) => this.tools.manifest = Some(m),
                    Err(e) => this.tools.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_optiscaler(&mut self, cx: &mut Context<Self>) {
        let Some(game) = self.selected_game() else {
            return;
        };
        match optiscaler::uninstall(&game.id) {
            Ok(()) => self.tools.manifest = None,
            Err(e) => self.tools.error = Some(format!("{e:#}")),
        }
        cx.notify();
    }

    pub(super) fn render_proton(&self, game: &Game, cx: &mut Context<Self>) -> Option<Div> {
        // Steam picks the Proton for its games (per game, in Steam's Properties).
        if game.id.store == Store::Steam {
            return None;
        }
        let t = self.t();
        let current = self.profile.proton.clone();
        let mut chips = div().flex().flex_wrap().gap_2().child(
            widgets::chip("proton-auto", t.proton_auto, current.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.set_proton(None, cx))),
        );
        for (ix, (name, _)) in self.tools.protons.iter().enumerate() {
            let value = name.clone();
            chips = chips.child(
                widgets::chip(
                    ("proton", ix),
                    name.clone(),
                    current.as_deref() == Some(name),
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.set_proton(Some(value.clone()), cx)),
                ),
            );
        }
        let note = if self.tools.protons.is_empty() {
            t.proton_none
        } else {
            t.proton_auto_note
        };
        Some(
            widgets::section(t.proton_section)
                .child(chips)
                .child(widgets::label(note)),
        )
    }

    pub(super) fn render_lsfg(&self, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        let lsfg = &self.profile.lsfg;
        let mode_chip = |ix: usize, mode: LsfgMode, text: &'static str, cx: &mut Context<Self>| {
            widgets::chip(("lsfg-mode", ix), text, lsfg.mode == mode)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_lsfg(cx, |l| l.mode = mode)))
        };
        let mut card = widgets::section("lsfg-vk")
            .child(widgets::label(t.lsfg_hint))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(mode_chip(0, LsfgMode::Default, t.lsfg_default, cx))
                    .child(mode_chip(1, LsfgMode::On, t.lsfg_on, cx))
                    .child(mode_chip(2, LsfgMode::Off, t.lsfg_off, cx)),
            );
        card = match lsfg.mode {
            LsfgMode::Default => card.child(widgets::label(t.lsfg_default_note)),
            LsfgMode::Off => card.child(widgets::label(t.lsfg_off_note)),
            LsfgMode::On => {
                let btn = |id: &'static str,
                           glyph: &'static str,
                           f: fn(&mut arkor_core::Lsfg),
                           cx: &mut Context<Self>| {
                    widgets::step_button(id, glyph)
                        .on_click(cx.listener(move |this, _, _, cx| this.edit_lsfg(cx, f)))
                };
                card.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_6()
                        .child(widgets::stepper(
                            t.lsfg_multiplier,
                            format!("{}x", lsfg.multiplier),
                            btn("lsfg-mult-", "−", |l| l.step_multiplier(false), cx)
                                .when(lsfg.multiplier <= LSFG_MULTIPLIER_RANGE.0, |d| {
                                    d.opacity(0.4)
                                }),
                            btn("lsfg-mult+", "+", |l| l.step_multiplier(true), cx)
                                .when(lsfg.multiplier >= LSFG_MULTIPLIER_RANGE.1, |d| {
                                    d.opacity(0.4)
                                }),
                        ))
                        .child(widgets::stepper(
                            t.lsfg_flow_scale,
                            t.percent(lsfg.flow_scale),
                            btn("lsfg-flow-", "−", |l| l.step_flow_scale(false), cx)
                                .when(lsfg.flow_scale <= LSFG_FLOW_SCALE_RANGE.0, |d| {
                                    d.opacity(0.4)
                                }),
                            btn("lsfg-flow+", "+", |l| l.step_flow_scale(true), cx)
                                .when(lsfg.flow_scale >= LSFG_FLOW_SCALE_RANGE.1, |d| {
                                    d.opacity(0.4)
                                }),
                        )),
                )
                .child(
                    widgets::switch_row(
                        "lsfg-perf",
                        t.lsfg_performance,
                        t.lsfg_performance_hint,
                        lsfg.performance_mode,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.edit_lsfg(cx, |l| l.performance_mode = !l.performance_mode)
                    })),
                )
            }
        };
        if lsfg.mode == LsfgMode::On {
            if !self.tools.lsfg_layer {
                card = card.child(warning(t.lsfg_missing_layer));
            } else if !self.tools.lossless_dll {
                card = card.child(warning(t.lsfg_missing_dll));
            }
        }
        card
    }

    pub(super) fn render_optiscaler(&self, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        let tools = &self.tools;
        let mut card = widgets::section("OptiScaler").child(widgets::label(t.opti_hint));

        if let Some(m) = &tools.manifest {
            card = card
                .child(
                    div()
                        .text_sm()
                        .child(format!("{} · {} · {}", t.opti_installed, m.version, m.dll)),
                )
                .child(widgets::label(m.target_dir.display().to_string()))
                .child(widgets::label(t.opti_in_game))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            widgets::chip("opti-remove", t.opti_remove, false)
                                .on_click(cx.listener(|this, _, _, cx| this.remove_optiscaler(cx))),
                        )
                        .child(widgets::label(t.opti_remove_note)),
                );
        } else if let Some(busy) = tools.busy {
            card = card.child(widgets::label(busy));
        } else if tools.downloaded.is_none() {
            card = card.child(
                div().flex().child(
                    widgets::chip("opti-download", t.opti_download, false)
                        .on_click(cx.listener(|this, _, _, cx| this.download_optiscaler(cx))),
                ),
            );
        } else {
            let mut folders = div().flex().flex_col().gap_1();
            for (ix, folder) in tools.folders.iter().enumerate() {
                let f = folder.clone();
                folders = folders.child(
                    widgets::chip(
                        ("opti-folder", ix),
                        folder.display().to_string(),
                        tools.target.as_ref() == Some(folder),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tools.target = Some(f.clone());
                        cx.notify();
                    })),
                );
            }
            if let Some(chosen) = tools.target.as_ref().filter(|t| !tools.folders.contains(t)) {
                folders = folders.child(widgets::chip(
                    "opti-folder-custom",
                    chosen.display().to_string(),
                    true,
                ));
            }
            if tools.folders.is_empty() && tools.target.is_none() {
                folders = folders.child(widgets::label(t.opti_no_folder));
            }
            let mut proxies = div().flex().flex_wrap().gap_2();
            for (ix, proxy) in PROXY_DLLS.into_iter().enumerate() {
                proxies = proxies.child(
                    widgets::chip(
                        ("opti-proxy", ix),
                        format!("{proxy}.dll"),
                        tools.proxy == proxy,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tools.proxy = proxy;
                        cx.notify();
                    })),
                );
            }
            let version = tools
                .downloaded
                .as_ref()
                .map(|(tag, _)| tag.clone())
                .unwrap_or_default();
            card = card
                .child(widgets::label(t.opti_folder))
                .child(folders)
                .child(
                    div().flex().child(
                        widgets::chip("opti-pick-folder", t.opti_choose_folder, false)
                            .on_click(cx.listener(|this, _, _, cx| this.choose_opti_folder(cx))),
                    ),
                )
                .child(widgets::label(t.opti_proxy))
                .child(proxies)
                .child(widgets::label(t.opti_proxy_hint))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .when(tools.target.is_some(), |d| {
                            d.child(
                                widgets::primary_button(
                                    "opti-install",
                                    format!("{} ({version})", t.opti_install),
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.install_optiscaler(cx)),
                                ),
                            )
                        }),
                );
        }
        if let Some(error) = &tools.error {
            card = card.child(warning(error.clone()));
        }
        card
    }
}

fn warning(text: impl Into<gpui::SharedString>) -> Div {
    div().text_sm().text_color(rgb(ACCENT)).child(text.into())
}
