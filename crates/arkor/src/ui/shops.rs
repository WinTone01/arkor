//! Store games in the window: what is known about the accounts and their games, and the
//! actions on them (logging in, installing, removing). The page is in `shops_page`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use arkor_core::{GameId, Store};
use arkor_launch::shops::{self, Info, Installed, Login, Owned, Shop};
use arkor_launch::{downloads, progress};
use gpui::{App, Context};

use super::{Library, cover_art};

/// A store game: the shop and its id there.
pub(super) type ShopGame = (Shop, String);

#[derive(Default)]
pub(super) struct ShopsState {
    /// The login window program was looked for in the release already.
    helper_asked: bool,
    /// The store shown; `None` = all of them.
    pub(super) tab: Option<Shop>,
    pub(super) only_installed: bool,
    pub(super) query: String,
    owned: HashMap<Shop, Vec<Owned>>,
    installed: HashMap<ShopGame, Installed>,
    logged_in: HashSet<Shop>,
    accounts: HashMap<Shop, String>,
    /// Asked the store in this session already.
    asked: HashSet<Shop>,
    pub(super) refreshing: HashSet<Shop>,
    pub(super) errors: HashMap<Shop, String>,
    covers: HashMap<ShopGame, Option<PathBuf>>,
    /// Covers to fetch, the one asked for last first.
    wanted: Vec<ShopGame>,
    fetching: usize,
    wide: HashMap<ShopGame, Option<PathBuf>>,
    /// Wide pictures being fetched one by one.
    wide_pending: HashSet<ShopGame>,
    /// All pictures being fetched at once (see `Library::shops_prefetch`).
    pub(super) prefetch: Option<Prefetch>,
    /// Store answers asked for ahead of time, and how many are on their way.
    info_asked: HashSet<ShopGame>,
    info_running: usize,
    /// Pictures fetched (or tried) in this session.
    tried: HashSet<ShopGame>,
    pub(super) login: Option<LoginUi>,
    pub(super) game: Option<GameUi>,
    /// A window to open with the page (see `Library::open`).
    pub(super) on_open: Option<(String, String)>,
    /// Removing (uninstalling) right now.
    pub(super) removing: HashSet<ShopGame>,
}

/// Fetching every picture the stores have for the account's games: the covers first (the
/// page waits for them), then the wide ones.
pub(super) struct Prefetch {
    pub(super) done: Arc<AtomicUsize>,
    pub(super) total: usize,
    pub(super) covers_ready: bool,
    started: Instant,
}

/// The page shows its games after this even when pictures are missing.
const WAIT_AT_MOST: Duration = Duration::from_secs(20);

/// Covers fetched at the same time.
const PARALLEL: usize = 8;

/// Fetches with a few threads; counts into `done`.
fn fetch_all<T: Send>(jobs: Vec<T>, threads: usize, done: &AtomicUsize, fetch: impl Fn(T) + Sync) {
    let jobs = std::sync::Mutex::new(jobs);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let job = jobs.lock().unwrap_or_else(|e| e.into_inner()).pop();
                    let Some(job) = job else { return };
                    fetch(job);
                    done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
}

/// The login window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum LoginMode {
    /// Not yet open.
    Starting,
    /// Arkor's own login window (`arkor-login`), by its process id: the code is taken from
    /// it, nothing has to be copied.
    Window(u32),
    /// That window was closed before the login ended.
    Closed,
    /// The user's browser: the code is copied by hand.
    Browser,
}

/// Arkor's login window program, next to Arkor (or on the PATH).
fn login_helper() -> Option<PathBuf> {
    let next = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("arkor-login")))
        .filter(|p| p.is_file());
    next.or_else(|| arkor_stores::epic::find_in_path("arkor-login"))
}

pub(super) struct LoginUi {
    pub(super) shop: Shop,
    /// Set when the store's page is known.
    pub(super) login: Option<Login>,
    pub(super) busy: bool,
    pub(super) error: Option<String>,
    /// The field to paste into is shown.
    pub(super) paste: bool,
    /// Where the login happens.
    pub(super) mode: LoginMode,
    /// The login window's input, for the gamepad's buttons in console mode.
    pub(super) pad: Option<std::sync::Arc<std::sync::Mutex<std::process::ChildStdin>>>,
    /// What was on the clipboard before: not the code.
    clipboard: Option<String>,
    request: u64,
}

/// The window of one store game (installing it, or what can be done when it is installed).
pub(super) struct GameUi {
    pub(super) shop: Shop,
    pub(super) game: Owned,
    pub(super) info: Option<Result<Info, String>>,
    pub(super) linux: bool,
    pub(super) language: String,
    /// The DLC chosen to install with it.
    pub(super) dlcs: HashSet<String>,
    /// The language list is open.
    pub(super) languages_open: bool,
    pub(super) dir: PathBuf,
    pub(super) error: Option<String>,
    pub(super) confirm_remove: bool,
    /// How it will run (Windows builds): its Proton (`None` = the default) and prefix.
    pub(super) proton: Option<String>,
    pub(super) prefix: Option<PathBuf>,
    pub(super) proton_open: bool,
    /// Also in the app menu, and on the desktop.
    pub(super) menu: bool,
    pub(super) desktop: bool,
    /// What is known about it on Linux (anti-cheat, ProtonDB, Steam Deck).
    pub(super) compat: Option<arkor_launch::compat::Compat>,
    /// "Install anyway" was asked for a game whose anti-cheat keeps Linux out.
    pub(super) confirm_anyway: bool,
    request: u64,
}

impl GameUi {
    pub(super) fn key(&self) -> ShopGame {
        (self.shop, self.game.id.clone())
    }

    /// The store's answer: its language and every DLC chosen to begin with.
    fn take_info(&mut self, info: Result<Info, String>, wanted: &str) {
        if let Ok(info) = &info {
            self.language = pick_language(&info.languages, wanted);
            self.dlcs = info.dlcs.iter().map(|d| d.id.clone()).collect();
        }
        self.info = Some(info);
    }

    /// What the choices need on disk: the game and the chosen DLC.
    pub(super) fn needed(&self) -> Option<u64> {
        let info = self.info.as_ref()?.as_ref().ok()?;
        let game = info.disk.or(info.download)?.max(info.download.unwrap_or(0));
        let dlc: u64 = info
            .dlcs
            .iter()
            .filter(|d| self.dlcs.contains(&d.id))
            .filter_map(|d| d.size)
            .sum();
        Some(game + dlc)
    }

    /// Why it cannot be installed, if so (not enough space).
    pub(super) fn missing_space(&self) -> Option<(u64, u64)> {
        let need = self.needed()?;
        let free = shops::free_space(&self.dir)?;
        (need > free).then_some((need, free))
    }
}

impl ShopsState {
    /// Reads what is known on disk (small files).
    pub(super) fn load() -> Self {
        let mut s = Self::default();
        for shop in Shop::ALL {
            s.read(shop);
        }
        s
    }

    fn read(&mut self, shop: Shop) {
        if shops::logged_in(shop) {
            self.logged_in.insert(shop);
        } else {
            self.logged_in.remove(&shop);
        }
        match shops::account(shop) {
            Some(name) => self.accounts.insert(shop, name),
            None => self.accounts.remove(&shop),
        };
        self.owned.insert(shop, shops::owned(shop));
        self.read_installed(shop);
    }

    fn read_installed(&mut self, shop: Shop) {
        self.installed.retain(|(s, _), _| *s != shop);
        for g in shops::installed(shop) {
            self.installed.insert((shop, g.id.clone()), g);
        }
    }

    pub(super) fn logged_in(&self, shop: Shop) -> bool {
        self.logged_in.contains(&shop)
    }

    pub(super) fn account(&self, shop: Shop) -> Option<&str> {
        self.accounts.get(&shop).map(String::as_str)
    }

    pub(super) fn owned(&self, shop: Shop) -> &[Owned] {
        self.owned.get(&shop).map_or(&[], Vec::as_slice)
    }

    pub(super) fn is_installed(&self, shop: Shop, id: &str) -> bool {
        self.installed.contains_key(&(shop, id.to_string()))
    }

    pub(super) fn installed(&self, shop: Shop, id: &str) -> Option<&Installed> {
        self.installed.get(&(shop, id.to_string()))
    }

    pub(super) fn find(&self, shop: Shop, id: &str) -> Option<&Owned> {
        self.owned(shop).iter().find(|g| g.id == id)
    }

    /// The games of `tab` that match the search, by title.
    pub(super) fn shown(&self, tab: Option<Shop>) -> Vec<(Shop, usize)> {
        let needle = self.query.trim().to_lowercase();
        let mut out: Vec<(Shop, usize)> = Shop::ALL
            .into_iter()
            .filter(|s| tab.is_none_or(|t| t == *s))
            .flat_map(|s| (0..self.owned(s).len()).map(move |i| (s, i)))
            .filter(|(s, i)| {
                let g = &self.owned(*s)[*i];
                (needle.is_empty() || g.title.to_lowercase().contains(&needle))
                    && (!self.only_installed || self.is_installed(*s, &g.id))
            })
            .collect();
        out.sort_by_cached_key(|(s, i)| self.owned(*s)[*i].title.to_lowercase());
        out
    }

    /// Tests put games here without a store.
    #[cfg(test)]
    pub(super) fn set_owned(&mut self, shop: Shop, games: Vec<Owned>) {
        self.logged_in.insert(shop);
        self.asked.insert(shop);
        self.owned.insert(shop, games);
    }
}

impl Library {
    /// Asks the logged-in stores for their games once per session.
    pub(super) fn shops_opened(&mut self, cx: &mut Context<Self>) {
        match self.shops.on_open.take() {
            Some((what, shop)) if what == "login" => {
                if let Some(shop) = Shop::from_key(&shop) {
                    self.shop_login(shop, cx);
                }
            }
            Some((shop, id)) => {
                if let Some(shop) = Shop::from_key(&shop) {
                    self.shop_open_game(shop, &id, cx);
                }
            }
            None => {}
        }
        for shop in Shop::ALL {
            if self.shops.logged_in(shop) && !self.shops.asked.contains(&shop) {
                self.shops_refresh(shop, cx);
            }
        }
        // What the account owned last time can be pictured right away.
        self.shops_prefetch(cx);
    }

    pub(super) fn shops_refresh(&mut self, shop: Shop, cx: &mut Context<Self>) {
        // Headless tests never talk to a store.
        if cfg!(test) || !self.shops.refreshing.insert(shop) {
            return;
        }
        self.shops.asked.insert(shop);
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { shops::refresh(shop) }).await;
            let _ = this.update(cx, |this, cx| {
                this.shops.refreshing.remove(&shop);
                match result {
                    Ok(_) => {
                        this.shops.errors.remove(&shop);
                    }
                    Err(e) => {
                        this.shops.errors.insert(shop, format!("{e:#}"));
                    }
                }
                this.shops.read(shop);
                this.shops_prefetch(cx);
                // A screen opened on a game that was not known yet.
                if let super::console::Screen::ShopGame(s, id) = this.console.screen()
                    && s == shop
                    && this.shops.game.is_none()
                {
                    this.shop_open_game(s, &id, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether the page should still wait for its pictures (`(done, total)` then).
    pub(super) fn shops_preparing(&self) -> Option<(usize, usize)> {
        let p = self.shops.prefetch.as_ref()?;
        (!p.covers_ready && p.started.elapsed() < WAIT_AT_MOST)
            .then(|| (p.done.load(Ordering::Relaxed).min(p.total), p.total))
    }

    /// Fetches every missing picture of the account's games, covers first.
    pub(super) fn shops_prefetch(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || self.shops.prefetch.is_some() {
            return;
        }
        let mut covers = Vec::new();
        let mut wides = Vec::new();
        for shop in Shop::ALL {
            for g in self.shops.owned(shop).to_vec() {
                if !self.shops.tried.insert((shop, g.id.clone())) {
                    continue;
                }
                if shops::covers::cover(shop, &g.id).is_none() {
                    covers.push((shop, g.clone()));
                }
                if shops::covers::wide(shop, &g.id).is_none() {
                    wides.push((shop, g.clone()));
                }
            }
        }
        if covers.is_empty() && wides.is_empty() {
            return;
        }
        // The games at the top of the page first (the threads take from the end).
        covers.reverse();
        wides.reverse();
        let done = Arc::new(AtomicUsize::new(0));
        self.shops.prefetch = Some(Prefetch {
            done: done.clone(),
            total: covers.len(),
            covers_ready: covers.is_empty(),
            started: Instant::now(),
        });
        cx.notify();
        let bg = cx.background_executor().clone();
        let timer = bg.clone();
        // The count on the page moves while it waits.
        cx.spawn(async move |this, cx| {
            loop {
                timer.timer(Duration::from_millis(200)).await;
                let waiting = this.update(cx, |this, cx| {
                    cx.notify();
                    this.shops_preparing().is_some()
                });
                if !matches!(waiting, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let counter = done.clone();
            bg.spawn(async move {
                fetch_all(covers, PARALLEL, &counter, |(shop, g)| {
                    let _ = shops::covers::fetch_cover(shop, &g);
                })
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                // Read from disk again: what was missing is there now.
                this.shops.covers.clear();
                if let Some(p) = this.shops.prefetch.as_mut() {
                    p.covers_ready = true;
                }
                cx.notify();
            });
            let unused = Arc::new(AtomicUsize::new(0));
            bg.spawn(async move {
                fetch_all(wides, PARALLEL / 2, &unused, |(shop, g)| {
                    let _ = shops::covers::fetch_wide(shop, &g);
                })
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                this.shops.wide.clear();
                this.shops.prefetch = None;
                // Games a refresh found meanwhile.
                this.shops_prefetch(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether the wide picture of a store game is still on its way.
    pub(super) fn shop_wide_pending(&self, shop: Shop, id: &str) -> bool {
        let key = (shop, id.to_string());
        self.shops.wide_pending.contains(&key)
            || (self.shops.prefetch.is_some() && shops::covers::wide(shop, id).is_none())
    }

    /// A helper's error in plain words (the usual ones).
    pub(super) fn shop_error(&self, error: &str) -> String {
        if error.contains("Login attempt failed") || error.contains("did not accept") {
            self.tr(
                "The code was not accepted: open the page again",
                "Kod kabul edilmedi: sayfayı yeniden aç",
            )
            .into()
        } else if login_ended(error) {
            self.tr(
                "Login ended: log in again",
                "Giriş sona erdi: yeniden giriş yap",
            )
            .into()
        } else if error == "the store helper failed" || error.contains("request failed") {
            self.tr("The store did not answer", "Mağaza yanıt vermedi")
                .into()
        } else {
            error.to_string()
        }
    }

    // ---- Pictures ----

    /// The portrait picture of a store game; asks for it when it is not there yet.
    pub(super) fn shop_cover(
        &mut self,
        shop: Shop,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<PathBuf> {
        let key = (shop, id.to_string());
        if let Some(known) = self.shops.covers.get(&key) {
            return known.clone();
        }
        let path = shops::covers::cover(shop, id);
        self.shops.covers.insert(key.clone(), path.clone());
        if path.is_none() && !cfg!(test) {
            self.shops.wanted.push(key);
            self.fetch_covers(cx);
        }
        path
    }

    /// A few at a time; what was scrolled to last comes first.
    fn fetch_covers(&mut self, cx: &mut Context<Self>) {
        while self.shops.fetching < 4
            && let Some((shop, id)) = self.shops.wanted.pop()
        {
            let Some(game) = self.shops.find(shop, &id).cloned() else {
                continue;
            };
            self.shops.fetching += 1;
            let bg = cx.background_executor().clone();
            cx.spawn(async move |this, cx| {
                let path = bg
                    .spawn(async move { shops::covers::fetch_cover(shop, &game).ok() })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.shops.fetching -= 1;
                    this.shops.covers.insert((shop, id), path);
                    this.fetch_covers(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// The wide picture of a store game; asks for it when it is not there yet.
    pub(super) fn shop_wide(
        &mut self,
        shop: Shop,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<PathBuf> {
        let key = (shop, id.to_string());
        if let Some(known) = self.shops.wide.get(&key) {
            return known.clone();
        }
        let path = shops::covers::wide(shop, id);
        // Asked for again once the picture is there.
        if path.is_some() || self.shops.prefetch.is_none() {
            self.shops.wide.insert(key.clone(), path.clone());
        }
        if path.is_none()
            && !cfg!(test)
            && self.shops.prefetch.is_none()
            && let Some(game) = self.shops.find(shop, id).cloned()
        {
            self.shops.wide_pending.insert(key.clone());
            let bg = cx.background_executor().clone();
            cx.spawn(async move |this, cx| {
                let path = bg
                    .spawn(async move { shops::covers::fetch_wide(shop, &game).ok() })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.shops.wide_pending.remove(&key);
                    this.shops.wide.insert(key, path);
                    cx.notify();
                });
            })
            .detach();
        }
        path
    }

    // ---- Logging in and out ----

    /// Opens the login window and the store's page in the browser.
    pub(super) fn shop_login(&mut self, shop: Shop, cx: &mut Context<Self>) {
        // Steam is logged in in Steam itself: it opens (with its window) for that.
        if shop == Shop::Steam {
            self.console.steam_wanted = Some(Instant::now());
            if !cfg!(test) {
                let _ = std::process::Command::new("steam")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            self.status = Some(
                self.tr(
                    "Log in in Steam; Arkor sees your library then.",
                    "Steam'de giriş yap; Arkor kütüphaneni sonra görür.",
                )
                .into(),
            );
            self.shops.asked.remove(&shop);
            cx.notify();
            return;
        }
        self.fields.remove("shop-code");
        let request = self.shops.login.as_ref().map_or(0, |l| l.request) + 1;
        self.shops.login = Some(LoginUi {
            shop,
            login: None,
            busy: true,
            error: None,
            paste: false,
            mode: LoginMode::Starting,
            pad: None,
            clipboard: clipboard_text(cx),
            request,
        });
        cx.notify();
        if cfg!(test) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { shops::begin_login(shop) }).await;
            let _ = this.update(cx, |this, cx| {
                let Some(l) = this.shops.login.as_mut().filter(|l| l.request == request) else {
                    return;
                };
                l.busy = false;
                match result {
                    Ok(login) => {
                        l.login = Some(login);
                        this.shop_login_window(cx);
                    }
                    Err(e) => l.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens the store's page in Arkor's login window; without one, in the browser.
    pub(super) fn shop_login_window(&mut self, cx: &mut Context<Self>) {
        let Some(l) = self.shops.login.as_mut() else {
            return;
        };
        let Some(login) = l.login.clone() else {
            return;
        };
        let (shop, request) = (l.shop, l.request);
        let console = self.console.on;
        let turkish = self.global.language == arkor_core::Language::Tr;
        // An older updater did not bring the login window: fetched once.
        if !cfg!(test)
            && login_helper().is_none()
            && !std::mem::replace(&mut self.shops.helper_asked, true)
        {
            l.mode = LoginMode::Starting;
            let bg = cx.background_executor().clone();
            cx.spawn(async move |this, cx| {
                let got = bg
                    .spawn(async { arkor_launch::self_update::add_login_window() })
                    .await;
                if let Err(e) = got {
                    eprintln!("arkor: the login window could not be fetched: {e:#}");
                }
                let _ = this.update(cx, |this, cx| {
                    if this
                        .shops
                        .login
                        .as_ref()
                        .is_some_and(|l| l.request == request)
                    {
                        this.shop_login_window(cx);
                    }
                });
            })
            .detach();
            return cx.notify();
        }
        let mut child = login_helper().and_then(|bin| {
            let mut cmd = std::process::Command::new(bin);
            // Console mode: full screen, with the gamepad and the on-screen keyboard.
            if console {
                cmd.arg("--console");
            }
            if turkish {
                cmd.arg("--tr");
            }
            cmd.args([shop.key(), &login.url])
                .stdin(if console {
                    std::process::Stdio::piped()
                } else {
                    std::process::Stdio::null()
                })
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
        });
        let Some(mut child) = child.take() else {
            return self.shop_login_browser(cx);
        };
        l.mode = LoginMode::Window(child.id());
        l.pad = child
            .stdin
            .take()
            .map(|s| std::sync::Arc::new(std::sync::Mutex::new(s)));
        l.error = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let out = bg.spawn(async move { child.wait_with_output() }).await;
            let _ = this.update(cx, |this, cx| {
                let Some(l) = this.shops.login.as_mut().filter(|l| l.request == request) else {
                    return;
                };
                match out {
                    Ok(out) if out.status.success() => {
                        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        l.mode = LoginMode::Closed;
                        match shops::find_code(shop, &text) {
                            Some(code) => this.shop_finish_login(code, cx),
                            None => {
                                if let Some(l) = this.shops.login.as_mut() {
                                    l.error = Some("no login code came".into());
                                }
                            }
                        }
                    }
                    // Closed by the user.
                    Ok(out) if out.status.code() == Some(1) => l.mode = LoginMode::Closed,
                    // No window could be opened (no WebKitGTK): the browser it is.
                    _ => this.shop_login_browser(cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The browser way: the store's page there, the code copied back by hand.
    pub(super) fn shop_login_browser(&mut self, cx: &mut Context<Self>) {
        let Some(l) = self.shops.login.as_mut() else {
            return;
        };
        if let LoginMode::Window(pid) = l.mode {
            // SAFETY: plain syscall on the login window's own process.
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
        l.mode = LoginMode::Browser;
        let request = l.request;
        if let Some(login) = &l.login {
            cx.open_url(&login.url);
        }
        self.watch_clipboard(request, cx);
        cx.notify();
    }

    /// While the login window is open: a code (or the page's address) copied in the browser
    /// is taken as soon as Arkor's window is in use again.
    fn watch_clipboard(&mut self, request: u64, cx: &mut Context<Self>) {
        let timer = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                timer.timer(Duration::from_millis(600)).await;
                let open = this.update(cx, |this, cx| {
                    let active = this.window_active;
                    let Some(l) = this.shops.login.as_ref().filter(|l| l.request == request) else {
                        return false;
                    };
                    if !active || l.busy {
                        return true;
                    }
                    let text = clipboard_text(cx);
                    if text == l.clipboard {
                        return true;
                    }
                    let found = text.as_deref().and_then(|t| clipboard_code(l.shop, t));
                    if let Some(l) = this.shops.login.as_mut() {
                        l.clipboard = text;
                    }
                    if let Some(code) = found {
                        this.shop_finish_login(code, cx);
                    }
                    true
                });
                if !matches!(open, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// Takes what was pasted into the login window.
    pub(super) fn shop_login_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(l) = self.shops.login.as_ref() else {
            return;
        };
        match shops::find_code(l.shop, text) {
            Some(code) => self.shop_finish_login(code, cx),
            None => {
                let message = self.tr(
                    "There is no login code in this text.",
                    "Bu metinde giriş kodu yok.",
                );
                if let Some(l) = self.shops.login.as_mut() {
                    l.error = Some(message.into());
                }
                cx.notify();
            }
        }
    }

    fn shop_finish_login(&mut self, code: String, cx: &mut Context<Self>) {
        let Some(l) = self.shops.login.as_mut() else {
            return;
        };
        let (Some(login), false) = (l.login.clone(), l.busy) else {
            return;
        };
        l.busy = true;
        l.error = None;
        let (shop, request) = (l.shop, l.request);
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move { shops::finish_login(&login, &code) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.shops.read(shop);
                let current = this.shops.login.as_mut().filter(|l| l.request == request);
                match result {
                    Ok(()) => {
                        if current.is_some() {
                            this.shops.login = None;
                        }
                        this.shops_refresh(shop, cx);
                    }
                    Err(e) => {
                        if let Some(l) = current {
                            l.busy = false;
                            l.error = Some(format!("{e:#}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn shop_logout(&mut self, shop: Shop, cx: &mut Context<Self>) {
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { shops::logout(shop) }).await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.status = Some(format!("{e:#}").into());
                }
                this.shops.asked.remove(&shop);
                this.shops.errors.remove(&shop);
                this.shops.read(shop);
                cx.notify();
            });
        })
        .detach();
    }

    // ---- Installing and removing ----

    /// Opens the window of a store game.
    pub(super) fn shop_open_game(&mut self, shop: Shop, id: &str, cx: &mut Context<Self>) {
        let game = match self.shops.find(shop, id).cloned() {
            Some(g) => g,
            // Installed, but the account's list does not have it (logged out).
            None => match self.shops.installed(shop, id) {
                Some(g) => Owned {
                    id: g.id.clone(),
                    title: g.title.clone(),
                    cover: None,
                    wide: None,
                    linux: g.linux,
                    windows: !g.linux,
                    namespace: None,
                },
                None => return,
            },
        };
        let linux = game.linux;
        self.shops.game = Some(GameUi {
            shop,
            linux,
            language: String::new(),
            dlcs: HashSet::new(),
            languages_open: false,
            dir: self.global.games_dir(),
            game,
            info: None,
            error: None,
            confirm_remove: false,
            proton: None,
            prefix: None,
            proton_open: false,
            menu: true,
            desktop: false,
            compat: None,
            confirm_anyway: false,
            request: 0,
        });
        if !self.shops.is_installed(shop, id) {
            self.shop_ask_info(cx);
        }
        self.shop_ask_compat(cx);
        cx.notify();
    }

    /// The lookup of a store game on AreWeAntiCheatYet, ProtonDB and Steam Deck.
    fn compat_lookup(&self, shop: Shop, id: &str) -> arkor_launch::compat::Lookup {
        let game = self.shops.find(shop, id);
        arkor_launch::compat::Lookup {
            name: game.map(|g| g.title.clone()).unwrap_or_default(),
            platform: shop.key().into(),
            id: id.to_string(),
            epic_namespace: game.and_then(|g| g.namespace.clone()),
        }
    }

    /// What is known about the open game on Linux (cached at once, else asked for).
    fn shop_ask_compat(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.shops.game.as_ref() else {
            return;
        };
        let (shop, id) = (g.shop, g.game.id.clone());
        let lookup = self.compat_lookup(shop, &id);
        if let Some(c) = arkor_launch::compat::cached(&lookup) {
            if let Some(g) = self.shops.game.as_mut() {
                g.compat = Some(c);
            }
            return;
        }
        if cfg!(test) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let c = bg
                .spawn(async move { arkor_launch::compat::lookup(&lookup) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(g) = this
                    .shops
                    .game
                    .as_mut()
                    .filter(|g| g.shop == shop && g.game.id == id)
                {
                    g.compat = Some(c);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Asks the store about a game ahead of time (hovered or focused), so its window has the
    /// answer when it opens. Two at a time at most.
    pub(super) fn shop_prefetch_info(&mut self, shop: Shop, id: &str, cx: &mut Context<Self>) {
        let key = (shop, id.to_string());
        if cfg!(test)
            || !self.shops.logged_in(shop)
            || self.shops.is_installed(shop, id)
            || self.shops.info_running >= 2
            || !self.shops.info_asked.insert(key)
        {
            return;
        }
        let linux = self.shops.find(shop, id).is_some_and(|g| g.linux);
        if shops::cached_info(shop, id, linux).is_some() {
            return;
        }
        self.shops.info_running += 1;
        let id = id.to_string();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let _ = bg.spawn(async move { shops::info(shop, &id, linux) }).await;
            let _ = this.update(cx, |this, _| this.shops.info_running -= 1);
        })
        .detach();
    }

    /// Asks the store what installing the open game means.
    pub(super) fn shop_ask_info(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.shops.game.as_mut() else {
            return;
        };
        g.request += 1;
        g.info = None;
        g.error = None;
        let (shop, id, linux, request) = (g.shop, g.game.id.clone(), g.linux, g.request);
        let wanted = match self.global.language {
            arkor_core::Language::Tr => "tr",
            arkor_core::Language::En => "en",
        };
        // A recent answer is there at once.
        if let Some(info) = shops::cached_info(shop, &id, linux) {
            g.take_info(Ok(info), wanted);
            cx.notify();
            return;
        }
        cx.notify();
        if cfg!(test) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let asked = id.clone();
            let result = bg
                .spawn(async move { shops::info(shop, &asked, linux) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(g) = this
                    .shops
                    .game
                    .as_mut()
                    .filter(|g| g.shop == shop && g.game.id == id && g.request == request)
                else {
                    return;
                };
                g.take_info(result.map_err(|e| format!("{e:#}")), wanted);
                cx.notify();
            });
        })
        .detach();
    }

    /// Queues the open game's installation.
    pub(super) fn shop_install(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.shops.game.as_mut() else {
            return;
        };
        let info = match &g.info {
            Some(Ok(info)) => info.clone(),
            // Not in yet: the usual choices (the download finds the sizes itself).
            None => Info::default(),
            Some(Err(_)) => return,
        };
        // A late answer must not reopen anything.
        g.request += 1;
        let dlcs = g.dlcs.iter().cloned().collect();
        let mut order = shops::order(&g.game, &info, &g.dir, g.linux, &g.language, dlcs);
        order.proton = g.proton.clone();
        order.prefix = g.prefix.clone();
        order.menu = g.menu;
        order.desktop = g.desktop;
        match shops::install(g.shop, &g.game.id, order) {
            Ok(()) => self.shops.game = None,
            Err(e) => g.error = Some(format!("{e:#}")),
        }
        cx.notify();
    }

    pub(super) fn shop_repair(&mut self, shop: Shop, id: &str, cx: &mut Context<Self>) {
        downloads::enqueue(&shops::repair_key(shop, id));
        self.shops.game = None;
        cx.notify();
    }

    /// Deletes an installed store game and its library entry (its prefix and settings stay,
    /// so a later installation goes on where it was).
    pub(super) fn shop_uninstall(&mut self, shop: Shop, id: &str, cx: &mut Context<Self>) {
        let key = (shop, id.to_string());
        if !self.shops.removing.insert(key.clone()) {
            return;
        }
        self.shops.game = None;
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let (shop, id) = key.clone();
            let result = bg.spawn(async move { shops::uninstall(shop, &id) }).await;
            let _ = this.update(cx, |this, cx| {
                this.shops.removing.remove(&key);
                this.shops.read_installed(key.0);
                match result {
                    Ok(()) => this.forget_shop_game(&key),
                    Err(e) => this.status = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn forget_shop_game(&mut self, (shop, id): &ShopGame) {
        let Some(entry) = self.shop_entry(*shop, id) else {
            return;
        };
        let open = self.entry().is_some_and(|g| g.id == entry);
        self.library.remove(&entry);
        self.save_library();
        if open {
            self.set_selection(super::Selection::Grid);
        }
        self.reload_games();
    }

    /// The library entry of a store game.
    pub(super) fn shop_entry(&self, shop: Shop, id: &str) -> Option<String> {
        let origin = shops::origin(shop, id);
        self.library
            .games
            .iter()
            .find(|g| g.origin.as_deref() == Some(&origin))
            .map(|g| g.id.clone())
    }

    /// The store game behind a library entry, when Arkor installed it.
    pub(super) fn entry_shop(&self, entry: &arkor_core::library::LibraryGame) -> Option<ShopGame> {
        let (shop, id) = shops::from_origin(entry.origin.as_deref()?)?;
        self.shops
            .is_installed(shop, id)
            .then(|| (shop, id.to_string()))
    }

    /// Opens the library page of a store game, adding it when it is not in the library.
    pub(super) fn shop_show_in_library(&mut self, shop: Shop, id: &str, cx: &mut Context<Self>) {
        if self.shop_entry(shop, id).is_none() {
            self.add_shop_game(shop, id, cx);
        }
        if let Some(entry) = self.shop_entry(shop, id) {
            self.shops.game = None;
            self.select_game(&GameId::new(Store::Local, entry), cx);
        }
    }

    /// Plays an installed store game.
    pub(super) fn shop_play(&mut self, shop: Shop, id: &str, cx: &mut Context<Self>) {
        self.shop_show_in_library(shop, id, cx);
        if self.shop_entry(shop, id).is_some() {
            self.play(cx);
        }
    }

    /// Games whose installation ended join the library; called when the queue moved.
    pub(super) fn shops_after_download(&mut self, cx: &mut Context<Self>) {
        let fresh = shops::take_fresh();
        for shop in Shop::ALL {
            self.shops.read_installed(shop);
        }
        for (shop, id) in fresh {
            self.add_shop_game(shop, &id, cx);
        }
    }

    fn add_shop_game(&mut self, shop: Shop, id: &str, cx: &mut Context<Self>) {
        let new = match shops::library_entry(shop, id) {
            Ok(new) => new,
            Err(e) => {
                self.status = Some(format!("{e:#}").into());
                return;
            }
        };
        if let Some(entry) = self.shop_entry(shop, id) {
            // Installed again (perhaps elsewhere, or the other platform's build).
            if let Some(g) = self.library.get_mut(&entry) {
                g.kind = new.kind;
                g.exe = new.exe;
                g.working_dir = new.working_dir;
            }
            self.save_library();
            self.reload_games();
            return;
        }
        let Some(added) = self.add_entry(new, cx) else {
            return;
        };
        // What was chosen when it was installed.
        if let Some(order) = shops::order_of(shop, id) {
            if let Some(prefix) = order.prefix.clone()
                && let Some(g) = self.library.get_mut(&added.id)
            {
                g.prefix = Some(prefix);
                self.save_library();
            }
            if order.proton.is_some() {
                let mut profile = arkor_core::Profile::load(&added).unwrap_or_default();
                profile.proton = order.proton.clone();
                if let Err(e) = profile.save(&added) {
                    self.status = Some(format!("{e:#}").into());
                }
            }
            if order.menu || order.desktop {
                let name = self
                    .library
                    .get(&added.id)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
                let _ = arkor_launch::shortcuts::add_menu_entry(&added, &name, None, order.desktop);
            }
        }
        let bg = cx.background_executor().clone();
        let (id, entry) = (id.to_string(), added.id);
        cx.spawn(async move |this, cx| {
            let (shop_id, library_id) = (id.clone(), entry.clone());
            let result = bg
                .spawn(async move { shops::covers::use_as_cover(shop, &shop_id, &library_id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(_) => this.refresh_cover(&entry),
                    // The store had none: SteamGridDB, when it knows the game.
                    Err(_) => this.auto_cover(&entry, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A newer version of an installed store game is known.
    pub(super) fn shop_update(&self, shop: Shop, id: &str) -> bool {
        let key = shops::key(shop, id);
        self.downloads.updates.iter().any(|u| u.key == key)
    }

    /// How a store game's download or repair stands: the queue key and its progress.
    pub(super) fn shop_job(&self, shop: Shop, id: &str) -> Option<(String, Option<f32>)> {
        let (active, items) = (progress::list(), downloads::items());
        if active.is_empty() && items.is_empty() {
            return None;
        }
        for key in [shops::key(shop, id), shops::repair_key(shop, id)] {
            if let Some(e) = active.iter().find(|e| e.key == key) {
                return Some((key, Some(e.fraction().unwrap_or(0.))));
            }
            if items.iter().any(|i| i.key == key) {
                return Some((key, None));
            }
        }
        None
    }
}

/// The store wants a new login.
pub(super) fn login_ended(error: &str) -> bool {
    let e = error.to_lowercase();
    ["log in", "login", "logged in", "credentials"]
        .iter()
        .any(|m| e.contains(m))
}

fn clipboard_text(cx: &App) -> Option<String> {
    // The headless test platform has no clipboard worth reading.
    if cfg!(test) {
        return None;
    }
    cx.read_from_clipboard().and_then(|c| c.text())
}

/// A login code on the clipboard. Stricter than pasted text: only what is clearly the
/// store's page or address (a bare code could be anything the user copied).
fn clipboard_code(shop: Shop, text: &str) -> Option<String> {
    let marker = match shop {
        Shop::Epic => "authorizationCode",
        Shop::Gog => "on_login_success",
        Shop::Amazon => "openid.oa2.authorization_code",
        Shop::Steam => return None,
    };
    let bare_epic = shop == Shop::Epic && text.trim().len() == 32;
    (text.len() < 8192 && (text.contains(marker) || bare_epic))
        .then(|| shops::find_code(shop, text))
        .flatten()
}

/// The interface's language when the game has it, else English, else the first.
fn pick_language(offered: &[String], wanted: &str) -> String {
    let find = |code: &str| {
        offered
            .iter()
            .find(|l| l.split('-').next() == Some(code))
            .cloned()
    };
    find(wanted)
        .or_else(|| find("en"))
        .or_else(|| offered.first().cloned())
        .unwrap_or_default()
}

/// Drops store pictures that were not drawn for a while when there are many.
pub(super) fn trim_covers(window: &mut gpui::Window) {
    static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
        return;
    }
    *last = Some(Instant::now());
    drop(last);
    cover_art::trim("arkor/stores", 140, window);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_choice() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_language(&l(&["en-US", "tr-TR"]), "tr"), "tr-TR");
        assert_eq!(pick_language(&l(&["de-DE", "en-US"]), "tr"), "en-US");
        assert_eq!(pick_language(&l(&["de-DE"]), "tr"), "de-DE");
        assert_eq!(pick_language(&[], "tr"), "");
    }

    #[test]
    fn the_clipboard_is_only_taken_when_it_is_clearly_a_login() {
        let code = "x".repeat(60);
        assert_eq!(clipboard_code(Shop::Gog, &code), None, "could be anything");
        let url = format!("https://embed.gog.com/on_login_success?origin=client&code={code}");
        assert_eq!(clipboard_code(Shop::Gog, &url), Some(code));
        assert_eq!(clipboard_code(Shop::Amazon, "ANabcDEFghiJKLmno"), None);
        assert!(clipboard_code(Shop::Epic, &"a1".repeat(16)).is_some());
        assert_eq!(clipboard_code(Shop::Epic, "just text"), None);
    }
}
