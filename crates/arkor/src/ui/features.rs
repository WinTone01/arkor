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
    Game,
    Runtime,
    Launch,
    Performance,
    Upscaling,
    Visuals,
    Mods,
    Logs,
}

impl GameTab {
    pub(super) const ALL: [GameTab; 9] = [
        GameTab::Overview,
        GameTab::Game,
        GameTab::Runtime,
        GameTab::Launch,
        GameTab::Performance,
        GameTab::Upscaling,
        GameTab::Visuals,
        GameTab::Mods,
        GameTab::Logs,
    ];

    fn icon(self) -> &'static str {
        match self {
            GameTab::Overview => "layout-dashboard",
            GameTab::Game => "gamepad-2",
            GameTab::Launch => "rocket",
            GameTab::Runtime => "wine",
            GameTab::Performance => "gauge",
            GameTab::Upscaling => "sparkles",
            GameTab::Visuals => "palette",
            GameTab::Mods => "puzzle",
            GameTab::Logs => "scroll-text",
        }
    }

    fn name(self, lib: &Library) -> &'static str {
        match self {
            GameTab::Overview => lib.tr("Overview", "Genel bakış"),
            GameTab::Game => lib.tr("Game", "Oyun"),
            GameTab::Launch => lib.tr("Launch", "Başlatma"),
            GameTab::Runtime => lib.tr("Compatibility", "Uyumluluk"),
            GameTab::Performance => lib.tr("Performance", "Performans"),
            GameTab::Upscaling => lib.tr("Upscaling & frame gen", "Ölçekleme ve kare üretimi"),
            GameTab::Visuals => lib.tr("Overlay & visuals", "Overlay ve görüntü"),
            GameTab::Mods => lib.tr("Mods", "Modlar"),
            GameTab::Logs => lib.tr("Logs", "Kayıtlar"),
        }
    }

    /// `arkor open <id>#<tab>` names (older names kept).
    pub(super) fn from_name(name: &str) -> Option<GameTab> {
        Some(match name {
            "overview" | "general" => GameTab::Overview,
            "game" | "details" | "files" | "cover" => GameTab::Game,
            "launch" => GameTab::Launch,
            "runtime" | "compatibility" | "wine" | "proton" => GameTab::Runtime,
            "performance" => GameTab::Performance,
            "upscaling" | "framegen" => GameTab::Upscaling,
            "mods" | "modding" => GameTab::Mods,
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
    /// One sentence shown on the closed panel: what the feature does.
    pub desc: (&'static str, &'static str),
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
/// Sub-tabs of "Upscaling & frame gen", like Goverlay's pages: (icon, English, Turkish,
/// feature ids). A sub-tab with nothing for this game or computer is left out.
pub(super) const UPSCALE_GROUPS: [(&str, &str, &str, &[&str]); 3] = [
    (
        "layers",
        "Frame generation",
        "Kare üretimi",
        &["lsfg", "mako", "afmf"],
    ),
    (
        "scan-eye",
        "Upscalers",
        "Ölçekleyiciler",
        &["optiscaler", "dlss-enabler", "streamline"],
    ),
    ("atom", "AMD", "AMD", &["amd-bundles"]),
];

type Ready = fn(&Library) -> bool;
type Start = fn(&mut Library, &mut Context<Library>);

/// Features that need a download before they can be used: whether it is there (Arkor's
/// copy or a system one), and how to get it. Until then their panel is a Download button.
fn download_of(id: &str) -> Option<(Ready, Start)> {
    Some(match id {
        "vksumi" => (
            |l| l.tools.layer_tools[0] || l.layer_present("ENABLE_VKSUMI"),
            |l, cx| l.enqueue_download("vksumi", cx),
        ),
        "vknemu" => (
            |l| l.tools.layer_tools[1] || l.layer_present("VKNEMU"),
            |l, cx| l.enqueue_download("vknemu", cx),
        ),
        "mako" => (
            |l| l.tools.layer_tools[2] || l.layer_present("ENABLE_MAKO"),
            |l, cx| l.enqueue_download("mako", cx),
        ),
        "afmf" => (
            |l| l.tools.layer_tools[3] || l.layer_present("AFMF_ENABLE"),
            |l, cx| l.enqueue_download("afmf", cx),
        ),
        "optiscaler" => (
            |l| l.tools.downloaded.is_some() || l.tools.manifest.is_some(),
            |l, cx| l.download_optiscaler(cx),
        ),
        "dlss-enabler" => (
            |l| l.tools.de_downloaded.is_some() || l.tools.de_manifest.is_some(),
            |l, cx| l.enqueue_download("dlss-enabler", cx),
        ),
        "reshade" => (
            |l| l.tools.rs_downloaded.is_some() || l.tools.rs_manifest.is_some(),
            |l, cx| l.enqueue_download("reshade", cx),
        ),
        "streamline" => (
            |l| l.tools.sl_downloaded.is_some() || l.tools.sl_manifest.is_some(),
            |l, cx| l.enqueue_download("streamline", cx),
        ),
        "mods" => (
            |l| l.mods_ready(),
            |l, cx| l.enqueue_download("amethyst", cx),
        ),
        _ => return None,
    })
}

/// HelixSR and DLSS-NR run only on AMD Radeon cards.
fn amd_wine_game(l: &Library, g: &LibraryGame) -> bool {
    l.tools.gpus.amd && g.needs_prefix()
}
/// AFMF (AMD Fluid Motion Frames) is offered on AMD cards only.
fn amd_only(l: &Library, _: &LibraryGame) -> bool {
    l.tools.gpus.amd
}
/// DLSS Enabler brings DLSS frame generation to AMD and Intel cards; NVIDIA has DLSS itself.
fn non_nvidia_wine_game(l: &Library, g: &LibraryGame) -> bool {
    (l.tools.gpus.amd || l.tools.gpus.intel) && g.needs_prefix()
}
fn wine_game(_: &Library, g: &LibraryGame) -> bool {
    g.needs_prefix()
}
fn proton_choice(_: &Library, g: &LibraryGame) -> bool {
    matches!(
        g.kind,
        GameKind::Windows | GameKind::Epic { .. } | GameKind::Amazon { .. }
    )
}
fn linux_game(_: &Library, g: &LibraryGame) -> bool {
    g.kind == GameKind::Linux
}
fn steam_game(_: &Library, g: &LibraryGame) -> bool {
    matches!(g.kind, GameKind::Steam { .. })
}
/// Saves in a store's cloud, or copies of any game's saves (Ludusavi).
fn cloud_saves(lib: &Library, _: &LibraryGame) -> bool {
    lib.saves_apply() || lib.saves.local.is_some()
}
fn has_other_layers(lib: &Library, _: &LibraryGame) -> bool {
    lib.has_other_layers()
}

pub(super) const FEATURES: &[Feature] = &[
    // Game
    Feature {
        id: "details",
        tab: GameTab::Game,
        icon: "file-pen",
        name: ("Name, category and cover", "Ad, kategori ve kapak"),
        desc: (
            "How the game shows in the library; duplicate or delete it.",
            "Oyunun kütüphanede nasıl göründüğü; kopyala ya da sil.",
        ),
        keywords: "name ad category kategori hidden gizli cover kapak steamgriddb duplicate çoğalt delete sil",
        applies: always,
        active: None,
        render: |l, cx| l.render_details_card(cx),
    },
    Feature {
        id: "executable",
        tab: GameTab::Game,
        icon: "file-terminal",
        name: ("Game files", "Oyun dosyaları"),
        desc: (
            "The executable, its arguments and the Wine prefix it runs in.",
            "Çalıştırılabilir dosya, argümanları ve çalıştığı Wine prefix'i.",
        ),
        keywords: "exe arguments argümanlar prefix wine protontricks app id",
        applies: always,
        active: None,
        render: |l, cx| l.render_exec_card(cx),
    },
    Feature {
        id: "saves",
        tab: GameTab::Game,
        icon: "cloud",
        name: ("Saves", "Oyun kayıtları"),
        desc: (
            "Copies of the saves to put back, and the store's cloud where there is one.",
            "Geri yüklenebilecek kayıt kopyaları, varsa mağazanın bulutu.",
        ),
        keywords: "save cloud sync backup restore copy ludusavi kayıt bulut eşitle yedek kopya geri yükle epic gog",
        applies: cloud_saves,
        active: None,
        render: |l, cx| l.render_saves(cx),
    },
    Feature {
        id: "shortcuts",
        tab: GameTab::Game,
        icon: "app-window",
        name: ("Shortcuts", "Kısayollar"),
        desc: (
            "Start it from the desktop, the app menu or Steam.",
            "Masaüstünden, uygulama menüsünden ya da Steam'den başlat.",
        ),
        keywords: "desktop masaüstü menu menü steam heroic lutris command komut",
        applies: always,
        active: None,
        render: |l, cx| l.render_shortcuts(cx),
    },
    // Compatibility
    Feature {
        id: "proton",
        tab: GameTab::Runtime,
        icon: "wine",
        name: ("Proton version", "Proton sürümü"),
        desc: (
            "Which Proton build runs the game.",
            "Oyunu hangi Proton derlemesinin çalıştıracağı.",
        ),
        keywords: "proton ge cachyos wine version sürüm latest",
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
        name: ("Steam: Proton and launch", "Steam: Proton ve başlatma"),
        desc: (
            "The Proton Steam uses for the game, and starting it through Arkor.",
            "Steam'in oyun için kullandığı Proton ve oyunu Arkor üzerinden başlatma.",
        ),
        keywords: "proton steam compatibility uyumluluk launch options başlatma seçenekleri verify doğrula",
        applies: steam_game,
        active: Some(|l| l.profile.steam.compat_tool.is_some()),
        render: |l, cx| Some(l.render_steam(cx)),
    },
    Feature {
        id: "tweaks",
        tab: GameTab::Runtime,
        icon: "toggle-right",
        name: ("Proton and driver switches", "Proton ve sürücü anahtarları"),
        desc: (
            "Wayland, HDR, DLSS/FSR upgrades, NTSYNC, WOW64 and more, one click each.",
            "Wayland, HDR, DLSS/FSR yükseltme, NTSYNC, WOW64 ve fazlası, tek tıkla.",
        ),
        keywords: "dxvk vkd3d nvapi fsr esync fsync ntsync hdr wayland async cpu gpu shader cache wow64 reflex antilag dlss xess",
        applies: always,
        active: Some(|l| !l.profile.tweaks.is_empty()),
        render: |l, cx| Some(l.render_tweaks(cx)),
    },
    Feature {
        id: "protonfixes",
        tab: GameTab::Runtime,
        icon: "wrench",
        name: ("Protonfixes", "Protonfix'ler"),
        desc: (
            "Known fixes for this game, applied by Proton when it knows its umu id.",
            "Bu oyun için bilinen düzeltmeler; Proton umu kimliğini bilince uygular.",
        ),
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
        desc: (
            "Run the game in a Steam runtime container for library compatibility.",
            "Kütüphane uyumluluğu için oyunu bir Steam runtime konteynerinde çalıştır.",
        ),
        keywords: "sniper soldier scout runtime container",
        applies: linux_game,
        active: Some(|l| l.profile.linux_runtime.is_some()),
        render: |l, cx| Some(l.render_runtime(cx)),
    },
    Feature {
        id: "prefix-tools",
        tab: GameTab::Runtime,
        icon: "folder-cog",
        name: ("Wine prefix tools", "Wine prefix araçları"),
        desc: (
            "winecfg, winetricks, running an installer in the prefix, stopping it.",
            "winecfg, winetricks, prefix'te kurulum dosyası çalıştırma, durdurma.",
        ),
        keywords: "winecfg winetricks regedit installer kurulum run çalıştır dll kill",
        applies: wine_game,
        active: None,
        render: |l, cx| Some(l.render_prefix_tools(cx)),
    },
    // Launch
    Feature {
        id: "launch-options",
        tab: GameTab::Launch,
        icon: "terminal",
        name: (
            "Launch options and commands",
            "Başlatma seçenekleri ve komutlar",
        ),
        desc: (
            "Extra words for the command line, commands before and after, a second program.",
            "Komut satırına ek kelimeler, önce ve sonra komutlar, ikinci bir program.",
        ),
        keywords: "%command% pre post script hook extra app trainer sleep uyku inhibit",
        applies: always,
        active: Some(|l| {
            !l.profile.launch_options.trim().is_empty()
                || !l.profile.pre_launch.is_empty()
                || !l.profile.post_launch.is_empty()
                || l.profile.extra_app.path.is_some()
        }),
        render: |l, cx| {
            let entry = l.entry()?.clone();
            let steam = matches!(entry.kind, GameKind::Steam { .. });
            Some(l.render_launch(steam, cx))
        },
    },
    Feature {
        id: "env",
        tab: GameTab::Launch,
        icon: "variable",
        name: ("Environment variables", "Ortam değişkenleri"),
        desc: (
            "KEY=VALUE pairs for this game only (they override the global ones).",
            "Sadece bu oyun için ANAHTAR=DEĞER çiftleri (globalleri ezer).",
        ),
        keywords: "env ortam değişken variable key value",
        applies: always,
        active: Some(|l| !l.profile.env.is_empty()),
        render: |l, cx| Some(l.render_env_card(cx)),
    },
    // Performance
    Feature {
        id: "sleep",
        tab: GameTab::Performance,
        icon: "coffee",
        name: ("Keep awake", "Uyku engelle"),
        desc: (
            "No sleep or screen blanking while playing.",
            "Oynarken uyku ya da ekran kararması yok.",
        ),
        keywords: "sleep uyku inhibit awake screen ekran blank",
        applies: always,
        active: Some(|l| l.profile.no_sleep),
        render: |l, cx| Some(l.render_sleep(cx)),
    },
    Feature {
        id: "gamescope",
        tab: GameTab::Performance,
        icon: "monitor",
        name: ("Gamescope", "Gamescope"),
        desc: (
            "Valve's compositor: resolution, upscaling, FPS limit, HDR, fullscreen.",
            "Valve'ın birleştiricisi: çözünürlük, ölçekleme, FPS limiti, HDR, tam ekran.",
        ),
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
        desc: (
            "Lowers the frame rate while the game is in the background.",
            "Oyun arka plandayken kare hızını düşürür.",
        ),
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
        desc: (
            "Generated frames between real ones (needs Lossless Scaling on Steam).",
            "Gerçek kareler arasına üretilmiş kareler (Steam'de Lossless Scaling gerekir).",
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
        desc: (
            "Open-source frame generation and spatial upscaling layer.",
            "Açık kaynak kare üretimi ve uzamsal ölçekleme katmanı.",
        ),
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
        desc: (
            "AMD Fluid Motion Frames for any game.",
            "Her oyun için AMD Fluid Motion Frames.",
        ),
        keywords: "afmf amd fluid motion frames kare üretimi fps",
        applies: amd_only,
        active: Some(|l| l.profile.layers.afmf.enabled),
        render: |l, cx| Some(l.render_afmf(cx)),
    },
    Feature {
        id: "optiscaler",
        tab: GameTab::Upscaling,
        icon: "scan-eye",
        name: ("OptiScaler", "OptiScaler"),
        desc: (
            "Swap the game's DLSS/FSR/XeSS for another upscaler, e.g. FSR 4.",
            "Oyunun DLSS/FSR/XeSS'ini başka bir ölçekleyiciyle değiştir, ör. FSR 4.",
        ),
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
        desc: (
            "DLSS frame generation on AMD and Intel GPUs.",
            "AMD ve Intel GPU'larda DLSS kare üretimi.",
        ),
        keywords: "dlss enabler frame generation amd intel",
        applies: non_nvidia_wine_game,
        active: Some(|l| l.tools.de_manifest.is_some()),
        render: |l, cx| Some(l.render_dlss_enabler(cx)),
    },
    Feature {
        id: "streamline",
        tab: GameTab::Upscaling,
        icon: "refresh-cw",
        name: ("Streamline / DLSS update", "Streamline / DLSS güncelleme"),
        desc: (
            "Replace the game's NVIDIA DLSS files with newer ones.",
            "Oyunun NVIDIA DLSS dosyalarını yenileriyle değiştir.",
        ),
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
        desc: (
            "Neural upscalers for AMD RDNA cards.",
            "AMD RDNA kartlar için yapay zekâ ölçekleyiciler.",
        ),
        keywords: "helixsr dlss-nr dlssnr amd rdna fsr4 neural",
        applies: amd_wine_game,
        active: Some(|l| l.tools.dlssnr.is_some()),
        render: |l, cx| Some(l.render_amd_bundles(cx)),
    },
    // Overlay & visuals
    Feature {
        id: "mangohud",
        tab: GameTab::Visuals,
        icon: "gauge",
        name: ("MangoHud", "MangoHud"),
        desc: (
            "FPS, temperatures and load drawn over the game.",
            "Oyunun üstünde FPS, sıcaklık ve yük.",
        ),
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
        desc: (
            "Sharpening, anti-aliasing and colour effects for Vulkan games.",
            "Vulkan oyunları için keskinleştirme, kenar yumuşatma ve renk efektleri.",
        ),
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
        desc: (
            "ReShade shaders installed in the game folder.",
            "Oyun klasörüne kurulan ReShade shader'ları.",
        ),
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
        desc: ("Vibrance and contrast.", "Canlılık ve kontrast."),
        keywords: "vksumi vibrance canlılık contrast kontrast colour renk",
        applies: always,
        active: Some(|l| l.profile.layers.vksumi),
        render: |l, cx| Some(l.render_vksumi(cx)),
    },
    Feature {
        id: "other-layers",
        tab: GameTab::Visuals,
        icon: "layers-2",
        name: ("Other Vulkan layers", "Diğer Vulkan katmanları"),
        desc: (
            "Other layers installed on this computer, e.g. OBS capture.",
            "Bu bilgisayarda kurulu diğer katmanlar, ör. OBS yakalama.",
        ),
        keywords: "vulkan layer katman obs",
        applies: has_other_layers,
        active: Some(|l| !l.profile.layers.other.is_empty()),
        render: |l, cx| Some(l.render_other_layers(cx)),
    },
    // Mods
    Feature {
        id: "mods",
        tab: GameTab::Mods,
        icon: "puzzle",
        name: ("Mods", "Modlar"),
        desc: (
            "Install, switch and order mods with Amethyst Mod Manager.",
            "Amethyst Mod Manager ile mod kur, aç-kapat ve sırala.",
        ),
        keywords: "mod mods nexus thunderstore fomod bepinex amethyst modlar eklenti",
        applies: always,
        active: None,
        render: |l, cx| l.render_mods(cx),
    },
    // Logs
    Feature {
        id: "trouble",
        tab: GameTab::Logs,
        icon: "stethoscope",
        name: ("Why did it not start?", "Neden açılmadı?"),
        desc: (
            "Looks for the reason and offers fixes that take one press.",
            "Sebebini arar, tek tuşla düzeltmeler sunar.",
        ),
        keywords: "trouble problem sorun çöktü crash fix düzelt neden açılmadı başlamıyor",
        applies: always,
        active: None,
        render: |l, cx| l.render_trouble(cx),
    },
    Feature {
        id: "logs",
        tab: GameTab::Logs,
        icon: "scroll-text",
        name: ("Launch log", "Başlatma kaydı"),
        desc: (
            "What happened on the last start, and Proton's own log.",
            "Son başlatmada ne olduğu ve Proton'un kendi kaydı.",
        ),
        keywords: "log kayıt proton_log error hata",
        applies: always,
        active: None,
        render: |l, cx| {
            let id = l.selected_game()?.id.clone();
            Some(l.render_logs(&id, cx))
        },
    },
];

/// Quick toggles on the Overview tab: (key, feature whose settings it opens, toggle).
pub(super) type Toggle = fn(&mut Library);

pub(super) const QUICK: [(&str, &str, Toggle); 5] = [
    ("mangohud", "mangohud", |l| {
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
    ("gamescope", "gamescope", |l| {
        l.profile.gamescope.enabled = !l.profile.gamescope.enabled
    }),
    ("lsfg", "lsfg", |l| {
        l.profile.lsfg.mode = if l.profile.lsfg.mode == LsfgMode::On {
            LsfgMode::Off
        } else {
            LsfgMode::On
        }
    }),
    ("vkbasalt", "vkbasalt", |l| {
        l.profile.vkbasalt.enabled = !l.profile.vkbasalt.enabled
    }),
    ("no-sleep", "sleep", |l| {
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

    pub(super) fn quick_label(&self, key: &str) -> (&'static str, &'static str, bool) {
        let p = &self.profile;
        match key {
            "mangohud" => (
                "MangoHud",
                "gauge",
                (feature("mangohud").active.unwrap())(self),
            ),
            "gamescope" => ("Gamescope", "monitor", p.gamescope.enabled),
            "lsfg" => ("lsfg-vk", "layers", p.lsfg.mode == LsfgMode::On),
            "vkbasalt" => ("vkBasalt", "wand-sparkles", p.vkbasalt.enabled),
            _ => (self.tr("Keep awake", "Uyku engelle"), "coffee", p.no_sleep),
        }
    }

    /// The most used switches as tiles: the tile switches, its link opens the settings.
    fn render_quick(&mut self, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_wrap().gap_3();
        for (ix, (key, feature_id, toggle)) in QUICK.into_iter().enumerate() {
            let (label, icon, on) = self.quick_label(key);
            let tab = feature(feature_id).tab;
            grid = grid.child(
                div()
                    .id(("quick", ix))
                    .w(px(230.))
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
                    .child(style::icon(
                        icon,
                        18.,
                        if on {
                            pal::accent_light()
                        } else {
                            pal::muted()
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
                            .child(
                                div()
                                    .id(("quick-more", ix))
                                    .text_xs()
                                    .text_color(pal::faint())
                                    .hover(|s| s.text_color(pal::accent_light()))
                                    .child(self.tr("Settings ›", "Ayarlar ›"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.open_feature(tab, feature_id);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(widgets::switch(on)),
            );
        }
        widgets::section(self.tr("Quick switches", "Hızlı anahtarlar")).child(grid)
    }

    /// Goes to `tab` with `feature_id`'s panel open.
    fn open_feature(&mut self, tab: GameTab, feature_id: &'static str) {
        self.game_tab = tab;
        self.feature_query.clear();
        self.fields.inputs.remove("feature-search");
        self.game_ui.expanded.insert(feature_id, true);
    }

    /// How the game starts, in a few lines, each leading to where it is changed.
    fn render_summary(&mut self, entry: &LibraryGame, cx: &mut Context<Self>) -> Div {
        let mut rows: Vec<(&'static str, String, GameTab, &'static str)> = Vec::new();
        match &entry.kind {
            GameKind::Windows | GameKind::Epic { .. } | GameKind::Amazon { .. } => {
                let proton = match &self.profile.proton {
                    Some(p) => p.clone(),
                    None => match &self.global.defaults.proton {
                        Some(d) => format!(
                            "{} ({d})",
                            self.tr("Automatic: the default", "Otomatik: varsayılan")
                        ),
                        None => self.tr("Automatic", "Otomatik").to_string(),
                    },
                };
                rows.push(("Proton", proton, GameTab::Runtime, "proton"));
                rows.push((
                    "Wine prefix",
                    entry
                        .effective_prefix(&self.global.prefixes_dir())
                        .display()
                        .to_string(),
                    GameTab::Game,
                    "executable",
                ));
            }
            GameKind::Steam { appid } => rows.push((
                "Steam",
                format!(
                    "{} {appid}",
                    self.tr("Started by Steam, app", "Steam başlatır, uygulama")
                ),
                GameTab::Game,
                "executable",
            )),
            GameKind::Linux => rows.push((
                self.tr("Runs", "Çalışma"),
                match self.profile.runtime() {
                    Some(r) => format!(
                        "{} {}",
                        self.tr("Inside", "İçinde:"),
                        r.trim_start_matches("umu-")
                    ),
                    None => self
                        .tr("Directly (native Linux)", "Doğrudan (yerel Linux)")
                        .to_string(),
                },
                GameTab::Runtime,
                "linux-runtime",
            )),
        }
        let launch = self.profile.launch_options.trim().to_string();
        rows.push((
            self.tr("Launch options", "Başlatma seçenekleri"),
            if launch.is_empty() {
                self.tr("None", "Yok").to_string()
            } else {
                launch
            },
            GameTab::Launch,
            "launch-options",
        ));
        let mut card = widgets::section(self.tr("How it starts", "Nasıl başlar"));
        for (ix, (name, value, tab, fid)) in rows.into_iter().enumerate() {
            if ix > 0 {
                card = card.child(widgets::divider());
            }
            card = card.child(widgets::setting_row(
                name,
                value,
                style::button(("summary-change", ix), self.tr("Change", "Değiştir"), false)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_feature(tab, fid);
                        cx.notify();
                    })),
            ));
        }
        card
    }

    /// A feature as a panel: closed it is one line (what it does, whether it is on);
    /// open it is the feature's card with a button to close it again.
    fn feature_panel(
        &mut self,
        f: &'static Feature,
        default_open: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let on = f.active.map(|a| a(self));
        let download = download_of(f.id)
            .filter(|(ready, _)| !ready(self))
            .map(|(_, start)| start);
        let open = download.is_none()
            && self
                .game_ui
                .expanded
                .get(f.id)
                .copied()
                .unwrap_or(default_open || on == Some(true));
        let id = f.id;
        if open {
            let card = (f.render)(self, cx)?;
            return Some(
                div()
                    .relative()
                    .child(card)
                    .child(
                        div()
                            .id(("panel-close", id.len() * 131 + id.as_bytes()[0] as usize))
                            .absolute()
                            .top_4()
                            .right_4()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded(px(8.))
                            .text_xs()
                            .text_color(pal::muted())
                            .cursor_pointer()
                            .hover(|s| s.bg(pal::raised()).text_color(pal::text()))
                            .child(self.tr("Close", "Kapat"))
                            .child(style::icon("chevron-up", 14., pal::muted()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.game_ui.expanded.insert(id, false);
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        Some(
            style::card()
                .id(("panel-open", id.len() * 131 + id.as_bytes()[0] as usize))
                .px_5()
                .py_4()
                .flex()
                .items_center()
                .gap_4()
                .cursor_pointer()
                .hover(|s| s.border_color(pal::accent().opacity(0.5)))
                .child(
                    div()
                        .size(px(36.))
                        .flex_none()
                        .rounded(px(10.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(if on == Some(true) {
                            pal::accent().opacity(0.18)
                        } else {
                            pal::raised()
                        })
                        .child(style::icon(
                            f.icon,
                            18.,
                            if on == Some(true) {
                                pal::accent_light()
                            } else {
                                pal::muted()
                            },
                        )),
                )
                .child(
                    div().flex_1().min_w_0().flex().flex_col().gap_0p5().child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.feature_name(f))
                            .child(widgets::hint_icon(
                                ("panel-hint", id.len() * 131 + id.as_bytes()[0] as usize),
                                self.tr(f.desc.0, f.desc.1),
                            )),
                    ),
                )
                .when_some(download, |row, start| {
                    // Nothing to set up before it is downloaded: just the button.
                    // Only this tool's own download counts, not others running at the same time.
                    let running = arkor_launch::progress::running(id);
                    let busy = running || arkor_launch::downloads::pending(id);
                    row.child(
                        style::button(
                            ("panel-download", id.len() * 131 + id.as_bytes()[0] as usize),
                            if running {
                                self.tr("Downloading…", "İndiriliyor…")
                            } else if busy {
                                self.tr("In queue", "Sırada")
                            } else {
                                self.tr("Download", "İndir")
                            },
                            !busy,
                        )
                        .child(style::icon(
                            "download",
                            13.,
                            if busy { pal::muted() } else { pal::on_accent() },
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if !arkor_launch::progress::running(id)
                                && !arkor_launch::downloads::pending(id)
                            {
                                // Open the panel once the download is there.
                                this.game_ui.expanded.insert(id, true);
                                start(this, cx);
                            }
                        })),
                    )
                })
                .when(download.is_none(), |row| {
                    row.children(on.map(|on| {
                        if on {
                            style::tag(self.tr("On", "Açık"), pal::accent_light())
                        } else {
                            style::tag(self.tr("Off", "Kapalı"), pal::faint())
                        }
                    }))
                    .child(style::icon("chevron-down", 16., pal::faint()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.game_ui.expanded.insert(id, true);
                        cx.notify();
                    }))
                })
                .into_any_element(),
        )
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
                let (tab, fid) = (f.tab, f.id);
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
                            this.open_feature(tab, fid);
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
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .child(entry.name.clone()),
                    )
                    .child(facts)
                    .child(div().flex_1())
                    .when(any_on, |d| d.child(on_tags)),
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
        let placeholder = self.tr("Search settings…", "Ayarlarda ara…");
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

        // Tabs that have something for this game, as the page's side menu.
        let tabs: Vec<GameTab> = GameTab::ALL
            .into_iter()
            .filter(|t| *t == GameTab::Overview || applicable.iter().any(|f| f.tab == *t))
            .collect();
        let searching = !self.feature_query.trim().is_empty();
        let entries = tabs
            .iter()
            .map(|&tab| {
                let on = applicable
                    .iter()
                    .filter(|f| f.tab == tab && f.active.is_some_and(|a| a(self)))
                    .count();
                style::NavEntry {
                    icon: tab.icon(),
                    label: tab.name(self),
                    badge: (on > 0 && tab != GameTab::Overview).then(|| on.to_string()),
                }
            })
            .collect();
        let selected = (!searching)
            .then(|| tabs.iter().position(|t| *t == self.game_tab))
            .flatten();
        let nav_tabs = tabs.clone();
        let nav = div()
            .w(px(208.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .child(search)
            .child(style::side_nav(
                "game-tab",
                entries,
                selected,
                cx,
                move |this, i, cx| {
                    this.game_tab = nav_tabs[i];
                    this.feature_query.clear();
                    this.fields.inputs.remove("feature-search");
                    cx.notify();
                },
            ));
        let page = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().flex().child(
                style::back_link("game-back", self.tr("Library", "Kütüphane")).on_click(
                    cx.listener(|this, _, _, cx| this.go(super::shell::Nav::Library, cx)),
                ),
            ))
            .child(hero)
            .children(self.render_saves_question(cx));
        let mut content = div().flex().flex_col().gap_4();

        let query = self.feature_query.trim().to_string();
        if !query.is_empty() {
            let hits: Vec<&'static Feature> = applicable
                .iter()
                .copied()
                .filter(|f| self.feature_matches(f, &query))
                .collect();
            content = content.child(widgets::label(match hits.len() {
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
                content = content.child(widgets::subheading(tab_name));
                if let Some(card) = (f.render)(self, cx) {
                    content = content.child(card);
                }
            }
            return page.child(style::with_side_nav(nav, content));
        }

        let tab = self.game_tab;
        content = content.child(
            div().flex().flex_col().gap_1().child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(style::icon(tab.icon(), 18., pal::accent_light()))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tab.name(self)),
                    ),
            ),
        );
        if tab == GameTab::Overview {
            content = content
                .child(self.render_quick(cx))
                .child(self.render_summary(&entry, cx));
        }
        let mut in_tab: Vec<&'static Feature> =
            applicable.into_iter().filter(|f| f.tab == tab).collect();
        if tab == GameTab::Upscaling {
            // Sub-tabs at the top of the page, each with its own panels.
            let groups: Vec<usize> = (0..UPSCALE_GROUPS.len())
                .filter(|&g| in_tab.iter().any(|f| UPSCALE_GROUPS[g].3.contains(&f.id)))
                .collect();
            if !groups.is_empty() {
                let current = if groups.contains(&self.game_ui.upscale_group) {
                    self.game_ui.upscale_group
                } else {
                    groups[0]
                };
                let labels: Vec<(&'static str, &'static str)> = groups
                    .iter()
                    .map(|&g| {
                        (
                            UPSCALE_GROUPS[g].0,
                            self.tr(UPSCALE_GROUPS[g].1, UPSCALE_GROUPS[g].2),
                        )
                    })
                    .collect();
                let selected = groups.iter().position(|&g| g == current).unwrap_or(0);
                let map = groups.clone();
                content = content.child(style::tab_strip(
                    "upscale-group",
                    &labels,
                    selected,
                    cx,
                    move |this, i, cx| {
                        this.game_ui.upscale_group = map[i];
                        cx.notify();
                    },
                ));
                in_tab.retain(|f| UPSCALE_GROUPS[current].3.contains(&f.id));
            }
        }
        // Few panels: all open. Otherwise the ones that are on (and the first, if it has no
        // on/off state) start open.
        let all_open = in_tab.len() <= 2;
        for (ix, f) in in_tab.into_iter().enumerate() {
            let default_open = all_open || (ix == 0 && f.active.is_none());
            if let Some(panel) = self.feature_panel(f, default_open, cx) {
                content = content.child(panel);
            }
        }
        page.child(style::with_side_nav(nav, content))
    }
}
