//! Saves of store games: brought together with the cloud before a game starts and after it
//! ends, the question when both sides changed, and the copies that can be put back. The same
//! for the desktop (a card on the game's page) and console mode (a screen).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arkor_core::GameId;
use arkor_launch::progress::human_bytes;
use arkor_launch::saves::{self, CloudGame, Conflict, Prefer, Report, Snapshot, State, Why};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*};

use super::console::{Act, Item, Kind, Overlay, Row, Screen, Value};
use super::style::{self, pal};
use super::synth::Cue;
use super::{Library, widgets};

/// What is known of the selected game's saves (read when it is selected, and after changes).
#[derive(Debug, Clone)]
pub(super) struct Info {
    pub(super) game: CloudGame,
    pub(super) state: State,
    pub(super) copies: Vec<Snapshot>,
    pub(super) logged_in: bool,
}

/// Something is being done with a game's saves.
#[derive(Clone)]
pub(super) struct Busy {
    id: GameId,
    stop: Arc<AtomicBool>,
    /// The game starts when it is done.
    launch: bool,
}

#[derive(Default)]
pub(super) struct SavesUi {
    pub(super) info: Option<Info>,
    busy: Option<Busy>,
    /// This game's saves were looked at: it may start.
    ready: Option<GameId>,
    /// This game starts once the question about its saves is answered.
    asked: Option<GameId>,
    /// The copy that is asked about on the desktop.
    restoring: Option<String>,
}

impl SavesUi {
    /// The game whose start waits for its saves (its library id).
    pub(super) fn starting(&self, id: &str) -> bool {
        self.busy
            .as_ref()
            .is_some_and(|b| b.launch && b.id.id == id)
    }
}

impl Library {
    /// Reads how the selected game's saves stand. Cheap: a few small files.
    pub(super) fn saves_load(&mut self) {
        self.saves.restoring = None;
        self.saves.info = self
            .entry()
            .and_then(|g| saves::cloud_game(g, &self.global))
            .map(|game| Info {
                state: saves::state(&game.id),
                copies: saves::copies(&game.id),
                logged_in: arkor_launch::shops::logged_in(game.shop),
                game,
            });
    }

    /// The selected game's saves, unless its store keeps none.
    fn saves_shown(&self) -> Option<&Info> {
        self.saves
            .info
            .as_ref()
            .filter(|i| self.entry().is_some_and(|g| g.id == i.game.id.id))
            .filter(|i| i.state.cloud != Some(false))
    }

    fn saves_on(&self) -> bool {
        self.profile
            .cloud_saves
            .unwrap_or(!self.global.no_cloud_saves)
    }

    /// Before a game starts: whether it has to wait for its saves. It starts by itself then.
    pub(super) fn saves_first(&mut self, id: &GameId, cx: &mut Context<Self>) -> bool {
        if self.saves.ready.take().as_ref() == Some(id) {
            return false;
        }
        // Already on it.
        if self.saves.busy.as_ref().is_some_and(|b| &b.id == id) {
            if let Some(b) = self.saves.busy.as_mut() {
                b.launch = true;
            }
            return true;
        }
        let wanted = self
            .saves_shown()
            .filter(|i| &i.game.id == id && i.logged_in)
            .map(|i| i.game.clone())
            .filter(|_| self.saves_on() && !self.running.contains_key(&id.to_string()));
        let Some(game) = wanted else {
            return false;
        };
        self.saves_sync(game, Prefer::Ask, true, cx);
        true
    }

    /// Starts the game without looking at its saves (any more).
    pub(super) fn saves_skip(&mut self) {
        if let Some(b) = &self.saves.busy {
            b.stop.store(true, Ordering::Relaxed);
        }
    }

    fn saves_start(&mut self, id: &GameId, cx: &mut Context<Self>) {
        self.saves.ready = Some(id.clone());
        self.saves.asked = None;
        if self.console.on {
            return self.console_play(&id.to_string(), cx);
        }
        self.select_game(id, cx);
        self.play(cx);
    }

    /// Brings the saves here and in the cloud together.
    pub(super) fn saves_sync(
        &mut self,
        game: CloudGame,
        prefer: Prefer,
        launch: bool,
        cx: &mut Context<Self>,
    ) {
        if self.saves.busy.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.saves.busy = Some(Busy {
            id: game.id.clone(),
            stop: stop.clone(),
            launch,
        });
        self.status = Some(self.saves_looking().into());
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let of = game.clone();
            let result = bg
                .spawn(async move { saves::sync(&of, prefer, &stop) })
                .await;
            let _ = this.update(cx, |this, cx| this.saves_synced(&game, result, cx));
        })
        .detach();
    }

    pub(super) fn saves_looking(&self) -> &'static str {
        self.tr("Synchronising saves…", "Kayıtlar eşitleniyor…")
    }

    pub(super) fn saves_synced(
        &mut self,
        game: &CloudGame,
        result: anyhow::Result<Report>,
        cx: &mut Context<Self>,
    ) {
        let busy = self.saves.busy.take();
        let launch = busy.as_ref().is_some_and(|b| b.launch);
        let skipped = busy.is_some_and(|b| b.stop.load(Ordering::Relaxed));
        self.saves_load();
        match result {
            Ok(Report {
                conflict: Some(c), ..
            }) => {
                self.status = Some(
                    self.tr(
                        "The saves changed here and in the cloud.",
                        "Kayıtlar hem burada hem bulutta değişmiş.",
                    )
                    .into(),
                );
                self.saves_ask(game, &c, launch);
            }
            Ok(r) => {
                let done = match (r.downloaded, r.uploaded) {
                    (true, _) => self.tr("Newer saves downloaded.", "Yeni kayıtlar indirildi."),
                    (_, true) => {
                        self.tr("Saves uploaded to the cloud.", "Kayıtlar buluta yüklendi.")
                    }
                    _ => self.tr("The saves are up to date.", "Kayıtlar güncel."),
                };
                self.status = (!launch || r.downloaded || r.uploaded).then(|| done.into());
                if launch {
                    let status = self.status.take();
                    self.saves_start(&game.id, cx);
                    self.status = self.status.take().or(status);
                }
            }
            Err(e) => {
                let why = match skipped {
                    true => self
                        .tr("Saves not synchronised.", "Kayıtlar eşitlenmedi.")
                        .to_string(),
                    false => format!(
                        "{}: {e:#}",
                        self.tr("Saves not synchronised", "Kayıtlar eşitlenemedi")
                    ),
                };
                if launch {
                    self.saves_start(&game.id, cx);
                }
                self.status = Some(why.into());
            }
        }
        cx.notify();
    }

    /// Both sides changed: which one counts?
    fn saves_ask(&mut self, game: &CloudGame, c: &Conflict, launch: bool) {
        self.saves.asked = launch.then(|| game.id.clone());
        self.sound(super::sounds::Sound::Error);
        if !self.console.on {
            return;
        }
        self.console.launching = None;
        let title = format!(
            "{}: {}",
            game.name,
            self.tr(
                "the saves changed here and in the cloud. Which ones count?",
                "kayıtlar hem burada hem bulutta değişmiş. Hangisi geçerli?"
            )
        );
        let id = game.id.id.clone();
        let [here, cloud] = self.saves_sides(c);
        let mut options = vec![
            (here, Act::SavesKeep(id.clone(), true), false),
            (cloud, Act::SavesKeep(id.clone(), false), false),
        ];
        if launch {
            options.push((
                self.tr(
                    "Start with the ones here, decide later",
                    "Buradakilerle başlat, sonra karar ver",
                )
                .to_string(),
                Act::SavesLater(id),
                false,
            ));
        }
        options.push((self.tr("Close", "Kapat").to_string(), Act::Close, false));
        self.console_overlay(Overlay::Pick(title, options));
        // A button pressed by chance decides nothing.
        self.console.over_focus.row = 2;
    }

    /// What can be answered: this computer's, the cloud's.
    fn saves_sides(&self, c: &Conflict) -> [String; 2] {
        let side = |name: &str, at: Option<u64>, bytes: u64| {
            let mut out = name.to_string();
            if let Some(at) = at {
                out.push_str(&format!(" · {}", self.saves_when(at)));
            }
            format!("{out} · {}", human_bytes(bytes))
        };
        [
            side(
                self.tr("This computer's", "Bu bilgisayardaki"),
                c.here,
                c.here_bytes,
            ),
            side(self.tr("The cloud's", "Buluttaki"), c.cloud, c.cloud_bytes),
        ]
    }

    /// "3 h ago", with the date for older ones.
    fn saves_when(&self, at: u64) -> String {
        let secs = arkor_core::stats::now().saturating_sub(at);
        match secs {
            0..60 => self.tr("just now", "az önce").to_string(),
            _ => super::game_page::ago(secs, self.global.language),
        }
    }

    /// The question is answered.
    pub(super) fn saves_keep(&mut self, id: &str, here: bool, cx: &mut Context<Self>) {
        self.console.overlay = None;
        let Some(game) = self
            .library
            .get(id)
            .and_then(|g| saves::cloud_game(g, &self.global))
        else {
            return;
        };
        let launch = self.saves.asked.take().as_ref() == Some(&game.id);
        let prefer = if here { Prefer::Here } else { Prefer::Cloud };
        if launch && self.console.on {
            self.console_launch_screen(id);
            self.launch_over_soon(cx);
        }
        self.saves_sync(game, prefer, launch, cx);
        // The card is drawn anew without the question.
        cx.notify();
    }

    /// The game starts; the question stays.
    pub(super) fn saves_later(&mut self, id: &str, cx: &mut Context<Self>) {
        self.console.overlay = None;
        if let Some(gid) = self.library.get(id).map(|g| g.game_id()) {
            self.saves_start(&gid, cx);
        }
    }

    /// After a game ended: what it saved goes up.
    pub(super) fn saves_after(&mut self, id: &GameId, cx: &mut Context<Self>) {
        let Some(game) = self
            .library
            .get(&id.id)
            .and_then(|g| saves::cloud_game(g, &self.global))
        else {
            return;
        };
        let bg = cx.background_executor().clone();
        let of = id.clone();
        cx.spawn(async move |this, cx| {
            let Some(result) = bg.spawn(async move { saves::after_exit(&of) }).await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                // Started again meanwhile: that says enough itself.
                if this.saves.busy.is_none() && !this.running.contains_key(&id_text(&game)) {
                    this.saves_synced(&game, result, cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn saves_copy(&mut self, cx: &mut Context<Self>) {
        let Some(game) = self.saves_shown().map(|i| i.game.clone()) else {
            return;
        };
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { saves::copy_now(&game) }).await;
            let _ = this.update(cx, |this, cx| {
                this.saves_load();
                this.status = Some(match result {
                    Ok(0) => this
                        .tr(
                            "There is a copy of these saves already (or nothing saved yet).",
                            "Bu kayıtların kopyası zaten var (ya da henüz kayıt yok).",
                        )
                        .into(),
                    Ok(_) => this.tr("Copy kept.", "Kopya saklandı.").into(),
                    Err(e) => format!("{e:#}").into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn saves_restore(&mut self, copy: String, cx: &mut Context<Self>) {
        self.console.overlay = None;
        self.saves.restoring = None;
        let Some(id) = self.saves_shown().map(|i| i.game.id.clone()) else {
            return;
        };
        if self.running.contains_key(&id.to_string()) {
            self.status = Some(self.tr("Close the game first.", "Önce oyunu kapat.").into());
            return cx.notify();
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { saves::restore(&id, &copy) }).await;
            let _ = this.update(cx, |this, cx| {
                this.saves_load();
                this.status = Some(match result {
                    Ok(()) => this
                        .tr(
                            "Put back. What was there is kept as a copy, too.",
                            "Geri yüklendi. Önceki hali de kopya olarak duruyor.",
                        )
                        .into(),
                    Err(e) => format!("{e:#}").into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn saves_toggle(&mut self) {
        let on = !self.saves_on();
        // As in Settings again: nothing of its own.
        self.profile.cloud_saves = (on == self.global.no_cloud_saves).then_some(on);
        self.save();
    }

    /// How they stand, in one line.
    fn saves_line(&self, i: &Info) -> (&'static str, String) {
        if self.saves.busy.as_ref().is_some_and(|b| b.id == i.game.id) {
            return ("refresh-cw", self.saves_looking().into());
        }
        if !i.logged_in {
            let text = format!(
                "{} {}",
                self.tr("Not logged in to", "Giriş yapılmamış:"),
                i.game.shop.name()
            );
            return ("cloud-off", text);
        }
        if !self.saves_on() {
            return (
                "cloud-off",
                self.tr("Not synchronised", "Eşitlenmiyor").into(),
            );
        }
        if i.state.conflict.is_some() {
            return (
                "cloud-alert",
                self.tr(
                    "Changed here and in the cloud: choose",
                    "Hem burada hem bulutta değişmiş: seç",
                )
                .into(),
            );
        }
        if let Some(e) = &i.state.error {
            return ("cloud-alert", e.clone());
        }
        match i.state.synced {
            Some(at) => (
                "cloud",
                format!(
                    "{} {}",
                    self.tr("Synchronised", "Eşitlendi:"),
                    self.saves_when(at)
                ),
            ),
            None => (
                "cloud",
                self.tr("Not synchronised yet", "Henüz eşitlenmedi").into(),
            ),
        }
    }

    fn saves_why(&self, why: Why) -> &'static str {
        match why {
            Why::BeforeDownload => self.tr("before a download", "indirmeden önce"),
            Why::Cloud => self.tr("the cloud's", "buluttaki"),
            Why::BeforeRestore => self.tr("before putting back", "geri yüklemeden önce"),
            Why::Manual => self.tr("kept by you", "sen sakladın"),
        }
    }

    /// A copy: what it is of, and its size.
    fn saves_copy_text(&self, s: &Snapshot) -> (String, String) {
        let mut name = format!(
            "{} · {}",
            self.saves_when(s.saved.unwrap_or(s.time)),
            self.saves_why(s.why)
        );
        // Games with more than one save folder.
        if !["saves", "__default"].contains(&s.place.as_str()) {
            name.push_str(&format!(" · {}", s.place));
        }
        (
            name,
            format!(
                "{} {} · {}",
                s.files,
                self.tr("files", "dosya"),
                human_bytes(s.bytes)
            ),
        )
    }

    // ---- Console mode ----

    /// The button on a game's screen.
    pub(super) fn saves_pill(&self) -> Option<Item> {
        let i = self.saves_shown()?;
        let alert = i.state.conflict.is_some();
        Some(Item::pill(
            Act::Screen(Screen::Saves(i.game.id.id.clone())),
            if alert { "cloud-alert" } else { "cloud" },
            self.tr("Saves", "Oyun kayıtları"),
            alert,
        ))
    }

    /// The screen of a game's saves.
    pub(super) fn saves_rows(&self, id: &str) -> Vec<Row> {
        let Some(i) = self.saves_shown().filter(|i| i.game.id.id == id) else {
            return Vec::new();
        };
        let line = |act, icon, text: &str, value| {
            Row::new(Kind::Line, vec![Item::line(act, icon, text, value)])
        };
        let mut rows = vec![line(
            Act::SavesToggle,
            "cloud",
            self.tr("Synchronise with the cloud", "Bulutla eşitle"),
            Value::Switch(self.saves_on()),
        )];
        let (icon, text) = self.saves_line(i);
        let act = match i.state.conflict {
            Some(_) => Act::SavesAsk,
            None => Act::SavesSync,
        };
        rows.push(line(
            act,
            if icon == "cloud" { "refresh-cw" } else { icon },
            self.tr("Synchronise now", "Şimdi eşitle"),
            Value::Text(text),
        ));
        rows.push(line(
            Act::SavesCopy,
            "save",
            self.tr("Keep a copy now", "Şimdi bir kopya sakla"),
            Value::None,
        ));
        for (n, s) in i.copies.iter().enumerate() {
            let (name, size) = self.saves_copy_text(s);
            let mut row = line(
                Act::SavesAskRestore(s.id.clone()),
                "rotate-ccw",
                &name,
                Value::Text(size),
            );
            if n == 0 {
                row.title = Some(
                    self.tr(
                        "Copies that can be put back",
                        "Geri yüklenebilecek kopyalar",
                    )
                    .to_string(),
                );
            }
            rows.push(row);
        }
        rows
    }

    /// What one of this screen's things does.
    pub(super) fn saves_do(&mut self, act: Act, cx: &mut Context<Self>) {
        match act {
            Act::SavesToggle => self.saves_toggle(),
            Act::SavesKeep(id, here) => self.saves_keep(&id, here, cx),
            Act::SavesLater(id) => self.saves_later(&id, cx),
            Act::SavesCopy => self.saves_copy(cx),
            Act::SavesSync => {
                if let Some(game) = self.saves_syncable() {
                    self.saves_sync(game, Prefer::Ask, false, cx);
                }
            }
            Act::SavesAsk => {
                if let Some(i) = self.saves_shown().cloned()
                    && let Some(c) = &i.state.conflict
                {
                    self.saves_ask(&i.game, c, false);
                }
            }
            Act::SavesAskRestore(copy) => {
                let Some(text) = self
                    .saves_shown()
                    .and_then(|i| i.copies.iter().find(|s| s.id == copy))
                    .map(|s| self.saves_copy_text(s).0)
                else {
                    return;
                };
                let text = format!(
                    "{}: {text}. {}",
                    self.tr("Put this copy back?", "Bu kopya geri yüklensin mi?"),
                    self.tr(
                        "The saves there now are kept as a copy, too.",
                        "Şu anki kayıtlar da kopya olarak saklanır."
                    )
                );
                let yes = self.tr("Put back", "Geri yükle").to_string();
                self.cue(Cue::Open);
                self.console_overlay(Overlay::Ask(text, yes, Act::SavesRestore(copy)));
            }
            Act::SavesRestore(copy) => self.saves_restore(copy, cx),
            _ => {}
        }
        cx.notify();
    }

    /// The selected game, when its saves can be synced now.
    fn saves_syncable(&mut self) -> Option<CloudGame> {
        let i = self.saves_shown()?;
        let (game, logged_in) = (i.game.clone(), i.logged_in);
        let why = if !logged_in {
            self.tr("Log in to the store first.", "Önce mağazaya giriş yap.")
        } else if self.running.contains_key(&game.id.to_string()) {
            self.tr("Close the game first.", "Önce oyunu kapat.")
        } else {
            return Some(game);
        };
        self.status = Some(why.into());
        None
    }

    // ---- The desktop's card ----

    pub(super) fn render_saves(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let i = self.saves_shown()?.clone();
        let (icon, text) = self.saves_line(&i);
        let color = match icon {
            "cloud-alert" => pal::warning(),
            "cloud" => pal::success(),
            _ => pal::muted(),
        };
        let mut card = widgets::section(self.tr("Saves", "Oyun kayıtları"))
            .child(widgets::setting_row(
                self.tr("Synchronise with the cloud", "Bulutla eşitle"),
                self.tr(
                    "Saves are looked for in the store's cloud before the game starts, and uploaded when it ends. A copy is kept before anything is put over them.",
                    "Oyun başlamadan önce mağazanın bulutundaki kayıtlara bakılır, kapanınca yüklenir. Üzerine bir şey yazılmadan önce bir kopya saklanır.",
                ),
                widgets::switch(self.saves_on())
                    .id("cloud-saves")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.saves_toggle();
                        cx.notify();
                    })),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(style::icon(icon, 14., color))
                    .child(div().flex_1().min_w_0().child(widgets::label(text))),
            );
        let busy = self.saves.busy.is_some();
        card = card.child(
            div()
                .flex()
                .gap_2()
                .when(!busy, |d| {
                    d.child(
                        style::button(
                            "saves-sync",
                            self.tr("Synchronise now", "Şimdi eşitle"),
                            false,
                        )
                        .child(style::icon("refresh-cw", 13., pal::muted()))
                        .on_click(cx.listener(|this, _, _, cx| this.saves_do(Act::SavesSync, cx))),
                    )
                })
                .when(busy, |d| {
                    d.child(
                        style::button("saves-stop", self.tr("Stop", "Durdur"), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.saves_skip();
                                cx.notify();
                            }),
                        ),
                    )
                })
                .child(
                    style::button(
                        "saves-copy",
                        self.tr("Keep a copy now", "Şimdi bir kopya sakla"),
                        false,
                    )
                    .child(style::icon("save", 13., pal::muted()))
                    .on_click(cx.listener(|this, _, _, cx| this.saves_copy(cx))),
                ),
        );
        if i.copies.is_empty() {
            return Some(card);
        }
        card = card
            .child(widgets::divider())
            .child(widgets::subheading(self.tr(
                "Copies that can be put back",
                "Geri yüklenebilecek kopyalar",
            )));
        for (n, s) in i.copies.iter().enumerate() {
            let (name, size) = self.saves_copy_text(s);
            let sure = self.saves.restoring.as_ref() == Some(&s.id);
            let id = s.id.clone();
            let text = match sure {
                true => self.tr("Really put back", "Gerçekten geri yükle"),
                false => self.tr("Put back", "Geri yükle"),
            };
            card = card.child(widgets::setting_row(
                name,
                size,
                style::button(("saves-restore", n), text, sure).on_click(cx.listener(
                    move |this, _, _, cx| {
                        if this.saves.restoring.as_ref() == Some(&id) {
                            this.saves_restore(id.clone(), cx);
                        } else {
                            this.saves.restoring = Some(id.clone());
                        }
                        cx.notify();
                    },
                )),
            ));
        }
        Some(card)
    }

    /// The question, over the game's page.
    pub(super) fn render_saves_question(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let i = self.saves_shown()?;
        let c = i.state.conflict.clone()?;
        if self.saves.busy.is_some() || !i.logged_in || !self.saves_on() {
            return None;
        }
        let gid = i.game.id.clone();
        let id = gid.id.clone();
        let [here, cloud] = self.saves_sides(&c);
        let mut row = div().flex().flex_wrap().gap_2();
        for (key, text, keep_here) in [("saves-here", here, true), ("saves-cloud", cloud, false)] {
            let id = id.clone();
            row =
                row.child(style::button(key, text, false).on_click(
                    cx.listener(move |this, _, _, cx| this.saves_keep(&id, keep_here, cx)),
                ));
        }
        if self.saves.asked.as_ref() == Some(&gid) {
            row = row.child(
                style::button(
                    "saves-later",
                    self.tr(
                        "Start with the ones here, decide later",
                        "Buradakilerle başlat, sonra karar ver",
                    ),
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.saves_later(&id, cx))),
            );
        }
        Some(
            style::card()
                .p_4()
                .border_color(pal::warning().opacity(0.5))
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(style::icon("cloud-alert", 16., pal::warning()))
                        .child(div().font_weight(gpui::FontWeight::MEDIUM).child(self.tr(
                            "The saves changed here and in the cloud. Which ones count?",
                            "Kayıtlar hem burada hem bulutta değişmiş. Hangisi geçerli?",
                        ))),
                )
                .child(row)
                .child(widgets::label(self.tr(
                    "The other side is kept as a copy (Game › Saves).",
                    "Diğer taraf kopya olarak saklanır (Oyun › Oyun kayıtları).",
                ))),
        )
    }

    pub(super) fn saves_apply(&self) -> bool {
        self.saves_shown().is_some()
    }
}

fn id_text(game: &CloudGame) -> String {
    game.id.to_string()
}
