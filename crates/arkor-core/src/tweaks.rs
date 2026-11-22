//! One-click compatibility and performance switches (Proton, DXVK/VKD3D, Mesa). Each is a set
//! of environment variables, or a wrapper command, enabled per game by id in `Profile::tweaks`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Upscaling,
    Display,
    Latency,
    Compatibility,
    Gpu,
    Input,
    Other,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Upscaling,
        Category::Display,
        Category::Latency,
        Category::Compatibility,
        Category::Gpu,
        Category::Input,
        Category::Other,
    ];
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
}

const fn t(
    id: &'static str,
    category: Category,
    env: &'static [(&'static str, &'static str)],
    label_en: &'static str,
    label_tr: &'static str,
    needs: Option<&'static str>,
) -> Tweak {
    Tweak {
        id,
        category,
        env,
        wrapper: None,
        label_en,
        label_tr,
        needs,
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
    ),
    t(
        "fsr4_upgrade",
        Upscaling,
        &[("PROTON_FSR4_UPGRADE", "1")],
        "Upgrade FSR to FSR 4",
        "FSR'yi FSR 4'e yükselt",
        None,
    ),
    t(
        "xess_upgrade",
        Upscaling,
        &[("PROTON_XESS_UPGRADE", "1")],
        "Upgrade XeSS to the latest version",
        "XeSS'i son sürüme yükselt",
        None,
    ),
    t(
        "dlss_rr",
        Upscaling,
        &[("NGX_DLSS_RR_OVERRIDE", "on")],
        "Force DLSS Ray Reconstruction",
        "DLSS Ray Reconstruction'ı zorla",
        Some("NVIDIA"),
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
    ),
    t(
        "wayland",
        Display,
        &[("PROTON_ENABLE_WAYLAND", "1")],
        "Wine Wayland driver (no XWayland)",
        "Wine Wayland sürücüsü (XWayland'siz)",
        None,
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
    ),
    t(
        "low_latency_layer",
        Latency,
        &[("LOW_LATENCY_LAYER", "1")],
        "Low-latency layer",
        "Düşük gecikme katmanı",
        Some("proton-cachyos"),
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
    ),
    t(
        "antilag",
        Latency,
        &[("ENABLE_LAYER_MESA_ANTI_LAG", "1")],
        "AMD Anti-Lag 2",
        "AMD Anti-Lag 2",
        Some("Mesa"),
    ),
    t(
        "no_ntsync",
        Compatibility,
        &[("PROTON_NO_NTSYNC", "1")],
        "Disable NTSYNC",
        "NTSYNC'i kapat",
        None,
    ),
    t(
        "wow64",
        Compatibility,
        &[("PROTON_USE_WOW64", "1")],
        "WOW64 (32-bit games on the 64-bit driver)",
        "WOW64 (32-bit oyunları 64-bit sürücüyle çalıştır)",
        None,
    ),
    t(
        "wined3d",
        Compatibility,
        &[("PROTON_USE_WINED3D", "1")],
        "Use WineD3D instead of DXVK",
        "DXVK yerine WineD3D kullan",
        None,
    ),
    t(
        "laa",
        Compatibility,
        &[("PROTON_FORCE_LARGE_ADDRESS_AWARE", "1")],
        "Let 32-bit games use more than 2 GB RAM",
        "32-bit oyunların 2 GB'den fazla RAM kullanmasına izin ver",
        None,
    ),
    t(
        "heap_delay_free",
        Compatibility,
        &[("PROTON_HEAP_DELAY_FREE", "1")],
        "Delay heap frees (fixes some crashes)",
        "Heap serbest bırakmayı geciktir (bazı çökmeleri düzeltir)",
        None,
    ),
    t(
        "shader_cache",
        Compatibility,
        &[("PROTON_LOCAL_SHADER_CACHE", "1")],
        "Per-game shader cache",
        "Oyuna özel shader önbelleği",
        Some("proton-cachyos"),
    ),
    t(
        "descriptor_heap",
        Compatibility,
        &[("VKD3D_CONFIG", "descriptor_heap")],
        "VKD3D alternative descriptor heap",
        "VKD3D alternatif descriptor heap",
        None,
    ),
    t(
        "rtas_mult",
        Compatibility,
        &[("VKD3D_RTAS_SIZE_MULT", "8")],
        "Bigger RT acceleration buffers (RT out-of-memory crashes)",
        "Daha büyük RT tamponları (RT bellek çökmeleri)",
        None,
    ),
    t(
        "steam_deck",
        Compatibility,
        &[("SteamDeck", "1")],
        "Pretend to be a Steam Deck",
        "Steam Deck gibi görün",
        None,
    ),
    t(
        "nvapi",
        Gpu,
        &[("PROTON_ENABLE_NVAPI", "1")],
        "Force-enable NVAPI",
        "NVAPI'yi zorla aç",
        Some("NVIDIA"),
    ),
    t(
        "hide_nvidia",
        Gpu,
        &[("PROTON_HIDE_NVIDIA_GPU", "1")],
        "Hide the NVIDIA GPU from the game",
        "NVIDIA GPU'yu oyundan gizle",
        None,
    ),
    t(
        "hide_amd",
        Gpu,
        &[("DXVK_CONFIG", "dxgi.hideAmdGpu = True")],
        "Hide the AMD GPU from the game",
        "AMD GPU'yu oyundan gizle",
        None,
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
    ),
    t(
        "zink",
        Gpu,
        &[("MESA_LOADER_DRIVER_OVERRIDE", "zink")],
        "OpenGL over Vulkan (Zink)",
        "OpenGL'i Vulkan üzerinden çalıştır (Zink)",
        Some("Mesa"),
    ),
    t(
        "radv_rt",
        Gpu,
        &[("RADV_PERFTEST", "rt,emulate_rt")],
        "Emulate ray tracing on older AMD GPUs",
        "Eski AMD GPU'larda ışın izlemeyi taklit et",
        Some("Mesa"),
    ),
    t(
        "radv_nofastclears",
        Gpu,
        &[("RADV_DEBUG", "nofastclears")],
        "Disable AMD fast clears",
        "AMD fast clear'ı kapat",
        Some("Mesa"),
    ),
    t(
        "staging_shm",
        Gpu,
        &[("STAGING_SHARED_MEMORY", "1")],
        "Staging shared memory (AMD)",
        "Staging paylaşımlı bellek (AMD)",
        None,
    ),
    t(
        "zluda",
        Gpu,
        &[("PROTON_ZLUDA", "1")],
        "ZLUDA (CUDA on AMD)",
        "ZLUDA (AMD'de CUDA)",
        Some("proton-cachyos"),
    ),
    t(
        "sdl",
        Input,
        &[("PROTON_USE_SDL", "1"), ("PROTON_PREFER_SDL", "1")],
        "SDL input instead of Steam Input (gamepad fixes)",
        "Steam Input yerine SDL girişi (kumanda düzeltmeleri)",
        None,
    ),
    t(
        "xbox_prompts",
        Input,
        &[("PROTON_STEAM_INPUT_XBOX_PROMPTS", "1")],
        "Xbox button prompts",
        "Xbox tuş simgeleri",
        None,
    ),
    t(
        "dualsense_haptics",
        Input,
        &[("PROTON_DUALSENSE_HAPTICS_PREFER_NON_EVENT", "1")],
        "DualSense haptics fix",
        "DualSense titreşim düzeltmesi",
        None,
    ),
    t(
        "sony_hidraw",
        Input,
        &[("PROTON_SONY_HIDRAW_XINPUT", "1")],
        "Sony controllers through hidraw",
        "Sony kumandalarını hidraw üzerinden kullan",
        None,
    ),
    t(
        "sony_names",
        Input,
        &[("PROTON_SONY_WINDOWS_DEVICE_NAMES", "1")],
        "Windows names for Sony controllers",
        "Sony kumandaları için Windows adları",
        None,
    ),
    t(
        "no_sony_xinput",
        Input,
        &[("PROTON_SONY_AUTO_XINPUT", "0")],
        "Don't map Sony controllers to XInput",
        "Sony kumandalarını XInput'a eşleme",
        None,
    ),
    t(
        "discord_bridge",
        Other,
        &[("PROTON_DISCORD_BRIDGE", "1")],
        "Discord bridge (game's own Rich Presence)",
        "Discord köprüsü (oyunun kendi Rich Presence'ı)",
        Some("proton-cachyos"),
    ),
    t(
        "obs_capture",
        Other,
        &[("OBS_VKCAPTURE", "1")],
        "OBS Vulkan game capture",
        "OBS Vulkan oyun yakalama",
        Some("obs-vkcapture"),
    ),
    Tweak {
        id: "game_performance",
        category: Other,
        env: &[],
        wrapper: Some("game-performance"),
        label_en: "CPU performance governor while playing",
        label_tr: "Oynarken CPU'yu performans moduna al",
        needs: Some("CachyOS"),
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
        "dlss_rr" | "nvapi" | "hide_nvidia" => Some(GpuNeed::Nvidia),
        _ => None,
    }
}
