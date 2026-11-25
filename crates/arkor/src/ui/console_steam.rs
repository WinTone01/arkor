//! Console mode's side of a Steam under control: a game's achievements, screenshots, add-ons
//! and workshop items, moving it to another disk, the steps of its start, and what Steam
//! asks (licences, cloud saves, how to start it), answered here by the user.

use std::path::PathBuf;

use arkor_core::library::GameKind;
use arkor_launch::progress::human_bytes;
use arkor_launch::shops::Shop;
use arkor_launch::steam_api::{self, News};
use arkor_launch::steam_control;
use gpui::Context;

use super::Library;
use super::console::{Act, Art, Item, Kind, Look, Overlay, Row, Screen, Value};
use super::console_sound::Cue;

/// What is shown of a game in Steam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SteamPage {
    Achievements,
    Screenshots,
    Dlc,
    Workshop,
}

#[derive(Debug, Clone)]
pub(super) enum SteamData {
    Achievements(Vec<steam_api::Achievement>),
    Screenshots(Vec<steam_api::Screenshot>),
    Dlc(Vec<steam_api::Dlc>),
    Workshop(Vec<steam_api::WorkshopItem>),
}

/// What was loaded for a page (`None` while it loads).
pub(super) type Loaded = (u32, SteamPage, Option<Result<SteamData, String>>);

impl Library {
    pub(super) fn steam_page_name(&self, p: SteamPage) -> &'static str {
        match p {
            SteamPage::Achievements => self.tr("Achievements", "Başarımlar"),
            SteamPage::Screenshots => self.tr("Screenshots", "Ekran görüntüleri"),
            SteamPage::Dlc => "DLC",
            SteamPage::Workshop => self.tr("Workshop", "Atölye"),
        }
    }

    /// The name of a Steam game.
    pub(super) fn steam_name(&self, appid: u32) -> String {
        let id = appid.to_string();
        let entry = self
            .library
            .games
            .iter()
            .find(|g| matches!(&g.kind, GameKind::Steam { appid } if *appid == id));
        entry
            .map(|g| g.name.clone())
            .or_else(|| self.shops.find(Shop::Steam, &id).map(|g| g.title.clone()))
            .or_else(|| {
                self.shops
                    .installed(Shop::Steam, &id)
                    .map(|g| g.title.clone())
            })
            .unwrap_or(id)
    }

    /// What Steam has for an installed game; nothing without control of Steam.
    pub(super) fn steam_pills(&self, appid: u32) -> Option<Row> {
        if !steam_control::on() {
            return None;
        }
        let page = |p: SteamPage, icon| {
            Item::pill(
                Act::Screen(Screen::Steam(appid, p)),
                icon,
                self.steam_page_name(p),
                false,
            )
        };
        let mut pills = vec![
            page(SteamPage::Achievements, "trophy"),
            page(SteamPage::Screenshots, "image"),
            page(SteamPage::Dlc, "package-plus"),
            page(SteamPage::Workshop, "wrench"),
        ];
        if steam_api::moving().is_none() {
            pills.push(Item::pill(
                Act::SteamMoveAsk(appid),
                "folder-input",
                self.tr("Move to another disk", "Başka diske taşı"),
                false,
            ));
        }
        Some(Row::new(Kind::Pills, pills))
    }

    // ---- The pages ----

    /// Loads (again) what the open page shows.
    pub(super) fn steam_page_load(&mut self, appid: u32, page: SteamPage, cx: &mut Context<Self>) {
        let known =
            matches!(&self.console.steam_page, Some((a, p, _)) if (*a, *p) == (appid, page));
        if !known {
            self.console.steam_page = Some((appid, page, None));
        }
        if cfg!(test) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let data = bg
                .spawn(async move {
                    let data = match page {
                        SteamPage::Achievements => {
                            steam_api::achievements(appid).map(SteamData::Achievements)
                        }
                        SteamPage::Screenshots => {
                            steam_api::screenshots(appid).map(SteamData::Screenshots)
                        }
                        SteamPage::Dlc => steam_api::dlcs(appid).map(SteamData::Dlc),
                        SteamPage::Workshop => steam_api::workshop(appid).map(SteamData::Workshop),
                    };
                    data.map_err(|e| format!("{e:#}"))
                })
                .await;
            let pictures: Vec<String> = match &data {
                Ok(SteamData::Achievements(l)) => l.iter().map(|a| a.image.clone()).collect(),
                Ok(SteamData::Workshop(l)) => l.iter().map(|w| w.image.clone()).collect(),
                _ => Vec::new(),
            };
            let current = this
                .update(cx, |this, cx| {
                    let current = matches!(&this.console.steam_page, Some((a, p, _)) if (*a, *p) == (appid, page));
                    if current {
                        this.console.steam_page = Some((appid, page, Some(data)));
                        cx.notify();
                    }
                    current
                })
                .unwrap_or(false);
            if current && !pictures.is_empty() {
                bg.spawn(async move { steam_api::fetch_pictures(&pictures) }).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }

    fn steam_picture(url: &str) -> Option<Art> {
        steam_api::picture(url)
            .filter(|p| p.is_file())
            .map(|p| Art::File(p, false))
    }

    pub(super) fn steam_page_rows(&self, appid: u32, page: SteamPage) -> Vec<Row> {
        let one = |item: Item| Row::new(Kind::Line, vec![item]);
        let note = |icon: &'static str, text: String| {
            vec![one(Item::line(Act::Nothing, icon, text, Value::None))]
        };
        let loaded = match &self.console.steam_page {
            Some((a, p, data)) if (*a, *p) == (appid, page) => data.as_ref(),
            _ => None,
        };
        let data = match loaded {
            None => {
                return note(
                    "loader-circle",
                    self.tr("Asking Steam…", "Steam'e soruluyor…").into(),
                );
            }
            Some(Err(e)) => return note("triangle-alert", e.clone()),
            Some(Ok(d)) => d,
        };
        let nothing = |text: &'static str| note("circle", text.to_string());
        match data {
            SteamData::Achievements(l) if l.is_empty() => {
                nothing(self.tr("This game has no achievements.", "Bu oyunun başarımı yok."))
            }
            SteamData::Achievements(l) => {
                let have = l.iter().filter(|a| a.unlocked).count();
                let mut rows: Vec<Row> = l.iter().map(|a| one(self.achievement_line(a))).collect();
                rows[0].title = Some(format!(
                    "{have} / {} · {}",
                    l.len(),
                    self.percent(have as f32 / l.len() as f32)
                ));
                rows
            }
            SteamData::Screenshots(l) if l.is_empty() => nothing(self.tr(
                "No screenshots of this game on this computer.",
                "Bu bilgisayarda bu oyunun ekran görüntüsü yok.",
            )),
            SteamData::Screenshots(l) => l
                .iter()
                .enumerate()
                .map(|(ix, s)| {
                    let mut item =
                        Item::line(Act::ShotMenu(ix), "image", self.ago(s.at), Value::None);
                    item.sub = s.path.rsplit('/').next().unwrap_or_default().to_string();
                    let path = PathBuf::from(&s.path);
                    if let Look::Line { art, .. } = &mut item.look
                        && path.is_file()
                    {
                        *art = Some(Art::File(path, true));
                    }
                    one(item)
                })
                .collect(),
            SteamData::Dlc(l) if l.is_empty() => {
                nothing(self.tr("No DLC of yours for this game.", "Bu oyun için DLC'n yok."))
            }
            SteamData::Dlc(l) => l
                .iter()
                .map(|d| {
                    let mut item = Item::line(
                        Act::SteamDlc(appid, d.id, !d.on),
                        "package-plus",
                        d.name.clone(),
                        Value::Switch(d.on),
                    );
                    let mut sub = Vec::new();
                    if d.size > 0. {
                        sub.push(human_bytes(d.size as u64));
                    }
                    if d.state == "installed" {
                        sub.push(self.tr("Installed", "Kurulu").into());
                    }
                    item.sub = sub.join(" · ");
                    one(item)
                })
                .collect(),
            SteamData::Workshop(l) if l.is_empty() => nothing(self.tr(
                "No workshop items are subscribed to.",
                "Abone olunan atölye öğesi yok.",
            )),
            SteamData::Workshop(l) => l
                .iter()
                .map(|w| {
                    let mut item = Item::line(
                        Act::WorkshopMenu(appid, w.id.clone()),
                        "wrench",
                        w.title.clone(),
                        Value::Text(
                            if w.off {
                                self.tr("Off", "Kapalı")
                            } else {
                                self.tr("On", "Açık")
                            }
                            .into(),
                        ),
                    );
                    item.sub = human_bytes(w.size as u64);
                    if let Look::Line { art, .. } = &mut item.look {
                        *art = Self::steam_picture(&w.image);
                    }
                    one(item)
                })
                .collect(),
        }
    }

    fn achievement_line(&self, a: &steam_api::Achievement) -> Item {
        let secret = a.hidden && !a.unlocked;
        let name = if secret {
            self.tr("Hidden achievement", "Gizli başarım").to_string()
        } else {
            a.name.clone()
        };
        let value = if a.unlocked {
            self.ago(a.at)
        } else if a.most > 0. {
            format!("{} / {}", a.progress as u64, a.most as u64)
        } else {
            String::new()
        };
        let mut item = Item::line(Act::Nothing, "lock", name, Value::Text(value));
        let mut sub = Vec::new();
        if !secret && !a.description.is_empty() {
            sub.push(a.description.clone());
        }
        if a.share > 0. {
            sub.push(format!(
                "{} {}",
                self.percent((a.share / 100.) as f32),
                self.tr("of players", "oyuncuda var")
            ));
        }
        item.sub = sub.join(" · ");
        if !a.unlocked && a.most > 0. {
            item.progress = Some((a.progress / a.most) as f32);
        }
        if let Look::Line { icon, art, .. } = &mut item.look {
            if a.unlocked {
                *icon = "trophy";
            }
            *art = (!secret).then(|| Self::steam_picture(&a.image)).flatten();
        }
        item
    }

    /// "3 days ago".
    fn ago(&self, at: u64) -> String {
        super::game_page::ago(
            arkor_core::stats::now().saturating_sub(at),
            self.global.language,
        )
    }

    // ---- What is done on them ----

    /// Asks Steam for something, then loads the open page again.
    fn steam_page_do(
        &mut self,
        what: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.console.overlay = None;
        let bg = cx.background_executor().clone();
        self.steam_do(what, |_| None, cx);
        let Some((appid, page, _)) = self.console.steam_page.clone() else {
            return;
        };
        // Steam needs a moment until it tells the new state.
        cx.spawn(async move |this, cx| {
            bg.timer(std::time::Duration::from_millis(900)).await;
            let _ = this.update(cx, |this, cx| this.steam_page_load(appid, page, cx));
        })
        .detach();
    }

    pub(super) fn steam_set_dlc(&mut self, app: u32, dlc: u32, on: bool, cx: &mut Context<Self>) {
        // Shown at once.
        if let Some((_, _, Some(Ok(SteamData::Dlc(l))))) = &mut self.console.steam_page
            && let Some(d) = l.iter_mut().find(|d| d.id == dlc)
        {
            d.on = on;
        }
        self.steam_page_do(move || steam_api::set_dlc(app, dlc, on), cx);
    }

    pub(super) fn workshop_menu(&mut self, app: u32, id: &str) {
        let Some((_, _, Some(Ok(SteamData::Workshop(l))))) = &self.console.steam_page else {
            return;
        };
        let Some(w) = l.iter().find(|w| w.id == id) else {
            return;
        };
        let (title, off) = (w.title.clone(), w.off);
        let options = vec![
            (
                if off {
                    self.tr("Use it", "Kullan")
                } else {
                    self.tr("Do not use it", "Kullanma")
                }
                .to_string(),
                Act::SteamWorkshop(app, id.to_string(), off),
                false,
            ),
            (
                self.tr("Unsubscribe", "Abonelikten çık").to_string(),
                Act::AskUnsubscribe(app, id.to_string(), title.clone()),
                false,
            ),
            (self.tr("Close", "Kapat").to_string(), Act::Close, false),
        ];
        self.console_overlay(Overlay::Pick(title, options));
    }

    pub(super) fn steam_set_workshop(
        &mut self,
        app: u32,
        id: String,
        on: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, _, Some(Ok(SteamData::Workshop(l))))) = &mut self.console.steam_page
            && let Some(w) = l.iter_mut().find(|w| w.id == id)
        {
            w.off = !on;
        }
        self.steam_page_do(move || steam_api::set_workshop(app, &id, on), cx);
    }

    pub(super) fn ask_unsubscribe(&mut self, app: u32, id: String, title: &str) {
        let text = format!(
            "{title}: {}",
            self.tr(
                "unsubscribe? Steam removes its files.",
                "abonelikten çıkılsın mı? Steam dosyalarını siler."
            )
        );
        let yes = self.tr("Unsubscribe", "Abonelikten çık").to_string();
        self.console_overlay(Overlay::Ask(text, yes, Act::Unsubscribe(app, id)));
    }

    pub(super) fn steam_unsubscribe(&mut self, app: u32, id: String, cx: &mut Context<Self>) {
        self.steam_page_do(move || steam_api::unsubscribe(app, &id), cx);
    }

    fn shot(&self, ix: usize) -> Option<steam_api::Screenshot> {
        match &self.console.steam_page {
            Some((_, _, Some(Ok(SteamData::Screenshots(l))))) => l.get(ix).cloned(),
            _ => None,
        }
    }

    pub(super) fn shot_menu(&mut self, ix: usize) {
        let Some(s) = self.shot(ix) else {
            return;
        };
        let options = vec![
            (
                self.tr("Show it", "Göster").to_string(),
                Act::ShotShow(PathBuf::from(&s.path)),
                false,
            ),
            (
                self.tr("Delete it", "Sil").to_string(),
                Act::AskShotDelete(ix),
                false,
            ),
            (self.tr("Close", "Kapat").to_string(), Act::Close, false),
        ];
        self.console_overlay(Overlay::Pick(self.ago(s.at), options));
    }

    pub(super) fn ask_shot_delete(&mut self, ix: usize) {
        let text = self
            .tr(
                "Delete this screenshot for good?",
                "Bu ekran görüntüsü kalıcı olarak silinsin mi?",
            )
            .to_string();
        let yes = self.tr("Delete it", "Sil").to_string();
        self.console_overlay(Overlay::Ask(text, yes, Act::ShotDelete(ix)));
    }

    pub(super) fn shot_delete(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(s) = self.shot(ix) {
            self.steam_page_do(move || steam_api::delete_screenshot(&s), cx);
        }
    }

    // ---- Disks ----

    /// Looks at Steam's library folders.
    pub(super) fn steam_look_folders(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || !steam_control::on() {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            if let Ok(found) = bg.spawn(async { steam_api::folders() }).await {
                let _ = this.update(cx, |this, cx| {
                    this.console.steam_folders = found;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(super) fn steam_folder_name(&self, f: &steam_api::Folder) -> String {
        let free = human_bytes(f.free.max(0.) as u64);
        // The free room first: paths are long, and cut off at their end.
        let home = dirs::home_dir().unwrap_or_default();
        let name = if !f.label.is_empty() {
            f.label.clone()
        } else if std::path::Path::new(&f.path).starts_with(&home) {
            self.tr("System disk", "Sistem diski").to_string()
        } else {
            f.path.trim_end_matches("/SteamLibrary").to_string()
        };
        match self.global.language {
            arkor_core::Language::Tr => format!("{free} boş · {name}"),
            arkor_core::Language::En => format!("{free} free · {name}"),
        }
    }

    /// Where to: Steam's other folders.
    pub(super) fn steam_move_ask(&mut self, appid: u32, cx: &mut Context<Self>) {
        if self.console.steam_folders.is_empty() {
            self.steam_look_folders(cx);
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let found = bg.spawn(async { steam_api::folders() }).await;
            let _ = this.update(cx, |this, cx| {
                match found {
                    Ok(f) => {
                        this.console.steam_folders = f;
                        this.steam_move_pick(appid);
                    }
                    Err(e) => this.status = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn steam_move_pick(&mut self, appid: u32) {
        let mut options: Vec<(String, Act, bool)> = self
            .console
            .steam_folders
            .iter()
            .filter(|f| f.mounted && !f.apps.contains(&appid))
            .map(|f| {
                (
                    self.steam_folder_name(f),
                    Act::SteamMove(appid, f.index),
                    false,
                )
            })
            .collect();
        let title = if options.is_empty() {
            self.tr(
                "Steam has no library folder on another disk.",
                "Steam'in başka diskte kütüphane klasörü yok.",
            )
            .to_string()
        } else {
            format!(
                "{}: {}",
                self.steam_name(appid),
                self.tr("move where?", "nereye taşınsın?")
            )
        };
        options.push((self.tr("Close", "Kapat").to_string(), Act::Close, false));
        self.console_overlay(Overlay::Pick(title, options));
    }

    pub(super) fn steam_move(&mut self, appid: u32, folder: i32, cx: &mut Context<Self>) {
        self.console.overlay = None;
        self.steam_do(
            move || steam_api::move_to(appid, folder),
            |this| {
                Some(this.tr(
                    "Steam moves it; see Downloads.",
                    "Steam taşıyor; İndirilenler'de görünür.",
                ))
            },
            cx,
        );
    }

    /// The game that is being moved, among the downloads.
    pub(super) fn steam_move_row(&self) -> Option<Row> {
        let m = steam_api::moving()?;
        let mut item = Item::line(
            Act::SteamMoveMenu,
            "folder-input",
            self.steam_name(m.appid),
            Value::Text(self.percent((m.progress / 100.) as f32)),
        );
        item.sub = self
            .tr(
                "Steam moves it to another disk",
                "Steam başka diske taşıyor",
            )
            .into();
        item.progress = Some((m.progress / 100.).clamp(0., 1.) as f32);
        if let Look::Line { art, .. } = &mut item.look {
            *art = Some(Art::Shop(Shop::Steam, m.appid.to_string()));
        }
        Some(Row::new(Kind::Line, vec![item]))
    }

    pub(super) fn steam_move_menu(&mut self) {
        let title = self
            .tr("Moving to another disk", "Başka diske taşınıyor")
            .to_string();
        let options = vec![
            (
                self.tr("Stop moving", "Taşımayı durdur").to_string(),
                Act::SteamMoveCancel,
                false,
            ),
            (self.tr("Close", "Kapat").to_string(), Act::Close, false),
        ];
        self.console_overlay(Overlay::Pick(title, options));
    }

    // ---- The start of a game, and what Steam asks ----

    /// The step Steam is at with starting `appid`.
    pub(super) fn steam_launch_task(&self, appid: &str) -> Option<String> {
        let a = steam_api::actions()
            .into_iter()
            .find(|a| a.game == appid && a.kind == "LaunchApp")?;
        let text = self.task_name(&a.task)?;
        Some(if a.total > 0. && a.done <= a.total {
            format!("{text} · {}", self.percent((a.done / a.total) as f32))
        } else {
            text.to_string()
        })
    }

    fn task_name(&self, task: &str) -> Option<&'static str> {
        Some(match task {
            "Starting" | "UpdatingAppInfo" => {
                self.tr("Steam checks the game", "Steam oyunu denetliyor")
            }
            "ShowEula" => self.tr("The licence", "Lisans sözleşmesi"),
            "DownloadingDepots" => self.tr("Steam downloads content", "Steam içerik indiriyor"),
            "DownloadingWorkshop" => self.tr(
                "Steam downloads workshop items",
                "Steam atölye öğelerini indiriyor",
            ),
            "UpdatingDRM" | "VerifyingFiles" => {
                self.tr("Steam checks the program", "Steam programı denetliyor")
            }
            "GettingLegacyKey" => self.tr("Steam gets the key", "Steam anahtarı alıyor"),
            "RunningInstallScript" => self.tr("The first-time setup runs", "İlk kurulum çalışıyor"),
            "ConnectingToSteam" => self.tr("Connecting to Steam", "Steam'e bağlanılıyor"),
            "SynchronizingCloud" => self.tr("Saves are synchronised", "Kayıtlar eşitleniyor"),
            "SynchronizingControllerConfig" => self.tr(
                "The controller's layout is synchronised",
                "Kol düzeni eşitleniyor",
            ),
            "ProcessingShaderCache" => {
                self.tr("Shaders are prepared", "Gölgelendiriciler hazırlanıyor")
            }
            "DelayLaunch" => self.tr("Steam waits a moment", "Steam biraz bekliyor"),
            "CreatingProcess" | "WaitingGameWindow" => self.tr("The game starts", "Oyun başlıyor"),
            _ => return None,
        })
    }

    /// Called now and then: what Steam asks is asked here, what happened is said.
    pub(super) fn steam_tick(&mut self, cx: &mut Context<Self>) {
        for n in steam_api::news() {
            self.steam_news(n, cx);
        }
        if !self.console.on {
            return;
        }
        // One at a time, and not into something that is being typed.
        if matches!(self.console.overlay, Some(Overlay::Keyboard)) {
            return;
        }
        if let Some((app, list)) = steam_api::licence_asked() {
            let key = format!("licence:{app}");
            if self.console.asked.as_deref() != Some(&key) {
                self.console.asked = Some(key);
                self.steam_show_licence(app, &list, Act::Licence(true), Act::Licence(false));
                cx.notify();
            }
            return;
        }
        let waiting = steam_api::actions()
            .into_iter()
            .find(|a| !a.request.is_empty());
        match waiting {
            Some(a) => {
                let key = format!("{}:{}:{}", a.id, a.request, a.details);
                if self.console.asked.as_deref() != Some(&key) {
                    self.console.asked = Some(key);
                    self.steam_ask(a, cx);
                }
            }
            None => self.console.asked = None,
        }
    }

    pub(super) fn steam_news(&mut self, n: News, cx: &mut Context<Self>) {
        match n {
            News::NotStarted(game, why) => {
                let name = self.steam_name(game.parse().unwrap_or(0));
                let l = self.console.launching.clone();
                if self.console.on
                    && let Some(l) = l.filter(|l| l.steam.as_deref() == Some(&game) && !l.came)
                {
                    // What waits for the game need not wait any longer.
                    if let Some(g) = self.library.get(&l.id) {
                        let gid = g.game_id().to_string();
                        if let Some(pid) = self.running.get(&gid) {
                            self.stopping.insert(gid.clone());
                            // SAFETY: plain syscall on the process group of `arkor launch`.
                            unsafe { libc::kill(-(*pid as i32), libc::SIGTERM) };
                        }
                    }
                    self.console.launching = None;
                    self.cue(Cue::Error);
                    let said = format!("Steam: {why}");
                    self.console_overlay(Overlay::Failed(l.id, name, vec![said]));
                } else {
                    self.status = Some(format!("{name}: {why}").into());
                }
            }
            News::Unlocked(app) => {
                self.cue(Cue::Done);
                self.status = Some(
                    format!(
                        "{}: {}",
                        self.tr("New achievement", "Yeni başarım"),
                        self.steam_name(app)
                    )
                    .into(),
                );
                if let Some((a, SteamPage::Achievements, _)) = self.console.steam_page
                    && a == app
                {
                    self.steam_page_load(app, SteamPage::Achievements, cx);
                }
            }
            News::Moved(app) => {
                self.cue(Cue::Done);
                self.status = Some(
                    format!("{}: {}", self.steam_name(app), self.tr("moved", "taşındı")).into(),
                );
                self.steam_look_folders(cx);
            }
            News::NotMoved(app) => {
                self.cue(Cue::Error);
                self.status = Some(
                    format!(
                        "{}: {}",
                        self.steam_name(app),
                        self.tr("was not moved", "taşınamadı")
                    )
                    .into(),
                );
            }
        }
        cx.notify();
    }

    /// The start screen steps aside for a question.
    fn steam_step_aside(&mut self) {
        if let Some(l) = self.console.launching.as_mut() {
            l.dismissed = true;
        }
    }

    /// Answered: the start screen is back.
    pub(super) fn steam_answered(&mut self) {
        self.console.overlay = None;
        if let Some(l) = self.console.launching.as_mut()
            && !l.updating
        {
            l.dismissed = false;
            l.at = std::time::Instant::now();
        }
    }

    /// A licence to read, and the two answers.
    pub(super) fn steam_show_licence(
        &mut self,
        app: u32,
        list: &[steam_api::Eula],
        yes: Act,
        no: Act,
    ) {
        self.steam_step_aside();
        let text = list
            .iter()
            .map(|e| e.text.as_str())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        let title = format!(
            "{}: {}",
            self.steam_name(app),
            self.tr("licence agreement", "lisans sözleşmesi")
        );
        self.cue(Cue::Open);
        // Nothing is accepted that could not be read here.
        let options = if text.is_empty() || list.iter().any(|e| e.text.is_empty()) {
            vec![
                (
                    self.tr("Read it in Steam's window", "Steam penceresinde oku")
                        .to_string(),
                    Act::ShowSteamWindow,
                ),
                (self.tr("Cancel", "Vazgeç").to_string(), no),
            ]
        } else {
            vec![
                (self.tr("I accept", "Kabul ediyorum").to_string(), yes),
                (self.tr("Cancel", "Vazgeç").to_string(), no),
            ]
        };
        let text = if text.is_empty() {
            self.tr(
                "The text could not be read here.",
                "Metin burada okunamadı.",
            )
            .to_string()
        } else {
            text
        };
        self.console.read_at = 0.;
        let last = options.len() - 1;
        self.console_overlay(Overlay::Read(title, text, options));
        // It may come while a button is pressed: nothing is accepted by accident.
        self.console.over_focus.set(0, last);
    }

    /// Something a launch waits for.
    pub(super) fn steam_ask(&mut self, a: steam_api::Action, cx: &mut Context<Self>) {
        let app: u32 = a.game.parse().unwrap_or(0);
        let name = self.steam_name(app);
        let id = a.id;
        let say = |what: &str| Act::SteamAnswer(id, what.to_string());
        let cancel = (
            self.tr("Do not start it", "Başlatma").to_string(),
            Act::SteamCancelAction(id),
            false,
        );
        let t = |en: &'static str, tr: &'static str| self.tr(en, tr).to_string();
        let (title, mut options) = match (a.request.as_str(), a.details.as_str()) {
            ("ShowEula", _) => {
                let bg = cx.background_executor().clone();
                cx.spawn(async move |this, cx| {
                    let list = bg
                        .spawn(async move { steam_api::eulas(app) })
                        .await
                        .unwrap_or_default();
                    let _ = this.update(cx, |this, cx| {
                        this.console.licence = Some((app, list.clone()));
                        this.steam_show_licence(
                            app,
                            &list,
                            Act::SteamLicence(id),
                            Act::SteamCancelAction(id),
                        );
                        cx.notify();
                    });
                })
                .detach();
                return;
            }
            ("ShowLaunchOption", _) => {
                let bg = cx.background_executor().clone();
                cx.spawn(async move |this, cx| {
                    let list = bg
                        .spawn(async move { steam_api::launch_options(app) })
                        .await
                        .unwrap_or_default();
                    let _ = this.update(cx, |this, cx| {
                        let mut options: Vec<(String, Act, bool)> = list
                            .into_iter()
                            .map(|o| {
                                let text = if o.text.is_empty() || o.text.starts_with('#') {
                                    this.tr("Play", "Oyna").to_string()
                                } else {
                                    o.text
                                };
                                (text, Act::SteamAnswer(id, o.index.to_string()), false)
                            })
                            .collect();
                        options.push((
                            this.tr("Do not start it", "Başlatma").to_string(),
                            Act::SteamCancelAction(id),
                            false,
                        ));
                        let title = format!(
                            "{}: {}",
                            this.steam_name(app),
                            this.tr("how to start it?", "nasıl başlatılsın?")
                        );
                        this.steam_step_aside();
                        this.console_overlay(Overlay::Pick(title, options));
                        cx.notify();
                    });
                })
                .detach();
                return;
            }
            ("SynchronizingCloud" | "SynchronizingControllerConfig", "cloudconflict") => (
                t(
                    "the saves here and in Steam's cloud differ. Which ones stay? The others are lost.",
                    "buradaki kayıtlarla Steam bulutundakiler farklı. Hangisi kalsın? Öteki silinir.",
                ),
                vec![
                    (
                        t(
                            "Keep the ones of this computer",
                            "Bu bilgisayardakiler kalsın",
                        ),
                        say("KeepLocal"),
                        false,
                    ),
                    (
                        t("Keep the ones of the cloud", "Buluttakiler kalsın"),
                        say("KeepRemote"),
                        false,
                    ),
                ],
            ),
            ("SynchronizingCloud" | "SynchronizingControllerConfig", "syncfailed") => (
                t(
                    "the saves could not be synchronised with Steam's cloud.",
                    "kayıtlar Steam bulutuyla eşitlenemedi.",
                ),
                vec![(
                    t("Start it anyway", "Yine de başlat"),
                    say("IgnoreCloud"),
                    false,
                )],
            ),
            ("SynchronizingCloud" | "SynchronizingControllerConfig", "pendingcloudsessions") => (
                t(
                    "saves of another computer are still on their way to the cloud.",
                    "başka bir bilgisayardaki kayıtlar henüz buluta gitmedi.",
                ),
                vec![(
                    t("Start it anyway", "Yine de başlat"),
                    say("IgnorePendingCloudSessions"),
                    false,
                )],
            ),
            ("RunningInstallScript", _) => (
                t(
                    "its first-time setup failed.",
                    "ilk kurulumu başarısız oldu.",
                ),
                vec![(
                    t("Start it anyway", "Yine de başlat"),
                    say("IgnoreInstallError"),
                    false,
                )],
            ),
            ("KickingOtherSession", _) => (
                t(
                    "your account is playing on another computer. Starting here ends that.",
                    "hesabın başka bir bilgisayarda oynuyor. Burada başlatmak orayı kapatır.",
                ),
                vec![(
                    t("Start it here", "Burada başlat"),
                    say("KickOtherSession"),
                    false,
                )],
            ),
            // Steam answers these itself.
            ("ShowInterstitials" | "ShowDurationControl", _) => return,
            _ => (
                t(
                    "Steam asks something in its own window.",
                    "Steam kendi penceresinde bir şey soruyor.",
                ),
                vec![(
                    t("Show Steam's window", "Steam penceresini göster"),
                    Act::ShowSteamWindow,
                    false,
                )],
            ),
        };
        options.push(cancel);
        self.steam_step_aside();
        self.cue(Cue::Open);
        let last = options.len() - 1;
        self.console_overlay(Overlay::Pick(format!("{name}: {title}"), options));
        // It may come while a button is pressed: it begins at what changes nothing.
        self.console.over_focus.set(last, 0);
        cx.notify();
    }

    pub(super) fn steam_answer(&mut self, id: u32, what: String, cx: &mut Context<Self>) {
        self.steam_answered();
        self.steam_do(move || steam_api::answer(id, &what), |_| None, cx);
    }

    pub(super) fn steam_cancel_action(&mut self, id: u32, cx: &mut Context<Self>) {
        self.console.overlay = None;
        // The game is not started: what waits for it ends.
        if let Some(l) = self.console.launching.take()
            && let Some(g) = self.library.get(&l.id)
        {
            let gid = g.game_id().to_string();
            if let Some(pid) = self.running.get(&gid) {
                self.stopping.insert(gid.clone());
                // SAFETY: plain syscall on the process group of `arkor launch`.
                unsafe { libc::kill(-(*pid as i32), libc::SIGTERM) };
            }
        }
        self.steam_do(move || steam_api::cancel(id), |_| None, cx);
    }

    /// The licence a launch waits for was accepted by the user.
    pub(super) fn steam_accept(&mut self, id: u32, cx: &mut Context<Self>) {
        let Some((app, list)) = self.console.licence.take() else {
            return;
        };
        self.steam_answered();
        self.steam_do(
            move || {
                for e in &list {
                    steam_api::accept_eula(app, e)?;
                }
                steam_api::answer(id, "ShowEula")
            },
            |_| None,
            cx,
        );
    }
}
