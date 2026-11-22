//! The game page, built from a registry of features.
//!
//! Every setting a game can have is one `Feature`: which tab it lives on, its icon, name,
//! search words, whether it applies to this kind of game, whether it is switched on, and
//! the card that edits it. Tabs, the "on" tags in the header, the quick toggles and the
//! settings search all read this one list, so adding a feature is one entry here plus its
//! card function.

use arkor_core::HudMode;
use arkor_core::library::{GameKind, LibraryGame};
use arkor_core::profile::LsfgMode;
use gpui::prelude::FluentBuilder;
use gpui::*;

use super::style::{self, pal};
use super::{Library, widgets};

/// Tabs of the game page, grouped by what the user wants to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum GameTab {
    #[default]
    Overview,
    Launch,
    Runtime,
    Performance,
    Upscaling,
    Visuals,
    Logs,
}

impl GameTab {
    pub(super) const ALL: [GameTab; 7] = [
        GameTab::Overview,
        GameTab::Launch,
        GameTab::Runtime,
        GameTab::Performance,
        GameTab::Upscaling,
        GameTab::Visuals,
        GameTab::Logs,
    ];

    fn icon(self) -> &'static str {
        match self {
            GameTab::Overview => "layout-dashboard",
            GameTab::Launch => "rocket",
            GameTab::Runtime => "wine",
            GameTab::Performance => "gauge",
            GameTab::Upscaling => "sparkles",
            GameTab::Visuals => "palette",
            GameTab::Logs => "scroll-text",
        }
    }

    fn name(self, lib: &Library) -> &'static str {
        match self {
            GameTab::Overview => lib.tr("Overview", "Genel bakış"),
            GameTab::Launch => lib.tr("Launch", "Başlatma"),
            GameTab::Runtime => lib.tr("Runtime", "Çalışma ortamı"),
            GameTab::Performance => lib.tr("Performance", "Performans"),
            GameTab::Upscaling => lib.tr("Upscaling & frame gen", "Ölçekleme ve kare üretimi"),
            GameTab::Visuals => lib.tr("Overlay & visuals", "Overlay ve görüntü"),
            GameTab::Logs => lib.tr("Logs", "Kayıtlar"),
        }
    }

    /// `arkor open <id>#<tab>` names (older names kept).
    pub(super) fn from_name(name: &str) -> Option<GameTab> {
        Some(match name {
            "overview" | "general" => GameTab::Overview,
            "launch" => GameTab::Launch,
            "runtime" | "wine" | "proton" => GameTab::Runtime,
            "performance" => GameTab::Performance,
            "upscaling" | "mods" | "framegen" => GameTab::Upscaling,
            "visuals" | "overlay" | "mangohud" => GameTab::Visuals,
            "logs" => GameTab::Logs,
            _ => return None,
        })
    }
}

type Render = fn(&mut Library, &mut Context<Library>) -> Option<Div>;

pub(super) struct Feature {
    pub id: &'static str,
    pub tab: GameTab,
    pub icon: &'static str,
    pub name: (&'static str, &'static str),
    /// Extra words the settings search matches (both languages, lower case).
    pub keywords: &'static str,
    pub applies: fn(&Library, &LibraryGame) -> bool,
    /// Switched on for this game (shown as a tag in the header). `None`: not a toggle.
    pub active: Option<fn(&Library) -> bool>,
    pub render: Render,
}

fn always(_: &Library, _: &LibraryGame) -> bool {
    true
}
fn wine_game(_: &Library, g: &LibraryGame) -> bool {
    g.needs_prefix()
}
fn proton_choice(_: &Library, g: &LibraryGame) -> bool {
    matches!(g.kind, GameKind::Windows | GameKind::Epic { .. })
}
fn linux_game(_: &Library, g: &LibraryGame) -> bool {
    g.kind == GameKind::Linux
}
fn steam_game(_: &Library, g: &LibraryGame) -> bool {
    matches!(g.kind, GameKind::Steam { .. })
}
fn has_other_layers(lib: &Library, _: &LibraryGame) -> bool {
    lib.has_other_layers()
}

pub(super) const FEATURES: &[Feature] = &[
    // Overview
    Feature {
        id: "details",
        tab: GameTab::Overview,
        icon: "file-pen",
        name: ("Details", "Ayrıntılar"),
        keywords: "name ad category kategori hidden gizli cover kapak steamgriddb duplicate çoğalt delete sil",
        applies: always,
        active: None,
        render: |l, cx| l.render_details_card(cx),
    },
    // Launch
    Feature {
        id: "executable",
        tab: GameTab::Launch,
        icon: "file-terminal",
        name: ("Executable", "Çalıştırılabilir dosya"),
        keywords: "exe arguments argümanlar prefix wine protontricks app id",
        applies: always,
        active: None,
        render: |l, cx| l.render_exec_card(cx),
    },
    Feature {
        id: "launch-options",
        tab: GameTab::Launch,
        icon: "terminal",
        name: ("Launch options & hooks", "Başlatma seçenekleri ve komutlar"),
        keywords: "%command% pre post script hook extra app trainer sleep uyku inhibit",
        applies: always,
        active: Some(|l| {
            !l.profile.launch_options.trim().is_empty()
                || !l.profile.pre_launch.is_empty()
                || !l.profile.post_launch.is_empty()
        }),
        render: |l, cx| {
            let entry = l.entry()?.clone();
            let steam = matches!(entry.kind, GameKind::Steam { .. });
            Some(l.render_launch(&entry.id, steam, cx))
        },
    },
    Feature {
        id: "env",
        tab: GameTab::Launch,
        icon: "variable",
        name: ("Environment variables", "Ortam değişkenleri"),
        keywords: "env ortam değişken variable key value",
        applies: always,
        active: Some(|l| !l.profile.env.is_empty()),
        render: |l, cx| Some(l.render_env_card(cx)),
    },
    Feature {
        id: "shortcuts",
        tab: GameTab::Launch,
        icon: "app-window",
        name: ("Shortcuts", "Kısayollar"),
        keywords: "desktop masaüstü menu menü steam heroic lutris command komut",
        applies: always,
        active: None,
        render: |l, cx| l.render_shortcuts(cx),
    },
    // Runtime
    Feature {
        id: "proton",
        tab: GameTab::Runtime,
        icon: "wine",
        name: ("Proton version", "Proton sürümü"),
        keywords: "proton ge cachyos wine version sürüm",
        applies: proton_choice,
        active: Some(|l| l.profile.proton.is_some()),
        render: |l, cx| {
            let game = l.selected_game()?.clone();
            l.render_proton(&game, cx)
        },
    },
    Feature {
        id: "steam-proton",
        tab: GameTab::Runtime,
        icon: "wine",
        name: ("Proton", "Proton"),
        keywords: "proton steam",
        applies: steam_game,
        active: None,
        render: |l, cx| Some(l.render_steam_runtime_note(cx)),
    },
    Feature {
        id: "protonfixes",
        tab: GameTab::Runtime,
        icon: "wrench",
        name: ("Protonfixes (umu id)", "Protonfix (umu kimliği)"),
        keywords: "umu protonfix gameid fix düzeltme",
        applies: proton_choice,
        active: Some(|l| l.profile.umu_id.is_some()),
        render: |l, cx| Some(l.render_protonfix(cx)),
    },
    Feature {
        id: "linux-runtime",
        tab: GameTab::Runtime,
        icon: "box",
        name: ("Steam Linux Runtime", "Steam Linux Runtime"),
        keywords: "sniper soldier scout runtime container",
        applies: linux_game,
        active: Some(|l| l.profile.linux_runtime.is_some()),
        render: |l, cx| Some(l.render_runtime(cx)),
    },
    Feature {
        id: "prefix-tools",
        tab: GameTab::Runtime,
        icon: "folder-cog",
        name: ("Prefix tools", "Prefix araçları"),
        keywords: "winecfg winetricks regedit installer kurulum run çalıştır dll kill",
        applies: wine_game,
        active: None,
        render: |l, cx| Some(l.render_prefix_tools(cx)),
    },
    // Performance
    Feature {
        id: "tweaks",
        tab: GameTab::Performance,
        icon: "zap",
        name: ("Tweaks", "İnce ayarlar"),
        keywords: "gamemode dxvk vkd3d nvapi fsr esync fsync ntsync hdr wayland async cpu gpu shader cache",
        applies: always,
        active: Some(|l| l.profile.gamemode || !l.profile.tweaks.is_empty()),
        render: |l, cx| Some(l.render_tweaks(cx)),
    },
    Feature {
        id: "gamescope",
        tab: GameTab::Performance,
        icon: "monitor",
        name: ("Gamescope", "Gamescope"),
        keywords: "gamescope resolution çözünürlük fsr nis scaling hdr refresh fullscreen tam ekran fps limit",
        applies: always,
        active: Some(|l| l.profile.gamescope.enabled),
        render: |l, cx| Some(l.render_gamescope(cx)),
    },
    Feature {
        id: "vknemu",
        tab: GameTab::Performance,
        icon: "moon",
        name: ("Idle FPS limit (vkNemu)", "Boşta FPS sınırı (vkNemu)"),
        keywords: "vknemu idle boşta fps limit power güç",
        applies: always,
        active: Some(|l| l.profile.layers.vknemu.enabled),
        render: |l, cx| Some(l.render_vknemu(cx)),
    },
    // Upscaling & frame generation
    Feature {
        id: "lsfg",
        tab: GameTab::Upscaling,
        icon: "layers",
        name: (
            "Lossless Scaling frame gen (lsfg-vk)",
            "Lossless Scaling kare üretimi (lsfg-vk)",
        ),
        keywords: "lsfg lossless frame generation kare üretimi multiplier çarpan flow fps",
        applies: always,
        active: Some(|l| l.profile.lsfg.mode == LsfgMode::On),
        render: |l, cx| Some(l.render_lsfg(cx)),
    },
    Feature {
        id: "mako",
        tab: GameTab::Upscaling,
        icon: "layers-2",
        name: ("MAKO frame gen & scaling", "MAKO kare üretimi ve ölçekleme"),
        keywords: "mako frame generation kare üretimi spatial scaling ölçekleme fps",
        applies: always,
        active: Some(|l| l.profile.layers.mako.enabled || l.profile.layers.mako.scaling),
        render: |l, cx| Some(l.render_mako(cx)),
    },
    Feature {
        id: "afmf",
        tab: GameTab::Upscaling,
        icon: "fast-forward",
        name: ("AFMF frame gen", "AFMF kare üretimi"),
        keywords: "afmf amd fluid motion frames kare üretimi fps",
        applies: always,
        active: Some(|l| l.profile.layers.afmf.enabled),
        render: |l, cx| Some(l.render_afmf(cx)),
    },
    Feature {
        id: "optiscaler",
        tab: GameTab::Upscaling,
        icon: "scan-eye",
        name: ("OptiScaler", "OptiScaler"),
        keywords: "optiscaler dlss fsr fsr4 xess optipatcher edge upscaler ölçekleyici",
        applies: wine_game,
        active: Some(|l| l.tools.manifest.is_some()),
        render: |l, cx| Some(l.render_optiscaler(cx)),
    },
    Feature {
        id: "dlss-enabler",
        tab: GameTab::Upscaling,
        icon: "cpu",
        name: ("DLSS Enabler", "DLSS Enabler"),
        keywords: "dlss enabler frame generation amd intel",
        applies: wine_game,
        active: Some(|l| l.tools.de_manifest.is_some()),
        render: |l, cx| Some(l.render_dlss_enabler(cx)),
    },
    Feature {
        id: "streamline",
        tab: GameTab::Upscaling,
        icon: "refresh-cw",
        name: ("Streamline / DLSS update", "Streamline / DLSS güncelleme"),
        keywords: "streamline nvidia dlss update güncelle reflex nvngx",
        applies: wine_game,
        active: Some(|l| l.tools.sl_manifest.is_some()),
        render: |l, cx| Some(l.render_streamline(cx)),
    },
    Feature {
        id: "amd-bundles",
        tab: GameTab::Upscaling,
        icon: "atom",
        name: ("HelixSR & DLSS-NR (AMD)", "HelixSR ve DLSS-NR (AMD)"),
        keywords: "helixsr dlss-nr dlssnr amd rdna fsr4 neural",
        applies: wine_game,
        active: Some(|l| l.tools.dlssnr.is_some()),
        render: |l, cx| Some(l.render_amd_bundles(cx)),
    },
    // Overlay & visuals
    Feature {
        id: "mangohud",
        tab: GameTab::Visuals,
        icon: "gauge",
        name: ("MangoHud", "MangoHud"),
        keywords: "mangohud overlay fps hud frametime grafik log",
        applies: always,
        active: Some(|l| match l.profile.mangohud.mode {
            HudMode::Custom => true,
            HudMode::Global => l.global.mangohud.enabled,
            HudMode::Off => false,
        }),
        render: |l, cx| Some(l.render_mangohud_card(cx)),
    },
    Feature {
        id: "vkbasalt",
        tab: GameTab::Visuals,
        icon: "wand-sparkles",
        name: ("vkBasalt", "vkBasalt"),
        keywords: "vkbasalt cas sharpen keskinlik smaa fxaa lut reshade daltonize",
        applies: always,
        active: Some(|l| l.profile.vkbasalt.enabled),
        render: |l, cx| Some(l.render_vkbasalt(cx)),
    },
    Feature {
        id: "reshade",
        tab: GameTab::Visuals,
        icon: "brush",
        name: ("ReShade", "ReShade"),
        keywords: "reshade shader efekt effects",
        applies: wine_game,
        active: Some(|l| l.tools.rs_manifest.is_some()),
        render: |l, cx| Some(l.render_reshade(cx)),
    },
    Feature {
        id: "vksumi",
        tab: GameTab::Visuals,
        icon: "contrast",
        name: ("Colour grading (vkSumi)", "Renk ayarı (vkSumi)"),
        keywords: "vksumi vibrance canlılık contrast kontrast colour renk",
        applies: always,
        active: Some(|l| l.profile.layers.vksumi),
        render: |l, cx| Some(l.render_vksumi(cx)),
    },
    Feature {
        id: "other-layers",
        tab: GameTab::Visuals,
        icon: "layers-3",
        name: ("Other Vulkan layers", "Diğer Vulkan katmanları"),
        keywords: "vulkan layer katman obs",
        applies: has_other_layers,
        active: Some(|l| !l.profile.layers.other.is_empty()),
        render: |l, cx| Some(l.render_other_layers(cx)),
    },
    // Logs
    Feature {
        id: "logs",
        tab: GameTab::Logs,
        icon: "scroll-text",
        name: ("Logs", "Kayıtlar"),
        keywords: "log kayıt proton_log error hata",
        applies: always,
        active: None,
        render: |l, cx| {
            let id = l.selected_game()?.id.clone();
            Some(l.render_logs(&id, cx))
        },
    },
];

/// Quick toggles on the Overview tab: (feature id, toggle).
type Toggle = fn(&mut Library);

const QUICK: [(&str, Toggle); 6] = [
    ("mangohud", |l| {
        l.profile.mangohud.mode = match l.profile.mangohud.mode {
            HudMode::Off => {
                if l.profile.mangohud.settings.is_some() {
                    HudMode::Custom
                } else {
                    HudMode::Global
                }
            }
            _ => HudMode::Off,
        };
        // "Global" with MangoHud off globally does nothing; use the game's own layout then.
        if l.profile.mangohud.mode == HudMode::Global && !l.global.mangohud.enabled {
            l.profile
                .mangohud
                .settings
                .get_or_insert_with(|| l.global.mangohud.settings.clone());
            l.profile.mangohud.mode = HudMode::Custom;
        }
    }),
    ("tweaks", |l| l.profile.gamemode = !l.profile.gamemode),
    ("gamescope", |l| {
        l.profile.gamescope.enabled = !l.profile.gamescope.enabled
    }),
    ("lsfg", |l| {
        l.profile.lsfg.mode = if l.profile.lsfg.mode == LsfgMode::On {
            LsfgMode::Default
        } else {
            LsfgMode::On
        }
    }),
    ("vkbasalt", |l| {
        l.profile.vkbasalt.enabled = !l.profile.vkbasalt.enabled
    }),
    ("launch-options", |l| {
        l.profile.no_sleep = !l.profile.no_sleep
    }),
];

fn feature(id: &str) -> &'static Feature {
    FEATURES.iter().find(|f| f.id == id).expect("known feature")
}

impl Library {
    fn feature_name(&self, f: &Feature) -> &'static str {
        self.tr(f.name.0, f.name.1)
    }

    fn applicable(&self, entry: &LibraryGame) -> Vec<&'static Feature> {
        FEATURES
            .iter()
            .filter(|f| (f.applies)(self, entry))
            .collect()
    }

    pub(super) fn has_other_layers(&self) -> bool {
        self.tools
            .system_layers
            .iter()
            .any(|s| !super::enhance::HANDLED_LAYERS.contains(&s.enable.0.as_str()))
    }

    fn feature_matches(&self, f: &Feature, query: &str) -> bool {
        let hay = format!("{} {} {} {}", f.name.0, f.name.1, f.keywords, f.id).to_lowercase();
        query
            .to_lowercase()
            .split_whitespace()
            .all(|w| hay.contains(w))
    }

    fn quick_label(&self, id: &str) -> (&'static str, &'static str, bool) {
        let p = &self.profile;
        match id {
            "mangohud" => (
                "MangoHud",
                "gauge",
                (feature("mangohud").active.unwrap())(self),
            ),
            "tweaks" => ("GameMode", "zap", p.gamemode),
            "gamescope" => ("Gamescope", "monitor", p.gamescope.enabled),
            "lsfg" => ("lsfg-vk", "layers", p.lsfg.mode == LsfgMode::On),
            "vkbasalt" => ("vkBasalt", "wand-sparkles", p.vkbasalt.enabled),
            _ => (self.tr("Keep awake", "Uyku engelle"), "coffee", p.no_sleep),
        }
    }

    fn render_quick(&mut self, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_wrap().gap_3();
        for (ix, (id, toggle)) in QUICK.into_iter().enumerate() {
            let (label, icon, on) = self.quick_label(id);
            let tab = feature(id).tab;
            grid = grid.child(
                div()
                    .id(("quick", ix))
                    .w(px(196.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .rounded(px(14.))
                    .border_1()
                    .border_color(if on {
                        pal::accent().opacity(0.5)
                    } else {
                        pal::glass_spec().border
                    })
                    .bg(if on {
                        pal::accent().opacity(0.1)
                    } else {
                        pal::raised().opacity(0.4)
                    })
                    .cursor_pointer()
                    .hover(|s| s.border_color(pal::accent().opacity(0.6)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        toggle(this);
                        this.save();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(34.))
                            .flex_none()
                            .rounded(px(10.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if on {
                                pal::accent().opacity(0.2)
                            } else {
                                pal::raised()
                            })
                            .child(style::icon(
                                icon,
                                17.,
                                if on {
                                    pal::accent_light()
                                } else {
                                    pal::muted()
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if on {
                                        pal::accent_light()
                                    } else {
                                        pal::faint()
                                    })
                                    .child(if on {
                                        self.tr("On", "Açık")
                                    } else {
                                        self.tr("Off", "Kapalı")
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id(("quick-more", ix))
                            .p_1()
                            .rounded(px(6.))
                            .hover(|s| s.bg(pal::raised()))
                            .child(style::icon("chevron-right", 14., pal::faint()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.game_tab = tab;
                                cx.notify();
                            })),
                    ),
            );
        }
        widgets::section(self.tr("Quick settings", "Hızlı ayarlar"))
            .child(widgets::label(self.tr(
                "The most used switches. The arrow opens their full settings.",
                "En çok kullanılan anahtarlar. Ok, tüm ayarlarını açar.",
            )))
            .child(grid)
    }

    /// Cover, name, source, play time, Play / Stop and the features that are on.
    fn render_hero(&mut self, entry: &LibraryGame, cx: &mut Context<Self>) -> Div {
        let game_id = entry.game_id();
        let running = self.running.contains_key(&game_id.to_string());
        let (secs, last) = self.playtime(&game_id);
        let mut facts = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(pal::muted());
        facts = facts.child(style::tag(self.kind_label(entry), pal::accent_light()));
        if let Some(cat) = &entry.category {
            facts = facts.child(style::tag(cat.clone(), pal::muted()));
        }
        facts = facts.child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .child(style::icon("clock", 13., pal::faint()))
                .child(if secs > 0 {
                    arkor_core::stats::format_duration(secs)
                } else {
                    self.tr("Not played yet", "Henüz oynanmadı").to_string()
                }),
        );
        if let Some(last) = last {
            facts = facts.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(style::icon("calendar", 13., pal::faint()))
                    .child(super::game_page::ago(
                        arkor_core::stats::now().saturating_sub(last),
                        self.global.language,
                    )),
            );
        }

        // Features that are on, each a link to its tab.
        let mut on_tags = div().flex().flex_wrap().gap_1p5();
        let mut any_on = false;
        for f in self.applicable(entry) {
            if f.active.is_some_and(|active| active(self)) {
                any_on = true;
                let tab = f.tab;
                on_tags = on_tags.child(
                    div()
                        .id(("on-tag", f.id.len() * 31 + f.id.as_bytes()[0] as usize))
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .text_xs()
                        .cursor_pointer()
                        .bg(pal::accent().opacity(0.12))
                        .border_1()
                        .border_color(pal::accent().opacity(0.3))
                        .text_color(pal::accent_light())
                        .hover(|s| s.bg(pal::accent().opacity(0.2)))
                        .child(style::icon(f.icon, 12., pal::accent_light()))
                        .child(self.feature_name(f))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.game_tab = tab;
                            cx.notify();
                        })),
                );
            }
        }
        let play = if running {
            widgets::primary_button("stop", self.tr("Stop", "Durdur")).child(style::icon(
                "square",
                14.,
                pal::on_accent(),
            ))
        } else {
            widgets::primary_button("play", self.t().play).child(style::icon(
                "play",
                14.,
                pal::on_accent(),
            ))
        }
        .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx)));
        let folder = self.selected_game().and_then(|g| g.install_dir.clone());
        style::card()
            .p_5()
            .flex()
            .gap_5()
            .child(self.cover_tile(&entry.id, &entry.name, 112., 168.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_2xl().font_weight(FontWeight::BOLD).child(entry.name.clone()))
                    .child(facts)
                    .child(div().flex_1())
                    .child(if any_on {
                        on_tags
                    } else {
                        div().text_sm().text_color(pal::faint()).child(self.tr(
                            "Nothing extra is on for this game yet: see Quick settings below or the tabs.",
                            "Bu oyunda henüz ek bir şey açık değil: aşağıdaki Hızlı ayarlara ya da sekmelere bak.",
                        ))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap_2()
                    .child(play)
                    .children(folder.map(|dir| {
                        style::button("open-folder", self.tr("Open folder", "Klasörü aç"), false)
                            .child(style::icon("folder-open", 13., pal::muted()))
                            .on_click(move |_, _, _| {
                                let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
                            })
                    })),
            )
    }

    fn render_feature_search(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Entity<super::text_input::TextInput> {
        let placeholder = self.tr("Search this game's settings…", "Bu oyunun ayarlarında ara…");
        self.field(
            "feature-search",
            |this| this.feature_query.clone(),
            placeholder,
            cx,
            |this, text| {
                this.feature_query = text.to_string();
            },
        )
    }

    pub(super) fn render_game(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let Some(entry) = self.entry().cloned() else {
            return div().child(widgets::label(self.t().not_found));
        };
        let applicable = self.applicable(&entry);
        let hero = self.render_hero(&entry, cx);
        let search = self.render_feature_search(cx);

        // Tabs that have something for this game.
        let mut tabs = style::card().p_1p5().flex().flex_wrap().gap_1();
        for (ix, tab) in GameTab::ALL.into_iter().enumerate() {
            if tab != GameTab::Overview && !applicable.iter().any(|f| f.tab == tab) {
                continue;
            }
            let active = self.game_tab == tab && self.feature_query.trim().is_empty();
            let on = applicable
                .iter()
                .filter(|f| f.tab == tab && f.active.is_some_and(|a| a(self)))
                .count();
            tabs = tabs.child(
                div()
                    .id(("game-tab", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3p5()
                    .py_2()
                    .rounded(px(12.))
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(active, |s| {
                        s.bg(pal::text().opacity(0.07))
                            .text_color(pal::accent_light())
                    })
                    .when(!active, |s| {
                        s.text_color(pal::muted())
                            .hover(|h| h.text_color(pal::text()).bg(pal::text().opacity(0.04)))
                    })
                    .child(style::icon(
                        tab.icon(),
                        15.,
                        if active {
                            pal::accent_light()
                        } else {
                            pal::muted()
                        },
                    ))
                    .child(tab.name(self))
                    .when(on > 0 && tab != GameTab::Overview, |d| {
                        d.child(
                            div()
                                .px_1p5()
                                .rounded_full()
                                .text_xs()
                                .bg(pal::accent().opacity(0.2))
                                .text_color(pal::accent_light())
                                .child(on.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.game_tab = tab;
                        this.feature_query.clear();
                        this.fields.inputs.remove("feature-search");
                        cx.notify();
                    })),
            );
        }

        let mut page = div().flex().flex_col().gap_4().child(hero).child(
            div().flex().flex_col().gap_3().child(tabs).child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(style::icon("search", 15., pal::faint()))
                    .child(div().flex_1().child(search)),
            ),
        );

        let query = self.feature_query.trim().to_string();
        if !query.is_empty() {
            let hits: Vec<&'static Feature> = applicable
                .iter()
                .copied()
                .filter(|f| self.feature_matches(f, &query))
                .collect();
            page = page.child(widgets::label(match hits.len() {
                0 => self
                    .tr(
                        "No setting matches. Try another word, e.g. fps, proton, dlss.",
                        "Eşleşen ayar yok. Başka bir kelime dene, ör. fps, proton, dlss.",
                    )
                    .to_string(),
                n => format!("{n} {}", self.tr("matching settings", "eşleşen ayar")),
            }));
            for f in hits {
                let tab_name = f.tab.name(self);
                page = page.child(widgets::label(format!("{tab_name} ›")).text_xs());
                if let Some(card) = (f.render)(self, cx) {
                    page = page.child(card);
                }
            }
            return page;
        }

        if self.game_tab == GameTab::Overview {
            page = page.child(self.render_quick(cx));
        }
        let tab = self.game_tab;
        for f in applicable.into_iter().filter(|f| f.tab == tab) {
            if let Some(card) = (f.render)(self, cx) {
                page = page.child(card);
            }
        }
        page
    }
}
