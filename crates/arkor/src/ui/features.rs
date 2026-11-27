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
use arkor_core::tweaks::Category;
use gpui::prelude::FluentBuilder;
use gpui::*;

use super::style::{self, pal};
use super::{Library, widgets};

/// Areas of the game page, named after what the player wants (a smoother game, a nicer
/// picture, a game that starts), not after the tools that do it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum GameTab {
    #[default]
    Overview,
    Game,
    Smooth,
    Picture,
    Hud,
    Runtime,
    Launch,
    Mods,
    Help,
}

impl GameTab {
    pub(super) const ALL: [GameTab; 9] = [
        GameTab::Overview,
        GameTab::Game,
        GameTab::Smooth,
        GameTab::Picture,
        GameTab::Hud,
        GameTab::Runtime,
        GameTab::Launch,
        GameTab::Mods,
        GameTab::Help,
    ];

    pub(super) fn icon(self) -> &'static str {
        match self {
            GameTab::Overview => "layout-dashboard",
            GameTab::Game => "gamepad-2",
            GameTab::Smooth => "zap",
            GameTab::Picture => "palette",
            GameTab::Hud => "gauge",
            GameTab::Runtime => "wine",
            GameTab::Launch => "rocket",
            GameTab::Mods => "puzzle",
            GameTab::Help => "life-buoy",
        }
    }

    pub(super) fn name(self, lib: &Library) -> &'static str {
        match self {
            GameTab::Overview => lib.tr("Overview", "Genel bakış"),
            GameTab::Game => lib.tr("Game", "Oyun"),
            GameTab::Smooth => lib.tr("Smoother & faster", "Daha akıcı ve hızlı"),
            GameTab::Picture => lib.tr("Picture", "Görüntü"),
            GameTab::Hud => lib.tr("Performance overlay", "Performans göstergesi"),
            GameTab::Runtime => lib.tr("Compatibility", "Uyumluluk"),
            GameTab::Launch => lib.tr("Launch", "Başlatma"),
            GameTab::Mods => lib.tr("Mods", "Modlar"),
            GameTab::Help => lib.tr("Troubleshooting", "Sorun giderme"),
        }
    }

    /// One sentence under the area's title: what it is for.
    pub(super) fn desc(self, lib: &Library) -> &'static str {
        match self {
            GameTab::Overview => lib.tr(
                "The most used switches and how the game starts.",
                "En çok kullanılan anahtarlar ve oyunun nasıl başladığı.",
            ),
            GameTab::Game => lib.tr(
                "Its name, cover, files, saves and shortcuts.",
                "Adı, kapağı, dosyaları, kayıtları ve kısayolları.",
            ),
            GameTab::Smooth => lib.tr(
                "More FPS, smoother motion and quicker response. Pick what you want below.",
                "Daha fazla FPS, daha akıcı hareket ve daha çabuk tepki. Aşağıdan ne istediğini seç.",
            ),
            GameTab::Picture => lib.tr(
                "Sharpness, colours, resolution and HDR.",
                "Keskinlik, renkler, çözünürlük ve HDR.",
            ),
            GameTab::Hud => lib.tr(
                "Shows FPS, temperatures and usage on top of the game.",
                "Oyunun üstünde FPS, sıcaklık ve kullanım değerlerini gösterir.",
            ),
            GameTab::Runtime => lib.tr(
                "How a Windows game runs on Linux, and fixes for games that crash or will not start.",
                "Bir Windows oyununun Linux'ta nasıl çalıştığı; çöken ya da açılmayan oyunlar için düzeltmeler.",
            ),
            GameTab::Launch => lib.tr(
                "Extra options and commands when the game starts. For experienced users.",
                "Oyun başlarken ek seçenekler ve komutlar. Deneyimli kullanıcılar için.",
            ),
            GameTab::Mods => lib.tr(
                "Add, switch on and off and order mods.",
                "Mod ekle, aç-kapat ve sırala.",
            ),
            GameTab::Help => lib.tr(
                "Find out why the game did not start, and what happened at the last start.",
                "Oyunun neden açılmadığını ve son başlatmada ne olduğunu öğren.",
            ),
        }
    }

    /// `arkor open <id>#<tab>` names (older names kept).
    pub(super) fn from_name(name: &str) -> Option<GameTab> {
        Some(match name {
            "overview" | "general" => GameTab::Overview,
            "game" | "details" | "files" | "cover" => GameTab::Game,
            "launch" => GameTab::Launch,
            "runtime" | "compatibility" | "wine" | "proton" => GameTab::Runtime,
            "smooth" | "performance" | "upscaling" | "framegen" | "fps" => GameTab::Smooth,
            "picture" | "visuals" | "graphics" => GameTab::Picture,
            "overlay" | "mangohud" | "hud" => GameTab::Hud,
            "mods" | "modding" => GameTab::Mods,
            "help" | "logs" | "trouble" => GameTab::Help,
            _ => return None,
        })
    }
}

type Render = fn(&mut Library, &mut Context<Library>) -> Option<Div>;

pub(super) struct Feature {
    pub id: &'static str,
    pub tab: GameTab,
    /// The category of its area it is in (a key of `GROUPS`), or "" for areas without them.
    pub group: &'static str,
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
/// A category inside an area: what the player picks first, before the tools.
pub(super) struct Group {
    pub tab: GameTab,
    pub key: &'static str,
    pub icon: &'static str,
    pub name: (&'static str, &'static str),
    pub desc: (&'static str, &'static str),
}

pub(super) const GROUPS: &[Group] = &[
    Group {
        tab: GameTab::Smooth,
        key: "upscale",
        icon: "scan-eye",
        name: ("Upscaling: more FPS", "Ölçekleme: daha fazla FPS"),
        desc: (
            "The game draws at a lower resolution and a smart upscaler (DLSS, FSR, XeSS) makes it sharp again.",
            "Oyun daha düşük çözünürlükte çizer, akıllı bir ölçekleyici (DLSS, FSR, XeSS) görüntüyü yeniden netleştirir.",
        ),
    },
    Group {
        tab: GameTab::Smooth,
        key: "framegen",
        icon: "layers",
        name: (
            "Frame generation: smoother motion",
            "Kare üretimi: daha akıcı hareket",
        ),
        desc: (
            "Adds made-up frames between the real ones, so 60 FPS can look like 120. Works best when the game already runs well.",
            "Gerçek karelerin arasına üretilmiş kareler ekler; 60 FPS 120 gibi görünebilir. Oyun zaten iyi çalışıyorsa en iyi sonucu verir.",
        ),
    },
    Group {
        tab: GameTab::Smooth,
        key: "latency",
        icon: "mouse-pointer-click",
        name: ("Less input delay", "Daha az gecikme"),
        desc: (
            "The game reacts sooner to your mouse, keyboard and gamepad. Good for shooters and fighting games.",
            "Oyun fareye, klavyeye ve kola daha çabuk tepki verir. Nişancı ve dövüş oyunları için iyidir.",
        ),
    },
    Group {
        tab: GameTab::Smooth,
        key: "stutter",
        icon: "activity",
        name: ("Fewer stutters & power", "Takılma ve güç"),
        desc: (
            "Less hitching, a lower frame rate while you are away, and no sleep while you play.",
            "Daha az takılma, sen başında değilken daha düşük kare hızı ve oynarken uykuya geçmeme.",
        ),
    },
    Group {
        tab: GameTab::Picture,
        key: "effects",
        icon: "wand-sparkles",
        name: ("Sharpness & colours", "Keskinlik ve renkler"),
        desc: (
            "Make a blurry picture sharper, colours more vivid, or add ReShade effects.",
            "Bulanık görüntüyü keskinleştir, renkleri canlandır ya da ReShade efektleri ekle.",
        ),
    },
    Group {
        tab: GameTab::Picture,
        key: "screen",
        icon: "monitor",
        name: ("Resolution, window & HDR", "Çözünürlük, pencere ve HDR"),
        desc: (
            "Force a resolution or refresh rate, fullscreen, and HDR on HDR screens.",
            "Çözünürlüğü ya da yenileme hızını zorla, tam ekran yap, HDR ekranda HDR aç.",
        ),
    },
    Group {
        tab: GameTab::Picture,
        key: "layers",
        icon: "layers-2",
        name: ("Other layers", "Diğer katmanlar"),
        desc: (
            "Other picture add-ons installed on this computer.",
            "Bu bilgisayarda kurulu diğer görüntü eklentileri.",
        ),
    },
    Group {
        tab: GameTab::Runtime,
        key: "proton",
        icon: "wine",
        name: ("Proton & versions", "Proton ve sürümler"),
        desc: (
            "Proton lets Windows games run on Linux. Arkor picks one for you; change it only if the game has problems.",
            "Proton, Windows oyunlarının Linux'ta çalışmasını sağlar. Arkor senin için birini seçer; yalnızca oyunda sorun varsa değiştir.",
        ),
    },
    Group {
        tab: GameTab::Runtime,
        key: "fixes",
        icon: "bandage",
        name: ("Crash & start-up fixes", "Çökme ve açılma düzeltmeleri"),
        desc: (
            "Switches that help when a game crashes, freezes, shows a black screen or will not start.",
            "Oyun çöktüğünde, donduğunda, siyah ekran verdiğinde ya da açılmadığında yardımcı olan anahtarlar.",
        ),
    },
    Group {
        tab: GameTab::Runtime,
        key: "gpu",
        icon: "cpu",
        name: ("Graphics card", "Ekran kartı"),
        desc: (
            "Which card the game uses, and what it is told about it.",
            "Oyunun hangi kartı kullandığı ve kart hakkında ona ne söylendiği.",
        ),
    },
    Group {
        tab: GameTab::Runtime,
        key: "pads",
        icon: "gamepad-2",
        name: ("Gamepads", "Oyun kolları"),
        desc: (
            "Fixes for gamepads that are not found, have the wrong buttons or do not vibrate.",
            "Bulunmayan, tuşları yanlış görünen ya da titreşmeyen kollar için düzeltmeler.",
        ),
    },
    Group {
        tab: GameTab::Runtime,
        key: "tools",
        icon: "wrench",
        name: ("Tools & extras", "Araçlar ve ekler"),
        desc: (
            "Wine tools, Discord, recording and the Epic overlay.",
            "Wine araçları, Discord, kayıt ve Epic katmanı.",
        ),
    },
];

pub(super) fn group(key: &str) -> Option<&'static Group> {
    GROUPS.iter().find(|g| g.key == key)
}

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
        "anr" => (
            |l| l.tools.anr_downloaded.is_some() || l.tools.anr_manifest.is_some(),
            |l, cx| l.enqueue_download("anr", cx),
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
/// A game with a folder of its own to mod (not a ROM).
fn moddable(_: &Library, g: &LibraryGame) -> bool {
    !matches!(g.kind, GameKind::Emulated { .. })
}
fn steam_game(_: &Library, g: &LibraryGame) -> bool {
    matches!(g.kind, GameKind::Steam { .. })
}
/// Saves in a store's cloud, or copies of any game's saves (Ludusavi).
fn cloud_saves(lib: &Library, g: &LibraryGame) -> bool {
    // An emulator keeps an emulator game's saves.
    !matches!(g.kind, GameKind::Emulated { .. }) && (lib.saves_apply() || lib.saves.local.is_some())
}
fn has_other_layers(lib: &Library, _: &LibraryGame) -> bool {
    lib.has_other_layers()
}

pub(super) const FEATURES: &[Feature] = &[
    // Game
    Feature {
        id: "details",
        tab: GameTab::Game,
        group: "",
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
        group: "",
        icon: "file-terminal",
        name: ("Game files", "Oyun dosyaları"),
        desc: (
            "Which program starts the game, and the folder a Windows game keeps its settings in (its Wine prefix).",
            "Oyunu hangi programın başlattığı ve bir Windows oyununun ayarlarını tuttuğu klasör (Wine prefix'i).",
        ),
        keywords: "exe arguments argümanlar prefix wine protontricks app id",
        applies: always,
        active: None,
        render: |l, cx| l.render_exec_card(cx),
    },
    Feature {
        id: "saves",
        tab: GameTab::Game,
        group: "",
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
        id: "copies",
        tab: GameTab::Game,
        group: "",
        icon: "copy",
        name: ("Copies in other stores", "Diğer mağazalardaki kopyalar"),
        desc: (
            "The same game from another store, joined into this card.",
            "Başka bir mağazadan aynı oyun, bu kartta birleştirilmiş.",
        ),
        keywords: "copy copies kopya duplicate merge birleştir store mağaza epic gog steam",
        applies: |l, g| l.has_copies(&g.id),
        active: None,
        render: |l, cx| l.render_copies(cx),
    },
    Feature {
        id: "shortcuts",
        tab: GameTab::Game,
        group: "",
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
        group: "proton",
        icon: "wine",
        name: ("Proton version", "Proton sürümü"),
        desc: (
            "Proton runs Windows games on Linux. Automatic suits most games; try another version if this one has problems.",
            "Proton, Windows oyunlarını Linux'ta çalıştırır. Çoğu oyun için Otomatik yeterlidir; sorun olursa başka bir sürüm dene.",
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
        group: "proton",
        icon: "wine",
        name: ("Steam: Proton and launch", "Steam: Proton ve başlatma"),
        desc: (
            "Steam starts this game. Pick the Proton Steam uses, and let Arkor's settings apply when it does.",
            "Bu oyunu Steam başlatır. Steam'in kullanacağı Proton'u seç ve Steam başlatırken Arkor'un ayarlarının uygulanmasını sağla.",
        ),
        keywords: "proton steam compatibility uyumluluk launch options başlatma seçenekleri verify doğrula",
        applies: steam_game,
        active: Some(|l| l.profile.steam.compat_tool.is_some()),
        render: |l, cx| Some(l.render_steam(cx)),
    },
    Feature {
        id: "tweaks-upscaling",
        tab: GameTab::Smooth,
        group: "upscale",
        icon: "sparkles",
        name: (
            "Upgrade the game's own upscaler",
            "Oyunun kendi ölçekleyicisini yükselt",
        ),
        desc: (
            "Newer DLSS, FSR 4 or XeSS in games that ship an older one. One click each.",
            "Eski sürüm getiren oyunlarda daha yeni DLSS, FSR 4 ya da XeSS. Her biri tek tıkla.",
        ),
        keywords: "dlss fsr fsr4 xess upgrade yükselt ray reconstruction",
        applies: |l, g| l.tweaks_apply(Category::Upscaling, g),
        active: Some(|l| l.tweaks_on(Category::Upscaling) > 0),
        render: |l, cx| Some(l.render_tweak_category(Category::Upscaling, cx)),
    },
    Feature {
        id: "tweaks-framegen",
        tab: GameTab::Smooth,
        group: "framegen",
        icon: "fast-forward",
        name: ("Driver frame generation", "Sürücünün kare üretimi"),
        desc: (
            "Frame generation done by the graphics driver.",
            "Ekran kartı sürücüsünün yaptığı kare üretimi.",
        ),
        keywords: "smooth motion nvidia frame generation kare üretimi",
        applies: |l, g| l.tweaks_apply(Category::FrameGen, g),
        active: Some(|l| l.tweaks_on(Category::FrameGen) > 0),
        render: |l, cx| Some(l.render_tweak_category(Category::FrameGen, cx)),
    },
    Feature {
        id: "tweaks-latency",
        tab: GameTab::Smooth,
        group: "latency",
        icon: "mouse-pointer-click",
        name: ("Input delay switches", "Gecikme anahtarları"),
        desc: (
            "Reflex, Anti-Lag and low-latency modes.",
            "Reflex, Anti-Lag ve düşük gecikme modları.",
        ),
        keywords: "latency gecikme reflex antilag anti-lag low input lag",
        applies: |l, g| l.tweaks_apply(Category::Latency, g),
        active: Some(|l| l.tweaks_on(Category::Latency) > 0),
        render: |l, cx| Some(l.render_tweak_category(Category::Latency, cx)),
    },
    Feature {
        id: "tweaks-stutter",
        tab: GameTab::Smooth,
        group: "stutter",
        icon: "activity",
        name: ("Stutter switches", "Takılma anahtarları"),
        desc: (
            "Bigger shader caches and a faster processor while playing.",
            "Daha büyük shader önbelleği ve oynarken daha hızlı işlemci.",
        ),
        keywords: "shader cache önbellek stutter takılma cpu governor performance",
        applies: |l, g| l.tweaks_apply(Category::Stutter, g),
        active: Some(|l| l.tweaks_on(Category::Stutter) > 0),
        render: |l, cx| Some(l.render_tweak_category(Category::Stutter, cx)),
    },
    Feature {
        id: "tweaks-display",
        tab: GameTab::Picture,
        group: "screen",
        icon: "sun",
        name: ("HDR & window switches", "HDR ve pencere anahtarları"),
        desc: (
            "HDR, and drawing the window straight on Wayland.",
            "HDR ve pencereyi doğrudan Wayland üzerinde çizme.",
        ),
        keywords: "hdr wayland display ekran",
        applies: |l, g| l.tweaks_apply(Category::Display, g),
        active: Some(|l| l.tweaks_on(Category::Display) > 0),
        render: |l, cx| Some(l.render_tweak_category(Category::Display, cx)),
    },
    Feature {
        id: "tweaks-fixes",
        tab: GameTab::Runtime,
        group: "fixes",
        icon: "bandage",
        name: ("Fix switches", "Düzeltme anahtarları"),
        desc: (
            "Each one fixes a kind of crash, freeze or start-up problem. Try one at a time.",
            "Her biri bir tür çökme, donma ya da açılma sorununu düzeltir. Teker teker dene.",
        ),
        keywords: "ntsync wow64 wined3d large address aware laa heap crash çökme donma siyah ekran black screen steam deck zink vkd3d",
        applies: |l, g| l.tweaks_apply(Category::Fixes, g),
        active: Some(|l| l.tweaks_on(Category::Fixes) > 0),
        render: |l, cx| Some(l.render_tweak_category(Category::Fixes, cx)),
    },
    Feature {
        id: "tweaks-gpu",
        tab: GameTab::Runtime,
        group: "gpu",
        icon: "cpu",
        name: ("Graphics card switches", "Ekran kartı anahtarları"),
        desc: (
            "What the game is told about the graphics card, and driver options.",
            "Oyuna ekran kartı hakkında ne söylendiği ve sürücü seçenekleri.",
        ),
        keywords: "nvapi nvidia amd hide gizle spoof zluda radv ray tracing",
        applies: |l, g| l.tweaks_apply(Category::Gpu, g),
        active: Some(|l| l.tweaks_on(Category::Gpu) > 0),
        render: |l, cx| Some(l.render_tweak_category(Category::Gpu, cx)),
    },
    Feature {
        id: "tweaks-input",
        tab: GameTab::Runtime,
        group: "pads",
        icon: "gamepad-2",
        name: ("Gamepad switches", "Kol anahtarları"),
        desc: (
            "SDL, Xbox buttons, PlayStation gamepads.",
            "SDL, Xbox tuşları, PlayStation kolları.",
        ),
        keywords: "gamepad controller kol kumanda sdl xbox dualsense playstation sony haptics titreşim",
        applies: |l, g| l.tweaks_apply(Category::Input, g),
        active: Some(|l| l.tweaks_on(Category::Input) > 0),
        render: |l, cx| Some(l.render_tweak_category(Category::Input, cx)),
    },
    Feature {
        id: "tweaks-sharing",
        tab: GameTab::Runtime,
        group: "tools",
        icon: "share-2",
        name: ("Discord & recording", "Discord ve kayıt"),
        desc: (
            "Show your game on Discord, record it with OBS.",
            "Oyununu Discord'da göster, OBS ile kaydet.",
        ),
        keywords: "discord rich presence obs capture kayıt yayın stream",
        applies: |l, g| l.tweaks_apply(Category::Sharing, g),
        active: Some(|l| l.tweaks_on(Category::Sharing) > 0),
        render: |l, cx| Some(l.render_tweak_category(Category::Sharing, cx)),
    },
    Feature {
        id: "protonfixes",
        tab: GameTab::Runtime,
        group: "proton",
        icon: "wrench",
        name: ("Protonfixes", "Protonfix'ler"),
        desc: (
            "Ready-made fixes for known problem games, applied by themselves once the game is recognised.",
            "Bilinen sorunlu oyunlar için hazır düzeltmeler; oyun tanınınca kendiliğinden uygulanır.",
        ),
        keywords: "umu protonfix gameid fix düzeltme",
        applies: proton_choice,
        active: Some(|l| l.profile.umu_id.is_some()),
        render: |l, cx| Some(l.render_protonfix(cx)),
    },
    Feature {
        id: "eos-overlay",
        tab: GameTab::Runtime,
        group: "tools",
        icon: "users",
        name: ("Epic overlay", "Epic katmanı"),
        desc: (
            "Epic's friends, achievements and invites inside games that use Epic Online Services.",
            "Epic Online Services kullanan oyunlarda Epic'in arkadaşları, başarımları ve davetleri.",
        ),
        keywords: "eos overlay epic friends achievements arkadaş başarım katman",
        applies: |_, g| matches!(g.kind, GameKind::Epic { .. }),
        active: None,
        render: |l, cx| l.render_eos(cx),
    },
    Feature {
        id: "dxvk",
        tab: GameTab::Runtime,
        group: "proton",
        icon: "layers",
        name: ("DXVK & VKD3D-Proton version", "DXVK ve VKD3D-Proton sürümü"),
        desc: (
            "DXVK and VKD3D turn the game's DirectX graphics into Vulkan. Proton brings its own; pick another version only to fix a problem.",
            "DXVK ve VKD3D oyunun DirectX grafiklerini Vulkan'a çevirir. Proton kendi sürümünü getirir; yalnızca bir sorunu çözmek için başkasını seç.",
        ),
        keywords: "dxvk vkd3d directx d3d11 d3d12 dx11 dx12 version sürüm",
        applies: proton_choice,
        active: Some(|l| l.profile.dxvk.is_some() || l.profile.vkd3d.is_some()),
        render: |l, cx| Some(l.render_dxvk(cx)),
    },
    Feature {
        id: "gpu",
        tab: GameTab::Runtime,
        group: "gpu",
        icon: "cpu",
        name: ("Graphics card", "Ekran kartı"),
        desc: (
            "Which card the game renders on, on computers with more than one.",
            "Birden çok ekran kartı olan bilgisayarlarda oyunun hangisinde çalışacağı.",
        ),
        keywords: "gpu prime dri_prime optimus nvidia amd intel laptop dizüstü ekran kartı",
        applies: |l, _| l.tools.cards.len() > 1,
        active: Some(|l| l.profile.gpu.is_some()),
        render: |l, cx| Some(l.render_gpu_choice(cx)),
    },
    Feature {
        id: "linux-runtime",
        tab: GameTab::Runtime,
        group: "proton",
        icon: "box",
        name: ("Steam Linux Runtime", "Steam Linux Runtime"),
        desc: (
            "Runs a Linux game inside Steam's runtime, so it finds the libraries it was made for. Try it if a Linux game will not start.",
            "Linux oyununu Steam'in çalışma ortamında açar; oyun yapıldığı kütüphaneleri bulur. Bir Linux oyunu açılmıyorsa dene.",
        ),
        keywords: "sniper soldier scout runtime container",
        applies: linux_game,
        active: Some(|l| l.profile.linux_runtime.is_some()),
        render: |l, cx| Some(l.render_runtime(cx)),
    },
    Feature {
        id: "prefix-tools",
        tab: GameTab::Runtime,
        group: "tools",
        icon: "folder-cog",
        name: ("Wine prefix tools", "Wine prefix araçları"),
        desc: (
            "Each Windows game has a small Windows folder of its own (its prefix). Change its settings, add Windows parts, run an installer in it.",
            "Her Windows oyununun kendine ait küçük bir Windows klasörü (prefix) vardır. Ayarlarını değiştir, Windows parçaları ekle, içinde bir kurulum dosyası çalıştır.",
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
        group: "",
        icon: "terminal",
        name: (
            "Launch options and commands",
            "Başlatma seçenekleri ve komutlar",
        ),
        desc: (
            "Extra start options (such as -dx11), commands to run before and after, and a second program to start with the game.",
            "Ek başlatma seçenekleri (ör. -dx11), önce ve sonra çalışacak komutlar ve oyunla birlikte açılacak ikinci bir program.",
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
        group: "",
        icon: "variable",
        name: ("Environment variables", "Ortam değişkenleri"),
        desc: (
            "Advanced: settings handed to the game as NAME=VALUE. Change them only when a guide tells you to.",
            "Gelişmiş: oyuna AD=DEĞER olarak verilen ayarlar. Yalnızca bir rehber söylüyorsa değiştir.",
        ),
        keywords: "env ortam değişken variable key value",
        applies: always,
        active: Some(|l| !l.profile.env.is_empty()),
        render: |l, cx| Some(l.render_env_card(cx)),
    },
    // Performance
    Feature {
        id: "sleep",
        tab: GameTab::Smooth,
        group: "stutter",
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
        tab: GameTab::Picture,
        group: "screen",
        icon: "monitor",
        name: ("Gamescope", "Gamescope"),
        desc: (
            "Valve's game window, as on the Steam Deck: force a resolution, upscale, cap FPS, fullscreen and HDR.",
            "Steam Deck'teki gibi Valve'ın oyun penceresi: çözünürlüğü zorla, ölçekle, FPS'i sınırla, tam ekran ve HDR.",
        ),
        keywords: "gamescope resolution çözünürlük fsr nis scaling hdr refresh fullscreen tam ekran fps limit",
        applies: always,
        active: Some(|l| l.profile.gamescope.enabled),
        render: |l, cx| Some(l.render_gamescope(cx)),
    },
    Feature {
        id: "vknemu",
        tab: GameTab::Smooth,
        group: "stutter",
        icon: "moon",
        name: ("Idle FPS limit (vkNemu)", "Boşta FPS sınırı (vkNemu)"),
        desc: (
            "Lowers the frame rate while you are away or the game is in the background, to save power and heat.",
            "Sen başında değilken ya da oyun arka plandayken kare hızını düşürür; güç ve ısıdan tasarruf eder.",
        ),
        keywords: "vknemu idle boşta fps limit power güç",
        applies: always,
        active: Some(|l| l.profile.layers.vknemu.enabled),
        render: |l, cx| Some(l.render_vknemu(cx)),
    },
    // Smoother & faster
    Feature {
        id: "lsfg",
        tab: GameTab::Smooth,
        group: "framegen",
        icon: "layers",
        name: (
            "Lossless Scaling frame gen (lsfg-vk)",
            "Lossless Scaling kare üretimi (lsfg-vk)",
        ),
        desc: (
            "Doubles or triples the frame rate with generated frames. Needs the Lossless Scaling app, bought on Steam.",
            "Üretilmiş karelerle kare hızını ikiye ya da üçe katlar. Steam'den satın alınan Lossless Scaling uygulaması gerekir.",
        ),
        keywords: "lsfg lossless frame generation kare üretimi multiplier çarpan flow fps",
        applies: always,
        active: Some(|l| l.profile.lsfg.mode == LsfgMode::On),
        render: |l, cx| Some(l.render_lsfg(cx)),
    },
    Feature {
        id: "mako",
        tab: GameTab::Smooth,
        group: "framegen",
        icon: "layers-2",
        name: ("MAKO frame gen & scaling", "MAKO kare üretimi ve ölçekleme"),
        desc: (
            "Free frame generation and upscaling for Vulkan and DXVK games.",
            "Vulkan ve DXVK oyunları için ücretsiz kare üretimi ve ölçekleme.",
        ),
        keywords: "mako frame generation kare üretimi spatial scaling ölçekleme fps",
        applies: always,
        active: Some(|l| l.profile.layers.mako.enabled || l.profile.layers.mako.scaling),
        render: |l, cx| Some(l.render_mako(cx)),
    },
    Feature {
        id: "afmf",
        tab: GameTab::Smooth,
        group: "framegen",
        icon: "fast-forward",
        name: ("AFMF frame gen", "AFMF kare üretimi"),
        desc: (
            "AMD's frame generation (Fluid Motion Frames), for any game on a Radeon card.",
            "AMD'nin kare üretimi (Fluid Motion Frames); Radeon kartta her oyun için.",
        ),
        keywords: "afmf amd fluid motion frames kare üretimi fps",
        applies: amd_only,
        active: Some(|l| l.profile.layers.afmf.enabled),
        render: |l, cx| Some(l.render_afmf(cx)),
    },
    Feature {
        id: "anr",
        tab: GameTab::Smooth,
        group: "framegen",
        icon: "brain",
        name: (
            "ANR FG (AI frame generation)",
            "ANR FG (yapay zekâ kare üretimi)",
        ),
        desc: (
            "AI frame generation in place of DLSS frame generation, for DirectX 12 and Vulkan games on any graphics card.",
            "DirectX 12 ve Vulkan oyunlarında, her ekran kartında DLSS kare üretiminin yerine geçen yapay zekâ kare üretimi.",
        ),
        keywords: "anr apex neural rendering frame generation kare üretimi dlss-g dlssg fg ai yapay zeka",
        applies: wine_game,
        active: Some(|l| l.tools.anr_manifest.is_some()),
        render: |l, cx| Some(l.render_anr(cx)),
    },
    Feature {
        id: "optiscaler",
        tab: GameTab::Smooth,
        group: "upscale",
        icon: "scan-eye",
        name: ("OptiScaler", "OptiScaler"),
        desc: (
            "Lets a game that only has DLSS use FSR 4 or XeSS instead (and the other way round), and adds frame generation.",
            "Yalnızca DLSS'i olan bir oyunun FSR 4 ya da XeSS kullanmasını (ya da tersini) sağlar ve kare üretimi ekler.",
        ),
        keywords: "optiscaler dlss fsr fsr4 xess optipatcher edge upscaler ölçekleyici",
        applies: wine_game,
        active: Some(|l| l.tools.manifest.is_some()),
        render: |l, cx| Some(l.render_optiscaler(cx)),
    },
    Feature {
        id: "dlss-enabler",
        tab: GameTab::Smooth,
        group: "upscale",
        icon: "cpu",
        name: ("DLSS Enabler", "DLSS Enabler"),
        desc: (
            "Brings DLSS frame generation to AMD and Intel cards in games that offer DLSS.",
            "DLSS sunan oyunlarda AMD ve Intel kartlara DLSS kare üretimini getirir.",
        ),
        keywords: "dlss enabler frame generation amd intel",
        applies: non_nvidia_wine_game,
        active: Some(|l| l.tools.de_manifest.is_some()),
        render: |l, cx| Some(l.render_dlss_enabler(cx)),
    },
    Feature {
        id: "streamline",
        tab: GameTab::Smooth,
        group: "upscale",
        icon: "refresh-cw",
        name: ("Streamline / DLSS update", "Streamline / DLSS güncelleme"),
        desc: (
            "Swaps the game's old DLSS files for the newest ones, for a sharper picture.",
            "Oyunun eski DLSS dosyalarını en yenileriyle değiştirir; görüntü daha net olur.",
        ),
        keywords: "streamline nvidia dlss update güncelle reflex nvngx",
        applies: wine_game,
        active: Some(|l| l.tools.sl_manifest.is_some()),
        render: |l, cx| Some(l.render_streamline(cx)),
    },
    Feature {
        id: "amd-bundles",
        tab: GameTab::Smooth,
        group: "upscale",
        icon: "atom",
        name: ("HelixSR & DLSS-NR (AMD)", "HelixSR ve DLSS-NR (AMD)"),
        desc: (
            "AI upscalers for Radeon RX 7000 and 9000 cards: HelixSR and DLSS-NR.",
            "Radeon RX 7000 ve 9000 kartlar için yapay zekâ ölçekleyicileri: HelixSR ve DLSS-NR.",
        ),
        keywords: "helixsr dlss-nr dlssnr amd rdna fsr4 neural",
        applies: amd_wine_game,
        active: Some(|l| l.tools.dlssnr.is_some()),
        render: |l, cx| Some(l.render_amd_bundles(cx)),
    },
    // Performance overlay and picture
    Feature {
        id: "mangohud",
        tab: GameTab::Hud,
        group: "",
        icon: "gauge",
        name: ("MangoHud", "MangoHud"),
        desc: (
            "Shows FPS, temperatures and usage on top of the game.",
            "Oyunun üstünde FPS, sıcaklık ve kullanım değerlerini gösterir.",
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
        tab: GameTab::Picture,
        group: "effects",
        icon: "wand-sparkles",
        name: ("vkBasalt", "vkBasalt"),
        desc: (
            "Sharpens a blurry picture and adds effects such as smoother edges and colour filters.",
            "Bulanık görüntüyü keskinleştirir; kenar yumuşatma ve renk filtreleri gibi efektler ekler.",
        ),
        keywords: "vkbasalt cas sharpen keskinlik smaa fxaa lut reshade daltonize",
        applies: always,
        active: Some(|l| l.profile.vkbasalt.enabled),
        render: |l, cx| Some(l.render_vkbasalt(cx)),
    },
    Feature {
        id: "reshade",
        tab: GameTab::Picture,
        group: "effects",
        icon: "brush",
        name: ("ReShade", "ReShade"),
        desc: (
            "The well-known effects pack: hundreds of filters, installed into the game folder.",
            "Bilinen efekt paketi: oyun klasörüne kurulan yüzlerce filtre.",
        ),
        keywords: "reshade shader efekt effects",
        applies: wine_game,
        active: Some(|l| l.tools.rs_manifest.is_some()),
        render: |l, cx| Some(l.render_reshade(cx)),
    },
    Feature {
        id: "vksumi",
        tab: GameTab::Picture,
        group: "effects",
        icon: "contrast",
        name: ("Colour grading (vkSumi)", "Renk ayarı (vkSumi)"),
        desc: (
            "More vivid colours and stronger contrast.",
            "Daha canlı renkler ve daha güçlü kontrast.",
        ),
        keywords: "vksumi vibrance canlılık contrast kontrast colour renk",
        applies: always,
        active: Some(|l| l.profile.layers.vksumi),
        render: |l, cx| Some(l.render_vksumi(cx)),
    },
    Feature {
        id: "other-layers",
        tab: GameTab::Picture,
        group: "layers",
        icon: "layers-2",
        name: ("Other Vulkan layers", "Diğer Vulkan katmanları"),
        desc: (
            "Other picture add-ons found on this computer, such as OBS capture.",
            "Bu bilgisayarda bulunan diğer görüntü eklentileri, ör. OBS yakalama.",
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
        group: "",
        icon: "puzzle",
        name: ("Mods", "Modlar"),
        desc: (
            "Install, switch and order mods with Amethyst Mod Manager.",
            "Amethyst Mod Manager ile mod kur, aç-kapat ve sırala.",
        ),
        keywords: "mod mods nexus thunderstore fomod bepinex amethyst modlar eklenti",
        applies: moddable,
        active: None,
        render: |l, cx| l.render_mods(cx),
    },
    // Logs
    Feature {
        id: "trouble",
        tab: GameTab::Help,
        group: "",
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
        tab: GameTab::Help,
        group: "",
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
                self.tr("FPS counter (MangoHud)", "FPS göstergesi (MangoHud)"),
                "gauge",
                (feature("mangohud").active.unwrap())(self),
            ),
            "gamescope" => (
                self.tr("Game window (Gamescope)", "Oyun penceresi (Gamescope)"),
                "monitor",
                p.gamescope.enabled,
            ),
            "lsfg" => (
                self.tr("Frame generation (lsfg-vk)", "Kare üretimi (lsfg-vk)"),
                "layers",
                p.lsfg.mode == LsfgMode::On,
            ),
            "vkbasalt" => (
                self.tr("Sharpening (vkBasalt)", "Keskinleştirme (vkBasalt)"),
                "wand-sparkles",
                p.vkbasalt.enabled,
            ),
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
        self.game_ui.group = FEATURES
            .iter()
            .find(|f| f.id == feature_id)
            .map(|f| f.group)
            .filter(|g| !g.is_empty());
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
            GameKind::Emulated { system, .. } => rows.push((
                self.tr("Emulator", "Emülatör"),
                format!(
                    "{} · {}",
                    arkor_launch::emu::games::system_name(system),
                    self.emu_choice(entry)
                        .map(|c| c.label())
                        .unwrap_or_else(|| "?".into())
                ),
                GameTab::Game,
                "executable",
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
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(self.feature_name(f)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(pal::muted())
                                .child(self.tr(f.desc.0, f.desc.1)),
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
                    this.game_ui.group = None;
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
                let tab_name = match group(f.group) {
                    Some(g) => format!("{} › {}", f.tab.name(self), self.tr(g.name.0, g.name.1)),
                    None => f.tab.name(self).to_string(),
                };
                content = content.child(widgets::subheading(tab_name));
                if let Some(card) = (f.render)(self, cx) {
                    content = content.child(card);
                }
            }
            return page.child(style::with_side_nav(nav, content));
        }

        let tab = self.game_tab;
        let mut in_tab: Vec<&'static Feature> =
            applicable.into_iter().filter(|f| f.tab == tab).collect();
        // The area's categories that have something for this game.
        let groups: Vec<&'static Group> = GROUPS
            .iter()
            .filter(|g| g.tab == tab && in_tab.iter().any(|f| f.group == g.key))
            .collect();
        let current = self
            .game_ui
            .group
            .and_then(group)
            .filter(|g| groups.iter().any(|h| h.key == g.key))
            .or_else(|| (groups.len() == 1).then(|| groups[0]));
        let (icon, title, desc) = match current {
            Some(g) => (
                g.icon,
                self.tr(g.name.0, g.name.1),
                self.tr(g.desc.0, g.desc.1),
            ),
            None => (tab.icon(), tab.name(self), tab.desc(self)),
        };
        if current.is_some() && groups.len() > 1 {
            content = content.child(div().flex().child(
                style::back_link("group-back", tab.name(self)).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.game_ui.group = None;
                        cx.notify();
                    },
                )),
            ));
        }
        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(style::icon(icon, 18., pal::accent_light()))
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        ),
                )
                .child(div().text_sm().text_color(pal::muted()).child(desc)),
        );
        if tab == GameTab::Overview {
            content = content
                .child(self.render_quick(cx))
                .child(self.render_summary(&entry, cx))
                .children(self.render_about(cx));
        }
        match current {
            Some(g) => in_tab.retain(|f| f.group == g.key),
            None if !groups.is_empty() => {
                // The categories first; the tools come after a category is picked.
                let mut grid = div().flex().flex_wrap().gap_3();
                for (ix, g) in groups.iter().enumerate() {
                    let on = in_tab
                        .iter()
                        .filter(|f| f.group == g.key && f.active.is_some_and(|a| a(self)))
                        .count();
                    let key = g.key;
                    grid = grid.child(
                        style::card()
                            .id(("group-card", ix))
                            .w(px(320.))
                            .flex_grow(1.)
                            .p_4()
                            .flex()
                            .gap_3()
                            .cursor_pointer()
                            .hover(|s| s.border_color(pal::accent().opacity(0.5)))
                            .child(
                                div()
                                    .size(px(40.))
                                    .flex_none()
                                    .rounded(px(12.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(pal::accent().opacity(if on > 0 { 0.2 } else { 0.1 }))
                                    .child(style::icon(g.icon, 20., pal::accent_light())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(self.tr(g.name.0, g.name.1)),
                                            )
                                            .when(on > 0, |d| {
                                                d.child(style::tag(
                                                    format!("{on} {}", self.tr("on", "açık")),
                                                    pal::accent_light(),
                                                ))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(pal::muted())
                                            .child(self.tr(g.desc.0, g.desc.1)),
                                    ),
                            )
                            .child(style::icon("chevron-right", 16., pal::faint()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.game_ui.group = Some(key);
                                cx.notify();
                            })),
                    );
                }
                content = content.child(grid);
                in_tab.retain(|f| f.group.is_empty());
            }
            None => {}
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
