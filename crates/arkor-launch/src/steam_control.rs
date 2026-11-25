//! Steam's client, controlled: its downloads are read and steered through the interface of
//! its own windows (the one its Downloads page uses), reached over Chromium's developer port.
//!
//! The port is only opened when the user switched this on, and only by a Steam that Arkor
//! starts: Steam looks for an empty marker file when it starts, which is put there for that
//! moment and taken away again as soon as the port is open. It listens on this computer only,
//! but it asks for no password: any program running here could use it, which is why it is off
//! unless asked for.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::Deserialize;
use serde_json::{Value, json};

const PORT: u16 = 8080;
const MARKER: &str = ".cef-enable-remote-debugging";

/// 0: not looked up yet, 1: off, 2: on.
static WANTED: AtomicU8 = AtomicU8::new(0);
/// The last state was read through the port.
static ON: AtomicBool = AtomicBool::new(false);
static CLIENT: Mutex<Option<Client>> = Mutex::new(None);

/// Switched on or off in the settings; it takes effect when Steam starts the next time.
pub fn set_wanted(on: bool) {
    WANTED.store(if on { 2 } else { 1 }, Ordering::Relaxed);
    if !on {
        ON.store(false, Ordering::Relaxed);
        *lock() = None;
    }
}

/// Whether it is switched on (the programs that start games look it up themselves).
pub fn wanted() -> bool {
    match WANTED.load(Ordering::Relaxed) {
        0 => {
            let on =
                !cfg!(test) && arkor_core::Global::load_or_import().is_ok_and(|g| g.steam_control);
            WANTED.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        v => v == 2,
    }
}

/// Switched on or off: a Steam that runs starts anew, with its port or without. Not while a
/// game runs (then it takes effect later). Blocking.
pub fn apply(on: bool) -> anyhow::Result<()> {
    set_wanted(on);
    if !crate::steam_link::steam_running() || crate::steam_link::any_game_running() {
        return Ok(());
    }
    crate::steam_link::shutdown()?;
    crate::steam_link::start_silent()
}

/// Whether Steam answers now.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

fn lock() -> std::sync::MutexGuard<'static, Option<Client>> {
    CLIENT.lock().unwrap_or_else(|e| e.into_inner())
}

fn address() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], PORT))
}

fn port_open() -> bool {
    TcpStream::connect_timeout(&address(), Duration::from_millis(300)).is_ok()
}

/// Steam is about to be started: lets it open the port, when that is wanted and nothing else
/// has it. The marker goes as soon as the port is open.
pub(crate) fn before_start() {
    let Some(marker) = crate::steam_link::root().map(|r| r.join(MARKER)) else {
        return;
    };
    // One that is there belongs to another program (it stays).
    if !wanted() || cfg!(test) || marker.exists() || port_open() {
        return;
    }
    let Some(note) = note() else {
        return;
    };
    let _ = note.parent().map(std::fs::create_dir_all);
    if std::fs::write(&note, b"").is_err() || std::fs::write(&marker, b"").is_err() {
        return;
    }
    std::thread::spawn(move || {
        let began = Instant::now();
        while !port_open() && began.elapsed() < Duration::from_secs(120) {
            std::thread::sleep(Duration::from_millis(250));
        }
        let _ = std::fs::remove_file(marker);
        let _ = std::fs::remove_file(note);
    });
}

/// A marker a crash left behind would open the port of every later start.
pub(crate) fn clean_up() {
    let note = note();
    if let (Some(note), Some(marker)) = (note, crate::steam_link::root().map(|r| r.join(MARKER)))
        // Not one that Steam is being started with at this moment.
        && note.metadata().and_then(|m| m.modified()).is_ok_and(|t| {
            t.elapsed().is_ok_and(|e| e > Duration::from_secs(150))
        })
    {
        let _ = std::fs::remove_file(marker);
        let _ = std::fs::remove_file(note);
    }
}

fn note() -> Option<PathBuf> {
    Some(dirs::state_dir()?.join("arkor/steam-marker"))
}

// ---- The connection: just enough of WebSocket, for text ----

struct Client {
    stream: TcpStream,
    next: u64,
}

/// The size of the largest answer taken.
const MOST: usize = 8 * 1024 * 1024;

fn connect() -> anyhow::Result<TcpStream> {
    let s = TcpStream::connect_timeout(&address(), Duration::from_millis(600))?;
    s.set_read_timeout(Some(Duration::from_secs(6)))?;
    s.set_write_timeout(Some(Duration::from_secs(3)))?;
    Ok(s)
}

/// The path of the page that has Steam's interface.
fn page_path(list: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Target {
        title: String,
        #[serde(rename = "webSocketDebuggerUrl")]
        url: Option<String>,
    }
    let targets: Vec<Target> = serde_json::from_str(list).ok()?;
    let url = targets
        .into_iter()
        .find(|t| t.title == "SharedJSContext")?
        .url?;
    // Only here, whatever it says.
    let path = url.strip_prefix("ws://")?.split_once('/')?.1.to_string();
    path.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/-".contains(&b))
        .then(|| format!("/{path}"))
}

fn http_list() -> anyhow::Result<String> {
    let mut s = connect()?;
    write!(
        s,
        "GET /json HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nConnection: close\r\n\r\n"
    )?;
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        anyhow::ensure!(head.len() < 16_384, "no answer");
        s.read_exact(&mut byte)?;
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).to_lowercase();
    let length: usize = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .context("no length")?;
    anyhow::ensure!(length < MOST, "too long");
    let mut body = vec![0; length];
    s.read_exact(&mut body)?;
    Ok(String::from_utf8(body)?)
}

/// A text frame as a client sends it.
fn frame(text: &str, mask: [u8; 4]) -> Vec<u8> {
    let data = text.as_bytes();
    let mut out = vec![0x81];
    match data.len() {
        n @ 0..=125 => out.push(0x80 | n as u8),
        n @ 126..=65_535 => {
            out.push(0x80 | 126);
            out.extend((n as u16).to_be_bytes());
        }
        n => {
            out.push(0x80 | 127);
            out.extend((n as u64).to_be_bytes());
        }
    }
    out.extend(mask);
    out.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    out
}

/// The next message.
fn message(s: &mut impl Read) -> anyhow::Result<String> {
    let mut all = Vec::new();
    loop {
        let mut head = [0u8; 2];
        s.read_exact(&mut head)?;
        let n = match head[1] & 127 {
            126 => {
                let mut b = [0u8; 2];
                s.read_exact(&mut b)?;
                u16::from_be_bytes(b) as usize
            }
            127 => {
                let mut b = [0u8; 8];
                s.read_exact(&mut b)?;
                usize::try_from(u64::from_be_bytes(b))?
            }
            n => n as usize,
        };
        anyhow::ensure!(all.len() + n < MOST, "too long");
        let mut data = vec![0; n];
        s.read_exact(&mut data)?;
        match head[0] & 15 {
            8 => anyhow::bail!("closed"),
            // Pings and pongs.
            9 | 10 => continue,
            _ => all.extend(data),
        }
        if head[0] & 128 != 0 {
            return Ok(String::from_utf8(all)?);
        }
    }
}

impl Client {
    fn open() -> anyhow::Result<Client> {
        let path = page_path(&http_list()?).context("Steam is not ready")?;
        let mut stream = connect()?;
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: YXJrb3ItYXJrb3ItYXJrbw==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )?;
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            anyhow::ensure!(head.len() < 16_384, "no answer");
            stream.read_exact(&mut byte)?;
            head.push(byte[0]);
        }
        anyhow::ensure!(head.starts_with(b"HTTP/1.1 101"), "refused");
        Ok(Client { stream, next: 0 })
    }

    /// Runs `js` in Steam's page; its (awaited) value.
    fn run(&mut self, js: &str) -> anyhow::Result<Value> {
        self.next += 1;
        let ask = json!({
            "id": self.next,
            "method": "Runtime.evaluate",
            "params": {"expression": js, "awaitPromise": true, "returnByValue": true},
        });
        // The mask only keeps old proxies from being confused; there is none here.
        let mask = (self.next as u32 ^ 0x5a17_c3e9).to_be_bytes();
        self.stream.write_all(&frame(&ask.to_string(), mask))?;
        let began = Instant::now();
        loop {
            anyhow::ensure!(began.elapsed() < Duration::from_secs(8), "no answer");
            let answer: Value = serde_json::from_str(&message(&mut self.stream)?)?;
            if answer["id"] != self.next {
                continue;
            }
            let result = &answer["result"];
            anyhow::ensure!(
                result.get("exceptionDetails").is_none(),
                "Steam: {}",
                result["exceptionDetails"]["exception"]["description"]
                    .as_str()
                    .unwrap_or("an error")
                    .lines()
                    .next()
                    .unwrap_or_default()
            );
            return Ok(result["result"]["value"].clone());
        }
    }
}

/// Runs `js` over the connection that is open, or a new one.
pub(crate) fn run(js: &str) -> anyhow::Result<Value> {
    anyhow::ensure!(wanted(), "not switched on");
    let mut client = lock();
    if let Some(c) = client.as_mut() {
        match c.run(js) {
            Ok(v) => return Ok(v),
            // Steam's own refusal; the connection is fine.
            Err(e) if e.to_string().starts_with("Steam: ") => return Err(e),
            Err(_) => *client = None,
        }
    }
    let mut c = Client::open()?;
    let v = c.run(js)?;
    *client = Some(c);
    Ok(v)
}

// ---- Downloads ----

/// What Steam's Downloads page has and what it does with games, kept up to date by Steam
/// itself.
const WATCH: &str = r"(async () => {
  if (typeof SteamClient === 'undefined' || !SteamClient.Downloads || typeof appStore === 'undefined') return null;
  const g = globalThis;
  if (!g.__arkor) {
    const k = g.__arkor = { items: [], overview: null, actions: {}, errors: [], unlocked: [], install: null, moving: null };
    const quiet = f => { try { f(); } catch (e) {} };
    SteamClient.Downloads.RegisterForDownloadItems((all, list) => {
      for (const x of list) {
        if (x.remote_client_id !== '0') continue;
        k.items = x.item_data.map(i => ({ ...(k.items.find(o => o.appid === i.appid) || {}), ...i }));
      }
    });
    SteamClient.Downloads.RegisterForDownloadOverview(o => {
      if (o.remote_client_id === undefined || o.remote_client_id === '0') k.overview = o;
    });
    const at = (id, game, kind) => k.actions[id] || (k.actions[id] = { id, game: String(game), kind: String(kind), task: '', request: '', details: '', done: 0, total: 0 });
    quiet(() => SteamClient.Apps.RegisterForGameActionStart((id, game, kind) => { at(id, game, kind); }));
    quiet(() => SteamClient.Apps.RegisterForGameActionEnd(id => { delete k.actions[id]; }));
    quiet(() => SteamClient.Apps.RegisterForGameActionTaskChange((id, game, kind, task) => {
      const a = at(id, game, kind); a.task = String(task); a.request = ''; a.details = ''; a.done = a.total = 0; }));
    quiet(() => SteamClient.Apps.RegisterForGameActionUserRequest((id, game, kind, request, details) => {
      if (request === 'CreatingProcess') return;
      const a = at(id, game, kind); a.request = String(request); a.details = String(details || ''); }));
    quiet(() => SteamClient.Apps.RegisterForGameActionShowError((id, game, kind, error, details) => {
      k.errors.push([String(game), String(error) + (details ? ' ' + details : '')]); delete k.actions[id]; }));
    quiet(() => SteamClient.Installs.RegisterForShowInstallWizard(i => {
      k.install = { state: i.eInstallState || 0, app: i.currentAppID || 0, error: i.eAppError || 0 }; }));
    quiet(() => SteamClient.InstallFolder.RegisterForMoveContentProgress(m => {
      k.moving = { appid: m.appid, error: m.eError, progress: m.flProgress || 0, bytes: String(m.strBytesMoved || ''), total: String(m.strTotalBytesToMove || '') }; }));
    quiet(() => SteamClient.GameSessions.RegisterForAchievementNotification(n => {
      if (n.unAppID && !k.unlocked.includes(n.unAppID)) k.unlocked.push(n.unAppID); }));
    try { for (const a of await SteamClient.Apps.GetActiveGameActions()) { const x = at(a.nGameActionID, a.gameid, a.strActionName); x.task = a.strTaskName || ''; } } catch (e) {}
  }
  const k = g.__arkor, o = k.overview;
  for (const a of Object.values(k.actions)) {
    try { SteamClient.Apps.GetGameActionDetails(a.id, d => { if (d) { a.done = Number(d.strNumDone) || 0; a.total = Number(d.strNumTotal) || 0; } }); } catch (e) {}
  }
  const name = id => { try { return appStore.GetAppOverviewByAppID(id)?.display_name || ''; } catch (e) { return ''; } };
  const part = (p, n) => p && p[n] ? [p[n].bytes_in_progress, p[n].bytes_total] : [0, 0];
  return JSON.stringify({
    now: o ? { appid: o.update_appid, state: o.update_state, speed: o.update_network_bytes_per_second,
      paused: o.paused, bytes: part(o.progress, 2) } : null,
    items: k.items.filter(i => !i.completed).map(i => ({ appid: i.appid, name: name(i.appid), active: !!i.active,
      paused: !!i.paused, place: i.queue_index, bytes: part((i.update_type_info || [])[0]?.progress, 2) })),
    happening: { actions: Object.values(k.actions), errors: k.errors.splice(0), unlocked: k.unlocked.splice(0), moving: k.moving },
  });
})()";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(crate) struct Now {
    pub appid: u32,
    pub state: String,
    pub speed: f64,
    pub paused: bool,
    pub bytes: (f64, f64),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(crate) struct Queued {
    pub appid: u32,
    pub name: String,
    pub active: bool,
    pub paused: bool,
    pub place: i32,
    pub bytes: (f64, f64),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub(crate) struct State {
    pub now: Option<Now>,
    pub items: Vec<Queued>,
    #[serde(default)]
    pub happening: crate::steam_api::Happening,
}

/// Steam's downloads as it has them; `None` when it does not answer.
pub(crate) fn state() -> Option<State> {
    if !wanted() {
        return None;
    }
    let mut state: Option<State> = run(WATCH)
        .ok()
        .and_then(|v| serde_json::from_str(v.as_str()?).ok())
        // Until Steam is logged in and has said something.
        .filter(|s: &State| s.now.is_some());
    crate::steam_api::told(state.as_mut().map(|s| std::mem::take(&mut s.happening)));
    ON.store(state.is_some(), Ordering::Relaxed);
    state
}

pub fn pause(appid: u32) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Downloads.PauseAppUpdate({appid}, '0'); 1"
    ))?;
    Ok(())
}

/// Goes on with one, and with downloading at all.
pub fn resume(appid: u32) -> anyhow::Result<()> {
    run(&format!(
        "SteamClient.Downloads.ResumeAppUpdate({appid}, '0'); SteamClient.Downloads.EnableAllDownloads(true, '0'); 1"
    ))?;
    Ok(())
}

/// Puts one at `place` of the queue (0 = the one that downloads).
pub fn place(appid: u32, place: usize) -> anyhow::Result<()> {
    let go = if place == 0 {
        "SteamClient.Downloads.EnableAllDownloads(true, '0');"
    } else {
        ""
    };
    run(&format!(
        "SteamClient.Downloads.SetQueueIndex({appid}, {place}, '0'); {go} 1"
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_as_a_client_sends_them() {
        assert_eq!(
            frame("Hello", [0x37, 0xfa, 0x21, 0x3d]),
            [
                0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58
            ]
        );
        assert_eq!(frame(&"x".repeat(300), [0; 4])[..4], [0x81, 0xfe, 1, 44]);
        assert_eq!(frame(&"x".repeat(70_000), [0; 4])[1], 0xff);
    }

    #[test]
    fn messages_are_put_together() {
        // A ping in between two parts.
        let mut raw: &[u8] = &[
            0x01, 3, b'H', b'e', b'l', 0x89, 1, b'!', 0x80, 2, b'l', b'o',
        ];
        assert_eq!(message(&mut raw).unwrap(), "Hello");
        let mut closed: &[u8] = &[0x88, 0];
        assert!(message(&mut closed).is_err());
        let mut huge: &[u8] = &[0x81, 127, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        assert!(message(&mut huge).is_err(), "no memory for nonsense");
    }

    #[test]
    fn only_steams_page_is_spoken_to() {
        let list = r#"[{"title":"Steam","webSocketDebuggerUrl":"ws://127.0.0.1:8080/devtools/page/AA"},
            {"title":"SharedJSContext","webSocketDebuggerUrl":"ws://127.0.0.1:8080/devtools/page/0B1C"}]"#;
        assert_eq!(page_path(list).as_deref(), Some("/devtools/page/0B1C"));
        assert_eq!(page_path("[]"), None);
        let odd = r#"[{"title":"SharedJSContext","webSocketDebuggerUrl":"ws://x/a b\r\nX: y"}]"#;
        assert_eq!(page_path(odd), None);
    }

    #[test]
    fn reads_what_steam_says() {
        // As a real Steam answered.
        let s: State = serde_json::from_str(
            r#"{"now":{"appid":1305420,"state":"Downloading","speed":10251294,"paused":false,"bytes":[374018224,2180800320]},
            "items":[{"appid":1305420,"name":"Agrou","active":true,"paused":false,"place":0,"bytes":[244991616,2180800320]}]}"#,
        )
        .unwrap();
        assert_eq!(s.now.unwrap().bytes.1, 2_180_800_320.);
        assert_eq!((s.items[0].place, s.items[0].active), (0, true));
    }

    /// With a Steam whose port is open: what it downloads (nothing is changed).
    #[test]
    #[ignore = "needs a running Steam with its port open"]
    fn real_steam() {
        set_wanted(true);
        for _ in 0..3 {
            println!("{:#?}", state());
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
