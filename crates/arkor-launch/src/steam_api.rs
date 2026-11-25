//! What can be done with a Steam under control (see `steam_control`): installing, removing
//! and moving games without its windows, the steps of a launch and its questions, the
//! verification of files, achievements, add-ons, workshop items and screenshots. All of it
//! is what Steam's own windows call; nothing of the account, the store or chats is touched.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::steam_control::{self, run};

/// A text as it is safe inside the JavaScript.
fn quoted(s: &str) -> String {
    json!(s).to_string()
}

fn parsed<T: serde::de::DeserializeOwned>(js: &str) -> anyhow::Result<T> {
    let v = run(js)?;
    let text = v.as_str().context("Steam gave no answer")?;
    Ok(serde_json::from_str(text)?)
}

/// Whether Steam is under control and answers now (also outside the main program).
pub fn ready() -> bool {
    steam_control::wanted()
        && run("typeof SteamClient === 'object' && typeof SteamClient.Apps === 'object' && typeof appStore === 'object'")
            .is_ok_and(|v| v == Value::Bool(true))
}

// ---- What happens in Steam, told with every look ----

/// Something Steam does with a game: starting it, checking its files.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Action {
    pub id: u32,
    pub game: String,
    /// `LaunchApp`, `VerifyApp`, …
    pub kind: String,
    /// The step it is at.
    pub task: String,
    /// What it waits to be answered, and more about that.
    pub request: String,
    pub details: String,
    pub done: f64,
    pub total: f64,
}

/// Where the move of a game to another disk stands.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Move {
    pub appid: u32,
    /// 0: done, 20: goes on, else what went wrong.
    pub error: u32,
    /// Percent.
    pub progress: f64,
    pub bytes: String,
    pub total: String,
}

/// Steam's installation, as its window would show it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub(crate) struct Install {
    pub state: u32,
    pub app: u32,
    pub error: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub(crate) struct Happening {
    pub actions: Vec<Action>,
    /// Games whose start failed, and why.
    pub errors: Vec<(String, String)>,
    /// Games an achievement was unlocked in.
    pub unlocked: Vec<u32>,
    pub moving: Option<Move>,
}

static HAPPENING: Mutex<Option<Happening>> = Mutex::new(None);
/// Errors and unlocked achievements not yet taken by the window.
static NEWS: Mutex<Vec<News>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, PartialEq)]
pub enum News {
    /// A game was not started by Steam, and why.
    NotStarted(String, String),
    Unlocked(u32),
    Moved(u32),
    NotMoved(u32),
}

/// Takes what the last look told.
pub(crate) fn told(now: Option<Happening>) {
    let mut before = HAPPENING.lock().unwrap_or_else(|e| e.into_inner());
    let mut news = NEWS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(n) = &now {
        for (game, why) in &n.errors {
            news.push(News::NotStarted(game.clone(), why.clone()));
        }
        for app in &n.unlocked {
            news.push(News::Unlocked(*app));
        }
        let was = before.as_ref().and_then(|b| b.moving.clone());
        if let (Some(w), Some(m)) = (was, &n.moving)
            && w.appid == m.appid
            && w.error == MOVING
            && m.error != MOVING
        {
            news.push(match m.error {
                0 => News::Moved(m.appid),
                _ => News::NotMoved(m.appid),
            });
        }
    }
    news.truncate(50);
    *before = now;
}

const MOVING: u32 = 20;

pub fn news() -> Vec<News> {
    std::mem::take(&mut NEWS.lock().unwrap_or_else(|e| e.into_inner()))
}

pub(crate) fn has_news() -> bool {
    !NEWS.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// What Steam does now.
pub fn actions() -> Vec<Action> {
    HAPPENING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|h| h.actions.clone())
        .unwrap_or_default()
}

/// The game that is being moved.
pub fn moving() -> Option<Move> {
    HAPPENING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|h| h.moving.clone())
        .filter(|m| m.error == MOVING)
}

/// Goes on with what an action asked.
pub fn answer(action: u32, what: &str) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Apps.ContinueGameAction({action}, {}); const a = globalThis.__arkor?.actions?.[{action}]; if (a) a.request = ''; 1",
        quoted(what)
    ))?;
    Ok(())
}

pub fn cancel(action: u32) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Apps.CancelGameAction({action}); if (globalThis.__arkor) delete globalThis.__arkor.actions[{action}]; 1"
    ))?;
    Ok(())
}

// ---- Licences ----

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Eula {
    pub id: String,
    pub version: u32,
    /// Its text; empty when it could not be read.
    pub text: String,
}

/// The licences of a game that have not been accepted.
pub fn eulas(appid: u32) -> anyhow::Result<Vec<Eula>> {
    parsed(&format!(
        r"(async () => {{
          const all = await SteamClient.Apps.LoadEula({appid});
          const out = [];
          for (const e of all) {{
            let text = '';
            try {{
              const page = await (await fetch(e.url)).text();
              const d = new DOMParser().parseFromString(page, 'text/html');
              d.querySelectorAll('script,style,head').forEach(x => x.remove());
              d.querySelectorAll('p,br,div,li,h1,h2,h3,tr').forEach(x => x.append('\n'));
              text = d.body.textContent.replace(/[ \t]+/g, ' ').replace(/\n\s*\n\s*/g, '\n\n').trim().slice(0, 200000);
            }} catch (x) {{}}
            out.push({{ id: String(e.id), version: Number(e.version) || 0, text }});
          }}
          return JSON.stringify(out);
        }})()"
    ))
}

/// The user accepted one.
pub fn accept_eula(appid: u32, e: &Eula) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Apps.MarkEulaAccepted({appid}, {}, {}); 1",
        quoted(&e.id),
        e.version
    ))?;
    Ok(())
}

// ---- Library folders ----

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Folder {
    #[serde(rename = "nFolderIndex")]
    pub index: i32,
    #[serde(rename = "strFolderPath")]
    pub path: String,
    #[serde(rename = "strUserLabel", default)]
    pub label: String,
    #[serde(rename = "nFreeSpace")]
    pub free: f64,
    #[serde(rename = "bIsDefaultFolder")]
    pub default: bool,
    #[serde(rename = "bIsMounted")]
    pub mounted: bool,
    #[serde(rename = "apps", default)]
    pub apps: Vec<u32>,
}

pub fn folders() -> anyhow::Result<Vec<Folder>> {
    parsed(
        "SteamClient.InstallFolder.GetInstallFolders().then(l => JSON.stringify(l.map(f => ({ nFolderIndex: f.nFolderIndex, strFolderPath: f.strFolderPath, strUserLabel: f.strUserLabel || '', nFreeSpace: Number(f.nFreeSpace), bIsDefaultFolder: !!f.bIsDefaultFolder, bIsMounted: !!f.bIsMounted, apps: (f.vecApps || []).map(a => a.nAppID) }))))",
    )
}

/// Makes a folder one of Steam's libraries (it is created); its number.
pub fn add_folder(path: &std::path::Path) -> anyhow::Result<i32> {
    std::fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    let path = path.to_string_lossy().into_owned();
    let known = folders()?;
    if let Some(f) = known.iter().find(|f| f.path == path) {
        return Ok(f.index);
    }
    run(&format!(
        "Promise.resolve(SteamClient.InstallFolder.AddInstallFolder({})).then(() => 1)",
        quoted(&path)
    ))?;
    let began = Instant::now();
    loop {
        if let Some(f) = folders()?.into_iter().find(|f| f.path == path) {
            return Ok(f.index);
        }
        anyhow::ensure!(
            began.elapsed() < Duration::from_secs(10),
            "Steam did not take {path} as a library folder"
        );
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Moves an installed game to another folder (Steam does it; see `moving`).
pub fn move_to(appid: u32, folder: i32) -> anyhow::Result<()> {
    // Refused at once or begun: the refusal comes as a number.
    let v = run(&format!(
        "Promise.race([SteamClient.InstallFolder.MoveInstallFolderForApp({appid}, {folder}).then(() => 0, e => Number(e) || 1), new Promise(r => setTimeout(() => r(0), 1500))])"
    ))?;
    match v.as_u64() {
        Some(0) | None => Ok(()),
        Some(17) => anyhow::bail!("not enough space on that disk"),
        Some(n) => anyhow::bail!("Steam cannot move it now ({n})"),
    }
}

pub fn cancel_move() -> anyhow::Result<()> {
    run("SteamClient.InstallFolder.CancelMove(); 1")?;
    Ok(())
}

// ---- Installing and removing ----

/// A licence an installation waits for.
static WAITS: Mutex<Option<(u32, Vec<Eula>)>> = Mutex::new(None);
static ACCEPTED: Mutex<Option<bool>> = Mutex::new(None);

/// The game whose installation waits for its licence, and the licence.
pub fn licence_asked() -> Option<(u32, Vec<Eula>)> {
    WAITS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The user's answer to that.
pub fn answer_licence(yes: bool) {
    *WAITS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *ACCEPTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(yes);
}

const SHOW_CONFIG: u32 = 7;
const SHOW_EULAS: u32 = 8;
const COMPLETE: u32 = 14;
const FAILED: u32 = 15;
const CANCELED: u32 = 16;

/// What Steam asks that only its own window can answer.
pub const ASKS: &str = "Steam asks something in its own window";

/// Begins an installation in `folder` (`None`: Steam's default one). `stop` is asked now and
/// then. Blocking until Steam has it in its queue.
pub(crate) fn install(
    appid: u32,
    folder: Option<i32>,
    stop: impl Fn() -> bool,
) -> anyhow::Result<()> {
    let state = || -> anyhow::Result<Install> {
        parsed("JSON.stringify(globalThis.__arkor?.install || {})")
    };
    run(&format!(
        "globalThis.__arkor && (globalThis.__arkor.install = null); SteamClient.Installs.OpenInstallWizard([{appid}], '0'); 1"
    ))?;
    let began = Instant::now();
    let mut configured = false;
    let mut began_asking = false;
    loop {
        if stop() {
            let _ = run("SteamClient.Installs.CancelInstall(); 1");
            anyhow::bail!(crate::tools::STOPPED);
        }
        let i = state()?;
        match i.state {
            SHOW_CONFIG if !configured => {
                configured = true;
                let place = folder
                    .map(|f| format!("await SteamClient.Installs.SetInstallFolder({f});"))
                    .unwrap_or_default();
                run(&format!(
                    "(async () => {{ {place} await SteamClient.Installs.ContinueInstall(); return 1; }})()"
                ))?;
            }
            SHOW_EULAS => {
                let list = eulas(appid).unwrap_or_default();
                anyhow::ensure!(!list.is_empty(), ASKS);
                if !began_asking {
                    began_asking = true;
                    *ACCEPTED.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    *WAITS.lock().unwrap_or_else(|e| e.into_inner()) = Some((appid, list.clone()));
                    crate::progress::touch();
                }
                let said = ACCEPTED.lock().unwrap_or_else(|e| e.into_inner()).take();
                match said {
                    Some(true) => {
                        for e in &list {
                            accept_eula(appid, e)?;
                        }
                        run("SteamClient.Installs.ContinueInstall(); 1")?;
                        began_asking = false;
                    }
                    Some(false) => {
                        let _ = run("SteamClient.Installs.CancelInstall(); 1");
                        anyhow::bail!("the licence was not accepted");
                    }
                    None if began.elapsed() > Duration::from_secs(1800) => {
                        answer_licence(false);
                    }
                    None => {}
                }
            }
            // A key, a password, a sign-up.
            4 | 6 | 11 | 13 => anyhow::bail!(ASKS),
            FAILED => anyhow::bail!("Steam could not install it ({})", i.error),
            CANCELED => anyhow::bail!(crate::tools::STOPPED),
            COMPLETE => return Ok(()),
            0 if configured && arkor_stores::steam_library::progress(appid).is_some() => {
                return Ok(());
            }
            _ => {}
        }
        let patience = if began_asking { 2000 } else { 60 };
        anyhow::ensure!(
            began.elapsed() < Duration::from_secs(patience),
            "Steam did not answer"
        );
        std::thread::sleep(Duration::from_millis(400));
    }
}

/// Uninstalls a game (or calls its installation off) without Steam's question: the user was
/// asked by Arkor. `false`: Steam did not do it. Blocking.
pub(crate) fn uninstall(appid: u32) -> anyhow::Result<bool> {
    run(&format!(
        "SteamClient.Installs.OpenUninstallWizard([{appid}], '0', true); 1"
    ))?;
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(45) {
        if arkor_stores::steam_library::manifest(appid).is_none() {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    Ok(false)
}

/// Checks a game's files. `stop` is asked now and then; `report` hears of done and total.
pub(crate) fn verify(
    appid: u32,
    stop: impl Fn() -> bool,
    report: impl Fn(u64, u64),
) -> anyhow::Result<()> {
    let v = run(&format!(
        "SteamClient.Apps.VerifyApp({appid}).then(r => r.nGameActionID)"
    ))?;
    let id = v
        .as_u64()
        .filter(|i| *i > 0)
        .context("Steam did not begin")?;
    let began = Instant::now();
    let mut seen = false;
    loop {
        if stop() {
            let _ = cancel(id as u32);
            anyhow::bail!(crate::tools::STOPPED);
        }
        #[derive(Deserialize)]
        struct Now {
            there: bool,
            done: f64,
            total: f64,
        }
        let now: Now = parsed(&format!(
            r"(async () => {{
              const all = await SteamClient.Apps.GetActiveGameActions();
              if (!all.some(a => a.nGameActionID === {id})) return JSON.stringify({{ there: false, done: 0, total: 0 }});
              const d = await Promise.race([new Promise(r => SteamClient.Apps.GetGameActionDetails({id}, r)), new Promise(r => setTimeout(() => r({{}}), 800))]);
              return JSON.stringify({{ there: true, done: Number(d.strNumDone) || 0, total: Number(d.strNumTotal) || 0 }});
            }})()"
        ))?;
        seen |= now.there;
        if !now.there && (seen || began.elapsed() > Duration::from_secs(4)) {
            return Ok(());
        }
        report(now.done as u64, now.total as u64);
        std::thread::sleep(Duration::from_millis(500));
    }
}

// ---- A game's settings in Steam ----

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AppSettings {
    pub options: String,
    pub tool: String,
    /// 250: chosen for this game.
    pub priority: i32,
}

/// The details Steam tells about a game, put through `f`.
fn details(appid: u32, f: &str) -> String {
    format!(
        "new Promise(r => {{ let h; const t = setTimeout(() => r(null), 4000); h = SteamClient.Apps.RegisterForAppDetails({appid}, d => {{ clearTimeout(t); setTimeout(() => h && h.unregister && h.unregister(), 0); r(JSON.stringify({f})); }}); }})"
    )
}

pub fn app_settings(appid: u32) -> anyhow::Result<AppSettings> {
    parsed(&details(
        appid,
        "{ options: d.strLaunchOptions || '', tool: d.strCompatToolName || '', priority: d.nCompatToolPriority || 0 }",
    ))
}

pub fn set_launch_options(appid: u32, options: &str) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Apps.SetAppLaunchOptions({appid}, {}); 1",
        quoted(options)
    ))?;
    Ok(())
}

/// Whether the pad's middle button brings Steam forward ("Guide button focuses Steam").
pub fn set_guide_focus(on: bool) -> anyhow::Result<()> {
    // Steam's settings message with only this field: 14002, a yes or no.
    let message = [0x90, 0xeb, 0x06, u8::from(on)];
    let text = base64(&message);
    run(&format!(
        "SteamClient.Settings.SetSetting({}); 1",
        quoted(&text)
    ))?;
    Ok(())
}

fn base64(data: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ABC[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// `None`: Steam's own choice.
pub fn set_tool(appid: u32, tool: Option<&str>) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Apps.SpecifyCompatTool({appid}, {}); 1",
        quoted(tool.unwrap_or_default())
    ))?;
    Ok(())
}

/// One of the ways to start a game.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LaunchOption {
    pub index: i64,
    pub text: String,
}

pub fn launch_options(appid: u32) -> anyhow::Result<Vec<LaunchOption>> {
    parsed(&format!(
        "SteamClient.Apps.GetLaunchOptionsForApp({appid}).then(l => JSON.stringify(l.map(o => ({{ index: o.nIndex, text: o.strDescription || '' }}))))"
    ))
}

// ---- Achievements, add-ons, workshop, screenshots ----

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Achievement {
    pub name: String,
    pub description: String,
    pub unlocked: bool,
    /// Unix seconds.
    pub at: u64,
    pub image: String,
    pub hidden: bool,
    pub progress: f64,
    pub most: f64,
    /// Percent of all players.
    pub share: f64,
}

/// A game's achievements: the unlocked ones first, the newest of them on top.
pub fn achievements(appid: u32) -> anyhow::Result<Vec<Achievement>> {
    let mut all: Vec<Achievement> = parsed(&format!(
        "SteamClient.Apps.GetMyAchievementsForApp('{appid}').then(r => JSON.stringify(((r.data || {{}}).rgAchievements || []).map(a => ({{ name: a.strName || '', description: a.strDescription || '', unlocked: !!a.bAchieved, at: Number(a.rtUnlocked) || 0, image: a.strImage || '', hidden: !!a.bHidden, progress: Number(a.flCurrentProgress) || 0, most: Number(a.flMaxProgress) || 0, share: Number(a.flAchieved) || 0 }}))))"
    ))?;
    all.sort_by_key(|a| (!a.unlocked, std::cmp::Reverse(a.at)));
    Ok(all)
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Dlc {
    pub id: u32,
    pub name: String,
    pub state: String,
    pub on: bool,
    pub size: f64,
}

pub fn dlcs(appid: u32) -> anyhow::Result<Vec<Dlc>> {
    parsed(&details(
        appid,
        "(d.vecDLC || []).map(x => ({ id: x.unAppID, name: x.strName || String(x.unAppID), state: x.strState || '', on: !!x.bEnabled, size: Number(x.lDiskUsageBytes) || 0 }))",
    ))
}

pub fn set_dlc(appid: u32, dlc: u32, on: bool) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Apps.SetDLCEnabled({appid}, {dlc}, {on}); 1"
    ))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WorkshopItem {
    pub id: String,
    pub title: String,
    pub size: f64,
    pub off: bool,
    pub image: String,
}

/// The workshop items the user has subscribed to for a game.
pub fn workshop(appid: u32) -> anyhow::Result<Vec<WorkshopItem>> {
    parsed(&format!(
        r"(async () => {{
          const all = await SteamClient.Apps.GetSubscribedWorkshopItems({appid});
          if (!all.length) return '[]';
          const more = new Map();
          try {{
            for (const d of await SteamClient.Apps.GetSubscribedWorkshopItemDetails({appid}, all.map(i => i.publishedfileid))) more.set(d.publishedfileid, d);
          }} catch (e) {{}}
          return JSON.stringify(all.map(i => ({{ id: String(i.publishedfileid), title: more.get(i.publishedfileid)?.title || String(i.publishedfileid),
            size: Number(i.file_size) || 0, off: !!i.disabled_locally, image: more.get(i.publishedfileid)?.preview_url || '' }})));
        }})()"
    ))
}

/// Ids of workshop items are numbers too long for JavaScript: texts of digits.
fn digits(id: &str) -> anyhow::Result<&str> {
    anyhow::ensure!(
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()),
        "not an id"
    );
    Ok(id)
}

pub fn set_workshop(appid: u32, id: &str, on: bool) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Apps.SetWorkshopItemsDisabledLocally({appid}, ['{}'], {}); 1",
        digits(id)?,
        !on
    ))?;
    Ok(())
}

pub fn unsubscribe(appid: u32, id: &str) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Apps.SubscribeWorkshopItem({appid}, '{}', false); 1",
        digits(id)?
    ))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Screenshot {
    pub game: String,
    pub handle: u64,
    pub path: String,
    pub at: u64,
}

/// A game's screenshots on this computer, the newest first.
pub fn screenshots(appid: u32) -> anyhow::Result<Vec<Screenshot>> {
    let mut all: Vec<Screenshot> = parsed(&format!(
        r"(async () => {{
          const n = await SteamClient.Screenshots.GetAllAppsLocalScreenshotsCount();
          const out = [];
          for (let i = 0; i < n && out.length < 300; i += 500) {{
            for (const s of await SteamClient.Screenshots.GetAllAppsLocalScreenshotsRange(i, Math.min(n - 1, i + 499))) {{
              if (s.nAppID !== {appid} && s.strGameID !== '{appid}') continue;
              out.push(s);
            }}
          }}
          return JSON.stringify(await Promise.all(out.slice(0, 300).map(async s => ({{ game: String(s.strGameID), handle: Number(s.hHandle), at: Number(s.nCreated) || 0,
            path: await SteamClient.Screenshots.GetLocalScreenshotPath(s.strGameID, s.hHandle) || '' }}))));
        }})()"
    ))?;
    all.sort_by_key(|s| std::cmp::Reverse(s.at));
    Ok(all)
}

pub fn delete_screenshot(s: &Screenshot) -> anyhow::Result<()> {
    let v = run(&format!(
        "SteamClient.Screenshots.DeleteLocalScreenshots([{{ gameID: {}, rgHandles: [{}] }}]).then(r => !!r.bSuccess)",
        quoted(&s.game),
        s.handle
    ))?;
    anyhow::ensure!(v == Value::Bool(true), "Steam did not delete it");
    Ok(())
}

/// Pictures of `urls` in the cache, fetched when they are not there. Blocking.
pub fn fetch_pictures(urls: &[String]) {
    std::thread::scope(|s| {
        for part in urls.chunks(urls.len().div_ceil(8).max(1)) {
            s.spawn(move || {
                for url in part {
                    if let Some(to) = picture(url)
                        && !to.is_file()
                    {
                        let _ = crate::progress::quietly(|| crate::tools::download(url, &to));
                    }
                }
            });
        }
    });
}

/// Where a picture of Steam's is kept.
pub fn picture(url: &str) -> Option<std::path::PathBuf> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    // Steam's own servers only.
    if ![
        "steamstatic.com",
        "steamusercontent.com",
        "steamcontent.com",
    ]
    .iter()
    .any(|h| host == *h || host.ends_with(&format!(".{h}")))
    {
        return None;
    }
    let name: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let name: String = name
        .chars()
        .rev()
        .take(120)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some(dirs::cache_dir()?.join("arkor/steam").join(name))
}

#[cfg(test)]
mod guide_tests {
    #[test]
    fn settings_message() {
        assert_eq!(super::base64(&[0x90, 0xeb, 0x06, 0]), "kOsGAA==");
        assert_eq!(super::base64(&[0x90, 0xeb, 0x06, 1]), "kOsGAQ==");
        assert_eq!(super::base64(b"Man"), "TWFu");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_cannot_leave_their_quotes() {
        assert_eq!(quoted("a\"b\\c\n"), r#""a\"b\\c\n""#);
        assert!(digits("3151296519").is_ok());
        assert!(digits("1'); evil('").is_err() && digits("").is_err());
    }

    #[test]
    fn reads_what_steam_answers() {
        // As a real Steam answered.
        let f: Vec<Folder> = serde_json::from_str(
            r#"[{"nFolderIndex":1,"strFolderPath":"/run/media/me/x/SteamLibrary","strUserLabel":"","nFreeSpace":22702751744,"bIsDefaultFolder":false,"bIsMounted":true,"apps":[730]}]"#,
        )
        .unwrap();
        assert_eq!((f[0].index, f[0].apps[0]), (1, 730));
        let a: Action = serde_json::from_str(r#"{"id":4,"game":"730","kind":"LaunchApp","task":"SynchronizingCloud","request":"SynchronizingCloud","details":"cloudconflict"}"#).unwrap();
        assert_eq!(
            (a.id, a.details.as_str(), a.total),
            (4, "cloudconflict", 0.)
        );
    }

    #[test]
    fn news_come_once() {
        let mv = |error| {
            Some(Happening {
                moving: Some(Move {
                    appid: 5,
                    error,
                    ..Move::default()
                }),
                ..Happening::default()
            })
        };
        news();
        told(mv(MOVING));
        assert_eq!(moving().map(|m| m.appid), Some(5));
        told(mv(0));
        assert!(moving().is_none());
        assert_eq!(news(), [News::Moved(5)]);
        told(mv(0));
        assert!(news().is_empty());
        told(Some(Happening {
            errors: vec![("730".into(), "AppError_12".into())],
            unlocked: vec![730],
            ..Happening::default()
        }));
        assert_eq!(news().len(), 2);
        assert!(news().is_empty());
        told(None);
    }

    #[test]
    fn pictures_come_from_steam_only() {
        let ok =
            picture("https://shared.steamstatic.com/community_assets/images/apps/230410/a.jpg");
        assert!(ok.is_some_and(|p| p.to_string_lossy().ends_with("230410_a_jpg")));
        assert!(picture("https://evilsteamstatic.com/a.jpg").is_none());
        assert!(picture("http://shared.steamstatic.com/a.jpg").is_none());
        assert!(picture("file:///etc/passwd").is_none());
    }
}
