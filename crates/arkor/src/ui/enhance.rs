//! Per-game extras on the game page: Proton choice, lsfg-vk frame generation and OptiScaler.

use std::path::PathBuf;

use arkor_core::profile::LSFG_MULTIPLIER_RANGE;
use arkor_core::{Game, LsfgMode, Store};
use arkor_launch::optiscaler::{self, Manifest, PROXY_DLLS};
use arkor_launch::tools;
use gpui::{Context, Div, PathPromptOptions, SharedString, Window, div, prelude::*, px};

use super::widgets::{self, theme::*};
use super::{Library, Selection};

/// Machine facts (scanned once) plus OptiScaler state for the selected game.
/// Layers that have their own card; "Other Vulkan layers" lists the rest.
pub(super) const HANDLED_LAYERS: [&str; 9] = [
    "AFMF_ENABLE",
    "VKNEMU",
    "ENABLE_MAKO",
    "ENABLE_MAKO_SPATIAL_SCALING",
    "ENABLE_VKSUMI",
    "ENABLE_VKBASALT",
    "MANGOHUD",
    "LSFGVK_ENV",
    "ENABLE_LSFG",
];

pub(super) struct GameTools {
    pub(super) protons: Vec<(String, PathBuf)>,
    pub(super) lossless_dll: bool,
    /// The installed lsfg-vk layer (library, user or system install, version).
    pub(super) lsfg_info: Option<tools::LsfgLayer>,
    /// Lossless Scaling's Steam folder.
    pub(super) ls_dir: Option<PathBuf>,
    /// GPU makers in this computer (AMD-only tools are hidden without an AMD card).
    pub(super) gpus: tools::Gpus,
    /// The graphics cards, to choose one per game when there is more than one.
    pub(super) cards: Vec<arkor_launch::gpu::Gpu>,
    pub(super) manifest: Option<Manifest>,
    pub(super) downloaded: Option<(String, PathBuf)>,
    pub(super) de_manifest: Option<Manifest>,
    pub(super) rs_manifest: Option<Manifest>,
    pub(super) rs_downloaded: Option<(String, PathBuf)>,
    pub(super) rs_proxy: &'static str,
    pub(super) rs_32bit: bool,
    pub(super) vkbasalt_installed: bool,
    /// Arkor-installed copies of `layers::TOOLS`, in order.
    pub(super) layer_tools: [bool; 4],
    pub(super) opti_channel: optiscaler::Channel,
    pub(super) sl_manifest: Option<Manifest>,
    pub(super) sl_downloaded: Option<(String, PathBuf)>,
    /// Streamline / DLSS DLLs the game ships that the downloaded SDK can replace.
    pub(super) sl_replaceable: usize,
    pub(super) fsr4: Option<Manifest>,
    /// Downloaded AMD bundles, per `bundles::Bundle::ALL`.
    pub(super) bundles: [Option<(String, PathBuf)>; 3],
    pub(super) dlssnr: Option<arkor_launch::bundles::DlssNr>,
    pub(super) dlssnr_model: &'static str,
    pub(super) optipatcher: bool,
    /// Implicit layers on the system that a variable switches on.
    pub(super) system_layers: Vec<arkor_launch::layers::SystemLayer>,
    /// The game's OptiScaler.ini text (when installed), cached for the settings chips.
    pub(super) ini: Option<(PathBuf, String)>,
    pub(super) de_downloaded: Option<(String, PathBuf)>,
    /// Folders holding the game's likely main executables, best first.
    pub(super) folders: Vec<PathBuf>,
    pub(super) target: Option<PathBuf>,
    pub(super) proxy: &'static str,
    pub(super) busy: Option<&'static str>,
    pub(super) error: Option<String>,
    pub(super) exe_cache: std::cell::RefCell<std::collections::HashMap<PathBuf, Vec<PathBuf>>>,
}

impl GameTools {
    fn load_ini(&mut self, id: &arkor_core::GameId) {
        self.ini =
            optiscaler::ini_path(id).and_then(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)));
    }

    /// Executables found in a prefix (cached per prefix; scanning is not free).
    pub(super) fn prefix_exes(&self, prefix: &std::path::Path) -> Vec<PathBuf> {
        let mut cache = self.exe_cache.borrow_mut();
        cache
            .entry(prefix.to_path_buf())
            .or_insert_with(|| {
                arkor_stores::prefix::find_executables(prefix)
                    .into_iter()
                    .filter(|c| {
                        c.path
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
                    })
                    .map(|c| c.path)
                    .collect()
            })
            .clone()
    }

    pub(super) fn scan() -> Self {
        Self {
            protons: tools::installed_protons(),
            lossless_dll: tools::lossless_dll().is_some(),
            lsfg_info: tools::lsfg_layer_quick(),
            ls_dir: tools::lossless_scaling_dir(),
            gpus: tools::gpus(),
            cards: if cfg!(test) {
                Vec::new()
            } else {
                arkor_launch::gpu::list()
            },
            manifest: None,
            downloaded: None,
            de_manifest: None,
            rs_manifest: None,
            rs_downloaded: None,
            rs_proxy: "dxgi",
            rs_32bit: false,
            vkbasalt_installed: tools::vkbasalt_installed(),
            opti_channel: optiscaler::Channel::Stable,
            sl_manifest: None,
            sl_downloaded: None,
            sl_replaceable: 0,
            fsr4: None,
            bundles: Default::default(),
            dlssnr: None,
            dlssnr_model: arkor_launch::bundles::DLSSNR_MODELS[0],
            optipatcher: false,
            layer_tools: std::array::from_fn(|i| {
                arkor_launch::layers::arkor_installed(arkor_launch::layers::TOOLS[i].id)
            }),
            system_layers: arkor_launch::layers::system_layers(),
            ini: None,
            de_downloaded: None,
            folders: Vec::new(),
            target: None,
            proxy: PROXY_DLLS[0],
            busy: None,
            error: None,
            exe_cache: Default::default(),
        }
    }

    /// Resets the per-game part for `game`.
    pub(super) fn select(&mut self, game: &Game) {
        self.manifest = Manifest::load(&game.id).ok().flatten();
        self.downloaded = optiscaler::downloaded().into_iter().next();
        self.de_manifest = arkor_launch::dlss_enabler::manifest(&game.id);
        self.rs_manifest = arkor_launch::reshade::manifest(&game.id);
        self.rs_downloaded = arkor_launch::reshade::downloaded().into_iter().next();
        self.load_ini(&game.id);
        self.optipatcher = optiscaler::optipatcher_installed(&game.id);
        self.bundles = arkor_launch::bundles::Bundle::ALL
            .map(|b| arkor_launch::bundles::downloaded(b).into_iter().next());
        self.dlssnr = arkor_launch::bundles::dlssnr(&game.id);
        self.fsr4 = Manifest::load_mod(optiscaler::Mod::Fsr4, &game.id)
            .ok()
            .flatten();
        self.sl_manifest = arkor_launch::streamline::manifest(&game.id);
        self.sl_downloaded = arkor_launch::streamline::downloaded().into_iter().next();
        self.sl_replaceable = match (&self.sl_downloaded, &game.install_dir) {
            (Some((_, sdk)), Some(dir)) => arkor_launch::streamline::replaceable(dir, sdk).len(),
            _ => 0,
        };
        self.de_downloaded = arkor_launch::dlss_enabler::downloaded().into_iter().next();
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
    /// Executables the game likely runs as: the configured exe (if a real .exe), plus .exe
    /// files in its prefix named like the game (for shortcut / launcher entries such as
    /// Ubisoft's `.url` files, where the game process is something else).
    /// For a game started through a shortcut (`.url` / `.lnk`, e.g. a launcher's), the
    /// folder next to the shortcut is not the game's: look in its prefix for programs named
    /// like the game and offer their folders for mods (OptiScaler, ReShade, HelixSR...).
    pub(super) fn find_game_folders(&mut self) {
        let Some(entry) = self.entry().cloned() else {
            return;
        };
        let shortcut = entry
            .exe
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("url") || e.eq_ignore_ascii_case("lnk"));
        if !shortcut || !entry.needs_prefix() {
            return;
        }
        let squash = |s: &str| {
            s.chars()
                .filter(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_lowercase())
                .collect::<String>()
        };
        let wanted = squash(&entry.name);
        if wanted.len() < 3 {
            return;
        }
        let prefix = entry.effective_prefix(&self.global.prefixes_dir());
        let mut folders: Vec<PathBuf> = Vec::new();
        for exe in self.tools.prefix_exes(&prefix) {
            let stem = exe
                .file_stem()
                .map(|s| squash(&s.to_string_lossy()))
                .unwrap_or_default();
            if (stem == wanted || stem.starts_with(&wanted))
                && let Some(parent) = exe.parent()
                && !folders.iter().any(|f| f == parent)
            {
                folders.push(parent.to_path_buf());
            }
        }
        if !folders.is_empty() {
            folders.truncate(4);
            self.tools.target = folders.first().cloned();
            self.tools.folders = folders;
        }
    }

    fn lsfg_suggestions(&self) -> Vec<String> {
        let Some(entry) = self.entry() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let is_exe = entry
            .exe
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        if is_exe && let Some(name) = entry.exe.file_name() {
            out.push(name.to_string_lossy().into_owned());
        }
        let squash = |s: &str| {
            s.chars()
                .filter(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_lowercase())
                .collect::<String>()
        };
        let wanted = squash(&entry.name);
        if entry.needs_prefix() && wanted.len() >= 3 {
            let prefix = entry.effective_prefix(&self.global.prefixes_dir());
            for c in self.tools.prefix_exes(&prefix) {
                let stem = c
                    .file_stem()
                    .map(|s| squash(&s.to_string_lossy()))
                    .unwrap_or_default();
                if (stem == wanted || stem.starts_with(&wanted))
                    && let Some(name) = c.file_name().map(|n| n.to_string_lossy().into_owned())
                    && !out.contains(&name)
                {
                    out.push(name);
                }
            }
        }
        out
    }

    fn toggle_tweak(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if !self.profile.tweaks.remove(id) {
            self.profile.tweaks.insert(id.to_string());
        }
        self.save();
        cx.notify();
    }

    pub(super) fn render_tweaks(&mut self, cx: &mut Context<Self>) -> Div {
        let selected = self.profile.tweaks.clone();
        widgets::section(self.tr("Proton and driver switches", "Proton ve sürücü anahtarları"))
            .child(self.tweak_list("tweak", &selected, cx, |this, id, cx| {
                this.toggle_tweak(id, cx)
            }))
    }

    pub(super) fn render_gamescope(&mut self, cx: &mut Context<Self>) -> Div {
        let num_field = |this: &mut Self,
                         key: &'static str,
                         get: fn(&arkor_core::profile::Gamescope) -> Option<u32>,
                         set: fn(&mut arkor_core::profile::Gamescope, Option<u32>),
                         placeholder: &'static str,
                         cx: &mut Context<Self>| {
            this.field(
                key,
                move |this| {
                    get(&this.profile.gamescope)
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                },
                placeholder,
                cx,
                move |this, text| {
                    set(
                        &mut this.profile.gamescope,
                        text.trim().parse().ok().filter(|v: &u32| *v > 0),
                    );
                    this.save();
                },
            )
        };
        let w = num_field(self, "gs-w", |g| g.width, |g, v| g.width = v, "2560", cx);
        let h = num_field(self, "gs-h", |g| g.height, |g, v| g.height = v, "1440", cx);
        let gw = num_field(
            self,
            "gs-gw",
            |g| g.game_width,
            |g, v| g.game_width = v,
            "1920",
            cx,
        );
        let gh = num_field(
            self,
            "gs-gh",
            |g| g.game_height,
            |g, v| g.game_height = v,
            "1080",
            cx,
        );
        let r = num_field(self, "gs-r", |g| g.refresh, |g, v| g.refresh = v, "144", cx);
        let extra = self.field(
            "gs-args",
            |this| this.profile.gamescope.args.join(" "),
            "--expose-wayland",
            cx,
            |this, text| {
                this.profile.gamescope.args = arkor_core::profile::shell_words(text);
                this.save();
            },
        );
        let gs = self.profile.gamescope.clone();
        let t = self.t();
        let mut card = widgets::section("Gamescope").child(
            widgets::switch_row("gamescope", "Gamescope", t.gamescope_hint, gs.enabled).on_click(
                cx.listener(|this, _, _, cx| {
                    this.profile.gamescope.enabled = !this.profile.gamescope.enabled;
                    this.save();
                    cx.notify();
                }),
            ),
        );
        if !gs.enabled {
            return card;
        }
        let pair = |label: &'static str, a, b| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(widgets::label(label))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(div().w(gpui::px(90.)).child(a))
                        .child("×")
                        .child(div().w(gpui::px(90.)).child(b)),
                )
        };
        let mut filters = div().flex().flex_wrap().gap_2().child(
            widgets::chip(
                "gs-filter-none",
                self.tr("No upscaling filter", "Ölçekleme filtresi yok"),
                gs.filter.is_none(),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.profile.gamescope.filter = None;
                this.save();
                cx.notify();
            })),
        );
        for (ix, f) in arkor_core::profile::GAMESCOPE_FILTERS
            .into_iter()
            .enumerate()
        {
            filters = filters.child(
                widgets::chip(
                    ("gs-filter", ix),
                    f.to_uppercase(),
                    gs.filter.as_deref() == Some(f),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.profile.gamescope.filter = Some(f.to_string());
                    this.save();
                    cx.notify();
                })),
            );
        }
        let flag = |id: &'static str,
                    text: &'static str,
                    on: bool,
                    f: fn(&mut arkor_core::profile::Gamescope),
                    cx: &mut Context<Self>| {
            widgets::chip(id, text, on).on_click(cx.listener(move |this, _, _, cx| {
                f(&mut this.profile.gamescope);
                this.save();
                cx.notify();
            }))
        };
        card = card
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_6()
                    .child(pair(self.tr("Output size", "Çıkış boyutu"), w, h))
                    .child(pair(
                        self.tr("Game renders at", "Oyunun çizim boyutu"),
                        gw,
                        gh,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(widgets::label(self.tr("Refresh (Hz)", "Yenileme (Hz)")))
                            .child(div().w(gpui::px(90.)).child(r)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(flag(
                        "gs-fullscreen",
                        self.tr("Fullscreen", "Tam ekran"),
                        gs.fullscreen,
                        |g| g.fullscreen = !g.fullscreen,
                        cx,
                    ))
                    .child(flag(
                        "gs-borderless",
                        self.tr("Borderless", "Kenarlıksız"),
                        gs.borderless,
                        |g| g.borderless = !g.borderless,
                        cx,
                    ))
                    .child(flag("gs-hdr", "HDR", gs.hdr, |g| g.hdr = !g.hdr, cx)),
            )
            .child(widgets::label(self.tr(
                "Upscaling filter (when the game renders smaller than the output)",
                "Ölçekleme filtresi (oyun çıkıştan küçük çizerken)",
            )))
            .child(filters)
            .child(widgets::label(self.tr("Extra arguments", "Ek argümanlar")))
            .child(extra)
            .child(widgets::label(format!(
                "gamescope {}",
                gs.to_args().join(" ")
            )));
        card
    }

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

    pub(super) fn selected_game(&self) -> Option<&Game> {
        match self.selection {
            Some(Selection::Game(ix)) => self.games.get(ix),
            _ => None,
        }
    }

    /// Stable / Edge choice plus the download button.
    fn opti_channel_row(&self, download: &'static str, cx: &mut Context<Self>) -> Div {
        use optiscaler::Channel;
        let mut row = div().flex().flex_wrap().items_center().gap_2();
        for (ix, (ch, label)) in [
            (Channel::Stable, self.tr("Stable", "Kararlı")),
            (Channel::Edge, self.tr("Edge (daily)", "Edge (günlük)")),
        ]
        .into_iter()
        .enumerate()
        {
            row = row.child(
                widgets::chip(("opti-channel", ix), label, self.tools.opti_channel == ch).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.tools.opti_channel = ch;
                        cx.notify();
                    }),
                ),
            );
        }
        row.child(
            widgets::chip("opti-download", download, false)
                .on_click(cx.listener(|this, _, _, cx| this.download_optiscaler(cx))),
        )
    }

    fn render_fsr4(&self, cx: &mut Context<Self>) -> Div {
        let row = div().flex().flex_wrap().items_center().gap_3();
        match &self.tools.fsr4 {
            Some(m) => row
                .child(div().text_sm().child(format!(
                    "{}: {}",
                    self.tr("Custom FSR4", "Özel FSR4"),
                    m.version
                )))
                .child(
                    widgets::chip(
                        "fsr4-remove",
                        self.tr("Use the bundled FSR4", "Paketteki FSR4'ü kullan"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some(id) = this.selected_game().map(|g| g.id.clone()) else {
                            return;
                        };
                        if let Err(e) = optiscaler::uninstall_mod(optiscaler::Mod::Fsr4, &id) {
                            this.tools.error = Some(format!("{e:#}"));
                        }
                        this.tools.fsr4 = None;
                        cx.notify();
                    })),
                ),
            None => row
                .child(widgets::label(self.tr(
                    "FSR4 build: the one bundled with OptiScaler",
                    "FSR4 sürümü: OptiScaler ile gelen",
                )))
                .child(
                    widgets::chip(
                        "fsr4-import",
                        self.tr(
                            "Import amd_fidelityfx_upscaler_dx12.dll…",
                            "amd_fidelityfx_upscaler_dx12.dll içe aktar…",
                        ),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        let title = this.tr("Choose an FSR4 DLL", "Bir FSR4 DLL'i seç");
                        this.prompt_path(false, title, cx, |this, path, cx| {
                            let Some(id) = this.selected_game().map(|g| g.id.clone()) else {
                                return;
                            };
                            match optiscaler::install_fsr4(&id, &path) {
                                Ok(m) => this.tools.fsr4 = Some(m),
                                Err(e) => this.tools.error = Some(format!("{e:#}")),
                            }
                            cx.notify();
                        });
                    })),
                ),
        }
    }

    pub(super) fn render_streamline(&self, cx: &mut Context<Self>) -> Div {
        let tools = &self.tools;
        let mut card = widgets::section("Streamline SDK");
        if let Some(m) = &tools.sl_manifest {
            card = card
                .child(div().text_sm().child(format!(
                    "{} {} · {} {}",
                    self.tr("Updated to", "Güncellendi:"),
                    m.version,
                    m.created.len(),
                    self.tr("files", "dosya")
                )))
                .child(
                    widgets::chip(
                        "sl-remove",
                        self.tr("Restore the game's DLLs", "Oyunun DLL'lerini geri yükle"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some(id) = this.selected_game().map(|g| g.id.clone()) else {
                            return;
                        };
                        match arkor_launch::streamline::uninstall(&id) {
                            Ok(()) => this.tools.sl_manifest = None,
                            Err(e) => this.tools.error = Some(format!("{e:#}")),
                        }
                        cx.notify();
                    })),
                );
        } else if let Some(busy) = tools.busy {
            card = card.child(widgets::label(busy));
        } else if let Some((v, _)) = &tools.sl_downloaded {
            if tools.sl_replaceable == 0 {
                card = card.child(widgets::label(self.tr(
                    "This game ships no Streamline or DLSS DLLs.",
                    "Bu oyunda Streamline ya da DLSS DLL'i yok.",
                )));
            } else {
                card = card.child(
                    widgets::chip(
                        "sl-install",
                        format!(
                            "{} {v} ({} {})",
                            self.tr("Update to", "Güncelle:"),
                            tools.sl_replaceable,
                            self.tr("files", "dosya")
                        ),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some(game) = this.selected_game() else {
                            return;
                        };
                        let (id, dir) = (game.id.clone(), game.install_dir.clone());
                        let (Some(dir), Some((v, sdk))) = (dir, this.tools.sl_downloaded.clone())
                        else {
                            return;
                        };
                        match arkor_launch::streamline::install(&id, &v, &sdk, &dir) {
                            Ok(m) => this.tools.sl_manifest = Some(m),
                            Err(e) => this.tools.error = Some(format!("{e:#}")),
                        }
                        cx.notify();
                    })),
                );
            }
        } else {
            card = card.child(
                widgets::chip(
                    "sl-download",
                    self.tr(
                        "Download the SDK (about 280 MB)",
                        "SDK'yı indir (yaklaşık 280 MB)",
                    ),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| this.download_streamline(cx))),
            );
        }
        card
    }

    pub(super) fn download_streamline(&mut self, cx: &mut Context<Self>) {
        self.tools.busy = Some("Downloading…");
        self.tools.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let _job = arkor_launch::progress::job("streamline", "Streamline SDK");
                    let v = arkor_launch::streamline::latest_version()?;
                    let dir = arkor_launch::streamline::download(&v)?;
                    anyhow::Ok((v, dir))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools.busy = None;
                match result {
                    Ok(done) => {
                        this.tools.sl_replaceable = this
                            .selected_game()
                            .and_then(|g| g.install_dir.clone())
                            .map_or(0, |d| {
                                arkor_launch::streamline::replaceable(&d, &done.1).len()
                            });
                        this.tools.sl_downloaded = Some(done);
                    }
                    Err(e) => this.tools.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Folder the AMD bundles go into: the chosen OptiScaler folder, else the game's own.
    fn bundle_game_dir(&self) -> Option<PathBuf> {
        self.tools
            .target
            .clone()
            .or_else(|| self.selected_game()?.install_dir.clone())
    }

    pub(super) fn download_bundle(&mut self, ix: usize, cx: &mut Context<Self>) {
        let b = arkor_launch::bundles::Bundle::ALL[ix];
        self.tools.busy = Some("Downloading…");
        self.tools.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let _job = arkor_launch::progress::job(
                        arkor_launch::bundles::Bundle::ALL[ix].key(),
                        arkor_launch::bundles::Bundle::ALL[ix].name(),
                    );
                    arkor_launch::bundles::download(b)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools.busy = None;
                match result {
                    Ok(done) => {
                        this.tools.bundles[ix] = Some(done);
                        this.sound(super::sounds::Sound::Done);
                    }
                    Err(e) => this.tools.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_amd_bundles(&self, cx: &mut Context<Self>) -> Div {
        use arkor_launch::bundles::{self, Bundle, DLSSNR_MODELS};
        let tools = &self.tools;
        let mut card = widgets::section(self.tr(
            "AMD upscalers: HelixSR, DLSS-NR",
            "AMD ölçekleyicileri: HelixSR, DLSS-NR",
        ));
        if !bundles::amd_gpu() {
            card = card.child(warning(self.tr(
                "These need an AMD Radeon RX 7000 / 9000 card; this computer has another GPU.",
                "Bunlar AMD Radeon RX 7000 / 9000 kart ister; bu bilgisayarda başka bir GPU var.",
            )));
        }
        let Some(game_dir) = self.bundle_game_dir() else {
            return card.child(widgets::label(self.tr(
                "This game has no folder to install into.",
                "Bu oyunun kurulacak bir klasörü yok.",
            )));
        };
        card = card.child(widgets::label(format!(
            "{} {}",
            self.tr("Game folder:", "Oyun klasörü:"),
            game_dir.display()
        )));
        let busy = tools.busy.is_some();

        // HelixSR
        card = card.child(div().text_sm().child("HelixSR — HLX DLSS / HLX FSR 4"));
        let installed = bundles::helixsr_installed(&game_dir);
        let mut row = div().flex().flex_wrap().gap_2();
        match &tools.bundles[0] {
            None => {
                row = row.child(
                    widgets::chip(
                        "helix-dl",
                        if busy {
                            "…".into()
                        } else {
                            self.tr(
                                "Download HelixSR (about 300 MB)",
                                "HelixSR'ı indir (yaklaşık 300 MB)",
                            )
                            .to_string()
                        },
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.tools.busy.is_none() {
                            this.download_bundle(0, cx);
                        }
                    })),
                )
            }
            Some((v, dir)) => {
                let (dir, gd) = (dir.clone(), game_dir.clone());
                row = row.child(
                    widgets::chip(
                        "helix-setup",
                        format!(
                            "{} {v}",
                            if installed {
                                self.tr("Update or change", "Güncelle ya da değiştir")
                            } else {
                                self.tr("Set up", "Kur")
                            }
                        ),
                        installed,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Err(e) = bundles::helixsr_setup(&dir, &gd) {
                            this.tools.error = Some(format!("{e:#}"));
                        }
                        cx.notify();
                    })),
                );
                if installed {
                    let (dir, gd) = (
                        tools.bundles[0]
                            .as_ref()
                            .map(|b| b.1.clone())
                            .unwrap_or_default(),
                        game_dir.clone(),
                    );
                    row = row.child(
                        widgets::chip("helix-remove", self.tr("Remove", "Kaldır"), false).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if let Err(e) = bundles::helixsr_remove(&dir, &gd) {
                                    this.tools.error = Some(format!("{e:#}"));
                                }
                                cx.notify();
                            }),
                        ),
                    );
                }
            }
        }
        card = card.child(row);

        // DLSS-NR
        card = card.child(div().text_sm().child("DLSS-NR — DLSS 5 neural rendering"));
        if let Some(d) = &tools.dlssnr {
            return card
                .child(div().text_sm().child(format!(
                    "{} · {} {}",
                    d.bundle.name(),
                    self.tr("installed", "kurulu"),
                    d.version
                )))
                .child(
                    div().flex().child(
                        widgets::chip(
                            "nr-remove",
                            self.tr("Remove DLSS-NR", "DLSS-NR'ı kaldır"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            let Some(id) = this.selected_game().map(|g| g.id.clone()) else {
                                return;
                            };
                            match bundles::dlssnr_remove(&id) {
                                Ok(()) => this.tools.dlssnr = None,
                                Err(e) => this.tools.error = Some(format!("{e:#}")),
                            }
                            cx.notify();
                        })),
                    ),
                );
        }
        let mut dl = div().flex().flex_wrap().gap_2();
        for (ix, b) in [(1usize, Bundle::DlssNrRdna4), (2, Bundle::DlssNrRdna3)] {
            dl = dl.child(match &tools.bundles[ix] {
                Some((v, _)) => widgets::chip(("nr-have", ix), format!("{} {v} ✓", b.name()), true),
                None => widgets::chip(
                    ("nr-dl", ix),
                    format!("{} {}", self.tr("Download", "İndir:"), b.name()),
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.tools.busy.is_none() {
                        this.download_bundle(ix, cx);
                    }
                })),
            });
        }
        card = card.child(dl);
        if tools.bundles[1].is_some() || tools.bundles[2].is_some() {
            let mut models = div().flex().flex_wrap().gap_1();
            for (ix, m) in DLSSNR_MODELS.into_iter().enumerate() {
                models = models.child(
                    widgets::chip(("nr-model", ix), m, tools.dlssnr_model == m).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.tools.dlssnr_model = m;
                            cx.notify();
                        }),
                    ),
                );
            }
            card = card
                .child(widgets::label(self.tr(
                    "Network the game starts with (can be switched in game):",
                    "Oyunun başlayacağı ağ (oyunda değiştirilebilir):",
                )))
                .child(models)
                .child(
                    div().flex().child(
                        widgets::chip(
                            "nr-install",
                            self.tr(
                                "Choose nvngx_dlssnr file and install…",
                                "nvngx_dlssnr dosyasını seç ve kur…",
                            ),
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let title = this.tr(
                                "Choose NVIDIA's nvngx_dlssnr.dll or its zip",
                                "NVIDIA'nın nvngx_dlssnr.dll dosyasını ya da zip'ini seç",
                            );
                            this.prompt_path(false, title, cx, |this, file, cx| {
                                let Some(id) = this.selected_game().map(|g| g.id.clone()) else {
                                    return;
                                };
                                let Some(game_dir) = this.bundle_game_dir() else {
                                    return;
                                };
                                let Some((ix, (v, dir))) = [1usize, 2]
                                    .into_iter()
                                    .find_map(|ix| this.tools.bundles[ix].clone().map(|b| (ix, b)))
                                else {
                                    return;
                                };
                                let bundle = Bundle::ALL[ix];
                                let model = this.tools.dlssnr_model;
                                this.tools.busy = Some("Installing…");
                                this.tools.error = None;
                                cx.notify();
                                let bg = cx.background_executor().clone();
                                cx.spawn(async move |this, cx| {
                                    let result = bg
                                        .spawn({
                                            let id = id.clone();
                                            async move {
                                                bundles::dlssnr_install(
                                                    &id, bundle, &v, &dir, &game_dir, &file, model,
                                                )
                                            }
                                        })
                                        .await;
                                    let _ = this.update(cx, |this, cx| {
                                        this.tools.busy = None;
                                        if let Err(e) = result {
                                            this.tools.error = Some(format!("{e:#}"));
                                            this.sound(super::sounds::Sound::Error);
                                        } else {
                                            this.sound(super::sounds::Sound::Done);
                                        }
                                        this.tools.dlssnr = bundles::dlssnr(&id);
                                        cx.notify();
                                    });
                                })
                                .detach();
                            });
                        })),
                    ),
                );
        }
        card
    }

    fn toggle_optipatcher(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_game().map(|g| g.id.clone()) else {
            return;
        };
        if self.tools.busy.is_some() {
            return;
        }
        let remove = self.tools.optipatcher;
        self.tools.busy = Some("…");
        self.tools.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn({
                    let id = id.clone();
                    async move {
                        if remove {
                            optiscaler::remove_optipatcher(&id)
                        } else {
                            optiscaler::install_optipatcher(&id)
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools.busy = None;
                if let Err(e) = result {
                    this.tools.error = Some(format!("{e:#}"));
                }
                this.tools.optipatcher = optiscaler::optipatcher_installed(&id);
                this.tools.load_ini(&id);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn download_optiscaler(&mut self, cx: &mut Context<Self>) {
        self.tools.busy = Some(self.t().opti_downloading);
        self.tools.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        let channel = self.tools.opti_channel;
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let _job = arkor_launch::progress::job("optiscaler", "OptiScaler");
                    let release = optiscaler::latest_release_in(channel)?;
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
                    Ok(m) => {
                        this.tools.manifest = Some(m);
                        if let Some(id) = this.selected_id() {
                            this.tools.load_ini(&id);
                        }
                    }
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
        // What "Automatic" resolves to, shown on its chip.
        let auto_label = match (&self.global.defaults.proton, &game_source_proton(self)) {
            (_, Some(src)) => format!("{} · {src}", t.proton_auto),
            (Some(d), None) => format!("{} · {d}", t.proton_auto),
            (None, None) => t.proton_auto.to_string(),
        };
        Some(
            widgets::section(t.proton_section)
                .child(self.proton_picker(
                    "proton-pick",
                    current.as_deref(),
                    auto_label,
                    cx,
                    |this, value, cx| this.set_proton(value, cx),
                ))
                .when(self.tools.protons.is_empty(), |d| {
                    d.child(widgets::label(t.proton_none))
                }),
        )
    }

    /// One line of a status panel: a coloured mark, what it is, what was found, and a fix.
    fn status_row(
        &self,
        ok: Option<bool>,
        title: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        action: Option<gpui::Stateful<Div>>,
    ) -> Div {
        use super::style::{self, pal};
        let (icon, color) = match ok {
            Some(true) => ("circle-check", pal::success()),
            Some(false) => ("circle-x", pal::danger()),
            None => ("triangle-alert", pal::warning()),
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded(px(10.))
            .bg(pal::raised().opacity(0.45))
            .child(style::icon(icon, 16., color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(title.into()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(pal::muted())
                            .child(detail.into()),
                    ),
            )
            .children(action)
    }

    /// What lsfg-vk needs, each with a way to fix it: the layer, Lossless Scaling on Steam,
    /// and the shaders of Lossless Scaling's "lsfg-vk" beta branch (lsfg-vk 2.x).
    fn lsfg_status(&self, cx: &mut Context<Self>) -> Div {
        let open = |url: &'static str| {
            move |_: &gpui::ClickEvent, _: &mut Window, _: &mut gpui::App| {
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
            }
        };
        let _ = cx;
        let layer = match &self.tools.lsfg_info {
            Some(l) => self.status_row(
                Some(true),
                "lsfg-vk",
                format!(
                    "{} {} · {}",
                    self.tr("Installed", "Kurulu"),
                    l.version.clone().unwrap_or_default(),
                    if l.user {
                        self.tr("in your user folder", "kullanıcı klasöründe")
                    } else {
                        self.tr("system package", "sistem paketi")
                    }
                ),
                None,
            ),
            None => self.status_row(
                Some(false),
                "lsfg-vk",
                self.tr(
                    "Not installed. Install it from the AUR (lsfg-vk) or with the script on lsfg-vk.dev.",
                    "Kurulu değil. AUR'dan (lsfg-vk) ya da lsfg-vk.dev'deki betikle kur.",
                ),
                Some(widgets::chip("lsfg-site", "lsfg-vk.dev", false).on_click(open("https://lsfg-vk.dev"))),
            ),
        };
        let ls = match &self.tools.ls_dir {
            Some(dir) => self.status_row(
                Some(true),
                self.tr("Lossless Scaling (Steam)", "Lossless Scaling (Steam)"),
                dir.display().to_string(),
                None,
            ),
            None => self.status_row(
                Some(false),
                self.tr("Lossless Scaling (Steam)", "Lossless Scaling (Steam)"),
                self.tr(
                    "Not installed. lsfg-vk uses its files; buy and install it on Steam (or mount the drive with your Steam library).",
                    "Kurulu değil. lsfg-vk onun dosyalarını kullanır; Steam'den al ve kur (ya da Steam kütüphanenin olduğu diski bağla).",
                ),
                Some(widgets::chip("ls-steam", self.tr("Open in Steam", "Steam'de aç"), false).on_click(open("steam://install/993090"))),
            ),
        };
        let shaders = self.tools.ls_dir.as_ref().map(|dir| {
            if dir.join("lsfg-vk.dll").is_file() {
                self.status_row(
                    Some(true),
                    self.tr("lsfg-vk 2 shaders (beta branch)", "lsfg-vk 2 gölgelendiricileri (beta dalı)"),
                    self.tr("lsfg-vk.dll found; Arkor gives it to lsfg-vk.", "lsfg-vk.dll bulundu; Arkor onu lsfg-vk'ye verir."),
                    None,
                )
            } else {
                self.status_row(
                    None,
                    self.tr("lsfg-vk 2 shaders (beta branch)", "lsfg-vk 2 gölgelendiricileri (beta dalı)"),
                    self.tr(
                        "lsfg-vk 2 needs lsfg-vk.dll: in Steam, Lossless Scaling → Properties → Betas → choose “lsfg-vk”.",
                        "lsfg-vk 2, lsfg-vk.dll ister: Steam'de Lossless Scaling → Özellikler → Betalar → “lsfg-vk”yi seç.",
                    ),
                    Some(widgets::chip("ls-beta", self.tr("Open in Steam", "Steam'de aç"), false).on_click(open("steam://nav/games/details/993090"))),
                )
            }
        });
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(layer)
            .child(ls)
            .children(shaders)
    }

    pub(super) fn render_lsfg(&mut self, cx: &mut Context<Self>) -> Div {
        use super::style::{self, pal};
        let process_field = self.submit_field(
            "lsfg-process",
            self.tr(
                "Add a process name, e.g. FarCry6.exe, then Enter",
                "Süreç adı ekle, ör. FarCry6.exe, sonra Enter",
            ),
            cx,
            |this, text, _| {
                let name = text.trim().to_string();
                if !name.is_empty() && !this.profile.lsfg.processes.contains(&name) {
                    this.profile.lsfg.processes.push(name);
                    this.save();
                }
            },
        );
        let suggestions = self.lsfg_suggestions();
        let t = self.t();
        let status = self.lsfg_status(cx);
        let lsfg = self.profile.lsfg.clone();
        let mode_chip = |ix: usize, mode: LsfgMode, text: &'static str, cx: &mut Context<Self>| {
            widgets::chip(("lsfg-mode", ix), text, lsfg.mode == mode)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_lsfg(cx, |l| l.mode = mode)))
        };
        let mut card = widgets::section(self.tr(
            "Lossless Scaling frame generation (lsfg-vk)",
            "Lossless Scaling kare üretimi (lsfg-vk)",
        ))
        .child(widgets::label(t.lsfg_hint))
        .child(status)
        .child(
            div()
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(self.tr("For this game", "Bu oyun için")),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(mode_chip(2, LsfgMode::Off, self.tr("Off", "Kapalı"), cx))
                .child(mode_chip(
                    1,
                    LsfgMode::On,
                    self.tr("On, with the settings below", "Açık, aşağıdaki ayarlarla"),
                    cx,
                )),
        );
        match lsfg.mode {
            LsfgMode::Off => {
                return card;
            }
            LsfgMode::On => {}
        }

        // What happens in game, in plain numbers.
        let m = lsfg.multiplier;
        let target = if lsfg.processes.is_empty() {
            self.tr(
                "every program the launch starts",
                "başlatmanın açtığı her program",
            )
            .to_string()
        } else {
            lsfg.processes.join(", ")
        };
        card = card.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .p_3()
                .rounded(px(12.))
                .bg(pal::accent().opacity(0.1))
                .border_1()
                .border_color(pal::accent().opacity(0.3))
                .child(style::icon("sparkles", 18., pal::accent_light()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(format!(
                            "{m}x {} — 30 → {} · 60 → {} FPS",
                            self.tr("frame generation", "kare üretimi"),
                            30 * m,
                            60 * m
                        )))
                        .child(div().text_sm().text_color(pal::muted()).child(format!(
                            "{} {}% · {} · {}: {target}",
                            self.tr("Flow scale", "Akış ölçeği"),
                            lsfg.flow_scale,
                            if lsfg.performance_mode {
                                self.tr("performance mode", "performans modu")
                            } else {
                                self.tr("quality mode", "kalite modu")
                            },
                            self.tr("applies to", "uygulandığı yer"),
                        ))),
                ),
        );

        // Multiplier: one click per value.
        let mut mults = div().flex().gap_2();
        for v in LSFG_MULTIPLIER_RANGE.0..=LSFG_MULTIPLIER_RANGE.1 {
            mults = mults.child(
                widgets::chip(
                    ("lsfg-mult", v as usize),
                    format!("{v}x"),
                    lsfg.multiplier == v,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.edit_lsfg(cx, |l| l.multiplier = v)),
                ),
            );
        }
        let mut flows = div().flex().flex_wrap().gap_2();
        for v in [50u32, 65, 75, 85, 100] {
            flows = flows.child(
                widgets::chip(
                    ("lsfg-flow", v as usize),
                    format!("{v}%"),
                    lsfg.flow_scale == v,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.edit_lsfg(cx, |l| l.flow_scale = v)),
                ),
            );
        }
        card = card
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(t.lsfg_multiplier),
                    )
                    .child(mults),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(t.lsfg_flow_scale),
                    )
                    .child(flows),
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
            );
        let lsfg = &lsfg;
        {
            // Which process gets frame generation (the Ubisoft Connect / EA app case).
            let mut procs = div().flex().flex_wrap().gap_2();
            for (ix, p) in lsfg.processes.iter().enumerate() {
                let name = p.clone();
                procs = procs.child(
                    widgets::chip(("lsfg-proc", ix), format!("{p}  ✕"), true).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.profile.lsfg.processes.retain(|x| x != &name);
                            this.save();
                            cx.notify();
                        }),
                    ),
                );
            }
            for (ix, s) in suggestions
                .iter()
                .enumerate()
                .filter(|(_, s)| !lsfg.processes.contains(s))
            {
                let name = s.clone();
                procs = procs.child(
                    widgets::chip(("lsfg-suggest", ix), format!("+ {s}"), false).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.profile.lsfg.processes.push(name.clone());
                            this.save();
                            cx.notify();
                        }),
                    ),
                );
            }
            card = card
                .child(widgets::label(self.tr("Game process", "Oyun süreci")))
                .child(widgets::label(if lsfg.processes.is_empty() {
                    self.tr(
                        "Not set: frame generation applies to every program the launch starts, including launchers like Ubisoft Connect. Pick the game's .exe so only the game gets it.",
                        "Ayarlı değil: kare üretimi başlatmanın açtığı her programa uygulanır, Ubisoft Connect gibi başlatıcılar dahil. Sadece oyuna uygulansın diye oyunun .exe'sini seç.",
                    )
                } else {
                    self.tr("Frame generation only in these processes.", "Kare üretimi sadece bu süreçlerde.")
                }))
                .child(procs)
                .child(process_field);
            let pacing = lsfg.pacing.clone();
            let pacing_chip = |ix: usize,
                               value: Option<&'static str>,
                               text: &'static str,
                               cx: &mut Context<Self>| {
                widgets::chip(("lsfg-pacing", ix), text, pacing.as_deref() == value).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.edit_lsfg(cx, |l| l.pacing = value.map(String::from))
                    }),
                )
            };
            card = card
                .child(widgets::label(self.tr("Frame pacing", "Kare zamanlaması")))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(pacing_chip(
                            0,
                            None,
                            self.tr("Default (vsync)", "Varsayılan (vsync)"),
                            cx,
                        ))
                        .child(pacing_chip(1, Some("none"), self.tr("None", "Yok"), cx)),
                )
                .child(
                    widgets::switch_row(
                        "lsfg-present",
                        self.tr("Override present mode", "Present modunu yönet"),
                        self.tr(
                            "Recommended; let lsfg-vk pick the swapchain mode",
                            "Önerilir; swapchain modunu lsfg-vk seçsin",
                        ),
                        lsfg.override_present_mode,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.edit_lsfg(cx, |l| l.override_present_mode = !l.override_present_mode)
                    })),
                )
                .child(
                    widgets::switch_row(
                        "lsfg-fp16",
                        self.tr("Half precision (FP16)", "Yarım hassasiyet (FP16)"),
                        self.tr(
                            "Faster; turn off if you see artifacts",
                            "Daha hızlı; bozulma görürsen kapat",
                        ),
                        lsfg.fp16,
                    )
                    .on_click(
                        cx.listener(|this, _, _, cx| this.edit_lsfg(cx, |l| l.fp16 = !l.fp16)),
                    ),
                );
        }
        if self.tools.lsfg_info.is_none() {
            card = card.child(warning(t.lsfg_missing_layer));
        } else if self.tools.ls_dir.is_none() {
            card = card.child(warning(t.lsfg_missing_dll));
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
                    widgets::switch_row(
                        "optipatcher",
                        "OptiPatcher",
                        self.tr(
                            "ASI plugin that exposes DLSS / DLSS-FG inputs without spoofing, in supported games",
                            "Desteklenen oyunlarda sahtecilik yapmadan DLSS / DLSS-FG girdilerini açan ASI eklentisi",
                        ),
                        tools.optipatcher,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_optipatcher(cx))),
                )
                .child(self.render_fsr4(cx))
                .child(self.render_opti_ini(cx))
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
            card = card.child(self.opti_channel_row(t.opti_download, cx));
        } else {
            card = card.child(self.opti_channel_row(
                self.tr("Download another build", "Başka bir sürüm indir"),
                cx,
            ));
            let mut folders = div().flex().flex_col().items_start().gap_1().max_w_full();
            for (ix, folder) in tools.folders.iter().enumerate() {
                let f = folder.clone();
                folders = folders.child(
                    widgets::chip(
                        ("opti-folder", ix),
                        short_dir(folder),
                        tools.target.as_ref() == Some(folder),
                    )
                    .max_w_full()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tools.target = Some(f.clone());
                        cx.notify();
                    })),
                );
            }
            if let Some(chosen) = tools.target.as_ref().filter(|t| !tools.folders.contains(t)) {
                folders = folders.child(
                    widgets::chip("opti-folder-custom", short_dir(chosen), true).max_w_full(),
                );
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

impl Library {
    fn set_ini(
        &mut self,
        section: &'static str,
        key: &'static str,
        value: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some((path, text)) = self.tools.ini.clone() else {
            return;
        };
        let new = optiscaler::ini_set(&text, section, key, value);
        let result =
            std::fs::write(&path, &new).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()));
        if result.is_ok() {
            self.tools.ini = Some((path, new));
        }
        self.report(result);
        cx.notify();
    }

    fn render_opti_ini(&self, cx: &mut Context<Self>) -> Div {
        let Some((_, text)) = &self.tools.ini else {
            return div();
        };
        let mut rows = div().flex().flex_col().gap_2().pt_2().child(widgets::label(self.tr(
            "OptiScaler.ini (auto = OptiScaler decides; the in-game menu can change these too):",
            "OptiScaler.ini (auto = OptiScaler karar verir; oyun içi menü de bunları değiştirebilir):",
        )));
        for (row, (section, key, values)) in optiscaler::INI_SETTINGS.iter().enumerate() {
            let current = optiscaler::ini_get(text, section, key).unwrap_or_else(|| "auto".into());
            let mut chips = div().flex().flex_wrap().gap_1();
            for (ix, v) in values.iter().enumerate() {
                let (section, key, v) = (*section, *key, *v);
                chips = chips.child(
                    widgets::chip(
                        ("opti-ini", row * 16 + ix),
                        v,
                        current.eq_ignore_ascii_case(v),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_ini(section, key, v, cx))),
                );
            }
            rows = rows.child(
                div()
                    .flex()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .w(gpui::px(200.))
                            .pt_1()
                            .text_sm()
                            .child(key.to_string()),
                    )
                    .child(chips.flex_1()),
            );
        }
        rows
    }

    pub(super) fn de_download(&mut self, cx: &mut Context<Self>) {
        self.tools.busy = Some(self.tr("Downloading DLSS Enabler…", "DLSS Enabler indiriliyor…"));
        self.tools.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let _job = arkor_launch::progress::job("dlss-enabler", "DLSS Enabler");
                    let build = arkor_launch::dlss_enabler::builds()?
                        .into_iter()
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("no DLSS Enabler build found"))?;
                    let dir = arkor_launch::dlss_enabler::download(&build)?;
                    anyhow::Ok((build.version, dir))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools.busy = None;
                match result {
                    Ok(done) => this.tools.de_downloaded = Some(done),
                    Err(e) => this.tools.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn de_install(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(target), Some((version, source))) = (
            self.selected_id(),
            self.tools.target.clone(),
            self.tools.de_downloaded.clone(),
        ) else {
            return;
        };
        match arkor_launch::dlss_enabler::install(&id, &version, &source, &target) {
            Ok(m) => self.tools.de_manifest = Some(m),
            Err(e) => self.tools.error = Some(format!("{e:#}")),
        }
        cx.notify();
    }

    fn de_remove(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        match arkor_launch::dlss_enabler::uninstall(&id) {
            Ok(()) => self.tools.de_manifest = None,
            Err(e) => self.tools.error = Some(format!("{e:#}")),
        }
        cx.notify();
    }

    pub(super) fn render_dlss_enabler(&mut self, cx: &mut Context<Self>) -> Div {
        let mut card = widgets::section("DLSS Enabler");
        let tools = &self.tools;
        if let Some(m) = &tools.de_manifest {
            return card
                .child(div().text_sm().child(format!(
                    "{} · {} · {}",
                    self.t().opti_installed,
                    m.version,
                    m.dll
                )))
                .child(widgets::label(m.target_dir.display().to_string()))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            widgets::chip("de-remove", self.t().opti_remove, false)
                                .on_click(cx.listener(|this, _, _, cx| this.de_remove(cx))),
                        )
                        .child(widgets::label(self.t().opti_remove_note)),
                );
        }
        card = match &tools.de_downloaded {
            None => card.child(
                div().flex().child(
                    widgets::chip(
                        "de-download",
                        self.tr("Download DLSS Enabler", "DLSS Enabler'ı indir"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.de_download(cx))),
                ),
            ),
            Some((version, _)) => match &tools.target {
                Some(target) => card.child(widgets::label(short_dir(target))).child(
                    div().flex().child(
                        widgets::primary_button(
                            "de-install",
                            format!("{} ({version})", self.t().opti_install),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.de_install(cx))),
                    ),
                ),
                None => card.child(widgets::label(self.t().opti_no_folder)),
            },
        };
        card
    }
}

fn warning(text: impl Into<gpui::SharedString>) -> Div {
    div().text_sm().text_color(ACCENT()).child(text.into())
}

impl Library {
    pub(super) fn rs_download(&mut self, cx: &mut Context<Self>) {
        self.tools.busy = Some(self.tr("Downloading ReShade…", "ReShade indiriliyor…"));
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let _job = arkor_launch::progress::job("reshade", "ReShade");
                    let v = arkor_launch::reshade::latest_version()?;
                    let dir = arkor_launch::reshade::download(&v)?;
                    anyhow::Ok((v, dir))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools.busy = None;
                match result {
                    Ok(done) => this.tools.rs_downloaded = Some(done),
                    Err(e) => this.tools.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_reshade(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        let mut card = widgets::section("ReShade");
        if let Some(m) = self.tools.rs_manifest.clone() {
            return card
                .child(
                    div()
                        .text_sm()
                        .child(format!("{} · {} · {}", t.opti_installed, m.version, m.dll)),
                )
                .child(widgets::label(m.target_dir.display().to_string()))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(widgets::chip("rs-remove", t.opti_remove, false).on_click(
                            cx.listener(|this, _, _, cx| {
                                if let Some(id) = this.selected_id() {
                                    match arkor_launch::reshade::uninstall(&id) {
                                        Ok(()) => this.tools.rs_manifest = None,
                                        Err(e) => this.tools.error = Some(format!("{e:#}")),
                                    }
                                }
                                cx.notify();
                            }),
                        ))
                        .child(widgets::label(t.opti_remove_note)),
                );
        }
        let Some((version, _)) = self.tools.rs_downloaded.clone() else {
            return card.child(
                div().flex().child(
                    widgets::chip(
                        "rs-download",
                        self.tr("Download ReShade + shaders", "ReShade + shader'ları indir"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.rs_download(cx))),
                ),
            );
        };
        let mut proxies = div().flex().flex_wrap().gap_2();
        for (ix, (proxy, api)) in arkor_launch::reshade::PROXIES.into_iter().enumerate() {
            proxies = proxies.child(
                widgets::chip(
                    ("rs-proxy", ix),
                    format!("{proxy}.dll · {api}"),
                    self.tools.rs_proxy == proxy,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tools.rs_proxy = proxy;
                    cx.notify();
                })),
            );
        }
        card = card
            .child(widgets::label(t.opti_proxy))
            .child(proxies)
            .child(
                div().flex().child(
                    widgets::chip(
                        "rs-32",
                        self.tr("32-bit game", "32-bit oyun"),
                        self.tools.rs_32bit,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tools.rs_32bit = !this.tools.rs_32bit;
                        cx.notify();
                    })),
                ),
            );
        match self.tools.target.clone() {
            Some(target) => card.child(widgets::label(short_dir(&target))).child(
                div().flex().child(
                    widgets::primary_button(
                        "rs-install",
                        format!("{} ({version})", t.opti_install),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let (Some(id), Some((v, src))) =
                            (this.selected_id(), this.tools.rs_downloaded.clone())
                        else {
                            return;
                        };
                        match arkor_launch::reshade::install(
                            &id,
                            &v,
                            &src,
                            &target,
                            this.tools.rs_proxy,
                            this.tools.rs_32bit,
                        ) {
                            Ok(m) => this.tools.rs_manifest = Some(m),
                            Err(e) => this.tools.error = Some(format!("{e:#}")),
                        }
                        cx.notify();
                    })),
                ),
            ),
            None => card.child(widgets::label(t.opti_no_folder)),
        }
    }

    fn edit_vkb(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut arkor_core::vkbasalt::VkBasalt),
    ) {
        f(&mut self.profile.vkbasalt);
        self.save();
        cx.notify();
    }

    pub(super) fn render_vkbasalt(&mut self, cx: &mut Context<Self>) -> Div {
        let toggle_key = self.field(
            "vkb-key",
            |this| this.profile.vkbasalt.toggle_key.clone(),
            "Home",
            cx,
            |this, text| {
                this.profile.vkbasalt.toggle_key = text.trim().to_string();
                this.save();
            },
        );
        let v = self.profile.vkbasalt.clone();
        let mut card = widgets::section("vkBasalt").child(
            widgets::switch_row(
                "vkbasalt",
                "vkBasalt",
                self.tr(
                    "Vulkan post-processing (sharpening, anti-aliasing, ReShade effects)",
                    "Vulkan son işleme (keskinleştirme, kenar yumuşatma, ReShade efektleri)",
                ),
                v.enabled,
            )
            .on_click(cx.listener(|this, _, _, cx| this.edit_vkb(cx, |v| v.enabled = !v.enabled))),
        );
        if !self.tools.vkbasalt_installed {
            let line = arkor_launch::pkg::install_line(&["vkbasalt"]).unwrap_or_default();
            card = card.child(warning(format!(
                "{} {line}",
                self.tr("vkBasalt is not installed:", "vkBasalt kurulu değil:")
            )));
        }
        if !v.enabled {
            return card;
        }
        let mut builtin = div().flex().flex_wrap().gap_2();
        for (ix, e) in arkor_core::vkbasalt::BUILTIN.into_iter().enumerate() {
            builtin = builtin.child(
                widgets::chip(
                    ("vkb-fx", ix),
                    e.to_uppercase(),
                    v.effects.iter().any(|x| x == e),
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.edit_vkb(cx, |v| v.toggle_effect(e))),
                ),
            );
        }
        let step = |id: &'static str,
                    glyph: &'static str,
                    f: fn(&mut arkor_core::vkbasalt::VkBasalt),
                    cx: &mut Context<Self>| {
            widgets::step_button(id, glyph)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_vkb(cx, f)))
        };
        card = card
            .child(widgets::label(self.tr(
                "Built-in effects (run in the order chosen)",
                "Yerleşik efektler (seçilme sırasıyla çalışır)",
            )))
            .child(builtin)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_6()
                    .child(widgets::stepper(
                        "CAS",
                        format!("{}%", v.cas_sharpness),
                        step(
                            "cas-",
                            "−",
                            |v| v.cas_sharpness = v.cas_sharpness.saturating_sub(5),
                            cx,
                        ),
                        step(
                            "cas+",
                            "+",
                            |v| v.cas_sharpness = (v.cas_sharpness + 5).min(100),
                            cx,
                        ),
                    ))
                    .child(widgets::stepper(
                        self.tr("DLS sharpness", "DLS keskinlik"),
                        format!("{}%", v.dls_sharpness),
                        step(
                            "dls-",
                            "−",
                            |v| v.dls_sharpness = v.dls_sharpness.saturating_sub(5),
                            cx,
                        ),
                        step(
                            "dls+",
                            "+",
                            |v| v.dls_sharpness = (v.dls_sharpness + 5).min(100),
                            cx,
                        ),
                    ))
                    .child(widgets::stepper(
                        self.tr("DLS denoise", "DLS gürültü giderme"),
                        format!("{}%", v.dls_denoise),
                        step(
                            "dlsd-",
                            "−",
                            |v| v.dls_denoise = v.dls_denoise.saturating_sub(5),
                            cx,
                        ),
                        step(
                            "dlsd+",
                            "+",
                            |v| v.dls_denoise = (v.dls_denoise + 5).min(100),
                            cx,
                        ),
                    )),
            )
            .child(widgets::label(self.tr(
                "LUT file (.cube or .png) for the LUT effect",
                "LUT efekti için LUT dosyası (.cube ya da .png)",
            )))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div().flex_1().truncate().text_sm().child(
                            v.lut_file
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|| "-".into()),
                        ),
                    )
                    .child(
                        widgets::chip("vkb-lut", self.tr("Choose…", "Seç…"), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                let title = this.tr("Choose a LUT file", "Bir LUT dosyası seç");
                                this.prompt_path(false, title, cx, |this, path, cx| {
                                    this.edit_vkb(cx, |v| v.lut_file = Some(path))
                                });
                            }),
                        ),
                    ),
            )
            .child(widgets::label(self.tr("Toggle key", "Açma/kapama tuşu")))
            .child(toggle_key);
        // ReShade effects through vkBasalt (shader pack from the ReShade download).
        let fx = arkor_launch::reshade::effects();
        if fx.is_empty() {
            card = card.child(widgets::label(self.tr(
                "ReShade effects (e.g. Daltonize for colour blindness) appear here after downloading ReShade in the Mods tab.",
                "ReShade efektleri (ör. renk körlüğü için Daltonize), Modlar sekmesinde ReShade indirilince burada görünür.",
            )));
        } else {
            let shaders = arkor_launch::reshade::shaders_dir();
            let mut row = div().flex().flex_wrap().gap_1();
            for (ix, path) in fx.into_iter().enumerate() {
                let p = path.display().to_string();
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let on = v.effects.contains(&p);
                let shaders = shaders.clone();
                row = row.child(widgets::chip(("vkb-reshade", ix), name, on).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let (p, shaders) = (p.clone(), shaders.clone());
                        this.edit_vkb(cx, |v| {
                            v.toggle_effect(&p);
                            v.reshade_shaders = shaders;
                        })
                    }),
                ));
            }
            card = card
                .child(widgets::label(
                    self.tr("ReShade effects", "ReShade efektleri"),
                ))
                .child(row);
        }
        card
    }

    fn edit_layers(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut arkor_core::layers::Layers),
    ) {
        f(&mut self.profile.layers);
        self.save();
        cx.notify();
    }

    /// Installs (or removes) one of `layers::TOOLS` in the background.
    pub(super) fn layer_tool_action(&mut self, ix: usize, remove: bool, cx: &mut Context<Self>) {
        self.tools.busy = Some(if remove { "…" } else { "Downloading…" });
        self.tools.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let tool = &arkor_launch::layers::TOOLS[ix];
            let result = bg
                .spawn(async move {
                    let _job = (!remove).then(|| arkor_launch::progress::job(tool.id, tool.name));
                    if remove {
                        arkor_launch::layers::uninstall(tool.id).map(|_| String::new())
                    } else {
                        arkor_launch::layers::install(tool)
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools.busy = None;
                match result {
                    Ok(version) => {
                        if !version.is_empty() {
                            this.status = Some(format!("{} {version} ✓", tool.name).into());
                        }
                    }
                    Err(e) => this.tools.error = Some(format!("{e:#}")),
                }
                this.tools.layer_tools = std::array::from_fn(|i| {
                    arkor_launch::layers::arkor_installed(arkor_launch::layers::TOOLS[i].id)
                });
                this.tools.system_layers = arkor_launch::layers::system_layers();
                cx.notify();
            });
        })
        .detach();
    }

    /// True when the layer that `key` enables is present (Arkor's copy or a system one).
    pub(super) fn layer_present(&self, key: &str) -> bool {
        self.tools.system_layers.iter().any(|l| l.enable.0 == key)
    }

    fn layer_install_row(&self, ix: usize, cx: &mut Context<Self>) -> Div {
        let ours = self.tools.layer_tools[ix];
        let busy = self.tools.busy.is_some();
        let (label, remove) = if ours {
            (self.tr("Remove", "Kaldır"), true)
        } else {
            (
                self.tr("Install (user folder)", "Kur (kullanıcı klasörü)"),
                false,
            )
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(widgets::label(if ours {
                self.tr("Installed by Arkor", "Arkor ile kuruldu")
            } else {
                self.tr("Not installed", "Kurulu değil")
            }))
            .child(
                widgets::chip(
                    ("layer-install", ix),
                    if busy {
                        "…".into()
                    } else {
                        label.to_string()
                    },
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.tools.busy.is_none() {
                        this.layer_tool_action(ix, remove, cx);
                    }
                })),
            )
    }

    pub(super) fn render_vknemu(&mut self, cx: &mut Context<Self>) -> Div {
        #[allow(unused_imports)]
        use arkor_launch::layers::TOOLS;
        let l = self.profile.layers.clone();
        #[allow(unused_variables)]
        let step = |id: &'static str,
                    glyph: &'static str,
                    f: fn(&mut arkor_core::layers::Layers),
                    cx: &mut Context<Self>| {
            widgets::step_button(id, glyph)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_layers(cx, f)))
        };
        let mut card = widgets::section("vkNemu");
        if let Some(err) = &self.tools.error {
            card = card.child(warning(err.clone()));
        }
        // vkNemu
        card = card.child(
            widgets::switch_row(
                "layer-vknemu",
                "vkNemu",
                self.tr(TOOLS[1].description_en, TOOLS[1].description_tr),
                l.vknemu.enabled,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.edit_layers(cx, |l| l.vknemu.enabled = !l.vknemu.enabled)
            })),
        );
        if !self.layer_present("VKNEMU") || self.tools.layer_tools[1] {
            card = card.child(self.layer_install_row(1, cx));
        }
        if l.vknemu.enabled {
            card = card.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_6()
                    .child(widgets::stepper(
                        self.tr("Idle FPS", "Boşta FPS"),
                        l.vknemu.idle_fps.to_string(),
                        step(
                            "nemu-fps-",
                            "−",
                            |l| l.vknemu.idle_fps = l.vknemu.idle_fps.saturating_sub(5).max(5),
                            cx,
                        ),
                        step(
                            "nemu-fps+",
                            "+",
                            |l| l.vknemu.idle_fps = (l.vknemu.idle_fps + 5).min(240),
                            cx,
                        ),
                    ))
                    .child(widgets::stepper(
                        self.tr("After (s)", "Sonra (sn)"),
                        l.vknemu.timeout_secs.to_string(),
                        step(
                            "nemu-t-",
                            "−",
                            |l| {
                                l.vknemu.timeout_secs =
                                    l.vknemu.timeout_secs.saturating_sub(10).max(10)
                            },
                            cx,
                        ),
                        step(
                            "nemu-t+",
                            "+",
                            |l| l.vknemu.timeout_secs = (l.vknemu.timeout_secs + 10).min(1800),
                            cx,
                        ),
                    ))
                    .child(
                        widgets::chip(
                            "nemu-unfocused",
                            self.tr("Also when unfocused", "Odak dışındayken de"),
                            l.vknemu.unfocused,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.edit_layers(cx, |l| l.vknemu.unfocused = !l.vknemu.unfocused)
                        })),
                    ),
            );
        }
        card
    }

    pub(super) fn render_mako(&mut self, cx: &mut Context<Self>) -> Div {
        #[allow(unused_imports)]
        use arkor_launch::layers::TOOLS;
        let l = self.profile.layers.clone();
        #[allow(unused_variables)]
        let step = |id: &'static str,
                    glyph: &'static str,
                    f: fn(&mut arkor_core::layers::Layers),
                    cx: &mut Context<Self>| {
            widgets::step_button(id, glyph)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_layers(cx, f)))
        };
        let mut card = widgets::section("MAKO");
        if let Some(err) = &self.tools.error {
            card = card.child(warning(err.clone()));
        }
        // MAKO
        card = card.child(
            widgets::switch_row(
                "layer-mako",
                self.tr("MAKO frame generation", "MAKO kare üretimi"),
                self.tr(TOOLS[2].description_en, TOOLS[2].description_tr),
                l.mako.enabled,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.edit_layers(cx, |l| l.mako.enabled = !l.mako.enabled)
            })),
        );
        card = card.child(
            widgets::switch_row(
                "layer-mako-scale",
                self.tr("MAKO spatial scaling", "MAKO uzamsal ölçekleme"),
                self.tr(
                    "Open upscaler, no Lossless.dll needed",
                    "Açık kaynak ölçekleyici, Lossless.dll gerekmez",
                ),
                l.mako.scaling,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.edit_layers(cx, |l| l.mako.scaling = !l.mako.scaling)
            })),
        );
        if !self.layer_present("ENABLE_MAKO") || self.tools.layer_tools[2] {
            card = card.child(self.layer_install_row(2, cx));
        }
        if l.mako.enabled && !self.tools.lossless_dll {
            card = card.child(warning(self.tr(
                "Lossless.dll not found: install Lossless Scaling from Steam.",
                "Lossless.dll bulunamadı: Lossless Scaling'i Steam'den kur.",
            )));
        }
        if l.mako.enabled || l.mako.scaling {
            let mut row = div().flex().flex_wrap().items_center().gap_6();
            if l.mako.enabled {
                row = row
                    .child(widgets::stepper(
                        self.tr("Multiplier", "Çarpan"),
                        format!("{}x", l.mako.multiplier),
                        step(
                            "mako-m-",
                            "−",
                            |l| l.mako.multiplier = l.mako.multiplier.saturating_sub(1).max(2),
                            cx,
                        ),
                        step(
                            "mako-m+",
                            "+",
                            |l| l.mako.multiplier = (l.mako.multiplier + 1).min(20),
                            cx,
                        ),
                    ))
                    .child(widgets::stepper(
                        self.tr("Flow scale", "Akış ölçeği"),
                        format!("{}%", l.mako.flow_scale),
                        step(
                            "mako-f-",
                            "−",
                            |l| l.mako.flow_scale = l.mako.flow_scale.saturating_sub(5).max(25),
                            cx,
                        ),
                        step(
                            "mako-f+",
                            "+",
                            |l| l.mako.flow_scale = (l.mako.flow_scale + 5).min(100),
                            cx,
                        ),
                    ))
                    .child(
                        widgets::chip(
                            "mako-perf",
                            self.tr("Performance mode", "Performans modu"),
                            l.mako.performance_mode,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.edit_layers(cx, |l| {
                                l.mako.performance_mode = !l.mako.performance_mode
                            })
                        })),
                    );
            }
            if l.mako.scaling {
                row = row.child(widgets::stepper(
                    self.tr("Scale", "Ölçek"),
                    format!("{:.2}x", l.mako.scaling_factor as f32 / 100.),
                    step(
                        "mako-s-",
                        "−",
                        |l| {
                            l.mako.scaling_factor =
                                l.mako.scaling_factor.saturating_sub(10).max(100)
                        },
                        cx,
                    ),
                    step(
                        "mako-s+",
                        "+",
                        |l| l.mako.scaling_factor = (l.mako.scaling_factor + 10).min(300),
                        cx,
                    ),
                ));
            }
            card = card.child(row);
            let mut headroom = div().flex().flex_wrap().gap_2();
            for (ix, pct) in arkor_core::layers::MAKO_HEADROOM.into_iter().enumerate() {
                let label = if pct == 0 {
                    self.tr("Off", "Kapalı").to_string()
                } else {
                    format!("{pct}%")
                };
                headroom = headroom.child(
                    widgets::chip(("mako-headroom", ix), label, l.mako.gpu_headroom == pct)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_layers(cx, move |l| l.mako.gpu_headroom = pct)
                        })),
                );
            }
            card = card
                .child(widgets::setting_row(
                    self.tr("Spare GPU power", "GPU'da boş pay"),
                    self.tr(
                        "Keeps part of the graphics card free so the generated frames come evenly. Try 10% if the picture stutters while the card is at 100%.",
                        "Ekran kartının bir kısmını boş tutar, böylece üretilen kareler düzenli gelir. Kart %100'deyken görüntü takılıyorsa %10'u dene.",
                    ),
                    headroom,
                ))
                .child(
                    widgets::switch_row(
                        "mako-hdr",
                        self.tr("MAKO's HDR brightness", "MAKO'nun HDR parlaklığı"),
                        self.tr(
                            "Off keeps the game's own HDR brightness. Turn on only if HDR looks too dark with MAKO.",
                            "Kapalıyken oyunun kendi HDR parlaklığı korunur. Yalnızca MAKO ile HDR fazla karanlık görünürse aç.",
                        ),
                        l.mako.hdr_exposure,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.edit_layers(cx, |l| l.mako.hdr_exposure = !l.mako.hdr_exposure)
                    })),
                );
        }
        card
    }

    pub(super) fn render_vksumi(&mut self, cx: &mut Context<Self>) -> Div {
        #[allow(unused_imports)]
        use arkor_launch::layers::TOOLS;
        let l = self.profile.layers.clone();
        #[allow(unused_variables)]
        let step = |id: &'static str,
                    glyph: &'static str,
                    f: fn(&mut arkor_core::layers::Layers),
                    cx: &mut Context<Self>| {
            widgets::step_button(id, glyph)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_layers(cx, f)))
        };
        let mut card = widgets::section("vkSumi");
        if let Some(err) = &self.tools.error {
            card = card.child(warning(err.clone()));
        }
        // vkSumi
        card = card.child(
            widgets::switch_row(
                "layer-vksumi",
                "vkSumi",
                self.tr(TOOLS[0].description_en, TOOLS[0].description_tr),
                l.vksumi,
            )
            .on_click(cx.listener(|this, _, _, cx| this.edit_layers(cx, |l| l.vksumi = !l.vksumi))),
        );
        if !self.layer_present("ENABLE_VKSUMI") || self.tools.layer_tools[0] {
            card = card.child(self.layer_install_row(0, cx));
        }
        card
    }

    pub(super) fn render_afmf(&mut self, cx: &mut Context<Self>) -> Div {
        #[allow(unused_imports)]
        use arkor_launch::layers::TOOLS;
        let l = self.profile.layers.clone();
        #[allow(unused_variables)]
        let step = |id: &'static str,
                    glyph: &'static str,
                    f: fn(&mut arkor_core::layers::Layers),
                    cx: &mut Context<Self>| {
            widgets::step_button(id, glyph)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_layers(cx, f)))
        };
        let mut card = widgets::section("AFMF");
        if let Some(err) = &self.tools.error {
            card = card.child(warning(err.clone()));
        }
        // AFMF
        card = card.child(
            widgets::switch_row(
                "layer-afmf",
                "AFMF",
                self.tr(TOOLS[3].description_en, TOOLS[3].description_tr),
                l.afmf.enabled,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.edit_layers(cx, |l| l.afmf.enabled = !l.afmf.enabled)
            })),
        );
        if !self.layer_present("AFMF_ENABLE") || self.tools.layer_tools[3] {
            card = card.child(self.layer_install_row(3, cx));
        }
        if l.afmf.enabled {
            use arkor_core::layers::AfmfMode;
            let mut modes = div().flex().flex_wrap().items_center().gap_2();
            for (ix, mode) in AfmfMode::ALL.into_iter().enumerate() {
                let label = match mode {
                    AfmfMode::Auto => self.tr("Auto", "Otomatik"),
                    AfmfMode::Quality => self.tr("Quality", "Kalite"),
                    AfmfMode::Performance => self.tr("Performance", "Performans"),
                };
                modes = modes.child(
                    widgets::chip(("afmf-mode", ix), label, l.afmf.mode == mode).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.edit_layers(cx, |l| l.afmf.mode = mode)
                        }),
                    ),
                );
            }
            let min = l.afmf.min_fps.unwrap_or(30);
            card = card.child(modes).child(widgets::stepper(
                self.tr("No generation below (FPS)", "Şunun altında üretme (FPS)"),
                min.to_string(),
                step(
                    "afmf-min-",
                    "−",
                    |l| l.afmf.min_fps = Some(l.afmf.min_fps.unwrap_or(30).saturating_sub(5)),
                    cx,
                ),
                step(
                    "afmf-min+",
                    "+",
                    |l| l.afmf.min_fps = Some((l.afmf.min_fps.unwrap_or(30) + 5).min(120)),
                    cx,
                ),
            ));
        }
        if l.frame_generators() > 1
            || (l.frame_generators() > 0
                && self.profile.lsfg.mode == arkor_core::profile::LsfgMode::On)
        {
            card = card.child(warning(self.tr(
                "More than one frame generator is on for this game; keep only one.",
                "Bu oyunda birden fazla kare üretici açık; yalnızca birini bırak.",
            )));
        }
        card
    }

    pub(super) fn render_other_layers(&mut self, cx: &mut Context<Self>) -> Div {
        #[allow(unused_imports)]
        use arkor_launch::layers::TOOLS;
        let l = self.profile.layers.clone();
        #[allow(unused_variables)]
        let step = |id: &'static str,
                    glyph: &'static str,
                    f: fn(&mut arkor_core::layers::Layers),
                    cx: &mut Context<Self>| {
            widgets::step_button(id, glyph)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_layers(cx, f)))
        };
        let mut card = widgets::section(self.tr("Other Vulkan layers", "Diğer Vulkan katmanları"));
        if let Some(err) = &self.tools.error {
            card = card.child(warning(err.clone()));
        }
        // Anything else on the system.

        let others: Vec<_> = self
            .tools
            .system_layers
            .iter()
            .filter(|s| !HANDLED_LAYERS.contains(&s.enable.0.as_str()))
            .cloned()
            .collect();
        if !others.is_empty() {
            let mut row = div().flex().flex_wrap().gap_1();
            for (ix, s) in others.into_iter().enumerate() {
                let on = l.other.contains(&format!("{}={}", s.enable.0, s.enable.1));
                let title = if s.description.is_empty() {
                    s.name.clone()
                } else {
                    format!("{} — {}", s.name, s.description)
                };
                row = row.child(widgets::chip(("layer-other", ix), title, on).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let (k, v) = s.enable.clone();
                        this.edit_layers(cx, |l| l.toggle_other(&k, &v))
                    }),
                ));
            }
            card = card
                .child(widgets::label(self.tr(
                    "Other layers on this system",
                    "Bu sistemdeki diğer katmanlar",
                )))
                .child(row);
        }
        card
    }
}

/// The Proton the selected game's own launcher (Heroic, Lutris…) chose, if any.
fn game_source_proton(lib: &Library) -> Option<String> {
    lib.entry()
        .and_then(|g| g.source_proton.as_ref())
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
}

/// A game folder short enough for a button: its last three parts ("…/games/Far Cry 5/bin"),
/// which tell candidate folders apart without running off the card.
pub(super) fn short_dir(path: &std::path::Path) -> String {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|p| p != "/")
        .collect();
    if parts.len() <= 3 {
        return path.display().to_string();
    }
    format!("…/{}", parts[parts.len() - 3..].join("/"))
}
