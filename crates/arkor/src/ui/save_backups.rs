//! Copies of any game's saves (found through Ludusavi's manifest, see
//! `arkor_launch::saves::local`): kept after the game is played, made on request, put back.
//! Shown where a game's saves have no store cloud: on its page and on its Saves screen.

use arkor_core::GameId;
use arkor_launch::saves::{self, Snapshot, Why};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*};

use super::console::{Act, Item, Kind, Row, Value};
use super::style::{self, pal};
use super::{Library, widgets};

/// What is known of the selected game's saves when no store keeps them.
#[derive(Debug, Clone)]
pub(super) struct Local {
    pub(super) id: GameId,
    pub(super) copies: Vec<Snapshot>,
    /// Ludusavi's name for the game and how many save folders it has here; why not, if not.
    /// `None` while it is looked up.
    pub(super) found: Option<Result<(String, usize), String>>,
    pub(super) looking: bool,
    pub(super) busy: bool,
}

impl Local {
    pub(super) fn new(id: GameId) -> Self {
        Local {
            copies: saves::copies(&id),
            id,
            found: None,
            looking: false,
            busy: false,
        }
    }
}

impl Library {
    /// Looks up where the selected game keeps its saves, off the UI thread (the manifest is
    /// fetched the first time).
    pub(super) fn saves_local_lookup(&mut self, cx: &mut Context<Self>) {
        let Some(local) = self.saves.local.as_mut() else {
            return;
        };
        if local.found.is_some() || local.looking {
            return;
        }
        local.looking = true;
        let id = local.id.clone();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let of = id.clone();
            let found = bg
                .spawn(async move {
                    saves::local_places(&of)
                        .map(|(name, places)| (name, places.len()))
                        .map_err(|e| format!("{e:#}"))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(l) = this.saves.local.as_mut().filter(|l| l.id == id) {
                    l.looking = false;
                    l.found = Some(found);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Keeps a copy of the selected game's saves now.
    pub(super) fn saves_backup_now(&mut self, cx: &mut Context<Self>) {
        let Some(local) = self.saves.local.as_mut().filter(|l| !l.busy) else {
            return;
        };
        local.busy = true;
        let id = local.id.clone();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let of = id.clone();
            let result = bg
                .spawn(async move { saves::backup(&of, Why::Manual) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(l) = this.saves.local.as_mut().filter(|l| l.id == id) {
                    l.busy = false;
                    l.copies = saves::copies(&id);
                }
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

    /// Copies after every game is played: on or off (for every game).
    pub(super) fn saves_backups_toggle(&mut self) {
        self.global.no_save_backups = !self.global.no_save_backups;
        if let Err(e) = self.global.save() {
            self.status = Some(format!("{e:#}").into());
        }
    }

    /// Where the saves were found, in a line.
    fn saves_local_line(&self, l: &Local) -> (&'static str, String) {
        match &l.found {
            None => (
                "loader",
                self.tr("Looking for its saves…", "Kayıtları aranıyor…")
                    .to_string(),
            ),
            Some(Ok((_, 0))) => (
                "circle-alert",
                self.tr(
                    "No saves of it on this computer yet.",
                    "Bu bilgisayarda henüz kaydı yok.",
                )
                .to_string(),
            ),
            Some(Ok((name, n))) => (
                "save",
                format!(
                    "{n} {} ({name})",
                    self.tr("save folders found", "kayıt klasörü bulundu")
                ),
            ),
            Some(Err(_)) => (
                "info",
                self.tr(
                    "Where this game keeps its saves is not known.",
                    "Bu oyunun kayıtlarını nerede tuttuğu bilinmiyor.",
                )
                .to_string(),
            ),
        }
    }

    /// The card on a game's page.
    pub(super) fn render_local_saves(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let l = self
            .saves
            .local
            .clone()
            .filter(|l| self.entry().is_some_and(|g| g.id == l.id.id))?;
        if l.found.is_none() && !l.looking {
            self.saves_local_lookup(cx);
        }
        let (icon, text) = self.saves_local_line(&l);
        let known = matches!(l.found, Some(Ok((_, n))) if n > 0);
        let mut card = widgets::section(self.tr("Saves", "Oyun kayıtları"))
            .child(widgets::setting_row(
                self.tr("Keep a copy after playing", "Oynadıktan sonra kopya sakla"),
                self.tr(
                    "When a game ends and its saves changed, a copy of them is kept (twelve per folder), to put back if a save goes wrong. For every game.",
                    "Oyun kapanınca kayıtları değiştiyse bir kopyası saklanır (klasör başına on iki), bir kayıt bozulursa geri yüklemek için. Bütün oyunlar için.",
                ),
                widgets::switch(!self.global.no_save_backups)
                    .id("save-backups")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.saves_backups_toggle();
                        cx.notify();
                    })),
            ))
            .child(widgets::setting_row(
                self.tr("Also while playing", "Oynarken de"),
                self.tr(
                    "A copy every so often while the game runs, so a crash or a power cut loses little. Only when the saves changed.",
                    "Oyun sürerken belli aralıklarla da kopya alınır; bir çökme ya da elektrik kesintisinde az şey kaybolur. Yalnızca kayıtlar değiştiyse.",
                ),
                {
                    let mut row = div().flex().flex_wrap().gap_1();
                    for (ix, m) in [0u32, 15, 30, 60, 120].into_iter().enumerate() {
                        let label = match m {
                            0 => self.tr("Off", "Kapalı").to_string(),
                            m if m < 60 => format!("{m} {}", self.tr("min", "dk")),
                            m => format!("{} {}", m / 60, self.tr("h", "sa")),
                        };
                        row = row.child(
                            widgets::chip(("save-every", ix), label, self.global.save_every_min == m)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.global.save_every_min = m;
                                    let r = this.global.save();
                                    this.report(r);
                                    cx.notify();
                                })),
                        );
                    }
                    row
                },
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(style::icon(icon, 14., pal::muted()))
                    .child(div().flex_1().min_w_0().child(widgets::label(text))),
            )
            .when(known, |c| {
                c.child(
                    div().flex().child(
                        style::button(
                            "saves-backup",
                            if l.busy {
                                self.tr("Keeping a copy…", "Kopya saklanıyor…")
                            } else {
                                self.tr("Keep a copy now", "Şimdi bir kopya sakla")
                            },
                            false,
                        )
                        .child(style::icon("save", 13., pal::muted()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.saves_backup_now(cx);
                            cx.notify();
                        })),
                    ),
                )
            });
        if l.copies.is_empty() {
            return Some(card);
        }
        card = card
            .child(widgets::divider())
            .child(widgets::subheading(self.tr(
                "Copies that can be put back",
                "Geri yüklenebilecek kopyalar",
            )));
        for (n, s) in l.copies.iter().enumerate() {
            card = card.child(self.saves_restore_row(n, s, cx));
        }
        Some(card)
    }

    /// The Saves screen of console mode, for a game no store keeps saves of.
    pub(super) fn local_saves_rows(&self, l: &Local) -> Vec<Row> {
        let line = |act, icon, text: &str, value| {
            Row::new(Kind::Line, vec![Item::line(act, icon, text, value)])
        };
        let (icon, text) = self.saves_local_line(l);
        let mut rows = vec![
            line(
                Act::SavesBackupsToggle,
                "clock",
                self.tr("Keep a copy after playing", "Oynadıktan sonra kopya sakla"),
                Value::Switch(!self.global.no_save_backups),
            ),
            line(
                Act::SavesBackup,
                icon,
                self.tr("Keep a copy now", "Şimdi bir kopya sakla"),
                Value::Text(text),
            ),
        ];
        for (n, s) in l.copies.iter().enumerate() {
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
}
