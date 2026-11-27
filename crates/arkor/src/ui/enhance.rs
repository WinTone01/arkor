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
    pub(super) bundles: [Option<(String, PathBuf)>; 4],
    pub(super) dlssnr: Option<arkor_launch::bundles::DlssNr>,
    /// The network a DLSS-NR install starts with, as an index of the package's list.
    pub(super) dlssnr_model: usize,
    pub(super) dlssnr_route: arkor_launch::bundles::NrRoute,
    pub(super) dlssnr_32: bool,
    pub(super) optipatcher: bool,
    /// Implicit layers on the system that a variable switches on.
    pub(super) system_layers: Vec<arkor_launch::layers::SystemLayer>,
    /// The game's OptiScaler.ini text (when installed), cached for the settings chips.
    pub(super) ini: Option<(PathBuf, String)>,
    pub(super) de_downloaded: Option<(String, PathBuf)>,
    pub(super) anr_manifest: Option<Manifest>,
    pub(super) anr_downloaded: Option<(String, PathBuf)>,
    /// The proxy name picked for ANR FG (`None`: what the game folder suggests).
    pub(super) anr_proxy: Option<&'static str>,
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
            dlssnr_model: 0,
            dlssnr_route: arkor_launch::bundles::NrRoute::Optiscaler,
            dlssnr_32: false,
            optipatcher: false,
            layer_tools: std::array::from_fn(|i| {
                arkor_launch::layers::arkor_installed(arkor_launch::layers::TOOLS[i].id)
            }),
            system_layers: arkor_launch::layers::system_layers(),
            ini: None,
            de_downloaded: None,
            anr_manifest: None,
            anr_downloaded: None,
            anr_proxy: None,
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
        self.anr_manifest = arkor_launch::anr::manifest(&game.id);
        self.anr_downloaded = arkor_launch::anr::downloaded().into_iter().next();
        self.anr_proxy = None;
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

    /// The switches of one category that this computer's cards can use.
    pub(super) fn tweaks_of(
        &self,
        c: arkor_core::tweaks::Category,
    ) -> Vec<&'static arkor_core::tweaks::Tweak> {
        let gpus = self.tools.gpus;
        arkor_core::tweaks::TWEAKS
            .iter()
            .filter(|t| t.category == c)
            .filter(|t| match arkor_core::tweaks::gpu_need(t.id) {
                Some(arkor_core::tweaks::GpuNeed::Amd) => gpus.amd,
                Some(arkor_core::tweaks::GpuNeed::Nvidia) => gpus.nvidia,
                None => true,
            })
            .collect()
    }

    /// Whether a category's switches are offered for `g` (Proton's switches only for
    /// Windows games; graphics and Discord ones for every game).
    pub(super) fn tweaks_apply(
        &self,
        c: arkor_core::tweaks::Category,
        g: &arkor_core::library::LibraryGame,
    ) -> bool {
        use arkor_core::tweaks::Category::*;
        let wine_only = matches!(c, Upscaling | Fixes | Input | Display);
        !self.tweaks_of(c).is_empty()
            && (!wine_only
                || g.needs_prefix()
                || matches!(g.kind, arkor_core::library::GameKind::Steam { .. }))
    }

    /// How many of a category's switches are on for this game.
    pub(super) fn tweaks_on(&self, c: arkor_core::tweaks::Category) -> usize {
        self.tweaks_of(c)
            .iter()
            .filter(|t| self.profile.tweaks.contains(t.id))
            .count()
    }

    /// One category's switches, each with what it does.
    pub(super) fn render_tweak_category(
        &mut self,
        c: arkor_core::tweaks::Category,
        cx: &mut Context<Self>,
    ) -> Div {
        let selected = self.profile.tweaks.clone();
        let mut card = widgets::section(format!(
            "{} · {} {}",
            self.tr("Switches", "Anahtarlar"),
            self.tweaks_on(c),
            self.tr("on", "açık")
        ));
        for (ix, tw) in self.tweaks_of(c).into_iter().enumerate() {
            let tid = tw.id;
            card = card.child(
                self.tweak_row(("tweak", ix), tw, selected.contains(tid))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_tweak(tid, cx))),
            );
        }
        card
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
        use arkor_launch::bundles::{self, Bundle};
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
        card = card
            .child(widgets::divider())
            .child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child("DLSS-NR"))
            .child(widgets::label(self.tr(
                "Runs NVIDIA's DLSS 5 picture model on AMD cards, so the game looks sharper and more detailed. You need a copy of NVIDIA's nvngx_dlssnr.dll (310.9.16 or 310.8.0) the first time.",
                "NVIDIA'nın DLSS 5 görüntü modelini AMD kartlarda çalıştırır; oyun daha keskin ve ayrıntılı görünür. İlk seferde NVIDIA'nın nvngx_dlssnr.dll dosyasının (310.9.16 ya da 310.8.0) bir kopyası gerekir.",
            )));
        if let Some(d) = &tools.dlssnr {
            let route = self.nr_route_name(d.route).0;
            return card
                .child(div().text_sm().child(format!(
                    "{} · {} {} · {route}",
                    d.bundle.name(),
                    self.tr("installed", "kurulu"),
                    d.version
                )))
                .child(widgets::label(self.tr(
                    "In the game: End opens NR Control (OptiScaler way), Home opens ReShade (ReShade ways). Ctrl+F11 switches the network.",
                    "Oyunda: End NR Control'ü açar (OptiScaler yolu), Home ReShade'i açar (ReShade yolları). Ctrl+F11 ağı değiştirir.",
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

        // 1. 32 or 64 bit.
        let bits32 = tools.dlssnr_32;
        card = card
            .child(widgets::subheading(self.tr("1. The game", "1. Oyun")))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children([false, true].map(|b32| {
                        widgets::chip(
                            ("nr-bits", usize::from(b32)),
                            if b32 {
                                self.tr("32-bit (older games)", "32-bit (eski oyunlar)")
                            } else {
                                self.tr("64-bit (most games)", "64-bit (çoğu oyun)")
                            },
                            bits32 == b32,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.tools.dlssnr_32 = b32;
                            if b32 && this.tools.dlssnr_route == bundles::NrRoute::Optiscaler {
                                this.tools.dlssnr_route = bundles::NrRoute::ReshadeFeederVulkan;
                            }
                            cx.notify();
                        }))
                    })),
            );

        // 2. The package.
        let candidates: &[usize] = if bits32 { &[3] } else { &[1, 2] };
        let mut dl = div().flex().flex_wrap().gap_2();
        for &ix in candidates {
            let b = Bundle::ALL[ix];
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
        card = card
            .child(widgets::subheading(self.tr("2. The package", "2. Paket")))
            .child(widgets::label(if bits32 {
                self.tr(
                    "The 32-bit package works with Radeon RX 9000 cards.",
                    "32-bit paket Radeon RX 9000 kartlarla çalışır.",
                )
            } else {
                self.tr(
                    "RDNA4 is for Radeon RX 9000 cards, RDNA3 for RX 7000.",
                    "RDNA4 Radeon RX 9000 kartlar, RDNA3 RX 7000 kartlar içindir.",
                )
            }))
            .child(dl);
        let Some((bundle_ix, (version, dir))) = candidates
            .iter()
            .find_map(|&ix| tools.bundles[ix].clone().map(|b| (ix, b)))
        else {
            return card;
        };
        let bundle = Bundle::ALL[bundle_ix];

        // 3. The way it hooks in.
        let mut routes = div().flex().flex_wrap().gap_2();
        for (ix, route) in bundles::NrRoute::ALL.into_iter().enumerate() {
            if bits32 && route == bundles::NrRoute::Optiscaler {
                continue;
            }
            let (name, hint) = self.nr_route_name(route);
            routes = routes.child(
                widgets::choice_card(
                    ("nr-route", ix),
                    if route == bundles::NrRoute::Optiscaler {
                        "scan-eye"
                    } else {
                        "wand-sparkles"
                    },
                    name,
                    hint,
                    tools.dlssnr_route == route,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tools.dlssnr_route = route;
                    cx.notify();
                })),
            );
        }
        card = card
            .child(widgets::subheading(
                self.tr("3. How it hooks in", "3. Nasıl bağlanacak"),
            ))
            .child(routes);

        // 4. The network it starts with.
        let models = bundle.models();
        let model_ix = tools.dlssnr_model.min(models.len() - 1);
        let mut row = div().flex().flex_wrap().gap_1();
        for (ix, m) in models.iter().enumerate() {
            row = row.child(
                widgets::chip(("nr-model", ix), *m, model_ix == ix).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.tools.dlssnr_model = ix;
                        cx.notify();
                    },
                )),
            );
        }
        card = card
            .child(widgets::subheading(self.tr("4. Network", "4. Ağ")))
            .child(widgets::label(self.tr(
                "The model the game starts with. The first one is the default; the OPTIMIZED ones are faster. You can switch in the game too.",
                "Oyunun başlayacağı model. İlki varsayılandır; OPTIMIZED olanlar daha hızlıdır. Oyunda da değiştirebilirsin.",
            )))
            .child(row);

        let has_models = bundles::dlssnr_has_models(&dir);
        let route = tools.dlssnr_route;
        let model = models[model_ix];
        let install = move |this: &mut Self, file: Option<PathBuf>, cx: &mut Context<Self>| {
            let Some(id) = this.selected_game().map(|g| g.id.clone()) else {
                return;
            };
            let Some(game_dir) = this.bundle_game_dir() else {
                return;
            };
            let (v, dir) = (version.clone(), dir.clone());
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
                                &id,
                                bundle,
                                &v,
                                &dir,
                                &game_dir,
                                file.as_deref(),
                                model,
                                route,
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
        };
        let install2 = install.clone();
        let mut buttons = div().flex().flex_wrap().gap_2();
        if has_models {
            buttons = buttons.child(
                widgets::primary_button("nr-install-now", self.tr("Install", "Kur"))
                    .on_click(cx.listener(move |this, _, _, cx| install(this, None, cx))),
            );
        }
        buttons = buttons.child(
            widgets::chip(
                "nr-install",
                if has_models {
                    self.tr(
                        "Install with another nvngx_dlssnr file…",
                        "Başka bir nvngx_dlssnr dosyasıyla kur…",
                    )
                } else {
                    self.tr(
                        "Choose nvngx_dlssnr file and install…",
                        "nvngx_dlssnr dosyasını seç ve kur…",
                    )
                },
                false,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let title = this.tr(
                    "Choose NVIDIA's nvngx_dlssnr.dll or its zip",
                    "NVIDIA'nın nvngx_dlssnr.dll dosyasını ya da zip'ini seç",
                );
                let install = install2.clone();
                this.prompt_path(false, title, cx, move |this, file, cx| {
                    install(this, Some(file), cx)
                });
            })),
        );
        card.child(buttons)
    }

    /// A DLSS-NR route's name and when to pick it.
    fn nr_route_name(&self, r: arkor_launch::bundles::NrRoute) -> (&'static str, &'static str) {
        use arkor_launch::bundles::NrRoute;
        match r {
            NrRoute::Optiscaler => (
                self.tr("Through OptiScaler", "OptiScaler ile"),
                self.tr(
                    "The game has a DLSS, FSR or XeSS option. Best choice when it does.",
                    "Oyunun DLSS, FSR ya da XeSS seçeneği var. Varsa en iyi seçim budur.",
                ),
            ),
            NrRoute::ReshadeFeeder => (
                self.tr("ReShade · DirectX 10-12", "ReShade · DirectX 10-12"),
                self.tr(
                    "Newer games without any upscaler option.",
                    "Hiç ölçekleme seçeneği olmayan yeni oyunlar.",
                ),
            ),
            NrRoute::ReshadeFeederVulkan => (
                self.tr(
                    "ReShade · Vulkan / DirectX 9",
                    "ReShade · Vulkan / DirectX 9",
                ),
                self.tr(
                    "Vulkan games and DirectX 9 games.",
                    "Vulkan oyunları ve DirectX 9 oyunları.",
                ),
            ),
            NrRoute::ReshadeFeederDx9 => (
                self.tr("ReShade · old DirectX 9", "ReShade · eski DirectX 9"),
                self.tr(
                    "Try this if an old DirectX 9 game looks wrong with the one before.",
                    "Eski bir DirectX 9 oyunu bir öncekiyle yanlış görünürse bunu dene.",
                ),
            ),
        }
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

impl Library {
    /// Installs, updates or removes ANR FG off the UI thread.
    pub(super) fn anr_job(&mut self, what: &'static str, cx: &mut Context<Self>) {
        use arkor_launch::anr;
        let Some(id) = self.selected_id() else { return };
        let target = self.tools.target.clone();
        let downloaded = self.tools.anr_downloaded.clone();
        let proxy = match (self.tools.anr_proxy, &target) {
            (Some(p), _) => p,
            (None, Some(dir)) => anr::default_proxy(dir),
            (None, None) => "dxgi",
        };
        self.tools.busy = Some("…");
        self.tools.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn({
                    let id = id.clone();
                    async move {
                        match what {
                            "remove" => anr::uninstall(&id).map(|()| None),
                            "update" => anr::update(&id).map(Some),
                            _ => {
                                let target =
                                    anyhow::Context::context(target, "no game folder chosen")?;
                                let (v, src) = anyhow::Context::context(
                                    downloaded,
                                    "ANR FG is not downloaded",
                                )?;
                                anr::install(&id, &v, &src, &target, proxy).map(Some)
                            }
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools.busy = None;
                match result {
                    Ok(m) => {
                        this.tools.anr_manifest = m;
                        this.sound(super::sounds::Sound::Done);
                    }
                    Err(e) => {
                        this.tools.error = Some(format!("{e:#}"));
                        this.sound(super::sounds::Sound::Error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_anr(&mut self, cx: &mut Context<Self>) -> Div {
        use arkor_launch::anr;
        let tools = &self.tools;
        let mut card = widgets::section("ANR FG").child(widgets::label(self.tr(
            "Apex Neural Rendering: AI frame generation that takes the place of DLSS frame generation, on NVIDIA, AMD and Intel cards. It comes with its own OptiScaler, so Arkor's OptiScaler, DLSS Enabler and DLSS-NR cannot be in the same game. In beta.",
            "Apex Neural Rendering: DLSS kare üretiminin yerine geçen yapay zekâ kare üretimi; NVIDIA, AMD ve Intel kartlarda. Kendi OptiScaler'ıyla gelir, bu yüzden aynı oyunda Arkor'un OptiScaler'ı, DLSS Enabler ve DLSS-NR olamaz. Beta aşamasında.",
        )));
        if let Some(err) = &tools.error {
            card = card.child(warning(err.clone()));
        }
        if let Some(busy) = tools.busy {
            return card.child(widgets::label(busy));
        }
        if let Some(m) = tools.anr_manifest.clone() {
            let newer = tools
                .anr_downloaded
                .as_ref()
                .filter(|(v, _)| *v != m.version)
                .map(|(v, _)| v.clone());
            let log = anr::latest_log(&m);
            return card
                .child(div().text_sm().child(format!(
                    "{} · {} · {}",
                    self.t().opti_installed,
                    m.version,
                    m.dll
                )))
                .child(widgets::label(m.target_dir.display().to_string()))
                .child(widgets::label(self.tr(
                    "In the game, turn on frame generation (the NVIDIA / DLSS option where there are several). Insert opens OptiScaler's menu. Arkor starts the game with the setting it needs; for Steam it is written into the launch options.",
                    "Oyunda kare üretimini aç (birden çok seçenek varsa NVIDIA / DLSS olanı). Insert OptiScaler menüsünü açar. Arkor oyunu gereken ayarla başlatır; Steam için başlatma seçeneklerine yazılır.",
                )))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .children(newer.map(|v| {
                            widgets::chip(
                                "anr-update",
                                format!("{} {v}", self.tr("Update to", "Güncelle:")),
                                true,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.anr_job("update", cx)))
                        }))
                        .children(log.map(|log| {
                            widgets::chip("anr-log", self.tr("Open its log", "Kaydını aç"), false)
                                .on_click(move |_, _, _| {
                                    let _ = std::process::Command::new("xdg-open").arg(&log).spawn();
                                })
                        }))
                        .child(
                            widgets::chip("anr-remove", self.t().opti_remove, false)
                                .on_click(cx.listener(|this, _, _, cx| this.anr_job("remove", cx))),
                        ),
                );
        }
        let Some((version, _)) = tools.anr_downloaded.clone() else {
            return card;
        };
        let Some(target) = tools.target.clone() else {
            return card.child(widgets::label(self.t().opti_no_folder));
        };
        let mut folders = div().flex().flex_col().items_start().gap_1().max_w_full();
        for (ix, folder) in tools.folders.iter().enumerate() {
            let f = folder.clone();
            folders = folders.child(
                widgets::chip(("anr-folder", ix), short_dir(folder), &target == folder)
                    .max_w_full()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tools.target = Some(f.clone());
                        cx.notify();
                    })),
            );
        }
        let suggested = anr::default_proxy(&target);
        let chosen = tools.anr_proxy.unwrap_or(suggested);
        let mut proxies = div().flex().flex_wrap().gap_2();
        for (ix, p) in anr::PROXIES.into_iter().enumerate() {
            let label = if p == suggested {
                format!("{p}.dll · {}", self.tr("suggested", "önerilen"))
            } else {
                format!("{p}.dll")
            };
            proxies = proxies.child(
                widgets::chip(("anr-proxy", ix), label, chosen == p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.tools.anr_proxy = Some(p);
                        cx.notify();
                    },
                )),
            );
        }
        card.child(widgets::subheading(self.tr(
            "The folder with the game's program",
            "Oyunun programının olduğu klasör",
        )))
        .child(folders)
        .child(widgets::subheading(self.tr("Loads as", "Yüklenme adı")))
        .child(widgets::label(self.tr(
            "dxgi for DirectX games, winmm for Vulkan games. Arkor suggests the right one; change it only if the game already uses that name for another mod.",
            "DirectX oyunları için dxgi, Vulkan oyunları için winmm. Arkor doğru olanı önerir; yalnızca oyun bu adı başka bir mod için zaten kullanıyorsa değiştir.",
        )))
        .child(proxies)
        .child(
            div().flex().child(
                widgets::primary_button(
                    "anr-install",
                    format!("{} ({version})", self.t().opti_install),
                )
                .on_click(cx.listener(|this, _, _, cx| this.anr_job("install", cx))),
            ),
        )
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

    /// The key that opens ReShade's menu in the game.
    fn reshade_menu_key(
        &self,
        m: &arkor_launch::optiscaler::Manifest,
        cx: &mut Context<Self>,
    ) -> Div {
        let current = arkor_launch::reshade::menu_key(m).unwrap_or(36);
        let mut keys = div().flex().flex_wrap().gap_2();
        for (ix, (code, name)) in arkor_launch::reshade::MENU_KEYS.into_iter().enumerate() {
            let m = m.clone();
            keys = keys.child(
                widgets::chip(("rs-key", ix), name, current == code).on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Err(e) = arkor_launch::reshade::set_menu_key(&m, code) {
                            this.tools.error = Some(format!("{e:#}"));
                        }
                        cx.notify();
                    },
                )),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(widgets::subheading(self.tr("Menu key", "Menü tuşu")))
            .child(widgets::label(self.tr(
                "Press this key in the game to open ReShade's menu and pick effects. Choose another if the game already uses it.",
                "Oyunda ReShade menüsünü açıp efekt seçmek için bu tuşa bas. Oyun bu tuşu zaten kullanıyorsa başkasını seç.",
            )))
            .child(keys)
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
                .child(self.reshade_menu_key(&m, cx))
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
        if !l.vksumi {
            return card;
        }
        use arkor_core::layers::{Daltonize, SumiKnob::*};
        let groups = [
            (
                self.tr("Light", "Işık"),
                &[Brightness, Contrast, Exposure, Gamma][..],
            ),
            (
                self.tr(
                    "Dark, middle and bright parts",
                    "Karanlık, orta ve parlak bölgeler",
                ),
                &[Shadows, Midtones, Highlights][..],
            ),
            (
                self.tr("Colour", "Renk"),
                &[Saturation, Vibrance, Hue, Temperature, Tint][..],
            ),
            (
                self.tr("Red, green and blue", "Kırmızı, yeşil ve mavi"),
                &[RedGain, GreenGain, BlueGain][..],
            ),
        ];
        card = card.child(widgets::label(self.tr(
            "0 means no change. The game reads these when it starts; Shift_R+F9 (or the key below) turns the grading off and on while you play.",
            "0 değişiklik yok demektir. Oyun bunları başlarken okur; oynarken Sağ Shift+F9 (ya da aşağıdaki tuş) renk ayarını kapatıp açar.",
        )));
        for (title, knobs) in groups {
            card = card.child(widgets::subheading(title));
            for &k in knobs {
                card = card.child(self.sumi_row(k, l.sumi.get(k), cx));
            }
        }
        let mut dalt = div().flex().flex_wrap().gap_2();
        for (ix, d) in Daltonize::ALL.into_iter().enumerate() {
            let name = match d {
                Daltonize::Off => self.tr("Off", "Kapalı"),
                Daltonize::Protan => self.tr("Red-blind (protan)", "Kırmızı körlüğü (protan)"),
                Daltonize::Deutan => self.tr("Green-blind (deutan)", "Yeşil körlüğü (deutan)"),
                Daltonize::Tritan => self.tr("Blue-blind (tritan)", "Mavi körlüğü (tritan)"),
            };
            dalt = dalt.child(
                widgets::chip(("sumi-dalt", ix), name, l.sumi.daltonize == d).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.edit_layers(cx, |l| l.sumi.daltonize = d)
                    }),
                ),
            );
        }
        let key = self.field(
            "sumi-key",
            |this| this.profile.layers.sumi.toggle_key.clone(),
            "Shift_R+F9",
            cx,
            |this, text| {
                this.profile.layers.sumi.toggle_key = text.trim().to_string();
                this.save();
            },
        );
        card.child(widgets::subheading(self.tr("Options", "Seçenekler")))
            .child(
                widgets::switch_row(
                    "sumi-compare",
                    self.tr("Split compare", "Bölünmüş karşılaştırma"),
                    self.tr(
                        "Half the screen graded, half as the game draws it, to see the difference.",
                        "Ekranın yarısı ayarlı, yarısı oyunun çizdiği gibi; farkı görmek için.",
                    ),
                    l.sumi.compare,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.edit_layers(cx, |l| l.sumi.compare = !l.sumi.compare)
                })),
            )
            .child(widgets::setting_row(
                self.tr("Colour blindness", "Renk körlüğü"),
                self.tr(
                    "Shifts colours so they are easier to tell apart.",
                    "Renkleri daha kolay ayırt edilecek şekilde kaydırır.",
                ),
                dalt,
            ))
            .child(widgets::setting_row(
                self.tr("On/off key", "Aç/kapa tuşu"),
                self.tr(
                    "Works in X11 and XWayland games (most Proton games).",
                    "X11 ve XWayland oyunlarında çalışır (çoğu Proton oyunu).",
                ),
                div().w(px(180.)).child(key),
            ))
            .child(
                div().flex().child(
                    widgets::chip(
                        "sumi-reset",
                        self.tr("Reset all to 0", "Hepsini 0'a döndür"),
                        false,
                    )
                    .on_click(
                        cx.listener(|this, _, _, cx| this.edit_layers(cx, |l| l.sumi.reset())),
                    ),
                ),
            )
    }

    /// A vkSumi control's name and what it does.
    pub(super) fn sumi_name(
        &self,
        k: arkor_core::layers::SumiKnob,
    ) -> (&'static str, &'static str) {
        use arkor_core::layers::SumiKnob::*;
        match k {
            Brightness => (
                self.tr("Brightness", "Parlaklık"),
                self.tr(
                    "Lighter or darker everywhere.",
                    "Her yeri aydınlatır ya da karartır.",
                ),
            ),
            Contrast => (
                self.tr("Contrast", "Kontrast"),
                self.tr(
                    "More or less difference between light and dark.",
                    "Açık ile koyu arasındaki farkı artırır ya da azaltır.",
                ),
            ),
            Exposure => (
                self.tr("Exposure", "Pozlama"),
                self.tr(
                    "Like a camera's exposure: more or less light.",
                    "Fotoğraf makinesindeki gibi: daha çok ya da daha az ışık.",
                ),
            ),
            Gamma => (
                self.tr("Gamma", "Gama"),
                self.tr(
                    "Lifts or lowers the middle tones.",
                    "Orta tonları yükseltir ya da düşürür.",
                ),
            ),
            Shadows => (
                self.tr("Shadows", "Gölgeler"),
                self.tr(
                    "Only the dark parts. Raise it to see more in dark scenes.",
                    "Yalnızca karanlık bölgeler. Karanlık sahnelerde daha çok görmek için artır.",
                ),
            ),
            Midtones => (
                self.tr("Midtones", "Orta tonlar"),
                self.tr(
                    "Only the middle brightness.",
                    "Yalnızca orta parlaklıktaki bölgeler.",
                ),
            ),
            Highlights => (
                self.tr("Highlights", "Parlak bölgeler"),
                self.tr(
                    "Only the bright parts. Lower it if the sky is too bright.",
                    "Yalnızca parlak bölgeler. Gökyüzü fazla parlaksa azalt.",
                ),
            ),
            Saturation => (
                self.tr("Saturation", "Doygunluk"),
                self.tr(
                    "Stronger or paler colours.",
                    "Daha güçlü ya da daha soluk renkler.",
                ),
            ),
            Vibrance => (
                self.tr("Vibrance", "Canlılık"),
                self.tr(
                    "Livens up dull colours, leaves strong ones alone.",
                    "Soluk renkleri canlandırır, güçlü olanlara dokunmaz.",
                ),
            ),
            Hue => (
                self.tr("Hue", "Renk tonu"),
                self.tr(
                    "Turns every colour around the colour wheel.",
                    "Her rengi renk çemberinde döndürür.",
                ),
            ),
            Temperature => (
                self.tr("Temperature", "Sıcaklık"),
                self.tr(
                    "Warmer (yellow) or cooler (blue).",
                    "Daha sıcak (sarı) ya da daha soğuk (mavi).",
                ),
            ),
            Tint => (
                self.tr("Tint", "Renk kayması"),
                self.tr("Towards green or magenta.", "Yeşile ya da eflatuna doğru."),
            ),
            RedGain => (
                self.tr("Red", "Kırmızı"),
                self.tr("More or less red.", "Daha çok ya da daha az kırmızı."),
            ),
            GreenGain => (
                self.tr("Green", "Yeşil"),
                self.tr("More or less green.", "Daha çok ya da daha az yeşil."),
            ),
            BlueGain => (
                self.tr("Blue", "Mavi"),
                self.tr("More or less blue.", "Daha çok ya da daha az mavi."),
            ),
        }
    }

    /// One control: its name and help, − and +, a bar that fills from the middle, the value.
    fn sumi_row(&self, k: arkor_core::layers::SumiKnob, v: i32, cx: &mut Context<Self>) -> Div {
        use super::style::pal;
        let (name, help) = self.sumi_name(k);
        let (lo, hi, _) = k.range();
        let ix = k as usize;
        const BAR: f32 = 200.;
        let half = BAR / 2.;
        let frac = if v >= 0 {
            v as f32 / hi as f32
        } else {
            v as f32 / lo as f32
        };
        let fill = (half * frac).max(if v != 0 { 2. } else { 0. });
        let bar = div()
            .relative()
            .w(px(BAR))
            .h(px(6.))
            .rounded_full()
            .bg(pal::text().opacity(0.12))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .rounded_full()
                    .bg(pal::accent())
                    .w(px(fill))
                    .left(px(if v >= 0 { half } else { half - fill })),
            )
            .child(
                div()
                    .absolute()
                    .top(px(-3.))
                    .left(px(half - 1.))
                    .w(px(2.))
                    .h(px(12.))
                    .bg(pal::muted()),
            );
        div()
            .flex()
            .items_center()
            .gap_3()
            .py_1()
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
                            .child(name),
                    )
                    .child(div().text_xs().text_color(pal::muted()).child(help)),
            )
            .child(widgets::step_button(("sumi-", ix), "−").on_click(
                cx.listener(move |this, _, _, cx| this.edit_layers(cx, |l| l.sumi.step(k, false))),
            ))
            .child(bar)
            .child(widgets::step_button(("sumi+", ix), "+").on_click(
                cx.listener(move |this, _, _, cx| this.edit_layers(cx, |l| l.sumi.step(k, true))),
            ))
            .child(
                div()
                    .id(("sumi-val", ix))
                    .w(px(56.))
                    .text_sm()
                    .text_right()
                    .cursor_pointer()
                    .text_color(if v == 0 {
                        pal::muted()
                    } else {
                        pal::accent_light()
                    })
                    .child(k.show(v))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit_layers(cx, |l| l.sumi.set(k, 0))
                    })),
            )
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
