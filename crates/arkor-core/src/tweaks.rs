//! One-click compatibility and performance switches (Proton, DXVK/VKD3D, Mesa). Each is a set
//! of environment variables, or a wrapper command, enabled per game by id in `Profile::tweaks`.

/// What a switch is for, in the words of someone who wants that, not how it works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// A sharper picture from the game's own upscaler.
    Upscaling,
    /// Extra frames in between.
    FrameGen,
    /// Less delay between input and screen.
    Latency,
    /// HDR and how the window is drawn.
    Display,
    /// Fewer stutters.
    Stutter,
    /// For games that crash, freeze or do not start.
    Fixes,
    /// What the game sees of the graphics card.
    Gpu,
    /// Gamepads.
    Input,
    /// Discord and recording.
    Sharing,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Upscaling,
        Category::FrameGen,
        Category::Latency,
        Category::Display,
        Category::Stutter,
        Category::Fixes,
        Category::Gpu,
        Category::Input,
        Category::Sharing,
    ];

    /// (English, Turkish) name.
    pub fn name(self) -> (&'static str, &'static str) {
        match self {
            Category::Upscaling => ("Sharper upscaling", "Daha net ölçekleme"),
            Category::FrameGen => ("Frame generation", "Kare üretimi"),
            Category::Latency => ("Lower input delay", "Daha az gecikme"),
            Category::Display => ("HDR and window", "HDR ve pencere"),
            Category::Stutter => ("Fewer stutters", "Daha az takılma"),
            Category::Fixes => ("Crash and start-up fixes", "Çökme ve açılma düzeltmeleri"),
            Category::Gpu => ("Graphics card tricks", "Ekran kartı ayarları"),
            Category::Input => ("Gamepads", "Oyun kolları"),
            Category::Sharing => ("Discord and recording", "Discord ve kayıt"),
        }
    }
}

pub struct Tweak {
    pub id: &'static str,
    pub category: Category,
    pub env: &'static [(&'static str, &'static str)],
    /// Command put in front of the game instead of (or besides) environment variables.
    pub wrapper: Option<&'static str>,
    pub label_en: &'static str,
    pub label_tr: &'static str,
    /// Which Proton builds honour it, when not all do.
    pub needs: Option<&'static str>,
    /// What it does, in a sentence anyone understands.
    pub help_en: &'static str,
    pub help_tr: &'static str,
}

#[allow(clippy::too_many_arguments)]
const fn t(
    id: &'static str,
    category: Category,
    env: &'static [(&'static str, &'static str)],
    label_en: &'static str,
    label_tr: &'static str,
    needs: Option<&'static str>,
    help_en: &'static str,
    help_tr: &'static str,
) -> Tweak {
    Tweak {
        id,
        category,
        env,
        wrapper: None,
        label_en,
        label_tr,
        needs,
        help_en,
        help_tr,
    }
}

use Category::*;

pub const TWEAKS: &[Tweak] = &[
    t(
        "dlss_upgrade",
        Upscaling,
        &[("PROTON_DLSS_UPGRADE", "1")],
        "Upgrade DLSS to the latest version",
        "DLSS'i son sürüme yükselt",
        None,
        "Games with an old DLSS use the newest one: a sharper picture and often more FPS. NVIDIA cards.",
        "Eski DLSS'li oyunlar en yenisini kullanır: daha net görüntü, çoğu zaman daha fazla FPS. NVIDIA kartlar.",
    ),
    t(
        "fsr4_upgrade",
        Upscaling,
        &[("PROTON_FSR4_UPGRADE", "1")],
        "Upgrade FSR to FSR 4",
        "FSR'yi FSR 4'e yükselt",
        None,
        "Games with FSR 3.1 use FSR 4 instead, for a much cleaner picture. Radeon RX 9000 cards (RX 7000 with newer Proton builds).",
        "FSR 3.1'li oyunlar onun yerine FSR 4 kullanır; görüntü çok daha temiz olur. Radeon RX 9000 kartlar (yeni Proton sürümleriyle RX 7000).",
    ),
    t(
        "xess_upgrade",
        Upscaling,
        &[("PROTON_XESS_UPGRADE", "1")],
        "Upgrade XeSS to the latest version",
        "XeSS'i son sürüme yükselt",
        None,
        "Games with XeSS use its newest version, for a cleaner picture.",
        "XeSS'li oyunlar en yeni sürümünü kullanır; görüntü daha temiz olur.",
    ),
    t(
        "dlss_rr",
        Upscaling,
        &[("NGX_DLSS_RR_OVERRIDE", "on")],
        "Force DLSS Ray Reconstruction",
        "DLSS Ray Reconstruction'ı zorla",
        Some("NVIDIA"),
        "Turns on Ray Reconstruction, which cleans up ray-traced light and reflections, even where the game hides the option.",
        "Işın izlemeli ışık ve yansımaları temizleyen Ray Reconstruction'ı, oyun seçeneği gizlese bile açar.",
    ),
    t(
        "hdr",
        Display,
        &[
            ("PROTON_ENABLE_HDR", "1"),
            ("DXVK_HDR", "1"),
            ("ENABLE_HDR_WSI", "1"),
        ],
        "Enable HDR",
        "HDR'yi aç",
        None,
        "Lets the game show HDR on an HDR screen. HDR must be on in your desktop's display settings too.",
        "HDR ekranda oyunun HDR göstermesini sağlar. Masaüstünün ekran ayarlarında da HDR açık olmalı.",
    ),
    t(
        "wayland",
        Display,
        &[("PROTON_ENABLE_WAYLAND", "1")],
        "Wine Wayland driver (no XWayland)",
        "Wine Wayland sürücüsü (XWayland'siz)",
        None,
        "Draws the game's window straight on Wayland. Can be smoother and look sharper on scaled screens; turn off if the game misbehaves.",
        "Oyunun penceresini doğrudan Wayland üzerinde çizer. Daha akıcı olabilir, ölçekli ekranlarda daha net görünür; oyun sorun çıkarırsa kapat.",
    ),
    t(
        "vkd3d_lowlatency",
        Latency,
        &[
            ("PROTON_VKD3D_LOWLATENCY", "1"),
            ("PROTON_DXVK_LOWLATENCY", "1"),
        ],
        "Low-latency frame pacing (DX8–DX12)",
        "Düşük gecikmeli kare zamanlaması (DX8–DX12)",
        Some("proton-cachyos"),
        "The game reacts sooner to your mouse and keys, because fewer frames wait in line.",
        "Sırada daha az kare beklediği için oyun fareye ve tuşlara daha çabuk tepki verir.",
    ),
    t(
        "low_latency_layer",
        Latency,
        &[("LOW_LATENCY_LAYER", "1")],
        "Low-latency layer",
        "Düşük gecikme katmanı",
        Some("proton-cachyos"),
        "Cuts the delay between pressing a key and seeing it on screen, like NVIDIA Reflex, on any graphics card.",
        "Tuşa basmakla ekranda görmek arasındaki gecikmeyi azaltır; NVIDIA Reflex gibi, her ekran kartında.",
    ),
    t(
        "reflex",
        Latency,
        &[
            ("LOW_LATENCY_LAYER", "1"),
            ("LOW_LATENCY_LAYER_REFLEX", "1"),
        ],
        "Expose NVIDIA Reflex",
        "NVIDIA Reflex'i aç",
        Some("proton-cachyos"),
        "Makes the Reflex option inside games work on any graphics card.",
        "Oyunların içindeki Reflex seçeneğini her ekran kartında çalışır hale getirir.",
    ),
    t(
        "antilag",
        Latency,
        &[("ENABLE_LAYER_MESA_ANTI_LAG", "1")],
        "AMD Anti-Lag 2",
        "AMD Anti-Lag 2",
        Some("Mesa"),
        "AMD's delay reducer: the game reacts sooner to your mouse and keys.",
        "AMD'nin gecikme azaltıcısı: oyun fareye ve tuşlara daha çabuk tepki verir.",
    ),
    t(
        "no_ntsync",
        Fixes,
        &[("PROTON_NO_NTSYNC", "1")],
        "Disable NTSYNC (for freezes and crashes)",
        "NTSYNC'i kapat (donma ve çökmeler için)",
        None,
        "Turns off a newer Wine speed-up. Only try it if the game freezes or crashes.",
        "Wine'ın yeni bir hızlandırmasını kapatır. Yalnızca oyun donuyor ya da çöküyorsa dene.",
    ),
    t(
        "wow64",
        Fixes,
        &[("PROTON_USE_WOW64", "1")],
        "WOW64 (32-bit games on the 64-bit driver)",
        "WOW64 (32-bit oyunları 64-bit sürücüyle çalıştır)",
        None,
        "Runs 32-bit Windows games a newer way that needs no 32-bit system libraries. Try it if an old game will not start.",
        "32-bit Windows oyunlarını 32-bit sistem kütüphanesi gerektirmeyen yeni bir yolla çalıştırır. Eski bir oyun açılmıyorsa dene.",
    ),
    t(
        "wined3d",
        Fixes,
        &[("PROTON_USE_WINED3D", "1")],
        "Use WineD3D instead of DXVK",
        "DXVK yerine WineD3D kullan",
        None,
        "Uses Wine's older graphics translator instead of DXVK. For very old games with a black screen or wrong colours.",
        "DXVK yerine Wine'ın eski grafik çeviricisini kullanır. Siyah ekran ya da yanlış renk veren çok eski oyunlar için.",
    ),
    t(
        "laa",
        Fixes,
        &[("PROTON_FORCE_LARGE_ADDRESS_AWARE", "1")],
        "Let 32-bit games use more than 2 GB RAM",
        "32-bit oyunların 2 GB'den fazla RAM kullanmasına izin ver",
        None,
        "Lets old 32-bit games use more than 2 GB of memory. Fixes crashes in big or heavily modded old games.",
        "Eski 32-bit oyunların 2 GB'den fazla bellek kullanmasına izin verir. Büyük ya da çok modlu eski oyunlardaki çökmeleri düzeltir.",
    ),
    t(
        "heap_delay_free",
        Fixes,
        &[("PROTON_HEAP_DELAY_FREE", "1")],
        "Delay heap frees (fixes some crashes)",
        "Bellek boşaltmayı geciktir (bazı çökmeleri düzeltir)",
        None,
        "Fixes crashes in a few games that let go of memory too early.",
        "Belleği erken bırakan birkaç oyundaki çökmeleri düzeltir.",
    ),
    t(
        "smooth_motion",
        FrameGen,
        &[("NVPRESENT_ENABLE_SMOOTH_MOTION", "1")],
        "NVIDIA Smooth Motion (frame generation by the driver)",
        "NVIDIA Smooth Motion (sürücünün kare üretimi)",
        Some("NVIDIA driver 570+, RTX 40/50"),
        "NVIDIA's driver adds frames in between, so motion looks smoother. RTX 40 and 50 cards.",
        "NVIDIA sürücüsü araya kare ekler, hareket daha akıcı görünür. RTX 40 ve 50 kartlar.",
    ),
    t(
        "big_shader_cache",
        Stutter,
        &[
            ("MESA_SHADER_CACHE_MAX_SIZE", "10G"),
            ("__GL_SHADER_DISK_CACHE_SIZE", "10737418240"),
            ("__GL_SHADER_DISK_CACHE_SKIP_CLEANUP", "1"),
        ],
        "10 GB shader cache (fewer stutters after updates)",
        "10 GB shader önbelleği (güncellemeden sonra daha az takılma)",
        None,
        "Keeps more of the game's prepared graphics on disk, so there is less stutter after the first play and after driver updates.",
        "Oyunun hazırlanmış grafiklerinden daha fazlasını diskte tutar; ilk oynayıştan ve sürücü güncellemelerinden sonra daha az takılır.",
    ),
    t(
        "shader_cache",
        Stutter,
        &[("PROTON_LOCAL_SHADER_CACHE", "1")],
        "Per-game shader cache",
        "Oyuna özel shader önbelleği",
        Some("proton-cachyos"),
        "Keeps this game's prepared graphics in its own folder, so they are not thrown away with other games'.",
        "Bu oyunun hazırlanmış grafiklerini kendi klasöründe tutar; diğer oyunlarınkiyle birlikte silinmez.",
    ),
    t(
        "descriptor_heap",
        Fixes,
        &[("VKD3D_CONFIG", "descriptor_heap")],
        "Different DirectX 12 resource handling (VKD3D)",
        "DirectX 12 için farklı kaynak yönetimi (VKD3D)",
        None,
        "Another way of handling DirectX 12 resources. Only try it if a DirectX 12 game crashes or looks wrong.",
        "DirectX 12 kaynaklarını başka bir yolla yönetir. Yalnızca bir DirectX 12 oyunu çöküyor ya da yanlış görünüyorsa dene.",
    ),
    t(
        "rtas_mult",
        Fixes,
        &[("VKD3D_RTAS_SIZE_MULT", "8")],
        "Bigger RT acceleration buffers (RT out-of-memory crashes)",
        "Daha büyük RT tamponları (RT bellek çökmeleri)",
        None,
        "Gives ray tracing more memory. Fixes \"out of memory\" crashes with ray tracing on.",
        "Işın izlemeye daha fazla bellek verir. Işın izleme açıkken çıkan \"bellek yetersiz\" çökmelerini düzeltir.",
    ),
    t(
        "steam_deck",
        Fixes,
        &[("SteamDeck", "1")],
        "Pretend to be a Steam Deck",
        "Steam Deck gibi görün",
        None,
        "Tells the game it runs on a Steam Deck. Some games then choose lighter settings or show gamepad buttons.",
        "Oyuna Steam Deck'te çalıştığını söyler. Bazı oyunlar o zaman daha hafif ayarlar seçer ya da kol tuşlarını gösterir.",
    ),
    t(
        "nvapi",
        Gpu,
        &[("PROTON_ENABLE_NVAPI", "1")],
        "Force-enable NVAPI",
        "NVAPI'yi zorla aç",
        Some("NVIDIA"),
        "Lets the game see NVIDIA features such as DLSS and Reflex when Proton hides them.",
        "Proton gizlediğinde oyunun DLSS ve Reflex gibi NVIDIA özelliklerini görmesini sağlar.",
    ),
    t(
        "hide_nvidia",
        Gpu,
        &[("PROTON_HIDE_NVIDIA_GPU", "1")],
        "Hide the NVIDIA GPU from the game",
        "NVIDIA GPU'yu oyundan gizle",
        None,
        "Hides the NVIDIA card from the game. For games that crash when they see it.",
        "NVIDIA kartı oyundan gizler. Onu görünce çöken oyunlar için.",
    ),
    t(
        "hide_amd",
        Gpu,
        &[("DXVK_CONFIG", "dxgi.hideAmdGpu = True")],
        "Hide the AMD GPU from the game",
        "AMD GPU'yu oyundan gizle",
        None,
        "Hides that the card is AMD. For games that turn features off on AMD cards.",
        "Kartın AMD olduğunu gizler. AMD kartlarda bazı özellikleri kapatan oyunlar için.",
    ),
    t(
        "spoof_nvidia",
        Gpu,
        &[
            ("LOW_LATENCY_LAYER", "1"),
            ("LOW_LATENCY_LAYER_SPOOF_NVIDIA", "1"),
        ],
        "Report the GPU as NVIDIA",
        "GPU'yu NVIDIA olarak göster",
        Some("proton-cachyos"),
        "Makes the game think it has an NVIDIA card, so it offers DLSS and Reflex (useful with OptiScaler).",
        "Oyunun NVIDIA kartı olduğunu sanmasını sağlar; böylece DLSS ve Reflex seçeneklerini sunar (OptiScaler ile işe yarar).",
    ),
    t(
        "zink",
        Fixes,
        &[("MESA_LOADER_DRIVER_OVERRIDE", "zink")],
        "OpenGL over Vulkan (Zink)",
        "OpenGL'i Vulkan üzerinden çalıştır (Zink)",
        Some("Mesa"),
        "Runs OpenGL games through Vulkan. Can fix glitches or speed up old OpenGL games.",
        "OpenGL oyunlarını Vulkan üzerinden çalıştırır. Eski OpenGL oyunlarındaki bozulmaları düzeltebilir ya da onları hızlandırabilir.",
    ),
    t(
        "radv_rt",
        Gpu,
        &[("RADV_PERFTEST", "rt,emulate_rt")],
        "Emulate ray tracing on older AMD GPUs",
        "Eski AMD GPU'larda ışın izlemeyi taklit et",
        Some("Mesa"),
        "Lets older AMD cards (RX 5000 and earlier) start games that require ray tracing. It is slow.",
        "Eski AMD kartların (RX 5000 ve öncesi) ışın izleme isteyen oyunları açmasını sağlar. Yavaştır.",
    ),
    t(
        "radv_nofastclears",
        Gpu,
        &[("RADV_DEBUG", "nofastclears")],
        "Disable AMD fast clears (flicker fix)",
        "AMD hızlı temizlemeyi kapat (titreme düzeltmesi)",
        Some("Mesa"),
        "Fixes flickering or garbage on screen in a few games on AMD cards.",
        "AMD kartlarda birkaç oyundaki titremeyi ya da ekrandaki bozuklukları düzeltir.",
    ),
    t(
        "staging_shm",
        Gpu,
        &[("STAGING_SHARED_MEMORY", "1")],
        "AMD shared-memory speed-up",
        "AMD paylaşımlı bellek hızlandırması",
        None,
        "A memory-sharing speed-up for AMD cards. Turn it off if the game crashes.",
        "AMD kartlar için bellek paylaşım hızlandırması. Oyun çökerse kapat.",
    ),
    t(
        "zluda",
        Gpu,
        &[("PROTON_ZLUDA", "1")],
        "ZLUDA (CUDA on AMD)",
        "ZLUDA (AMD'de CUDA)",
        Some("proton-cachyos"),
        "Lets parts made for NVIDIA's CUDA run on AMD cards (for example PhysX effects in some games).",
        "NVIDIA'nın CUDA'sı için yapılmış parçaların AMD kartlarda çalışmasını sağlar (ör. bazı oyunlardaki PhysX efektleri).",
    ),
    t(
        "sdl",
        Input,
        &[("PROTON_USE_SDL", "1"), ("PROTON_PREFER_SDL", "1")],
        "SDL input instead of Steam Input (gamepad fixes)",
        "Steam Input yerine SDL girişi (kumanda düzeltmeleri)",
        None,
        "Reads gamepads directly instead of through Steam Input. Fixes gamepads that are not found or are mixed up.",
        "Kolları Steam Input yerine doğrudan okur. Bulunmayan ya da karışan kolları düzeltir.",
    ),
    t(
        "xbox_prompts",
        Input,
        &[("PROTON_STEAM_INPUT_XBOX_PROMPTS", "1")],
        "Xbox button prompts",
        "Xbox tuş simgeleri",
        None,
        "Shows Xbox buttons in games, whichever gamepad you use.",
        "Hangi kolu kullanırsan kullan oyunlarda Xbox tuşlarını gösterir.",
    ),
    t(
        "dualsense_haptics",
        Input,
        &[("PROTON_DUALSENSE_HAPTICS_PREFER_NON_EVENT", "1")],
        "DualSense haptics fix",
        "DualSense titreşim düzeltmesi",
        None,
        "Fixes PlayStation 5 DualSense vibration in games that use it.",
        "Kullanan oyunlarda PlayStation 5 DualSense titreşimini düzeltir.",
    ),
    t(
        "sony_hidraw",
        Input,
        &[("PROTON_SONY_HIDRAW_XINPUT", "1")],
        "Sony controllers through hidraw",
        "Sony kumandalarını hidraw üzerinden kullan",
        None,
        "Lets games talk to PlayStation gamepads directly, so the light bar, touchpad and motion work.",
        "Oyunların PlayStation kollarıyla doğrudan konuşmasını sağlar; ışık, dokunmatik yüzey ve hareket algılama çalışır.",
    ),
    t(
        "sony_names",
        Input,
        &[("PROTON_SONY_WINDOWS_DEVICE_NAMES", "1")],
        "Windows names for Sony controllers",
        "Sony kumandaları için Windows adları",
        None,
        "Shows PlayStation gamepads to games under their Windows names, so games show PlayStation buttons.",
        "PlayStation kollarını oyunlara Windows'taki adlarıyla gösterir; oyunlar PlayStation tuşlarını gösterir.",
    ),
    t(
        "no_sony_xinput",
        Input,
        &[("PROTON_SONY_AUTO_XINPUT", "0")],
        "Don't map Sony controllers to XInput",
        "Sony kumandalarını XInput'a eşleme",
        None,
        "Stops presenting PlayStation gamepads as Xbox ones. For games with their own PlayStation support.",
        "PlayStation kollarını Xbox kolu gibi göstermeyi bırakır. Kendi PlayStation desteği olan oyunlar için.",
    ),
    t(
        "discord_bridge",
        Sharing,
        &[("PROTON_DISCORD_BRIDGE", "1")],
        "Discord bridge (game's own Rich Presence)",
        "Discord köprüsü (oyunun kendi Rich Presence'ı)",
        Some("proton-cachyos"),
        "Shows what you are playing on Discord, with the game's own details.",
        "Discord'da ne oynadığını oyunun kendi ayrıntılarıyla gösterir.",
    ),
    t(
        "obs_capture",
        Sharing,
        &[("OBS_VKCAPTURE", "1")],
        "OBS Vulkan game capture",
        "OBS Vulkan oyun yakalama",
        Some("obs-vkcapture"),
        "Lets OBS record or stream the game directly (needs obs-vkcapture).",
        "OBS'in oyunu doğrudan kaydetmesini ya da yayınlamasını sağlar (obs-vkcapture gerekir).",
    ),
    Tweak {
        id: "game_performance",
        category: Stutter,
        env: &[],
        wrapper: Some("game-performance"),
        label_en: "CPU performance governor while playing",
        label_tr: "Oynarken CPU'yu performans moduna al",
        needs: Some("CachyOS"),
        help_en: "Runs the processor at full speed while you play and sets it back afterwards.",
        help_tr: "Sen oynarken işlemciyi tam hızda çalıştırır, sonra eski haline getirir.",
    },
];

pub fn find(id: &str) -> Option<&'static Tweak> {
    TWEAKS.iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_every_category_used() {
        let mut ids: Vec<_> = TWEAKS.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), TWEAKS.len());
        for c in Category::ALL {
            assert!(TWEAKS.iter().any(|t| t.category == c), "{c:?} unused");
        }
        for t in TWEAKS {
            assert!(
                !t.help_en.is_empty() && !t.help_tr.is_empty(),
                "{} has no help",
                t.id
            );
        }
    }
}

/// GPU maker a switch is only useful with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuNeed {
    Amd,
    Nvidia,
}

/// Switches that do nothing without a given GPU maker (hidden when there is none).
pub fn gpu_need(id: &str) -> Option<GpuNeed> {
    match id {
        "antilag" | "radv_rt" | "radv_nofastclears" | "hide_amd" | "staging_shm" | "zluda" => {
            Some(GpuNeed::Amd)
        }
        "dlss_rr" | "nvapi" | "hide_nvidia" | "smooth_motion" => Some(GpuNeed::Nvidia),
        _ => None,
    }
}
