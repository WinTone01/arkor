//! Games from Epic, GOG and Amazon: logging in, what the account owns, downloading,
//! updating, repairing and removing.
//!
//! The work is done by the helpers in [`helper`] with folders of Arkor's own
//! (`~/.local/share/arkor/stores/<store>`); logins of other launchers are never touched.
//! Installing and updating run through the download queue (`crate::downloads`) with keys
//! like `game:epic:<id>`.

mod amazon;
pub mod covers;
mod epic;
mod gog;
pub mod helper;
mod steam;
pub mod vault;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Context;
use arkor_core::library::NewGame;
use arkor_core::{Game, GameId, Store, paths};
use serde::{Deserialize, Serialize};

use crate::progress;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shop {
    /// Through the Steam client itself (its login, its downloads).
    Steam,
    Epic,
    Gog,
    Amazon,
}

impl Shop {
    pub const ALL: [Shop; 4] = [Shop::Steam, Shop::Epic, Shop::Gog, Shop::Amazon];

    pub fn key(self) -> &'static str {
        match self {
            Shop::Epic => "epic",
            Shop::Gog => "gog",
            Shop::Amazon => "amazon",
            Shop::Steam => "steam",
        }
    }

    pub fn from_key(key: &str) -> Option<Shop> {
        Shop::ALL.into_iter().find(|s| s.key() == key)
    }

    pub fn name(self) -> &'static str {
        match self {
            Shop::Epic => "Epic Games",
            Shop::Gog => "GOG",
            Shop::Amazon => "Amazon Games",
            Shop::Steam => "Steam",
        }
    }

    fn store(self) -> Store {
        match self {
            Shop::Epic => Store::Epic,
            Shop::Gog => Store::Gog,
            Shop::Amazon => Store::Amazon,
            Shop::Steam => Store::Steam,
        }
    }
}

/// The store's folder, readable by the user only (it holds the login).
pub fn shop_dir(shop: Shop) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let dir = paths::stores_dir()?.join(shop.key());
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Writes through a temporary file, so a crash never leaves half a file.
fn write_json(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.new");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

// ---- Accounts ----

/// Whether Arkor is logged in. Reads a file only.
pub fn logged_in(shop: Shop) -> bool {
    if shop == Shop::Steam {
        return steam::logged_in();
    }
    vault::kept(shop) != vault::Kept::Nothing
}

/// The account's name, when known.
pub fn account(shop: Shop) -> Option<String> {
    if shop == Shop::Steam {
        return steam::account();
    }
    if !logged_in(shop) {
        return None;
    }
    let path = paths::stores_dir().ok()?.join(shop.key()).join("account");
    let name = std::fs::read_to_string(path).ok()?;
    Some(name.trim().to_string()).filter(|n| !n.is_empty())
}

fn save_account(shop: Shop, name: &str) -> anyhow::Result<()> {
    std::fs::write(shop_dir(shop)?.join("account"), name)?;
    Ok(())
}

/// A login that was started: the page to open, and what finishing it needs.
#[derive(Debug, Clone)]
pub struct Login {
    pub shop: Shop,
    /// The store's own page, opened in the browser.
    pub url: String,
    amazon: Option<amazon::Registration>,
}

/// Starts logging in (fetches the helper if needed). Blocking.
pub fn begin_login(shop: Shop) -> anyhow::Result<Login> {
    anyhow::ensure!(shop != Shop::Steam, "log in to Steam in Steam itself");
    helper::ensure(shop)?;
    let (url, amazon) = match shop {
        Shop::Epic => (epic::LOGIN_URL.to_string(), None),
        Shop::Gog => (gog::LOGIN_URL.to_string(), None),
        Shop::Amazon => {
            let r = amazon::begin_login()?;
            (r.url.clone(), Some(r))
        }
        Shop::Steam => unreachable!("checked above"),
    };
    Ok(Login { shop, url, amazon })
}

/// The login code in what the user pasted (the code itself, the page's text, or the address
/// the browser ended on), if there is one.
pub fn find_code(shop: Shop, text: &str) -> Option<String> {
    match shop {
        Shop::Epic => epic::find_code(text),
        Shop::Gog => gog::find_code(text),
        Shop::Amazon => amazon::find_code(text),
        Shop::Steam => None,
    }
}

/// Finishes logging in with the code from [`find_code`]. Blocking.
pub fn finish_login(login: &Login, code: &str) -> anyhow::Result<()> {
    vault::prepare(login.shop);
    let name = match login.shop {
        Shop::Epic => epic::finish_login(code)?,
        Shop::Gog => gog::finish_login(code)?,
        Shop::Amazon => amazon::finish_login(
            login.amazon.as_ref().context("login was not started")?,
            code,
        )?,
        Shop::Steam => anyhow::bail!("log in to Steam in Steam itself"),
    };
    anyhow::ensure!(
        logged_in(login.shop),
        "{} did not accept the code",
        login.shop.name()
    );
    let _ = save_account(login.shop, &name);
    Ok(())
}

/// Logs out of the store. Installed games stay. Blocking.
pub fn logout(shop: Shop) -> anyhow::Result<()> {
    match shop {
        Shop::Epic => epic::logout()?,
        Shop::Gog => gog::logout()?,
        Shop::Amazon => amazon::logout()?,
        Shop::Steam => anyhow::bail!("log out of Steam in Steam itself"),
    }
    vault::forget(shop);
    let dir = shop_dir(shop)?;
    for f in ["account", "owned.json"] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    Ok(())
}

// ---- The account's games ----

/// A game the account owns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Owned {
    pub id: String,
    pub title: String,
    /// Portrait picture (web address), when the store's list has one.
    #[serde(default)]
    pub cover: Option<String>,
    /// Wide picture (web address).
    #[serde(default)]
    pub wide: Option<String>,
    #[serde(default)]
    pub linux: bool,
    #[serde(default = "yes")]
    pub windows: bool,
    /// Epic's namespace of the game (what AreWeAntiCheatYet knows it by).
    #[serde(default)]
    pub namespace: Option<String>,
}

fn yes() -> bool {
    true
}

/// What the account owned at the last [`refresh`], sorted by title.
pub fn owned(shop: Shop) -> Vec<Owned> {
    if !logged_in(shop) {
        return Vec::new();
    }
    paths::stores_dir()
        .and_then(|d| read_json(&d.join(shop.key()).join("owned.json")))
        .unwrap_or_default()
}

/// Asks the store what the account owns. Blocking.
pub fn refresh(shop: Shop) -> anyhow::Result<Vec<Owned>> {
    let mut list = match shop {
        Shop::Epic => epic::owned()?,
        Shop::Gog => gog::owned()?,
        Shop::Amazon => amazon::owned()?,
        Shop::Steam => steam::owned()?,
    };
    list.sort_by_key(|g| g.title.to_lowercase());
    list.dedup_by(|a, b| a.id == b.id);
    write_json(&shop_dir(shop)?.join("owned.json"), &list)?;
    Ok(list)
}

// ---- Installed games ----

/// A game Arkor installed from a store.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub shop: Shop,
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    pub version: String,
    pub linux: bool,
    pub size: Option<u64>,
}

/// Games Arkor installed from `shop`. Reads files only.
pub fn installed(shop: Shop) -> Vec<Installed> {
    let list = match shop {
        Shop::Epic => epic::installed(),
        Shop::Gog => gog::installed(),
        Shop::Amazon => amazon::installed(),
        Shop::Steam => steam::installed(),
    };
    list.unwrap_or_default()
}

fn find_installed(shop: Shop, id: &str) -> Option<Installed> {
    installed(shop).into_iter().find(|g| g.id == id)
}

/// The library entry's origin for a store game (also how other launchers' games are known).
pub fn origin(shop: Shop, id: &str) -> String {
    format!("{}:{id}", shop.key())
}

/// The shop and id of a library entry's origin.
pub fn from_origin(origin: &str) -> Option<(Shop, &str)> {
    let (shop, id) = origin.split_once(':')?;
    Some((Shop::from_key(shop)?, id))
}

/// The library entry for an installed game.
pub fn library_entry(shop: Shop, id: &str) -> anyhow::Result<NewGame> {
    let game = find_installed(shop, id).context("the game is not installed")?;
    arkor_stores::to_new_game(&Game {
        id: GameId::new(shop.store(), id),
        name: game.title,
        install_dir: Some(game.path),
        size_on_disk: game.size,
    })
}

/// Games whose installation ended since the last call, for the library.
pub fn take_fresh() -> Vec<(Shop, String)> {
    std::mem::take(&mut *FRESH.lock().unwrap_or_else(|e| e.into_inner()))
}

static FRESH: Mutex<Vec<(Shop, String)>> = Mutex::new(Vec::new());

// ---- Installing ----

/// A DLC the account owns that has files of its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Dlc {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub size: Option<u64>,
}

/// What installing a game means, for the install dialog.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Info {
    pub download: Option<u64>,
    pub disk: Option<u64>,
    /// GOG: language codes to choose from.
    pub languages: Vec<String>,
    /// The folder's name the store insists on (GOG).
    pub folder: Option<String>,
    /// DLC the account owns for it, to choose from.
    pub dlcs: Vec<Dlc>,
    version: String,
    build: String,
}

/// How long a store's answer about a game is used again.
const INFO_AGE: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

fn info_file(shop: Shop, id: &str, linux: bool) -> Option<PathBuf> {
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let os = if linux { "linux" } else { "windows" };
    Some(
        dirs::cache_dir()?
            .join("arkor/stores/info")
            .join(format!("{}-{safe}-{os}.json", shop.key())),
    )
}

/// What the store said about a game lately, if it is not old. Reads a file only.
pub fn cached_info(shop: Shop, id: &str, linux: bool) -> Option<Info> {
    let path = info_file(shop, id, linux)?;
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|age| age < INFO_AGE));
    fresh.then(|| read_json(&path).ok()).flatten()
}

/// Asks the store about a game (an answer of the last hours is used again). Blocking.
pub fn info(shop: Shop, id: &str, linux: bool) -> anyhow::Result<Info> {
    if let Some(info) = cached_info(shop, id, linux) {
        return Ok(info);
    }
    let info = match shop {
        Shop::Epic => epic::info(id),
        Shop::Gog => gog::info(id, linux),
        Shop::Amazon => amazon::info(id),
        // Steam tells nothing ahead of time; its download says the size.
        Shop::Steam => Ok(Info::default()),
    }?;
    if let Some(path) = info_file(shop, id, linux) {
        let _ = write_json(&path, &info);
    }
    Ok(info)
}

/// How the user wants a game installed; kept while it is installed (updates need it, too).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Order {
    pub title: String,
    pub folder: PathBuf,
    pub linux: bool,
    /// GOG language code; empty = English.
    pub language: String,
    /// The DLC to install with it, by id.
    pub dlcs: Vec<String>,
    /// The Proton it runs with (`None` = the default from the settings).
    pub proton: Option<String>,
    /// Its Wine prefix (`None` = its own in the prefixes folder).
    pub prefix: Option<PathBuf>,
    /// Also an entry in the app menu, and one on the desktop.
    pub menu: bool,
    pub desktop: bool,
    /// Steam's library folder it goes to (`None`: Steam's default one).
    pub steam_folder: Option<i32>,
    /// A folder that becomes a new library of Steam's for it.
    pub steam_new_folder: Option<PathBuf>,
    /// The folder did not exist before, so an abandoned download may be deleted.
    fresh: bool,
    size: Option<u64>,
}

/// `games_dir` and what the install dialog was answered with, as an order.
pub fn order(
    game: &Owned,
    info: &Info,
    games_dir: &Path,
    linux: bool,
    language: &str,
    dlcs: Vec<String>,
) -> Order {
    let name = match &info.folder {
        Some(f) => f.clone(),
        None => folder_name(&game.title, &game.id),
    };
    let mut folder = games_dir.join(&name);
    // Never into a folder something else lives in (GOG's folder name is fixed).
    if info.folder.is_none() {
        for n in 2..100 {
            if !occupied(&folder) {
                break;
            }
            folder = games_dir.join(format!("{name} ({n})"));
        }
    }
    Order {
        title: game.title.clone(),
        fresh: !occupied(&folder),
        folder,
        linux,
        language: language.to_string(),
        // Only what the store offers (the dialog may know an older list).
        dlcs: dlcs
            .into_iter()
            .filter(|d| info.dlcs.iter().any(|o| o.id == *d))
            .collect(),
        size: info.download,
        ..Order::default()
    }
}

fn occupied(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some())
}

/// A folder name for a title: without what file systems (or Windows games) dislike.
fn folder_name(title: &str, id: &str) -> String {
    let name: String = title
        .chars()
        .filter(|c| !c.is_control() && !r#"<>:"/\|?*™®©"#.contains(*c))
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = name.trim_matches(['.', ' ']);
    if name.is_empty() {
        id.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
    } else {
        name.to_string()
    }
}

fn orders_file() -> anyhow::Result<PathBuf> {
    Ok(paths::stores_dir()?.join("orders.json"))
}

static ORDERS: Mutex<()> = Mutex::new(());

fn with_orders<R>(f: impl FnOnce(&mut BTreeMap<String, Order>) -> R) -> anyhow::Result<R> {
    let _lock = ORDERS.lock().unwrap_or_else(|e| e.into_inner());
    let path = orders_file()?;
    let mut orders: BTreeMap<String, Order> = if path.is_file() {
        read_json(&path)?
    } else {
        BTreeMap::new()
    };
    let before = orders.clone();
    let r = f(&mut orders);
    if orders != before {
        write_json(&path, &orders)?;
    }
    Ok(r)
}

/// How the user asked for a game to be installed (kept while it is installed).
pub fn order_of(shop: Shop, id: &str) -> Option<Order> {
    load_order(shop, id)
}

fn load_order(shop: Shop, id: &str) -> Option<Order> {
    with_orders(|o| o.get(&origin(shop, id)).cloned()).ok()?
}

/// The queue key of installing or updating a game.
pub fn key(shop: Shop, id: &str) -> String {
    format!("game:{}:{id}", shop.key())
}

/// The queue key of checking a game's files and fetching broken ones again.
pub fn repair_key(shop: Shop, id: &str) -> String {
    format!("repair:{}:{id}", shop.key())
}

/// The game a queue key is about, and whether it is a repair.
pub fn parse_key(key: &str) -> Option<(Shop, &str, bool)> {
    let (rest, repair) = match key.strip_prefix("game:") {
        Some(rest) => (rest, false),
        None => (key.strip_prefix("repair:")?, true),
    };
    let (shop, id) = from_origin(rest)?;
    Some((shop, id, repair))
}

/// A queue key's display name.
pub fn label(key: &str) -> Option<String> {
    let (shop, id, _) = parse_key(key)?;
    Some(
        load_order(shop, id)
            .map(|o| o.title)
            .or_else(|| find_installed(shop, id).map(|g| g.title))
            .or_else(|| {
                owned(shop)
                    .into_iter()
                    .find(|g| g.id == id)
                    .map(|g| g.title)
            })
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| id.to_string()),
    )
}

/// Queues a game's installation.
pub fn install(shop: Shop, id: &str, order: Order) -> anyhow::Result<()> {
    anyhow::ensure!(logged_in(shop), "not logged in to {}", shop.name());
    if let (Some(size), Some(free)) = (order.size, free_space(&order.folder)) {
        anyhow::ensure!(
            size < free,
            "not enough space: {} needed, {} free",
            progress::human_bytes(size),
            progress::human_bytes(free)
        );
    }
    with_orders(|o| o.insert(origin(shop, id), order))?;
    crate::downloads::enqueue(&key(shop, id));
    Ok(())
}

/// Does a queue item: installs, updates or repairs. Blocking.
pub(crate) fn run(key: &str) -> anyhow::Result<String> {
    let (shop, id, repair) = parse_key(key).context("not a game")?;
    anyhow::ensure!(
        logged_in(shop),
        "not logged in to {}: log in on the Stores page",
        shop.name()
    );
    let order = load_order(shop, id);
    if let Some(game) = find_installed(shop, id) {
        let order = order.unwrap_or_default();
        match shop {
            Shop::Epic => epic::update(&game, &order, repair)?,
            Shop::Gog => gog::update(&game, &order, repair)?,
            Shop::Amazon => amazon::update(&game, repair)?,
            Shop::Steam if repair => steam::repair(&game)?,
            Shop::Steam => {}
        }
        return Ok(game.title);
    }
    anyhow::ensure!(!repair, "the game is not installed");
    let order = order.context("the game's install settings are gone: install it again")?;
    let base = order.folder.parent().context("no games folder")?;
    std::fs::create_dir_all(base).with_context(|| format!("creating {}", base.display()))?;
    match shop {
        Shop::Epic => epic::install(id, &order)?,
        Shop::Gog => gog::install(id, &order)?,
        Shop::Amazon => amazon::install(id, &order)?,
        Shop::Steam => steam::install(id, &order)?,
    }
    let game = find_installed(shop, id).context("the download ended without the game")?;
    FRESH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((shop, id.to_string()));
    Ok(game.title)
}

/// Checks a finished helper, telling a stop apart from a failure.
fn check(out: &helper::Output) -> anyhow::Result<()> {
    anyhow::ensure!(out.ok, "{}", out.error());
    Ok(())
}

/// A queued download was cancelled: deletes what it had downloaded, unless the game is
/// installed (an update) or the folder was there before. Blocking.
pub(crate) fn cancelled(key: &str) {
    let Some((shop, id, false)) = parse_key(key) else {
        return;
    };
    if find_installed(shop, id).is_some() {
        return;
    }
    let order = with_orders(|o| o.remove(&origin(shop, id))).ok().flatten();
    if let Some(order) = order
        && order.fresh
        && deletable(&order.folder)
    {
        let _ = arkor_core::fs_guard::remove_dir(&order.folder);
    }
    if let Ok(dir) = shop_dir(shop) {
        match shop {
            Shop::Epic => {
                let _ = std::fs::remove_file(dir.join(format!("tmp/{id}.resume")));
            }
            Shop::Gog => gog::forget(&dir, id),
            Shop::Amazon => {}
            Shop::Steam => {
                if let Ok(appid) = id.parse()
                    && let Err(e) = steam::call_off(appid)
                {
                    eprintln!("arkor: the installation is still in Steam: {e:#}");
                }
            }
        }
    }
}

/// Calls off what Steam began to install, and waits until it is gone. Blocking.
pub fn steam_call_off(appid: u32) -> anyhow::Result<()> {
    let key = key(Shop::Steam, &appid.to_string());
    // A job of Arkor's calls it off itself when it ends.
    if crate::downloads::items().iter().any(|i| i.key == key) {
        crate::downloads::cancel(&key);
        let began = std::time::Instant::now();
        while arkor_stores::steam_library::progress(appid).is_some_and(|p| p.state & 4 == 0) {
            anyhow::ensure!(!crate::steam_link::any_game_running(), GAME_RUNS);
            anyhow::ensure!(
                began.elapsed() < std::time::Duration::from_secs(100),
                "Steam did not let go of it"
            );
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        return Ok(());
    }
    steam::call_off(appid)
}

/// Puts a game before Steam's other downloads. Blocking.
pub fn steam_first(appid: u32) -> anyhow::Result<()> {
    steam::first(appid)
}

pub(crate) fn steam_first_over() {
    steam::first_over();
}

/// Makes Steam go on with a download. Blocking.
pub fn steam_go_on(appid: u32) -> anyhow::Result<()> {
    steam::go_on(appid)
}

pub use steam::{GAME_RUNS, HELD, SteamLibrary, steam_libraries};

/// Whether a game folder may be deleted as a whole: a real folder, and nothing that could
/// hold more than one game (home, a drive, the games folder itself).
fn deletable(dir: &Path) -> bool {
    arkor_core::fs_guard::check(dir).is_ok_and(|real| !real.join("drive_c").exists())
}

/// Deletes an installed game's files. The library entry and its prefix (saves) are the
/// caller's. Blocking.
pub fn uninstall(shop: Shop, id: &str) -> anyhow::Result<()> {
    crate::downloads::cancel(&key(shop, id));
    crate::downloads::cancel(&repair_key(shop, id));
    let game = find_installed(shop, id).context("the game is not installed")?;
    match shop {
        Shop::Epic => epic::uninstall(&game)?,
        Shop::Gog => gog::uninstall(&game)?,
        Shop::Amazon => amazon::uninstall(&game)?,
        Shop::Steam => steam::uninstall(&game)?,
    }
    with_orders(|o| o.remove(&origin(shop, id)))?;
    Ok(())
}

/// Installed games with a newer version in the store: (game, newest version). Blocking.
pub fn updates() -> Vec<(Installed, String)> {
    let mut out = Vec::new();
    for shop in Shop::ALL {
        let games = installed(shop);
        if games.is_empty() || !logged_in(shop) {
            continue;
        }
        let found = match shop {
            Shop::Epic => epic::updates(&games),
            Shop::Gog => gog::updates(&games),
            Shop::Amazon => amazon::updates(&games),
            // Steam keeps its games up to date itself.
            Shop::Steam => Ok(Vec::new()),
        };
        out.extend(found.unwrap_or_default());
    }
    out
}

/// Free bytes where `path` is (or will be).
pub fn free_space(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = path.ancestors().find(|p| p.exists())?;
    let c = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs fills the zeroed struct for a valid path.
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(c.as_ptr(), &mut st) == 0).then(|| st.f_bavail as u64 * st.f_frsize as u64)
    }
}

/// The value of `name=` in a web address (or any text), percent-decoded.
fn query_value(text: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let at = text
        .match_indices(&needle)
        .find(|(i, _)| *i == 0 || text[..*i].ends_with(['?', '&', '#']))?;
    let rest = &text[at.0 + needle.len()..];
    let end = rest
        .find(|c: char| c == '&' || c == '#' || c.is_whitespace() || c == '"')
        .unwrap_or(rest.len());
    let mut out = Vec::new();
    let bytes = &rest.as_bytes()[..end];
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| rest.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok().filter(|v| !v.is_empty())
}

/// Codes are passed to a helper as an argument: plain characters only.
fn plain_code(code: &str, min: usize) -> bool {
    code.len() >= min
        && code.len() <= 2048
        && !code.starts_with('-')
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.~".contains(c))
}

#[cfg(test)]
mod tests;
