//! A game's mods, on its page: through Amethyst Mod Manager (see `arkor_launch::mods`).
//! The list is read when the panel is first drawn for a game and after every change; each
//! change is placed in the game folder at once. An installer's questions (a FOMOD) are
//! asked here; everything else Amethyst does opens in its own window.

use std::path::PathBuf;

use arkor_core::GameId;
use arkor_launch::mods::nexus::{self, Account, Login};
use arkor_launch::mods::{self, Answer, Choices, Installed, Link, Mod, amethyst, link};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, PathPromptOptions, div, prelude::*, px};
use serde_json::{Value, json};

use super::console::{Act, Item, Kind, Overlay, Row, Value as Shown};
use super::style::{self, pal};
use super::{Library, widgets};

/// What is known of the selected game's mods.
#[derive(Debug, Clone, Default)]
pub(super) enum Known {
    #[default]
    Nothing,
    Reading,
    /// Not linked yet: Amethyst's game it would be (if one fits), and where the game is.
    Unlinked {
        fits: Option<link::Candidate>,
        place: Option<link::Place>,
    },
    Linked {
        link: Link,
        mods: Vec<Mod>,
        /// The game's name on Nexus Mods, when its mods come from there.
        domain: Option<String>,
        /// The mod loader the mods need but the game folder lacks.
        loader: Option<&'static str>,
        /// Its community on Thunderstore, when its mods come from there.
        thunderstore: Option<String>,
    },
    Failed(String),
}

/// An installer's questions while they are answered.
#[derive(Debug, Clone)]
pub(super) struct Asking {
    archive: PathBuf,
    choices: Choices,
    /// FOMOD: `{step: {group: [options]}}`, starting from the author's recommendation.
    picked: Value,
}

#[derive(Debug, Default)]
pub(super) struct ModsUi {
    /// The game this is about (its id as a string).
    game: Option<String>,
    pub(super) known: Known,
    /// What is being done now (shown instead of the buttons).
    busy: Option<&'static str>,
    asking: Option<Asking>,
    /// The mod asked about before it is removed.
    removing: Option<String>,
    /// The Nexus Mods account (`None` until asked; `Some(None)`: nobody signed in).
    account: Option<Option<Account>>,
    /// Whether the browser hands Nexus links to Arkor (asked once).
    handler: Option<bool>,
    /// A login waits in the browser.
    signing_in: bool,
    /// Arkor's mod browser, while it is open.
    pub(super) browser: Option<Browser>,
}

/// Arkor's mod browser (`arkor-login browse`): Nexus Mods or Thunderstore in a window of
/// Arkor's, whose downloads come to the game it was opened for.
#[derive(Debug)]
pub(super) struct Browser {
    pid: u32,
    /// Its input, for the gamepad's buttons in console mode.
    pub(super) pad: Option<std::sync::Arc<std::sync::Mutex<std::process::ChildStdin>>>,
    /// The game whose mods it brings.
    game: GameId,
}

impl Library {
    /// Whether Arkor has Amethyst.
    pub(super) fn mods_ready(&self) -> bool {
        amethyst::installed_version().is_some()
    }

    fn mods_running(&self, id: &GameId) -> bool {
        self.running.contains_key(&id.to_string())
    }

    /// Reads the selected game's mods (again), off the UI thread.
    fn mods_load(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(game)) = (self.selected_id(), self.entry().cloned()) else {
            return;
        };
        self.mods.game = Some(id.to_string());
        self.mods.known = Known::Reading;
        let prefixes = self.global.prefixes_dir();
        let ask_account = self.mods.account.is_none();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let key = id.to_string();
            let (known, account, handler) = bg
                .spawn(async move {
                    let mut account = None;
                    let known = amethyst::with(|c| {
                        if let Some(mut link) = link::load(&id) {
                            // What its loaders need from Wine, kept for the next launch.
                            let _ = link::refresh(c, &id, &mut link);
                            let found = link::games(c)?.into_iter().find(|g| g.id == link.game);
                            let thunderstore = found
                                .as_ref()
                                .and_then(|g| g.thunderstore_community.clone());
                            let domain = found.and_then(|g| g.nexus.into_iter().next());
                            if ask_account && domain.is_some() {
                                account = nexus::account(c).ok();
                            }
                            let loader = link::place(&game, &prefixes)
                                .and_then(|p| mods::missing_loader(&p.game_dir));
                            return Ok(Known::Linked {
                                mods: mods::list(c, &link)?,
                                link,
                                domain,
                                loader,
                                thunderstore,
                            });
                        }
                        let place = link::place(&game, &prefixes);
                        let games = link::games(c)?;
                        let fits = link::best_match(
                            &games,
                            &game.name,
                            place.as_ref().and_then(|p| p.steam_appid.as_deref()),
                        )
                        .cloned();
                        Ok(Known::Unlinked { fits, place })
                    })
                    .unwrap_or_else(|e| Known::Failed(format!("{e:#}")));
                    let handler = ask_account.then(nexus::is_handler);
                    (known, account, handler)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if account.is_some() {
                    this.mods.account = account;
                }
                if handler.is_some() {
                    this.mods.handler = handler;
                }
                if this.mods.game.as_deref() == Some(key.as_str()) {
                    this.mods.known = known;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Does `work` with Amethyst off the UI thread, then reads the list again.
    fn mods_do(
        &mut self,
        doing: &'static str,
        cx: &mut Context<Self>,
        work: impl FnOnce(&mut mods::rpc::Client) -> anyhow::Result<()> + Send + 'static,
    ) {
        if self.mods.busy.is_some() {
            return;
        }
        self.mods.busy = Some(doing);
        self.mods.removing = None;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { amethyst::with(work) }).await;
            let _ = this.update(cx, |this, cx| {
                this.mods.busy = None;
                if let Err(e) = result {
                    this.status = Some(format!("{e:#}").into());
                }
                this.mods_load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn mods_link(
        &mut self,
        candidate: link::Candidate,
        place: link::Place,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected_id() else { return };
        self.mods_do(
            self.tr("Setting up…", "Hazırlanıyor…"),
            cx,
            move |c| link::connect(c, &id, &candidate, &place).map(|_| ()),
        );
    }

    /// Picks an archive, then installs it.
    fn mods_pick_archive(&mut self, link: Link, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(self.tr("Install a mod", "Mod kur").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.mods_install(link, path, None, cx));
        })
        .detach();
    }

    fn mods_install(
        &mut self,
        link: Link,
        archive: PathBuf,
        answer: Option<Answer>,
        cx: &mut Context<Self>,
    ) {
        if self.mods.busy.is_some() {
            return;
        }
        self.mods.busy = Some(self.tr("Installing…", "Kuruluyor…"));
        self.mods.asking = None;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let path = archive.clone();
            let result = bg
                .spawn(async move {
                    amethyst::with(|c| mods::install(c, &link, &path, answer.as_ref()))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.mods.busy = None;
                match result {
                    Ok(Installed::Done(name)) => {
                        this.status =
                            Some(format!("{} {name}", this.tr("Installed:", "Kuruldu:")).into());
                    }
                    Ok(Installed::Asks(choices)) => {
                        this.mods.asking = Some(Asking {
                            archive,
                            picked: choices.defaults(),
                            choices,
                        });
                    }
                    Err(e) => this.status = Some(format!("{e:#}").into()),
                }
                this.mods_load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Signs in to Nexus Mods in the browser; waits (up to the five minutes Nexus allows)
    /// until the login there ends.
    fn mods_sign_in(&mut self, cx: &mut Context<Self>) {
        if self.mods.signing_in {
            return;
        }
        self.mods.signing_in = true;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let started = bg.spawn(async { amethyst::with(nexus::login_start) }).await;
            let mut outcome = match started {
                Ok(page) => {
                    // In Arkor's own browser, whose login then stays for finding mods.
                    let _ = this.update(cx, |this, cx| this.mods_browse(page, cx));
                    None
                }
                Err(e) => Some(Err(e)),
            };
            for _ in 0..330 {
                if outcome.is_some() {
                    break;
                }
                bg.timer(std::time::Duration::from_secs(1)).await;
                let stopped = this
                    .update(cx, |this, _| !this.mods.signing_in)
                    .unwrap_or(true);
                if stopped {
                    let _ = bg
                        .spawn(async { amethyst::with(nexus::login_cancel) })
                        .await;
                    return;
                }
                match bg.spawn(async { amethyst::with(nexus::login_state) }).await {
                    Ok(Login::Done) => outcome = Some(Ok(())),
                    Ok(Login::Failed(why)) => outcome = Some(Err(anyhow::anyhow!(why))),
                    Ok(_) => {}
                    Err(e) => outcome = Some(Err(e)),
                }
            }
            let account = bg.spawn(async { amethyst::with(nexus::account) }).await;
            let _ = this.update(cx, |this, cx| {
                this.mods.signing_in = false;
                match outcome {
                    Some(Err(e)) => this.status = Some(format!("{e:#}").into()),
                    None => {
                        this.status = Some(
                            this.tr("The login was not finished.", "Giriş tamamlanmadı.")
                                .into(),
                        )
                    }
                    Some(Ok(())) => {}
                }
                this.mods.account = Some(account.ok().flatten());
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens Arkor's mod browser at `url` for the selected game; the user's browser when
    /// Arkor's cannot open.
    pub(super) fn mods_browse(&mut self, url: String, cx: &mut Context<Self>) {
        let Some(game) = self.selected_id() else {
            return;
        };
        if let Some(b) = self.mods.browser.take() {
            // SAFETY: plain syscall on the browser's own process.
            unsafe { libc::kill(b.pid as i32, libc::SIGTERM) };
        }
        let console = self.console.on;
        let child = super::shops::login_helper().and_then(|bin| {
            let mut cmd = std::process::Command::new(bin);
            if console {
                cmd.arg("--console");
            }
            if self.global.language == arkor_core::Language::Tr {
                cmd.arg("--tr");
            }
            cmd.args(["browse", &url])
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
        let Some(mut child) = child else {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            return;
        };
        let pid = child.id();
        if console && super::desk::available() {
            let bg = cx.background_executor().clone();
            cx.background_executor()
                .spawn(async move {
                    // It shows itself once its page is drawn (up to five seconds).
                    for wait in [250, 700, 1500, 3000, 5500] {
                        bg.timer(std::time::Duration::from_millis(wait)).await;
                        super::desk::show_process(pid);
                    }
                })
                .detach();
        }
        self.mods.browser = Some(Browser {
            pid,
            pad: child
                .stdin
                .take()
                .map(|s| std::sync::Arc::new(std::sync::Mutex::new(s))),
            game,
        });
        let (tx, rx) = async_channel::unbounded::<String>();
        let out = child.stdout.take();
        std::thread::spawn(move || {
            use std::io::BufRead;
            if let Some(out) = out {
                for line in std::io::BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    if tx.send_blocking(line).is_err() {
                        break;
                    }
                }
            }
            let _ = child.wait();
        });
        cx.spawn(async move |this, cx| {
            while let Ok(line) = rx.recv().await {
                if this
                    .update(cx, |this, cx| this.mods_browser_said(line, cx))
                    .is_err()
                {
                    return;
                }
            }
            // The browser is closed: Arkor's window takes the gamepad again.
            let _ = this.update(cx, |this, cx| {
                if this.mods.browser.as_ref().is_some_and(|b| b.pid == pid) {
                    this.mods.browser = None;
                    if this.console.on {
                        this.console.come_back = true;
                    }
                    this.mods.game = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// A line from the mod browser: something to install for its game.
    fn mods_browser_said(&mut self, line: String, cx: &mut Context<Self>) {
        let Some(game) = self.mods.browser.as_ref().map(|b| b.game.clone()) else {
            return;
        };
        let Some((what, rest)) = line.split_once(' ') else {
            return;
        };
        match what {
            "nxm" => self.mods_nxm(rest.to_string(), cx),
            "ts" | "file" => {
                let Some(link) = link::load(&game) else {
                    return;
                };
                if what == "file" {
                    self.mods_install(link, PathBuf::from(rest), None, cx);
                    return;
                }
                let parts: Vec<String> = rest.split(' ').map(str::to_string).collect();
                let [ns, name, version] = parts.as_slice() else {
                    return;
                };
                self.status =
                    Some(format!("{} {name}", self.tr("Installing:", "Kuruluyor:")).into());
                let (ns, name, version) = (ns.clone(), name.clone(), version.clone());
                self.mods_do(self.tr("Installing…", "Kuruluyor…"), cx, move |c| {
                    mods::install_thunderstore(c, &link, &ns, &name, Some(&version)).map(|_| ())
                });
            }
            _ => {}
        }
        cx.notify();
    }

    /// Installs the game's mod loader (BepInEx) from Thunderstore; without a pack there,
    /// the mod browser looks for it on Nexus Mods.
    pub(super) fn mods_get_loader(&mut self, cx: &mut Context<Self>) {
        let Known::Linked { link, domain, .. } = self.mods.known.clone() else {
            return;
        };
        if self.mods.busy.is_some() {
            return;
        }
        self.mods.busy = Some(self.tr("Installing BepInEx…", "BepInEx kuruluyor…"));
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    amethyst::with(|c| match mods::find_bepinex(c, &link)? {
                        Some((ns, name)) => {
                            mods::install_thunderstore(c, &link, &ns, &name, None).map(Some)
                        }
                        None => Ok(None),
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.mods.busy = None;
                match result {
                    Ok(Some(name)) => {
                        this.status =
                            Some(format!("{} {name}", this.tr("Installed:", "Kuruldu:")).into())
                    }
                    Ok(None) => match domain {
                        Some(d) => this.mods_browse(
                            format!("https://www.nexusmods.com/{d}/search/?keyword=BepInEx"),
                            cx,
                        ),
                        None => {
                            this.status = Some(
                                this.tr(
                                    "No BepInEx package was found for this game.",
                                    "Bu oyun için bir BepInEx paketi bulunamadı.",
                                )
                                .into(),
                            )
                        }
                    },
                    Err(e) => this.status = Some(format!("{e:#}").into()),
                }
                this.mods_load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn mods_sign_out(&mut self, cx: &mut Context<Self>) {
        self.mods.account = Some(None);
        self.mods_do(
            self.tr("Signing out…", "Çıkış yapılıyor…"),
            cx,
            nexus::logout,
        );
    }

    fn mods_check_updates(&mut self, link: Link, cx: &mut Context<Self>) {
        if self.mods.busy.is_some() {
            return;
        }
        self.mods.busy = Some(self.tr("Checking…", "Denetleniyor…"));
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move { amethyst::with(|c| nexus::check_updates(c, &link)) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.mods.busy = None;
                this.status = Some(match result {
                    Ok(0) => this
                        .tr("Every mod is up to date.", "Bütün modlar güncel.")
                        .into(),
                    Ok(n) => format!(
                        "{n} {}",
                        this.tr("mods have a newer version.", "modun yeni sürümü var.")
                    )
                    .into(),
                    Err(e) => format!("{e:#}").into(),
                });
                this.mods_load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Takes the Nexus links `arkor nxm` left, every second, for as long as the window is open.
    pub(super) fn watch_nxm(&mut self, cx: &mut Context<Self>) {
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                let links = bg.spawn(async { nexus::take_handed() }).await;
                for url in links {
                    if this.update(cx, |this, cx| this.mods_nxm(url, cx)).is_err() {
                        return;
                    }
                }
                bg.timer(std::time::Duration::from_secs(1)).await;
            }
        })
        .detach();
    }

    /// A mod from Nexus Mods' "Mod Manager Download": downloaded and installed for the
    /// linked game it belongs to (the one on screen first), whose Mods tab then shows it.
    fn mods_nxm(&mut self, url: String, cx: &mut Context<Self>) {
        let Some(nx) = nexus::parse_nxm(&url) else {
            return;
        };
        let selected = self.selected_id();
        let mut linked: Vec<(GameId, Link)> = self
            .games
            .iter()
            .filter_map(|g| link::load(&g.id).map(|l| (g.id.clone(), l)))
            .collect();
        linked.sort_by_key(|(id, _)| Some(id) != selected.as_ref());
        self.status = Some(
            format!(
                "{} {}…",
                self.tr(
                    "Downloading from Nexus Mods:",
                    "Nexus Mods'tan indiriliyor:"
                ),
                nx.domain
            )
            .into(),
        );
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    amethyst::with(|c| {
                        let games = link::games(c)?;
                        let Some((id, link)) = linked.into_iter().find(|(_, l)| {
                            games
                                .iter()
                                .any(|g| g.id == l.game && g.nexus.contains(&nx.domain))
                        }) else {
                            anyhow::bail!(
                                "no game in Arkor uses mods from {} yet: open its Mods tab first",
                                nx.domain
                            );
                        };
                        let archive = nexus::download(c, &link, &url)?;
                        let installed = mods::install(c, &link, &archive, None)?;
                        Ok((id, link, archive, installed))
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((id, _link, archive, installed)) => {
                        if this.console.on {
                            this.console_open(super::console::Screen::Mods(id.id.clone()), cx);
                        } else {
                            this.select_game(&id, cx);
                            this.game_tab = super::features::GameTab::Mods;
                        }
                        this.mods.game = None;
                        match installed {
                            Installed::Done(name) => {
                                this.status = Some(
                                    format!("{} {name}", this.tr("Installed:", "Kuruldu:")).into(),
                                );
                            }
                            Installed::Asks(choices) => {
                                this.mods.asking = Some(Asking {
                                    archive,
                                    picked: choices.defaults(),
                                    choices,
                                });
                                this.status = None;
                            }
                        }
                    }
                    Err(e) => this.status = Some(this.nexus_error(&e).into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// What went wrong with Nexus Mods, in the user's words where Amethyst's are known.
    fn nexus_error(&self, e: &anyhow::Error) -> String {
        let name = e
            .downcast_ref::<mods::rpc::RpcError>()
            .map(|r| r.name.as_str())
            .unwrap_or_default();
        match name {
            "nexus_not_authenticated" | "nexus_auth_expired" => self
                .tr(
                    "Sign in to Nexus Mods first: the game's Mods tab has the button.",
                    "Önce Nexus Mods'a giriş yap: düğmesi oyunun Modlar sekmesinde.",
                )
                .into(),
            "nexus_download_link_expired" => self
                .tr(
                    "That download link has expired: press \"Mod Manager Download\" again.",
                    "İndirme bağlantısının süresi dolmuş: \"Mod Manager Download\" düğmesine yeniden bas.",
                )
                .into(),
            "nexus_premium_required" => self
                .tr(
                    "Nexus Mods only lets Premium accounts download this way.",
                    "Nexus Mods bu yoldan indirmeye yalnızca Premium hesaplarda izin veriyor.",
                )
                .into(),
            "nexus_rate_limited" => self
                .tr(
                    "Nexus Mods asks to wait a little before the next request.",
                    "Nexus Mods bir sonraki istekten önce biraz beklemeyi istiyor.",
                )
                .into(),
            _ => format!("{e:#}"),
        }
    }

    fn render_nexus(&self, domain: &str, link: &Link, cx: &mut Context<Self>) -> Div {
        let account = self.mods.account.clone().flatten();
        let who = match (&account, self.mods.signing_in) {
            (_, true) => self
                .tr(
                    "Finish the login in your browser…",
                    "Girişi tarayıcında tamamla…",
                )
                .to_string(),
            (Some(a), _) => format!(
                "{} · {}",
                a.name,
                if a.premium {
                    "Premium"
                } else {
                    self.tr("free account", "ücretsiz hesap")
                }
            ),
            (None, _) => self.tr("not signed in", "giriş yapılmadı").to_string(),
        };
        let hint = match &account {
            Some(a) if !a.premium => self.tr(
                "On a mod's page, press \"Mod Manager Download\": Arkor downloads and installs it here.",
                "Bir modun sayfasında \"Mod Manager Download\" düğmesine bas: Arkor indirip buraya kurar.",
            ),
            Some(_) => self.tr(
                "\"Mod Manager Download\" on a mod's page installs it here.",
                "Bir modun sayfasındaki \"Mod Manager Download\" onu buraya kurar.",
            ),
            None => self.tr(
                "Sign in to download mods from Nexus Mods and to see their updates.",
                "Nexus Mods'tan mod indirmek ve güncellemelerini görmek için giriş yap.",
            ),
        };
        let page = nexus::mods_page(domain);
        let mut buttons = div().flex().flex_wrap().gap_2();
        buttons = match (&account, self.mods.signing_in) {
            (_, true) => buttons.child(
                style::button("nexus-cancel", self.tr("Cancel", "Vazgeç"), false).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.mods.signing_in = false;
                        cx.notify();
                    }),
                ),
            ),
            (None, _) => {
                let page = page.clone();
                buttons
                    .child(
                        style::button("nexus-login", self.tr("Sign in", "Giriş yap"), true)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mods_sign_in(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        style::button("nexus-find", self.tr("Find mods", "Mod bul"), false)
                            .child(style::icon("search", 13., pal::muted()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.mods_browse(page.clone(), cx);
                                cx.notify();
                            })),
                    )
            }
            (Some(_), _) => {
                let link = link.clone();
                buttons
                    .child(
                        style::button("nexus-find", self.tr("Find mods", "Mod bul"), true)
                            .child(style::icon("search", 13., pal::on_accent()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.mods_browse(page.clone(), cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        style::button(
                            "nexus-updates",
                            self.tr("Check for updates", "Güncellemeleri denetle"),
                            false,
                        )
                        .child(style::icon("refresh-cw", 13., pal::muted()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.mods_check_updates(link.clone(), cx);
                            cx.notify();
                        })),
                    )
                    .child(
                        style::button("nexus-logout", self.tr("Sign out", "Çıkış"), false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mods_sign_out(cx);
                                cx.notify();
                            })),
                    )
            }
        };
        let mut out = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(px(10.))
            .border_1()
            .border_color(pal::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(style::icon("globe", 14., pal::muted()))
                    .child(div().text_sm().child("Nexus Mods"))
                    .child(div().text_sm().text_color(pal::muted()).child(who)),
            )
            .child(widgets::label(hint))
            .child(buttons);
        if account.is_some() && self.mods.handler == Some(false) {
            out = out.child(widgets::setting_row(
                self.tr("Open Nexus links in Arkor", "Nexus bağlantılarını Arkor açsın"),
                self.tr(
                    "Your browser then hands \"Mod Manager Download\" to Arkor instead of another mod manager.",
                    "Tarayıcın \"Mod Manager Download\" bağlantılarını başka bir mod yöneticisi yerine Arkor'a verir.",
                ),
                style::button("nexus-handler", self.tr("Set", "Ayarla"), false).on_click(
                    cx.listener(|this, _, _, cx| {
                        match nexus::become_handler() {
                            Ok(()) => this.mods.handler = Some(true),
                            Err(e) => this.status = Some(format!("{e:#}").into()),
                        }
                        cx.notify();
                    }),
                ),
            ));
        }
        out
    }

    pub(super) fn render_mods(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let id = self.selected_id()?;
        if self.mods.game.as_deref() != Some(id.to_string().as_str()) {
            self.mods.asking = None;
            self.mods.removing = None;
            self.mods_load(cx);
        }
        let mut card = widgets::section(self.tr("Mods", "Modlar"));
        if let Some(asking) = self.mods.asking.clone() {
            return Some(card.child(self.render_mods_question(asking, cx)));
        }
        let running = self.mods_running(&id);
        match self.mods.known.clone() {
            Known::Nothing | Known::Reading => {
                card = card.child(widgets::label(self.tr(
                    "Asking Amethyst Mod Manager…",
                    "Amethyst Mod Manager'a soruluyor…",
                )));
            }
            Known::Failed(e) => {
                card = card.child(widgets::label(e)).child(
                    div().flex().gap_2().child(
                        style::button("mods-retry", self.tr("Try again", "Tekrar dene"), false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mods_load(cx);
                                cx.notify();
                            })),
                    ),
                );
            }
            Known::Unlinked { fits, place } => {
                card = card.child(self.render_mods_unlinked(fits, place, cx));
            }
            Known::Linked {
                link,
                mods,
                domain,
                loader,
                ..
            } => {
                if let Some(loader) = loader {
                    card = card.child(self.render_loader_warning(loader, cx));
                }
                if let Some(domain) = &domain {
                    card = card.child(self.render_nexus(domain, &link, cx));
                }
                card = card.child(self.render_mods_list(link, mods, running, cx));
            }
        }
        Some(card)
    }

    fn render_mods_unlinked(
        &self,
        fits: Option<link::Candidate>,
        place: Option<link::Place>,
        cx: &mut Context<Self>,
    ) -> Div {
        let (Some(fits), Some(place)) = (fits, place.clone()) else {
            let text = if place.is_none() {
                self.tr(
                    "The game's folder could not be found, so its mods cannot be set up here.",
                    "Oyunun klasörü bulunamadı, bu yüzden modları burada ayarlanamıyor.",
                )
            } else {
                self.tr(
                    "Amethyst Mod Manager does not know this game yet. It can still be set up as a custom game in Amethyst's own window.",
                    "Amethyst Mod Manager bu oyunu henüz tanımıyor. Amethyst'in kendi penceresinde özel oyun olarak eklenebilir.",
                )
            };
            return div()
                .flex()
                .flex_col()
                .gap_3()
                .child(widgets::label(text))
                .child(div().flex().child(self.mods_window_button(cx)));
        };
        let text = format!(
            "{} {}. {}",
            self.tr("Amethyst knows this game as", "Amethyst bu oyunu şu adla tanıyor:"),
            fits.name,
            self.tr(
                "Mods go into the game's folder as links; taking them out leaves it as it was.",
                "Modlar oyunun klasörüne bağlantı olarak konur; çıkarılınca klasör eski hâline döner.",
            ),
        );
        let busy = self.mods.busy;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(widgets::label(text))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(match busy {
                        Some(doing) => style::button("mods-link", doing, true),
                        None => style::button(
                            "mods-link",
                            self.tr("Use mods for this game", "Bu oyunda mod kullan"),
                            true,
                        )
                        .child(style::icon("puzzle", 13., pal::on_accent()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.mods_link(fits.clone(), place.clone(), cx);
                            cx.notify();
                        })),
                    })
                    .child(self.mods_window_button(cx)),
            )
    }

    /// The mods' loader is missing: they are placed, but nothing loads them.
    fn render_loader_warning(&self, loader: &str, cx: &mut Context<Self>) -> Div {
        let busy = self.mods.busy;
        div()
            .flex()
            .items_start()
            .gap_2()
            .p_3()
            .rounded(px(10.))
            .border_1()
            .border_color(pal::warning().opacity(0.4))
            .bg(pal::warning().opacity(0.08))
            .child(style::icon("circle-alert", 15., pal::warning()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(widgets::label(self.loader_text(loader)))
                    .child(
                        div().flex().child(match busy {
                            Some(doing) => style::button("mods-loader", doing, true),
                            None => style::button(
                                "mods-loader",
                                match self.global.language {
                                    arkor_core::Language::Tr => format!("{loader} kur"),
                                    _ => format!("Install {loader}"),
                                },
                                true,
                            )
                            .child(style::icon("download", 13., pal::on_accent()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mods_get_loader(cx);
                                cx.notify();
                            })),
                        }),
                    ),
            )
    }

    fn loader_text(&self, loader: &str) -> String {
        format!(
            "{loader} {} {loader}{}",
            self.tr(
                "is not installed, so these mods do not load. Install",
                "kurulu değil, bu yüzden bu modlar yüklenmiyor. Önce",
            ),
            self.tr(
                " itself first: the button below fetches it from Thunderstore.",
                "'in kendisini kur: aşağıdaki düğme onu Thunderstore'dan getirir.",
            )
        )
    }

    /// Thunderstore's page of the game's mods, in Arkor's browser.
    fn mods_thunderstore_button(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<Div>> {
        let Known::Linked {
            thunderstore: Some(community),
            ..
        } = &self.mods.known
        else {
            return None;
        };
        let page = format!("https://thunderstore.io/c/{community}/");
        Some(
            style::button("mods-thunderstore", "Thunderstore", false)
                .child(style::icon("search", 13., pal::muted()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.mods_browse(page.clone(), cx);
                    cx.notify();
                })),
        )
    }

    fn mods_window_button(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        style::button(
            "mods-window",
            self.tr("Open Amethyst", "Amethyst'i aç"),
            false,
        )
        .child(style::icon("external-link", 13., pal::muted()))
        .on_click(cx.listener(|this, _, _, cx| {
            if let Err(e) = amethyst::open_window() {
                this.status = Some(format!("{e:#}").into());
            }
            cx.notify();
        }))
    }

    fn render_mods_list(
        &self,
        link: Link,
        list: Vec<Mod>,
        running: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let busy = self.mods.busy;
        let locked = running || busy.is_some();
        let mut out = div().flex().flex_col().gap_3();
        let add_link = link.clone();
        out = out.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(match busy {
                    Some(doing) => style::button("mods-add", doing, true),
                    None => style::button("mods-add", self.tr("Install a mod…", "Mod kur…"), true)
                        .child(style::icon("plus", 13., pal::on_accent()))
                        .when(!running, |b| {
                            b.on_click(cx.listener(move |this, _, _, cx| {
                                this.mods_pick_archive(add_link.clone(), cx);
                            }))
                        }),
                })
                .children(self.mods_thunderstore_button(cx))
                .child(self.mods_window_button(cx)),
        );
        if running {
            out = out.child(widgets::label(self.tr(
                "The game is running: its mods can be changed once it ends.",
                "Oyun çalışıyor: modları oyun kapanınca değiştirilebilir.",
            )));
        }
        if list.is_empty() {
            return out.child(widgets::label(self.tr(
                "No mods yet. Install one from a downloaded archive (zip, 7z, rar), or with Amethyst's window.",
                "Henüz mod yok. İndirilmiş bir arşivden (zip, 7z, rar) ya da Amethyst'in penceresinden kurabilirsin.",
            )));
        }
        out = out.child(widgets::subheading(format!(
            "{} · {}",
            self.tr("Highest priority first", "En yüksek öncelik üstte"),
            list.len()
        )));
        let last = list.len() - 1;
        for (n, m) in list.into_iter().enumerate() {
            out = out.child(self.render_mod_row(n, last, m, &link, locked, cx));
        }
        out
    }

    fn render_mod_row(
        &self,
        n: usize,
        last: usize,
        m: Mod,
        link: &Link,
        locked: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let sure = self.mods.removing.as_deref() == Some(m.name.as_str());
        let mut meta = Vec::new();
        if let Some(v) = &m.version {
            meta.push(v.clone());
        }
        let fixed = locked || m.locked;
        let toggle = {
            let (link, name, on) = (link.clone(), m.name.clone(), m.enabled);
            widgets::switch(m.enabled)
                .id(("mod-on", n))
                .when(!fixed, |s| {
                    s.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let (link, name) = (link.clone(), name.clone());
                            this.mods_do(this.tr("Saving…", "Kaydediliyor…"), cx, move |c| {
                                mods::set_enabled(c, &link, &name, !on)
                            });
                            cx.notify();
                        }))
                })
        };
        let shift = |up: bool, cx: &mut Context<Self>| {
            let (link, name) = (link.clone(), m.name.clone());
            let edge = if up { n == 0 } else { n == last };
            style::icon_button(
                (if up { "mod-up" } else { "mod-down" }, n),
                if up { "chevron-up" } else { "chevron-down" },
                false,
            )
            .when(fixed || edge, |b| b.opacity(0.35))
            .when(!fixed && !edge, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    let (link, name) = (link.clone(), name.clone());
                    this.mods_do(this.tr("Moving…", "Taşınıyor…"), cx, move |c| {
                        mods::shift(c, &link, &name, up)
                    });
                    cx.notify();
                }))
            })
        };
        let remove = {
            let (link, name) = (link.clone(), m.name.clone());
            style::button(
                ("mod-remove", n),
                if sure {
                    self.tr("Really remove", "Gerçekten kaldır")
                } else {
                    ""
                },
                sure,
            )
            .when(!sure, |b| {
                b.size(px(32.))
                    .p_0()
                    .child(style::icon("trash", 14., pal::muted()))
            })
            .when(fixed, |b| b.opacity(0.35))
            .when(!fixed, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    if this.mods.removing.as_deref() == Some(name.as_str()) {
                        let (link, name) = (link.clone(), name.clone());
                        this.mods_do(this.tr("Removing…", "Kaldırılıyor…"), cx, move |c| {
                            mods::remove(c, &link, &name)
                        });
                    } else {
                        this.mods.removing = Some(name.clone());
                    }
                    cx.notify();
                }))
            })
        };
        let page = m.url.clone().map(|url| {
            style::icon_button(("mod-page", n), "external-link", false).on_click(move |_, _, _| {
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            })
        });
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(toggle)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(if m.enabled { pal::text() } else { pal::muted() })
                            .child(m.title.clone()),
                    )
                    .when(!meta.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(pal::muted())
                                .child(meta.join(" · ")),
                        )
                    })
                    .when(m.update, |d| {
                        d.child(style::tag(self.tr("Update", "Güncelleme"), pal::accent()))
                    })
                    .when(!m.needs.is_empty(), |d| {
                        d.child(style::tag(
                            format!("{} {}", self.tr("Needs", "Gerekli:"), m.needs.join(", ")),
                            pal::warning(),
                        ))
                    }),
            )
            .children(page)
            .child(shift(true, cx))
            .child(shift(false, cx))
            .child(remove)
    }

    /// An installer's questions: FOMOD steps with their groups and options; for other
    /// installers only the recommended choice.
    fn render_mods_question(&self, asking: Asking, cx: &mut Context<Self>) -> Div {
        let fomod = asking.choices.installer() == "fomod";
        let name = asking.choices.0["mod"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let mut out = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(widgets::label(format!(
                "{} {name}",
                self.tr(
                    "This mod asks what to install:",
                    "Bu mod neyin kurulacağını soruyor:"
                )
            )));
        if fomod {
            let steps = asking.choices.0["steps"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for step in steps.iter().filter(|s| s["visible_by_default"] != false) {
                let step_ix = step["index"].as_str().unwrap_or("0").to_string();
                out = out.child(widgets::subheading(
                    step["name"].as_str().unwrap_or_default().to_string(),
                ));
                for group in step["groups"].as_array().cloned().unwrap_or_default() {
                    out = out.child(self.render_fomod_group(&asking, &step_ix, &group, cx));
                }
            }
        } else {
            out = out.child(widgets::label(self.tr(
                "Its installer is set up the way its author recommends; Amethyst's window can choose otherwise.",
                "Kurulum, yazarının önerdiği biçimde yapılır; başka seçimler Amethyst'in penceresinden yapılabilir.",
            )));
        }
        let link = match &self.mods.known {
            Known::Linked { link, .. } => Some(link.clone()),
            _ => None,
        };
        let install = {
            let (link, archive, picked) =
                (link.clone(), asking.archive.clone(), asking.picked.clone());
            style::button("fomod-install", self.tr("Install", "Kur"), true).on_click(cx.listener(
                move |this, _, _, cx| {
                    let Some(link) = link.clone() else { return };
                    let answer = match fomod {
                        true => Answer::Fomod(picked.clone()),
                        false => Answer::Defaults,
                    };
                    this.mods_install(link, archive.clone(), Some(answer), cx);
                    cx.notify();
                },
            ))
        };
        out.child(div().flex().gap_2().child(install).child(
            style::button("fomod-cancel", self.tr("Cancel", "Vazgeç"), false).on_click(
                cx.listener(|this, _, _, cx| {
                    this.mods.asking = None;
                    cx.notify();
                }),
            ),
        ))
    }

    fn render_fomod_group(
        &self,
        asking: &Asking,
        step: &str,
        group: &Value,
        cx: &mut Context<Self>,
    ) -> Div {
        let gname = group["name"].as_str().unwrap_or_default().to_string();
        let kind = group["type"].as_str().unwrap_or("SelectAny").to_string();
        let picked: Vec<String> = asking.picked[step][&gname]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let mut opts = div().flex().flex_wrap().gap_2();
        for (n, opt) in group["options"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let oname = opt["name"].as_str().unwrap_or_default().to_string();
            let on = picked.contains(&oname);
            let hint = opt["description"].as_str().unwrap_or_default().to_string();
            let (step, gname, kind) = (step.to_string(), gname.clone(), kind.clone());
            let key = format!("{step}/{gname}/{n}");
            opts = opts.child(
                widgets::choice_card(
                    gpui::ElementId::Name(key.into()),
                    if on { "circle-check" } else { "circle" },
                    oname.clone(),
                    hint,
                    on,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(a) = this.mods.asking.as_mut() {
                        pick(&mut a.picked, &step, &gname, &kind, &oname);
                    }
                    cx.notify();
                })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .text_sm()
                    .text_color(pal::text_soft())
                    .child(group["label"].as_str().unwrap_or(&gname).to_string()),
            )
            .child(opts)
    }
}

impl ModsUi {
    /// An installer's questions, as if an archive had asked them (for tests).
    #[cfg(test)]
    pub(super) fn ask(&mut self, choices: Value) {
        self.asking = Some(Asking {
            archive: PathBuf::from("/mod.zip"),
            picked: Choices(choices.clone()).defaults(),
            choices: Choices(choices),
        });
    }
}

// ---- Console mode ----

impl Library {
    /// The Mods screen opened: its list is read again.
    pub(super) fn mods_opened(&mut self, cx: &mut Context<Self>) {
        self.mods.removing = None;
        if self.mods_ready() {
            self.mods_load(cx);
        }
    }

    /// The Mods screen of a game.
    pub(super) fn mods_rows(&self, id: &str) -> Vec<Row> {
        if self.selected_id().is_none_or(|g| g.id != id) {
            return Vec::new();
        }
        let line = |act, icon, text: &str, value| {
            Row::new(Kind::Line, vec![Item::line(act, icon, text, value)])
        };
        if !self.mods_ready() {
            let waiting = arkor_launch::downloads::pending("amethyst");
            return vec![line(
                Act::ModsGet,
                "download",
                self.tr("Get mod support", "Mod desteğini indir"),
                Shown::Text(if waiting {
                    self.tr("Downloading…", "İndiriliyor…").to_string()
                } else {
                    "Amethyst Mod Manager".to_string()
                }),
            )];
        }
        if let Some(asking) = &self.mods.asking {
            return self.mods_question_rows(asking);
        }
        let mut rows = Vec::new();
        if let Some(doing) = self.mods.busy {
            rows.push(line(Act::ModsReload, "loader", doing, Shown::None));
        }
        match &self.mods.known {
            Known::Nothing | Known::Reading => rows.push(line(
                Act::ModsReload,
                "loader",
                self.tr("Asking Amethyst…", "Amethyst'e soruluyor…"),
                Shown::None,
            )),
            Known::Failed(e) => rows.push(line(
                Act::ModsReload,
                "refresh-cw",
                self.tr("Try again", "Tekrar dene"),
                Shown::Text(e.clone()),
            )),
            Known::Unlinked { fits, place } => match (fits, place) {
                (Some(fits), Some(_)) => rows.push(line(
                    Act::ModsLink,
                    "puzzle",
                    self.tr("Use mods for this game", "Bu oyunda mod kullan"),
                    Shown::Text(fits.name.clone()),
                )),
                _ => rows.push(line(
                    Act::ModsReload,
                    "circle-alert",
                    self.tr(
                        "Amethyst does not know this game yet",
                        "Amethyst bu oyunu henüz tanımıyor",
                    ),
                    Shown::None,
                )),
            },
            Known::Linked {
                mods,
                domain,
                loader,
                thunderstore,
                ..
            } => {
                if let Some(loader) = loader {
                    rows.push(line(
                        Act::ModsGetLoader,
                        "circle-alert",
                        &format!("{loader} {}", self.tr("is not installed", "kurulu değil")),
                        Shown::Text(self.tr("install it", "kurmak için seç").to_string()),
                    ));
                }
                if let Some(d) = domain {
                    rows.push(line(
                        Act::ModsBrowse(nexus::mods_page(d)),
                        "search",
                        self.tr("Find mods on Nexus Mods", "Nexus Mods'ta mod bul"),
                        Shown::None,
                    ));
                }
                if let Some(community) = thunderstore {
                    rows.push(line(
                        Act::ModsBrowse(format!("https://thunderstore.io/c/{community}/")),
                        "search",
                        self.tr("Find mods on Thunderstore", "Thunderstore'da mod bul"),
                        Shown::None,
                    ));
                }
                if domain.is_some() {
                    let account = self.mods.account.clone().flatten();
                    let (act, who) = match (&account, self.mods.signing_in) {
                        (_, true) => (
                            Act::ModsSignIn,
                            self.tr("Finish in the browser…", "Tarayıcıda tamamla…")
                                .to_string(),
                        ),
                        (Some(a), _) => (Act::ModsSignOut, a.name.clone()),
                        (None, _) => (Act::ModsSignIn, self.tr("Sign in", "Giriş yap").to_string()),
                    };
                    rows.push(line(act, "globe", "Nexus Mods", Shown::Text(who)));
                    if account.is_some() {
                        rows.push(line(
                            Act::ModsUpdates,
                            "refresh-cw",
                            self.tr("Check for updates", "Güncellemeleri denetle"),
                            Shown::None,
                        ));
                    }
                }
                if mods.is_empty() {
                    rows.push(line(
                        Act::ModsReload,
                        "info",
                        self.tr("No mods yet", "Henüz mod yok"),
                        Shown::Text(
                            self.tr(
                                "Install them from Nexus Mods or the desktop",
                                "Nexus Mods'tan ya da masaüstünden kur",
                            )
                            .to_string(),
                        ),
                    ));
                }
                for (n, m) in mods.iter().enumerate() {
                    let mut item = Item::line(
                        Act::ModsToggle(m.name.clone()),
                        "puzzle",
                        m.title.clone(),
                        Shown::Switch(m.enabled),
                    );
                    let mut sub: Vec<String> = m.version.iter().cloned().collect();
                    if m.update {
                        sub.push(self.tr("update available", "güncelleme var").to_string());
                    }
                    if !m.needs.is_empty() {
                        sub.push(format!(
                            "{} {}",
                            self.tr("needs", "gerekli:"),
                            m.needs.join(", ")
                        ));
                    }
                    item.sub = sub.join(" · ");
                    let mut row = Row::new(Kind::Line, vec![item]);
                    if n == 0 {
                        row.title = Some(format!(
                            "{} · {}",
                            self.tr("Highest priority first", "En yüksek öncelik üstte"),
                            mods.len()
                        ));
                    }
                    rows.push(row);
                }
            }
        }
        rows
    }

    /// An installer's questions, a line per option; then install or not.
    fn mods_question_rows(&self, asking: &Asking) -> Vec<Row> {
        let mut rows = Vec::new();
        if asking.choices.installer() == "fomod" {
            let steps = asking.choices.0["steps"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for step in steps.iter().filter(|s| s["visible_by_default"] != false) {
                let ix = step["index"].as_str().unwrap_or("0").to_string();
                for group in step["groups"].as_array().cloned().unwrap_or_default() {
                    let gname = group["name"].as_str().unwrap_or_default().to_string();
                    let picked = &asking.picked[&ix][&gname];
                    for (n, opt) in group["options"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .enumerate()
                    {
                        let oname = opt["name"].as_str().unwrap_or_default().to_string();
                        let on = picked
                            .as_array()
                            .is_some_and(|a| a.iter().any(|x| x == oname.as_str()));
                        let mut item = Item::line(
                            Act::ModsPick(ix.clone(), gname.clone(), oname.clone()),
                            if on { "circle-check" } else { "circle" },
                            oname,
                            Shown::None,
                        );
                        item.sub = opt["description"].as_str().unwrap_or_default().to_string();
                        let mut row = Row::new(Kind::Line, vec![item]);
                        if n == 0 {
                            row.title = Some(group["label"].as_str().unwrap_or(&gname).to_string());
                        }
                        rows.push(row);
                    }
                }
            }
        }
        let name = asking.choices.0["mod"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let mut install = Row::new(
            Kind::Line,
            vec![Item::line(
                Act::ModsAnswer(true),
                "download",
                self.tr("Install", "Kur"),
                Shown::Text(name),
            )],
        );
        if rows.is_empty() {
            install.title = Some(
                self.tr(
                    "Installed the way its author recommends",
                    "Yazarının önerdiği biçimde kurulur",
                )
                .to_string(),
            );
        }
        rows.push(install);
        rows.push(Row::new(
            Kind::Line,
            vec![Item::line(
                Act::ModsAnswer(false),
                "x",
                self.tr("Cancel", "Vazgeç"),
                Shown::None,
            )],
        ));
        rows
    }

    /// The question's rows whether Amethyst is there or not (for tests).
    #[cfg(test)]
    pub(super) fn mods_rows_asking_for_test(&self) -> Vec<Row> {
        self.mods
            .asking
            .as_ref()
            .map(|a| self.mods_question_rows(a))
            .unwrap_or_default()
    }

    fn mods_link_of(&self) -> Option<Link> {
        match &self.mods.known {
            Known::Linked { link, .. } => Some(link.clone()),
            _ => None,
        }
    }

    /// What one of the Mods screen's things does.
    pub(super) fn mods_act(&mut self, act: Act, cx: &mut Context<Self>) {
        let running = self.selected_id().is_some_and(|id| self.mods_running(&id));
        let changes = matches!(
            act,
            Act::ModsToggle(_)
                | Act::ModsMenu(_)
                | Act::ModsMove(..)
                | Act::ModsAskRemove(_)
                | Act::ModsRemove(_)
                | Act::ModsLink
                | Act::ModsGetLoader
                | Act::ModsAnswer(true)
        );
        if running && changes {
            self.status = Some(self.tr("Close the game first.", "Önce oyunu kapat.").into());
            cx.notify();
            return;
        }
        match act {
            Act::ModsGet => self.enqueue_download("amethyst", cx),
            Act::ModsReload => self.mods_opened(cx),
            Act::ModsLink => {
                if let Known::Unlinked {
                    fits: Some(fits),
                    place: Some(place),
                } = self.mods.known.clone()
                {
                    self.mods_link(fits, place, cx);
                }
            }
            Act::ModsToggle(name) => {
                let on = match &self.mods.known {
                    Known::Linked { mods, .. } => {
                        mods.iter().find(|m| m.name == name).map(|m| m.enabled)
                    }
                    _ => None,
                };
                if let (Some(on), Some(link)) = (on, self.mods_link_of()) {
                    self.mods_do(self.tr("Saving…", "Kaydediliyor…"), cx, move |c| {
                        mods::set_enabled(c, &link, &name, !on)
                    });
                }
            }
            Act::ModsMenu(name) => {
                let options = vec![
                    (
                        self.tr("Move up", "Yukarı taşı").to_string(),
                        Act::ModsMove(name.clone(), true),
                        false,
                    ),
                    (
                        self.tr("Move down", "Aşağı taşı").to_string(),
                        Act::ModsMove(name.clone(), false),
                        false,
                    ),
                    (
                        self.tr("Remove", "Kaldır").to_string(),
                        Act::ModsAskRemove(name.clone()),
                        false,
                    ),
                ];
                self.console_overlay(Overlay::Pick(name, options));
            }
            Act::ModsMove(name, up) => {
                // Chosen from its menu: the menu goes.
                self.console.overlay = None;
                if let Some(link) = self.mods_link_of() {
                    self.mods_do(self.tr("Moving…", "Taşınıyor…"), cx, move |c| {
                        mods::shift(c, &link, &name, up)
                    });
                }
            }
            Act::ModsAskRemove(name) => {
                let text = format!(
                    "{} {name}",
                    self.tr(
                        "Remove this mod for good?",
                        "Bu mod tamamen kaldırılsın mı?"
                    )
                );
                let yes = self.tr("Remove", "Kaldır").to_string();
                self.console_overlay(Overlay::Ask(text, yes, Act::ModsRemove(name)));
            }
            Act::ModsRemove(name) => {
                // The question is answered: it goes.
                self.console.overlay = None;
                if let Some(link) = self.mods_link_of() {
                    self.mods_do(self.tr("Removing…", "Kaldırılıyor…"), cx, move |c| {
                        mods::remove(c, &link, &name)
                    });
                }
            }
            Act::ModsSignIn => {
                if self.mods.signing_in {
                    self.mods.signing_in = false;
                } else {
                    self.mods_sign_in(cx);
                }
            }
            Act::ModsSignOut => self.mods_sign_out(cx),
            Act::ModsUpdates => {
                if let Some(link) = self.mods_link_of() {
                    self.mods_check_updates(link, cx);
                }
            }
            Act::ModsPick(step, group, option) => {
                if let Some(a) = self.mods.asking.as_mut() {
                    let kind = a.choices.0["steps"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|s| s["index"] == step.as_str())
                        .flat_map(|s| s["groups"].as_array().cloned().unwrap_or_default())
                        .find(|g| g["name"] == group.as_str())
                        .and_then(|g| g["type"].as_str().map(str::to_string))
                        .unwrap_or_else(|| "SelectAny".into());
                    pick(&mut a.picked, &step, &group, &kind, &option);
                }
            }
            Act::ModsAnswer(true) => {
                if let (Some(a), Some(link)) = (self.mods.asking.clone(), self.mods_link_of()) {
                    let answer = match a.choices.installer() {
                        "fomod" => Answer::Fomod(a.picked),
                        _ => Answer::Defaults,
                    };
                    self.mods_install(link, a.archive, Some(answer), cx);
                }
            }
            Act::ModsAnswer(false) => self.mods.asking = None,
            Act::ModsBrowse(url) => self.mods_browse(url, cx),
            Act::ModsGetLoader => self.mods_get_loader(cx),
            _ => {}
        }
        cx.notify();
    }
}

/// Picks (or unpicks) an option of a FOMOD group, by the group's rule.
fn pick(picked: &mut Value, step: &str, group: &str, kind: &str, option: &str) {
    if !picked.is_object() {
        *picked = json!({});
    }
    if !picked[step].is_object() {
        picked[step] = json!({});
    }
    let now: Vec<String> = picked[step][group]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let has = now.iter().any(|o| o == option);
    let next: Vec<String> = match kind {
        "SelectExactlyOne" => vec![option.to_string()],
        "SelectAtMostOne" if has => Vec::new(),
        "SelectAtMostOne" => vec![option.to_string()],
        // Every option is installed; nothing to change.
        "SelectAll" => now,
        // At least one stays picked.
        "SelectAtLeastOne" if has && now.len() == 1 => now,
        _ if has => now.into_iter().filter(|o| o != option).collect(),
        _ => now.into_iter().chain([option.to_string()]).collect(),
    };
    picked[step][group] = json!(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_by_the_groups_rule() {
        let mut p = json!({"0": {"Quality": ["High"]}});
        pick(&mut p, "0", "Quality", "SelectExactlyOne", "Low");
        assert_eq!(p["0"]["Quality"], json!(["Low"]));
        pick(&mut p, "0", "Quality", "SelectExactlyOne", "Low");
        assert_eq!(p["0"]["Quality"], json!(["Low"]), "one always stays");

        pick(&mut p, "1", "Extras", "SelectAny", "A");
        pick(&mut p, "1", "Extras", "SelectAny", "B");
        assert_eq!(p["1"]["Extras"], json!(["A", "B"]));
        pick(&mut p, "1", "Extras", "SelectAny", "A");
        assert_eq!(p["1"]["Extras"], json!(["B"]));

        pick(&mut p, "1", "Patch", "SelectAtMostOne", "X");
        pick(&mut p, "1", "Patch", "SelectAtMostOne", "X");
        assert_eq!(p["1"]["Patch"], json!([]), "at most one: may be none");

        pick(&mut p, "1", "Core", "SelectAtLeastOne", "C");
        pick(&mut p, "1", "Core", "SelectAtLeastOne", "C");
        assert_eq!(p["1"]["Core"], json!(["C"]), "at least one stays");
    }
}
