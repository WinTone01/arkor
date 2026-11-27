//! Box art from the LaunchBox Games Database (ported from Irori's `launchbox`), for
//! emulator games, when the user switches it on.
//!
//! LaunchBox publishes its whole database as one file, `Metadata.zip` (games, their other
//! names and the file names of their images), and serves the images by file name from
//! `images.launchbox-app.com`. That file and those images are all Arkor reads: no page is
//! fetched. The file is large (over 100 MB), so it is downloaded at most once a week,
//! read as a stream ([`parse`]) and deleted once read; what stays is one small file per
//! system under `<emulators>/launchbox/systems/`.

mod matching;
mod parse;
mod text;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Context;
use serde::{Deserialize, Serialize};

pub use matching::{Lookup, best};

pub const METADATA_URL: &str = "https://gamesdb.launchbox-app.com/Metadata.zip";
pub const IMAGE_BASE: &str = "https://images.launchbox-app.com/";
/// How long an index is used before LaunchBox is asked again.
const REFRESH_AFTER: u64 = 7 * 24 * 3600;
const XML_NAME: &str = "Metadata.xml";
/// More than this is not LaunchBox's game list.
const MAX_XML: u64 = 4 << 30;

/// LaunchBox's platforms and Arkor's systems.
pub const PLATFORMS: &[(&str, &str)] = &[
    ("Nintendo Entertainment System", "nes"),
    ("Nintendo Famicom Disk System", "nes"),
    ("Super Nintendo Entertainment System", "snes"),
    ("Nintendo Satellaview", "snes"),
    ("Nintendo 64", "n64"),
    ("Nintendo 64DD", "n64"),
    ("Nintendo Game Boy", "gb"),
    ("Nintendo Game Boy Color", "gbc"),
    ("Nintendo Game Boy Advance", "gba"),
    ("Nintendo DS", "nds"),
    ("Nintendo 3DS", "3ds"),
    ("Nintendo GameCube", "gamecube"),
    ("Nintendo Wii", "wii"),
    ("Nintendo Wii U", "wiiu"),
    ("Nintendo Switch", "switch"),
    ("Sega Master System", "sms"),
    ("Sega Genesis", "genesis"),
    ("Sega Game Gear", "gamegear"),
    ("Sega CD", "segacd"),
    ("Sega Dreamcast", "dreamcast"),
    ("NEC TurboGrafx-16", "pce"),
    ("NEC TurboGrafx-CD", "pce"),
    ("Sony Playstation", "psx"),
    ("Sony Playstation 2", "ps2"),
    ("Sony Playstation 3", "ps3"),
    ("Sony Playstation 4", "ps4"),
    ("Sony PSP", "psp"),
    ("Sony PSP Minis", "psp"),
    ("Sony Playstation Vita", "psvita"),
    ("Microsoft Xbox", "xbox"),
    ("MS-DOS", "dos"),
    ("ScummVM", "scummvm"),
    ("Arcade", "arcade"),
    ("Windows", "doom"),
];

fn platforms() -> HashMap<String, String> {
    PLATFORMS
        .iter()
        .map(|(p, s)| (p.to_string(), s.to_string()))
        .collect()
}

/// One front box of a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxArt {
    /// The image's file name on LaunchBox's image server.
    pub file: String,
    /// LaunchBox's region ("North America", "Japan"), when it gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// A cleaned-up rebuild of the box rather than a scan.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reconstructed: bool,
}

impl BoxArt {
    pub fn url(&self) -> String {
        format!("{IMAGE_BASE}{}", self.file)
    }
}

/// A game in LaunchBox's database with its front boxes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchBoxGame {
    pub id: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternate: Vec<String>,
    pub boxes: Vec<BoxArt>,
}

/// One system's games that have a front box, sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemIndex {
    pub games: Vec<LaunchBoxGame>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    /// When the index was built (Unix seconds).
    updated_at: u64,
}

fn dir() -> PathBuf {
    super::dirs::root().join("launchbox")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether there is an index, and when it was built (Unix seconds).
pub fn built_at() -> Option<u64> {
    let s: State = super::read_json(&dir().join("state.json"));
    (s.updated_at > 0).then_some(s.updated_at)
}

/// Whether the index is older than a week (or missing).
pub fn due() -> bool {
    built_at().is_none_or(|at| now().saturating_sub(at) >= REFRESH_AFTER)
}

/// Downloads LaunchBox's database and builds the index from it; how many games have boxes.
/// Blocking (a 100 MB download, then about half a minute of reading).
pub fn refresh() -> anyhow::Result<usize> {
    let dir = dir();
    std::fs::create_dir_all(&dir)?;
    let zip = dir.join("Metadata.zip");
    crate::tools::download(METADATA_URL, &zip)?;
    let built = build(&zip);
    let _ = std::fs::remove_file(&zip);
    let games = built?;
    super::write_json(&dir.join("state.json"), &State { updated_at: now() })?;
    LOADED.lock().unwrap_or_else(|e| e.into_inner()).clear();
    Ok(games)
}

fn build(zip: &std::path::Path) -> anyhow::Result<usize> {
    use std::io::Read;
    let xml = super::zip::open_entry(zip, XML_NAME)
        .context("LaunchBox's games database holds no game list")?;
    let systems: BTreeMap<String, SystemIndex> = parse::read(xml.take(MAX_XML), &platforms())?;
    let mut games = 0;
    for (system, index) in &systems {
        super::write_json(&dir().join("systems").join(format!("{system}.json")), index)?;
        games += index.games.len();
    }
    Ok(games)
}

/// Removes the index.
pub fn remove() {
    let _ = std::fs::remove_dir_all(dir());
    LOADED.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Systems' indexes read so far.
static LOADED: Mutex<BTreeMap<String, std::sync::Arc<SystemIndex>>> = Mutex::new(BTreeMap::new());

fn load(system: &str) -> Option<std::sync::Arc<SystemIndex>> {
    if !system
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = loaded.get(system) {
        return Some(i.clone());
    }
    let text =
        std::fs::read_to_string(dir().join("systems").join(format!("{system}.json"))).ok()?;
    let index = std::sync::Arc::new(serde_json::from_str::<SystemIndex>(&text).ok()?);
    loaded.insert(system.to_string(), index.clone());
    Some(index)
}

/// The front box of the game of `system` whose file is named `stem`, of its own region
/// (`(Europe)` asks for the European box), when LaunchBox has the game.
pub fn cover_url(system: &str, stem: &str) -> Option<String> {
    let index = load(system)?;
    let name = super::names::parse(stem);
    let game = index.find(&name.title)?;
    best(&game.boxes, &name.tags).map(BoxArt::url)
}
