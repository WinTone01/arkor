//! UI strings. Every language is a `Strings` value, so a missing translation is a compile error.
//! English is the default (see `arkor_core::Language`).

use arkor_core::Language;
use arkor_core::mangohud::{Metric, Preset};

pub struct Strings {
    pub lang: Language,

    // Sidebar / navigation
    pub global_settings: &'static str,
    pub games_header: &'static str,
    pub on: &'static str,
    pub off: &'static str,
    pub pick_something: &'static str,
    pub not_found: &'static str,

    // Status messages (followed by ": <error>" where an error exists)
    pub saved: &'static str,
    pub save_failed: &'static str,
    pub profile_load_failed: &'static str,
    pub global_load_failed: &'static str,

    // Global page
    pub global_subtitle: &'static str,
    pub hud_default_hint: &'static str,
    pub language: &'static str,

    // Game page
    pub mode_global: &'static str,
    pub mode_custom: &'static str,
    pub mode_off: &'static str,
    pub mode_global_on_note: &'static str,
    pub mode_global_off_note: &'static str,
    pub mode_custom_note: &'static str,
    pub mode_off_note: &'static str,
    pub edit_global: &'static str,
    pub launch_section: &'static str,
    pub gamemode_hint: &'static str,
    pub gamescope_hint: &'static str,
    pub steam_launch_option: &'static str,
    pub play: &'static str,
    pub launching: &'static str,
    pub launch_failed: &'static str,
    pub arkor_launches_note: &'static str,
    pub steam_launches_note: &'static str,
    pub store_local: &'static str,
    pub remove_imported: &'static str,
    pub remove_imported_note: &'static str,
    pub prefix_label: &'static str,

    // Import from a Wine prefix
    pub import_prefix: &'static str,
    pub import_intro: &'static str,
    pub choose_prefix: &'static str,
    pub choose_file: &'static str,
    pub not_a_prefix: &'static str,
    pub scanning: &'static str,
    pub pick_exe: &'static str,
    pub no_exes_found: &'static str,

    // HUD editor
    pub presets_section: &'static str,
    pub custom_badge: &'static str,
    pub position_section: &'static str,
    pub preview: &'static str,
    pub screen_position: &'static str,
    pub orientation: &'static str,
    pub vertical: &'static str,
    pub horizontal: &'static str,
    pub size_section: &'static str,
    pub font_size: &'static str,
    pub bg_opacity: &'static str,
    pub fps_limit: &'static str,
    pub unlimited: &'static str,
    pub metrics_section: &'static str,
    pub group_frame: &'static str,
    pub group_other: &'static str,
    /// Indexed like `Preset::ALL`.
    pub presets: [&'static str; 4],
    /// Indexed like `Metric::ALL`.
    pub metrics: [&'static str; 19],
}

impl Strings {
    pub fn preset(&self, p: Preset) -> &'static str {
        self.presets[Preset::ALL
            .iter()
            .position(|&x| x == p)
            .expect("preset is in ALL")]
    }

    pub fn metric(&self, m: Metric) -> &'static str {
        self.metrics[Metric::ALL
            .iter()
            .position(|&x| x == m)
            .expect("metric is in ALL")]
    }

    /// "40%" in English, "%40" in Turkish.
    pub fn percent(&self, v: u32) -> String {
        match self.lang {
            Language::En => format!("{v}%"),
            Language::Tr => format!("%{v}"),
        }
    }
}

pub fn strings(lang: Language) -> &'static Strings {
    match lang {
        Language::En => &EN,
        Language::Tr => &TR,
    }
}

/// Language names are shown in their own language, so they are not translated.
pub const LANGUAGES: [(Language, &str); 2] = [(Language::En, "English"), (Language::Tr, "Türkçe")];

pub static EN: Strings = Strings {
    lang: Language::En,
    global_settings: "Global settings",
    games_header: "GAMES",
    on: "on",
    off: "off",
    pick_something: "Select a game or Global settings",
    not_found: "not found",
    saved: "Saved",
    save_failed: "Couldn't save",
    profile_load_failed: "Couldn't read profile, showing defaults",
    global_load_failed: "Couldn't read global settings, using defaults",
    global_subtitle: "Applies to every game set to “Use global” that has no settings of its own.",
    hud_default_hint: "Show in games by default",
    language: "Language",
    mode_global: "Use global",
    mode_custom: "Custom for this game",
    mode_off: "Off",
    mode_global_on_note: "Using the global layout (MangoHud on).",
    mode_global_off_note: "MangoHud is off globally, so it won't show in this game.",
    mode_custom_note: "This game's own layout. Global settings don't affect it.",
    mode_off_note: "MangoHud is turned off for this game even if MANGOHUD=1 is set system-wide.",
    edit_global: "Edit global settings",
    launch_section: "Launch",
    gamemode_hint: "Launch with Feral gamemoderun",
    gamescope_hint: "Run the game inside gamescope (MangoHud is drawn with --mangoapp)",
    steam_launch_option: "Steam launch option:",
    play: "Play",
    launching: "Starting…",
    launch_failed: "Launch failed",
    arkor_launches_note: "Arkor starts this game itself, so these settings always apply.",
    steam_launches_note: "Steam starts this game. Set the launch option below in Steam so these settings apply.",
    store_local: "Imported",
    remove_imported: "Remove from Arkor",
    remove_imported_note: "Only removes it from Arkor's list. The game files and the prefix stay.",
    prefix_label: "Prefix",
    import_prefix: "Import from prefix",
    import_intro: "Add a game Arkor didn't find on its own: choose its Wine prefix, then the game's executable or shortcut. Nothing is copied or changed.",
    choose_prefix: "Choose prefix folder…",
    choose_file: "Choose a file manually…",
    not_a_prefix: "Not a Wine prefix (no drive_c folder found)",
    scanning: "Looking for games…",
    pick_exe: "Pick the game's executable",
    no_exes_found: "No game executables found in this prefix. Choose the file manually.",
    presets_section: "Preset",
    custom_badge: "Custom",
    position_section: "Position and look",
    preview: "Preview",
    screen_position: "Screen position",
    orientation: "Layout",
    vertical: "Vertical",
    horizontal: "Horizontal",
    size_section: "Size and limit",
    font_size: "Font size",
    bg_opacity: "Background opacity",
    fps_limit: "FPS limit",
    unlimited: "Unlimited",
    metrics_section: "Shown metrics",
    group_frame: "Frame",
    group_other: "Other",
    presets: ["FPS only", "Basic", "Basic horizontal", "Full"],
    metrics: [
        "Usage",
        "Temperature",
        "Core clock",
        "Memory clock",
        "Power",
        "VRAM",
        "Usage",
        "Temperature",
        "Frequency",
        "Power",
        "RAM",
        "Battery",
        "FPS",
        "Frametime (ms)",
        "Frametime graph",
        "Resolution",
        "Wine/Proton version",
        "DXVK/VKD3D version",
        "Clock",
    ],
};

pub static TR: Strings = Strings {
    lang: Language::Tr,
    global_settings: "Global ayarlar",
    games_header: "OYUNLAR",
    on: "açık",
    off: "kapalı",
    pick_something: "Bir oyun ya da Global ayarlar'ı seç",
    not_found: "bulunamadı",
    saved: "Kaydedildi",
    save_failed: "Kaydedilemedi",
    profile_load_failed: "Profil okunamadı, varsayılan gösteriliyor",
    global_load_failed: "Global ayarlar okunamadı, varsayılanlar kullanılıyor",
    global_subtitle: "Kendi ayarı olmayan, “Global'i kullan” modundaki tüm oyunlara uygulanır.",
    hud_default_hint: "Oyunlarda varsayılan olarak göster",
    language: "Dil",
    mode_global: "Global'i kullan",
    mode_custom: "Bu oyuna özel",
    mode_off: "Kapalı",
    mode_global_on_note: "Global düzen kullanılıyor (MangoHud açık).",
    mode_global_off_note: "Global'de MangoHud kapalı, bu oyunda gösterilmeyecek.",
    mode_custom_note: "Bu oyunun kendi düzeni. Global ayarlar bunu etkilemez.",
    mode_off_note: "Sistem genelinde MANGOHUD=1 ayarlı olsa bile bu oyunda MangoHud kapatılır.",
    edit_global: "Global ayarları düzenle",
    launch_section: "Başlatma",
    gamemode_hint: "Feral gamemoderun ile başlat",
    gamescope_hint: "Oyunu gamescope içinde çalıştır (MangoHud --mangoapp ile çizilir)",
    steam_launch_option: "Steam başlatma seçeneği:",
    play: "Oyna",
    launching: "Başlatılıyor…",
    launch_failed: "Başlatılamadı",
    arkor_launches_note: "Bu oyunu Arkor kendisi başlatır, bu ayarlar her zaman uygulanır.",
    steam_launches_note: "Bu oyunu Steam başlatır. Ayarların uygulanması için aşağıdaki başlatma seçeneğini Steam'e ekle.",
    store_local: "İçe aktarılan",
    remove_imported: "Arkor'dan kaldır",
    remove_imported_note: "Sadece Arkor'un listesinden kaldırır. Oyun dosyaları ve prefix yerinde kalır.",
    prefix_label: "Prefix",
    import_prefix: "Prefix'ten içe aktar",
    import_intro: "Arkor'un kendiliğinden bulamadığı bir oyunu ekle: önce Wine prefix'ini, sonra oyunun exe'sini ya da kısayolunu seç. Hiçbir şey kopyalanmaz ya da değiştirilmez.",
    choose_prefix: "Prefix klasörünü seç…",
    choose_file: "Dosyayı elle seç…",
    not_a_prefix: "Bu bir Wine prefix'i değil (drive_c klasörü yok)",
    scanning: "Oyunlar aranıyor…",
    pick_exe: "Oyunun exe dosyasını seç",
    no_exes_found: "Bu prefix'te oyun exe'si bulunamadı. Dosyayı elle seç.",
    presets_section: "Hazır düzen",
    custom_badge: "Özel",
    position_section: "Konum ve görünüm",
    preview: "Önizleme",
    screen_position: "Ekrandaki yeri",
    orientation: "Yerleşim",
    vertical: "Dikey",
    horizontal: "Yatay",
    size_section: "Boyut ve sınır",
    font_size: "Yazı boyutu",
    bg_opacity: "Arka plan opaklığı",
    fps_limit: "FPS sınırı",
    unlimited: "Sınırsız",
    metrics_section: "Gösterilecekler",
    group_frame: "Kare",
    group_other: "Diğer",
    presets: ["Sadece FPS", "Temel", "Temel yatay", "Tam"],
    metrics: [
        "Kullanım",
        "Sıcaklık",
        "Çekirdek saati",
        "Bellek saati",
        "Güç",
        "VRAM",
        "Kullanım",
        "Sıcaklık",
        "Frekans",
        "Güç",
        "RAM",
        "Pil",
        "FPS",
        "Frametime (ms)",
        "Frametime grafiği",
        "Çözünürlük",
        "Wine/Proton sürümü",
        "DXVK/VKD3D sürümü",
        "Saat",
    ],
};
