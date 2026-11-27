//! Emulator games (see `arkor_launch::emu`): adding them from ROM folders, the emulator a
//! game plays with, the emulators themselves and the BIOS folder, on the desktop and in
//! console mode. Starting a game whose emulator is missing downloads it first.

use std::path::PathBuf;
use std::time::Duration;

use arkor_core::GameId;
use arkor_core::library::{GameKind, LibraryGame};
use arkor_launch::emu::{self, EngineRef, discovery, games, install, rom_sources};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*, px};

use super::console::{Act, FolderFor, Item, Kind, Row, Value};
use super::style::{self, pal};
use super::{Library, widgets};

#[derive(Default)]
pub(super) struct EmuUi {
    /// ROM folders already on the computer, once looked for.
    suggested: Option<Vec<rom_sources::RomSource>>,
    looking: bool,
    /// A ROM folder being read.
    scanning: bool,
    /// A game waiting for its emulator to download before it starts.
    waiting: Option<GameId>,
    /// Library packages already installed for emulators this session (never asked twice).
    tried: Vec<&'static str>,
    /// The emulator whose options and versions are shown on the Emulators page.
    detail: Option<String>,
    /// Covers are being looked up.
    covering: bool,
    /// The emulator of the user's own being written: its program and systems (the name and
    /// arguments are text fields).
    own_program: Option<PathBuf>,
    own_systems: Vec<String>,
    /// The versions an emulator offers, once asked for (an error as text).
    offered: std::collections::HashMap<String, Result<Vec<String>, String>>,
}

/// What an emulator is on this computer, in a few words.
fn state_of(lib: &Library, m: &emu::EngineManifest) -> String {
    let found = discovery::find(m);
    match found.first() {
        Some(i) => {
            let from = match i.origin {
                discovery::Origin::Managed => lib.tr("downloaded", "indirildi"),
                discovery::Origin::Flatpak => "Flatpak",
                discovery::Origin::System => lib.tr("installed", "kurulu"),
                discovery::Origin::Custom => lib.tr("added by you", "senin eklediğin"),
            };
            match &i.version {
                Some(v) => format!("{from} · {v}"),
                None => from.to_string(),
            }
        }
        None => match install::way(m) {
            install::Way::Download => lib
                .tr(
                    "not installed · Arkor downloads it",
                    "kurulu değil · Arkor indirir",
                )
                .into(),
            install::Way::Flatpak(_) => lib
                .tr(
                    "not installed · from Flathub",
                    "kurulu değil · Flathub'dan kurulur",
                )
                .into(),
            install::Way::Distribution(names) => format!(
                "{} {}",
                lib.tr(
                    "install from your distribution:",
                    "dağıtımının paketinden kur:"
                ),
                names.first().cloned().unwrap_or_default()
            ),
        },
    }
}

impl Library {
    /// The system and emulator choice of an emulator game.
    fn emu_parts(entry: &LibraryGame) -> Option<(&str, Option<&str>)> {
        match &entry.kind {
            GameKind::Emulated { system, emulator } => Some((system, emulator.as_deref())),
            _ => None,
        }
    }

    /// The emulator `entry` plays with now.
    pub(super) fn emu_choice(&self, entry: &LibraryGame) -> Option<EngineRef> {
        let (system, own) = Self::emu_parts(entry)?;
        emu::choice_for_game(system, own, &self.global.emulators)
    }

    /// Sets the emulator of the selected game (`None`: the system's default).
    pub(super) fn emu_set_choice(&mut self, choice: Option<String>, cx: &mut Context<Self>) {
        self.edit_entry(|g| {
            if let GameKind::Emulated { emulator, .. } = &mut g.kind {
                emulator.clone_from(&choice);
            }
        });
        cx.notify();
    }

    /// Before an emulator game starts: what it needs and lacks goes into the download
    /// queue, and the game starts once it is there. `true` while that is waited for.
    pub(super) fn emu_before_play(&mut self, id: &GameId, cx: &mut Context<Self>) -> bool {
        let Some(choice) = self.library.get(&id.id).and_then(|g| self.emu_choice(g)) else {
            return false;
        };
        let keys = emu::missing_keys(&choice);
        if cfg!(test) {
            return false;
        }
        if keys.is_empty() {
            return self.emu_libraries_first(id, &choice, cx);
        }
        for key in &keys {
            arkor_launch::downloads::enqueue(key);
        }
        self.watch_progress(cx);
        self.emu.waiting = Some(id.clone());
        self.status = Some(
            format!(
                "{} {}",
                self.tr(
                    "Downloading the emulator first:",
                    "Önce emülatör indiriliyor:"
                ),
                choice.label()
            )
            .into(),
        );
        cx.notify();
        let bg = cx.background_executor().clone();
        let id = id.clone();
        cx.spawn(async move |this, cx| {
            loop {
                bg.timer(Duration::from_secs(1)).await;
                if !keys.iter().any(|k| arkor_launch::downloads::pending(k)) {
                    break;
                }
            }
            let missing = bg.spawn(async move { emu::missing_keys(&choice) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.emu.waiting.as_ref() != Some(&id) {
                    return;
                }
                this.emu.waiting = None;
                if missing.is_empty() {
                    if this.selected_id().as_ref() == Some(&id) {
                        this.play(cx);
                    }
                } else {
                    let failed = arkor_launch::downloads::items()
                        .into_iter()
                        .find_map(|i| match i.state {
                            arkor_launch::downloads::State::Failed(e)
                                if missing.contains(&i.key) =>
                            {
                                Some(e)
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    this.status = Some(
                        format!(
                            "{} {failed}",
                            this.tr("The emulator was not installed.", "Emülatör kurulamadı.")
                        )
                        .into(),
                    );
                    this.sound(super::sounds::Sound::Error);
                }
                cx.notify();
            });
        })
        .detach();
        true
    }

    /// Looks for ROM folders on the computer (once).
    pub(super) fn emu_look(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || self.emu.looking || self.emu.suggested.is_some() {
            return;
        }
        self.emu.looking = true;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let found = bg.spawn(async { games::suggested_folders() }).await;
            let _ = this.update(cx, |this, cx| {
                this.emu.looking = false;
                this.emu.suggested = Some(found);
                cx.notify();
            });
        })
        .detach();
    }

    /// A game leaves the library: when a ROM folder Arkor reads holds it, it is remembered,
    /// so reading the folder again does not bring it back.
    pub(super) fn emu_note_removed(&mut self, entry: &str) {
        let Some(g) = self.library.get(entry) else {
            return;
        };
        if matches!(g.kind, GameKind::Emulated { .. })
            && self
                .global
                .emulators
                .rom_folders
                .iter()
                .any(|f| g.exe.starts_with(f))
            && !self.global.emulators.removed.contains(&g.exe)
        {
            self.global.emulators.removed.push(g.exe.clone());
            let r = self.global.save();
            self.report(r);
        }
    }

    /// Reads the ROM folders again now and then (at start, then every few minutes while
    /// Arkor runs): new games in them join the library.
    pub(super) fn emu_watch(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                let folders = this
                    .update(cx, |this, _| {
                        (!this.emu.scanning).then(|| this.global.emulators.rom_folders.clone())
                    })
                    .ok();
                let Some(folders) = folders else { break };
                let _ = this.update(cx, |this, cx| {
                    let lb = this.global.emulators.launchbox;
                    if lb && !arkor_launch::downloads::pending("launchbox") && emu::launchbox::due()
                    {
                        arkor_launch::downloads::enqueue("launchbox");
                    }
                    this.emu_fill_covers(cx);
                });
                if let Some(folders) = folders.filter(|f| !f.is_empty()) {
                    // Folders on a disk that is not there are left alone (their games stay).
                    let here: Vec<PathBuf> = folders.into_iter().filter(|f| f.is_dir()).collect();
                    let found = bg.spawn(async move { games::find_in(&here) }).await;
                    let _ = this.update(cx, |this, cx| {
                        let removed = this.global.emulators.removed.clone();
                        let added = games::add_except(&mut this.library, &found, &removed);
                        if added > 0 {
                            let r = this.library.save();
                            this.report(r);
                            this.reload_games();
                            this.emu_fill_covers(cx);
                            this.status = Some(
                                format!(
                                    "{added} {}",
                                    this.tr(
                                        "new games from your ROM folders",
                                        "yeni oyun ROM klasörlerinden eklendi"
                                    )
                                )
                                .into(),
                            );
                            cx.notify();
                        }
                    });
                }
                bg.timer(Duration::from_secs(180)).await;
            }
        })
        .detach();
    }

    /// Looks up covers for emulator games that have none (libretro's, or LaunchBox's when
    /// on), one after another in the background.
    pub(super) fn emu_fill_covers(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || self.emu.covering || self.global.emulators.no_covers {
            return;
        }
        let games: Vec<LibraryGame> = self
            .library
            .games
            .iter()
            .filter(|g| matches!(g.kind, GameKind::Emulated { .. }))
            .filter(|g| arkor_launch::covers::cover_for(g).is_none())
            .cloned()
            .collect();
        if games.is_empty() {
            return;
        }
        self.emu.covering = true;
        let launchbox = self.global.emulators.launchbox;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            for game in games {
                let found = bg
                    .spawn(async move {
                        let got = emu::art::cover(&game, launchbox);
                        (game.id, got)
                    })
                    .await;
                if let (id, Ok(Some(_))) = found {
                    let _ = this.update(cx, |this, cx| {
                        this.refresh_cover(&id);
                        cx.notify();
                    });
                }
            }
            let _ = this.update(cx, |this, _| this.emu.covering = false);
        })
        .detach();
    }

    /// Switches libretro's covers, or LaunchBox's (which downloads its database first).
    fn emu_set_covers(
        &mut self,
        libretro: Option<bool>,
        launchbox: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let e = &mut self.global.emulators;
        if let Some(on) = libretro {
            e.no_covers = !on;
        }
        if let Some(on) = launchbox {
            e.launchbox = on;
            if on {
                emu::art::forget_tried();
                if emu::launchbox::due() {
                    self.enqueue_download("launchbox", cx);
                }
            } else {
                emu::launchbox::remove();
            }
        }
        let r = self.global.save();
        self.report(r);
        self.emu_fill_covers(cx);
        cx.notify();
    }

    /// Stops reading a ROM folder (its games stay in the library).
    fn emu_forget_folder(&mut self, dir: &PathBuf, cx: &mut Context<Self>) {
        self.global.emulators.rom_folders.retain(|f| f != dir);
        let r = self.global.save();
        self.report(r);
        cx.notify();
    }

    /// Adds the games in a ROM folder to the library (read in the background), and keeps
    /// reading the folder for new ones.
    pub(super) fn emu_add_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        if self.emu.scanning {
            return;
        }
        if !self.global.emulators.rom_folders.contains(&dir) {
            self.global.emulators.rom_folders.push(dir.clone());
            let r = self.global.save();
            self.report(r);
        }
        self.emu.scanning = true;
        self.status = Some(
            self.tr(
                "Looking for games in the folder…",
                "Klasörde oyunlar aranıyor…",
            )
            .into(),
        );
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let found = bg.spawn(async move { games::find_in(&[dir]) }).await;
            let _ = this.update(cx, |this, cx| {
                this.emu.scanning = false;
                let added = games::add(&mut this.library, &found);
                let result = if added > 0 {
                    this.library.save()
                } else {
                    Ok(())
                };
                if let Err(e) = result {
                    this.status = Some(format!("{e:#}").into());
                } else {
                    this.reload_games();
                    this.emu_fill_covers(cx);
                    let had = found.len() - added;
                    this.status = Some(
                        match (added, had) {
                            (0, 0) => self_text(
                                this,
                                "No games were found there.",
                                "Orada oyun bulunamadı.",
                            ),
                            (0, _) => self_text(
                                this,
                                "These games are in the library already.",
                                "Bu oyunlar zaten kütüphanede.",
                            ),
                            (n, _) => format!(
                                "{n} {}",
                                this.tr("games added to the library", "oyun kütüphaneye eklendi")
                            ),
                        }
                        .into(),
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Puts an emulator in the download queue.
    pub(super) fn emu_install(&mut self, id: &str, cx: &mut Context<Self>) {
        self.enqueue_download(&format!("emu:{id}"), cx);
    }

    /// Installs an emulator's distribution package (asking for the password through polkit).
    pub(super) fn emu_install_package(&mut self, package: &'static str, cx: &mut Context<Self>) {
        self.emu_install_packages(vec![package], None, cx);
    }

    /// Installs distribution packages (polkit asks for the password); then starts `play`
    /// when it is given and they installed.
    fn emu_install_packages(
        &mut self,
        packages: Vec<&'static str>,
        play: Option<GameId>,
        cx: &mut Context<Self>,
    ) {
        self.watch_progress(cx);
        let label = packages.join(", ");
        self.status = Some(format!("{} {label}…", self.tr("Installing", "Kuruluyor:")).into());
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let _job = arkor_launch::progress::job("system-tools", &label);
                    arkor_launch::tools::install_packages(&packages)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let ok = result.is_ok();
                this.status = Some(match result {
                    Ok(()) => this.tr("Installed.", "Kuruldu.").into(),
                    Err(e) => format!("{e:#}").into(),
                });
                if ok
                    && let Some(id) = play
                    && this.selected_id().as_ref() == Some(&id)
                {
                    this.play(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A downloaded emulator that lacks system libraries: their packages are installed
    /// first (then the game starts), or what is missing is said. `true` when the game does
    /// not start now.
    fn emu_libraries_first(
        &mut self,
        id: &GameId,
        choice: &EngineRef,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(missing) = emu::launch::missing_libraries(choice) else {
            return false;
        };
        if missing.packages.is_empty()
            || missing.packages.iter().all(|p| self.emu.tried.contains(p))
        {
            self.status = Some(
                format!(
                    "{} {}: {}",
                    missing.name,
                    self.tr(
                        "needs libraries this computer lacks; install them with your distribution",
                        "bu bilgisayarda olmayan kütüphanelere ihtiyaç duyuyor; dağıtımının paketlerinden kur"
                    ),
                    missing.libs.join(", ")
                )
                .into(),
            );
            self.sound(super::sounds::Sound::Error);
            cx.notify();
            return true;
        }
        self.emu.tried.extend(missing.packages.iter().copied());
        if self.console.on {
            self.status = Some(
                format!(
                    "{} {}",
                    missing.name,
                    self.tr(
                        "needs a few system libraries; type the password to install them, then press Play again.",
                        "birkaç sistem kütüphanesine ihtiyaç duyuyor; kurmak için şifreyi yaz, sonra yeniden Oyna'ya bas."
                    )
                )
                .into(),
            );
            self.console_install_tools(missing.packages.iter().map(|p| p.to_string()).collect());
        } else {
            self.emu_install_packages(missing.packages, Some(id.clone()), cx);
        }
        true
    }

    /// Shows an emulator's options and versions on the Emulators page.
    pub(super) fn emu_show_detail(&mut self, id: Option<&str>) {
        self.emu.detail = id.map(str::to_string);
    }

    /// Opens an emulator on its own, to set its controls and graphics up.
    pub(super) fn emu_open(&mut self, id: &str, cx: &mut Context<Self>) {
        let r = emu::open::start(id, &self.global.emulators);
        self.status = Some(match r {
            Ok(()) => format!(
                "{} {}",
                emu::engine(id).map(|m| m.name.clone()).unwrap_or_default(),
                self.tr("opened", "açıldı")
            )
            .into(),
            Err(e) => format!("{e:#}").into(),
        });
        cx.notify();
    }

    /// Removes what Arkor installed of an emulator.
    pub(super) fn emu_remove(&mut self, id: &str, cx: &mut Context<Self>) {
        let r = install::remove(id);
        self.report(r);
        cx.notify();
    }

    /// The systems an emulator plays, short: `PS1`, `GB, GBC, GBA`, or `15 systems`.
    fn emu_systems_short(&self, m: &emu::EngineManifest) -> String {
        let names: Vec<String> = m
            .systems
            .iter()
            .filter_map(|s| emu::systems::get(s).map(|s| s.short.clone()))
            .collect();
        if names.len() > 3 {
            format!("{} {}", names.len(), self.tr("systems", "sistem"))
        } else {
            names.join(", ")
        }
    }

    // ---- The desktop ----

    /// The game files card of an emulator game: its file, system and emulator.
    pub(super) fn render_emulated_card(
        &mut self,
        entry: &LibraryGame,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some((system, own)) = Self::emu_parts(entry) else {
            return div();
        };
        let system = system.to_string();
        let own = own.map(str::to_string);
        let mut card = widgets::section(self.tr("Game files", "Oyun dosyaları"));
        card = card
            .child(widgets::setting_row(
                self.tr("Game file", "Oyun dosyası"),
                entry.exe.display().to_string(),
                style::button("pick-rom", self.tr("Change…", "Değiştir…"), false)
                    .child(style::icon("folder-open", 13., pal::muted()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let title = this.tr("Choose the game's file", "Oyunun dosyasını seç");
                        this.prompt_path(false, title, cx, |this, path, _| {
                            this.edit_entry(|g| g.exe = path)
                        });
                    })),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("System", "Sistem"),
                games::system_name(&system),
                div(),
            ));
        // The emulator: the system's default, or one of those that play it.
        let mut chips = div().flex().flex_wrap().gap_1().max_w(px(460.));
        chips = chips.child(
            widgets::chip(
                "emu-auto",
                self.tr("System default", "Sistemin varsayılanı"),
                own.is_none(),
            )
            .on_click(cx.listener(|this, _, _, cx| this.emu_set_choice(None, cx))),
        );
        for (ix, c) in emu::choices_for(&system).into_iter().enumerate() {
            let value = c.to_string();
            let on = own.as_deref() == Some(value.as_str());
            chips = chips.child(widgets::chip(("emu-choice", ix), c.label(), on).on_click(
                cx.listener(move |this, _, _, cx| this.emu_set_choice(Some(value.clone()), cx)),
            ));
        }
        let now = self.emu_choice(entry);
        let hint = match &now {
            Some(c) if emu::missing_keys(c).is_empty() => {
                format!("{} · {}", c.label(), self.tr("ready", "hazır"))
            }
            Some(c) => format!(
                "{} · {}",
                c.label(),
                self.tr("downloaded when you first play", "ilk oynayışta indirilir")
            ),
            None => self
                .tr("No emulator plays it", "Bunu oynatan emülatör yok")
                .into(),
        };
        card = card.child(widgets::divider()).child(widgets::setting_row(
            self.tr("Emulator", "Emülatör"),
            hint,
            chips,
        ));
        // BIOS: what the system needs.
        if let Some(sys) = emu::systems::get(&system)
            && !sys.bios.files.is_empty()
        {
            let dir = emu::bios_dir(&self.global.emulators);
            let index = emu::bios::cached_index();
            let state = emu::bios::status(sys, &index).state;
            let text = match state {
                emu::bios::SystemState::Ready => self.tr("BIOS ready", "BIOS hazır").to_string(),
                emu::bios::SystemState::Unrecognized => self
                    .tr(
                        "A BIOS is there, but not one Arkor knows; it may not work",
                        "Bir BIOS var ama Arkor tanımıyor; çalışmayabilir",
                    )
                    .into(),
                emu::bios::SystemState::Missing => format!(
                    "{} {}",
                    self.tr(
                        "Needed, from your own console: put one of these in the BIOS folder:",
                        "Gerekli, kendi konsolundan: şunlardan birini BIOS klasörüne koy:"
                    ),
                    sys.bios
                        .files
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                emu::bios::SystemState::Optional => self
                    .tr(
                        "Optional: some games or features use it",
                        "İsteğe bağlı: bazı oyunlar ya da özellikler kullanır",
                    )
                    .into(),
            };
            card = card.child(widgets::divider()).child(widgets::setting_row(
                "BIOS",
                text,
                bios_button(self, dir),
            ));
        }
        card
    }

    /// The Emulators page (Tools → Emulators): every emulator, what is installed, installing
    /// and removing them, and the BIOS folder.
    pub(super) fn render_emulators_page(&mut self, cx: &mut Context<Self>) -> Div {
        div()
            .child(Self::header(
                self.tr("Emulators", "Emülatörler"),
                self.tr(
                    "Install the emulators your games need, or let Arkor do it when a game first starts.",
                    "Oyunlarının ihtiyaç duyduğu emülatörleri kur, ya da bir oyun ilk açılırken Arkor kursun.",
                ),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(self.render_rom_folders(cx))
                    .child(self.render_emu_covers(cx))
                    .child(self.render_emulators(cx))
                    .child(self.render_own_emulator(cx))
                    .child(
                        div().flex().child(
                            style::button(
                                "emu-add-roms",
                                self.tr("Add games from a ROM folder…", "ROM klasöründen oyun ekle…"),
                                true,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.add.tab = super::add_game::AddTab::Roms;
                                this.go(super::shell::Nav::Add, cx);
                            })),
                        ),
                    ),
            )
    }

    /// Adding an emulator of the user's own, and those added.
    fn render_own_emulator(&mut self, cx: &mut Context<Self>) -> Div {
        let name = self.field(
            "own-emu-name",
            |_| String::new(),
            self.tr("Name, e.g. My emulator", "Ad, ör. Emülatörüm"),
            cx,
            |_, _| {},
        );
        let args = self.field(
            "own-emu-args",
            |_| String::new(),
            self.tr(
                "Arguments, e.g. --fullscreen {rom} (the game goes last without {rom})",
                "Argümanlar, ör. --fullscreen {rom} ({rom} yoksa oyun en sona eklenir)",
            ),
            cx,
            |_, _| {},
        );
        let mut card = widgets::section(self.tr("An emulator of your own", "Kendi emülatörün"));
        card = card.child(widgets::label(self.tr(
            "Any emulator program: Arkor starts it with the game, like the others.",
            "Herhangi bir emülatör programı: Arkor onu da diğerleri gibi oyunla başlatır.",
        )));
        for (ix, m) in emu::engines().iter().filter(|m| m.custom).enumerate() {
            let id = m.id.clone();
            card = card.child(widgets::setting_row(
                m.name.clone(),
                self.emu_systems_short(m),
                style::button(("own-emu-rm", ix), self.tr("Remove", "Kaldır"), false).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let r = emu::custom::remove(&id);
                        this.report(r);
                        cx.notify();
                    }),
                ),
            ));
        }
        let program = self
            .emu
            .own_program
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| self.tr("No program chosen", "Program seçilmedi").into());
        let mut systems = div().flex().flex_wrap().gap_1().max_w(px(560.));
        for (sx, s) in emu::systems::all()
            .iter()
            .filter(|s| emu::playable(&s.id))
            .enumerate()
        {
            let on = self.emu.own_systems.contains(&s.id);
            let id = s.id.clone();
            systems = systems.child(
                widgets::chip(("own-emu-sys", sx), s.short.clone(), on).on_click(cx.listener(
                    move |this, _, _, cx| {
                        let list = &mut this.emu.own_systems;
                        if list.contains(&id) {
                            list.retain(|x| *x != id);
                        } else {
                            list.push(id.clone());
                        }
                        cx.notify();
                    },
                )),
            );
        }
        card.child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("Name", "Ad"),
                "",
                div().w(px(300.)).child(name),
            ))
            .child(widgets::setting_row(
                self.tr("Program", "Program"),
                program,
                style::button("own-emu-pick", self.tr("Choose…", "Seç…"), false).on_click(
                    cx.listener(|this, _, _, cx| {
                        let title =
                            this.tr("Choose the emulator's program", "Emülatörün programını seç");
                        this.prompt_path(false, title, cx, |this, path, cx| {
                            this.emu.own_program = Some(path);
                            cx.notify();
                        });
                    }),
                ),
            ))
            .child(widgets::setting_row(
                self.tr("Arguments", "Argümanlar"),
                "",
                div().w(px(300.)).child(args),
            ))
            .child(widgets::setting_row(
                self.tr("Systems", "Sistemler"),
                "",
                systems,
            ))
            .child(
                div().flex().child(
                    style::button(
                        "own-emu-save",
                        self.tr("Add the emulator", "Emülatörü ekle"),
                        true,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.emu_save_own(cx))),
                ),
            )
    }

    fn emu_save_own(&mut self, cx: &mut Context<Self>) {
        let def = emu::custom::CustomEngine {
            name: self.field_text("own-emu-name", cx).unwrap_or_default(),
            program: self.emu.own_program.clone().unwrap_or_default(),
            args: self.field_text("own-emu-args", cx).unwrap_or_default(),
            systems: self.emu.own_systems.clone(),
        };
        match emu::custom::save(None, &def) {
            Ok(_) => {
                self.emu.own_program = None;
                self.emu.own_systems.clear();
                self.fields.inputs.remove("own-emu-name");
                self.fields.inputs.remove("own-emu-args");
                self.status = Some(self.tr("Emulator added.", "Emülatör eklendi.").into());
            }
            Err(e) => self.status = Some(format!("{e:#}").into()),
        }
        cx.notify();
    }

    /// Box art for emulator games, on the Emulators page.
    fn render_emu_covers(&mut self, cx: &mut Context<Self>) -> Div {
        let e = &self.global.emulators;
        let (libretro, launchbox) = (!e.no_covers, e.launchbox);
        let lb_state = if !launchbox {
            self.tr(
                "Off. On: about 100 MB once a week, for the box of each game's region.",
                "Kapalı. Açılınca: her oyunun kendi bölgesinin kutusu için haftada bir ~100 MB.",
            )
            .to_string()
        } else if arkor_launch::downloads::pending("launchbox") {
            self.tr("Downloading the database…", "Veritabanı indiriliyor…")
                .into()
        } else if emu::launchbox::built_at().is_some() {
            self.tr("Ready", "Hazır").into()
        } else {
            self.tr("Not downloaded yet", "Henüz indirilmedi").into()
        };
        widgets::section(self.tr("Covers", "Kapaklar"))
            .child(widgets::setting_row(
                self.tr("Box art from libretro", "libretro'dan kutu kapakları"),
                self.tr(
                    "By each ROM's name; no account.",
                    "ROM'un adına göre; hesap gerekmez.",
                ),
                widgets::switch(libretro)
                    .id("emu-covers")
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.emu_set_covers(Some(!libretro), None, cx)
                    })),
            ))
            .child(widgets::divider())
            .child(widgets::setting_row(
                self.tr("LaunchBox Games Database", "LaunchBox oyun veritabanı"),
                lb_state,
                widgets::switch(launchbox)
                    .id("emu-launchbox")
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.emu_set_covers(None, Some(!launchbox), cx)
                    })),
            ))
    }

    /// The ROM folders Arkor reads, on the Emulators page.
    fn render_rom_folders(&mut self, cx: &mut Context<Self>) -> Div {
        let mut card = widgets::section(self.tr("ROM folders", "ROM klasörleri"));
        card = card.child(widgets::label(self.tr(
            "Arkor reads these again when it starts and every few minutes: new games join the library by themselves. A game you remove from the library stays out.",
            "Arkor bunları açılışta ve birkaç dakikada bir yeniden okur: yeni oyunlar kütüphaneye kendiliğinden eklenir. Kütüphaneden kaldırdığın oyun bir daha eklenmez.",
        )));
        let folders = self.global.emulators.rom_folders.clone();
        if folders.is_empty() {
            card = card.child(widgets::label(self.tr("None yet.", "Henüz yok.")));
        }
        for (ix, dir) in folders.into_iter().enumerate() {
            let (again, forget) = (dir.clone(), dir.clone());
            card = card.child(widgets::setting_row(
                dir.display().to_string(),
                if dir.is_dir() {
                    ""
                } else {
                    self.tr("not there now (its disk?)", "şu an yok (diski takılı mı?)")
                },
                div()
                    .flex()
                    .gap_1()
                    .child(
                        style::button(("rom-again", ix), self.tr("Read now", "Şimdi oku"), false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.emu_add_folder(again.clone(), cx)
                            })),
                    )
                    .child(
                        style::button(
                            ("rom-forget", ix),
                            self.tr("Stop reading", "Okumayı bırak"),
                            false,
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.emu_forget_folder(&forget, cx)),
                        ),
                    ),
            ));
        }
        card
    }

    /// The emulators card (on the Emulators page).
    pub(super) fn render_emulators(&mut self, cx: &mut Context<Self>) -> Div {
        let mut card = widgets::section(self.tr("Emulators and BIOS", "Emülatörler ve BIOS"));
        card = card.child(widgets::label(self.tr(
            "Games of consoles you own, from your own ROM files. Arkor downloads the emulator a game needs when it first starts (or installs it from Flathub); one you already have is used.",
            "Sahip olduğun konsolların oyunları, kendi ROM dosyalarından. Bir oyunun ihtiyaç duyduğu emülatörü Arkor oyun ilk açılırken indirir (ya da Flathub'dan kurar); zaten kurulu olan kullanılır.",
        )));
        let dir = emu::bios_dir(&self.global.emulators);
        card = card.child(widgets::setting_row(
            self.tr("BIOS folder", "BIOS klasörü"),
            dir.display().to_string(),
            div()
                .flex()
                .gap_1()
                .child(bios_button(self, dir.clone()))
                .child(
                    style::button("bios-pick", self.tr("Change…", "Değiştir…"), false).on_click(
                        cx.listener(|this, _, _, cx| {
                            let title = this.tr("Choose the BIOS folder", "BIOS klasörünü seç");
                            this.prompt_path(true, title, cx, |this, path, _| {
                                this.global.emulators.bios_dir = Some(path);
                                let r = this.global.save();
                                this.report(r);
                            });
                        }),
                    ),
                ),
        ));
        for (ix, m) in emu::engines().iter().enumerate() {
            let id = m.id.clone();
            let installed = !discovery::find(m).is_empty();
            let ours = install::arkor_installed(&m.id);
            let title = format!("{} · {}", m.name, self.emu_systems_short(m));
            let hint = state_of(self, m);
            let mut actions = div().flex().gap_1();
            if installed && emu::open::can_open(&m.id) {
                let id = id.clone();
                actions = actions.child(
                    style::button(("emu-open", ix), self.tr("Open", "Aç"), false)
                        .child(style::icon("external-link", 13., pal::muted()))
                        .on_click(cx.listener(move |this, _, _, cx| this.emu_open(&id, cx))),
                );
            }
            if !installed && install::can_get(m) {
                let id = id.clone();
                actions = actions.child(
                    style::button(("emu-get", ix), self.tr("Install", "Kur"), false)
                        .on_click(cx.listener(move |this, _, _, cx| this.emu_install(&id, cx))),
                );
            } else if !installed && let Some(package) = install::distro_package(&m.id) {
                actions = actions.child(
                    style::button(("emu-pkg", ix), self.tr("Install", "Kur"), false).on_click(
                        cx.listener(move |this, _, _, cx| this.emu_install_package(package, cx)),
                    ),
                );
            }
            let open = self.emu.detail.as_deref() == Some(m.id.as_str());
            if installed || install::can_get(m) {
                let id = id.clone();
                actions = actions.child(
                    style::button(("emu-more", ix), self.tr("Options", "Seçenekler"), false)
                        .child(style::icon(
                            if open { "chevron-up" } else { "settings-2" },
                            13.,
                            pal::muted(),
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.emu.detail = if this.emu.detail.as_deref() == Some(id.as_str()) {
                                None
                            } else {
                                Some(id.clone())
                            };
                            cx.notify();
                        })),
                );
            }
            if ours {
                actions = actions.child(
                    style::button(("emu-remove", ix), self.tr("Remove", "Kaldır"), false)
                        .on_click(cx.listener(move |this, _, _, cx| this.emu_remove(&id, cx))),
                );
            }
            card = card
                .child(widgets::divider())
                .child(widgets::setting_row(title, hint, actions));
            if open {
                card = card.child(self.render_emu_detail(m, cx));
            }
        }
        card
    }

    /// An option's name, shown.
    fn emu_option_name(&self, id: &str) -> &'static str {
        match id {
            "fullscreen" => self.tr("Full screen", "Tam ekran"),
            "vsync" => self.tr(
                "Vertical sync (no tearing)",
                "Dikey eşitleme (yırtılma olmaz)",
            ),
            "show_fps" => self.tr("Show FPS", "FPS'i göster"),
            "video_driver" => self.tr("Graphics driver", "Grafik sürücüsü"),
            "pause_in_background" => self.tr(
                "Pause when another window is in front",
                "Başka pencere öndeyken duraklat",
            ),
            "exit_on_quit" => self.tr("Close when the game ends", "Oyun bitince kapat"),
            "aspect_ratio" => self.tr("Correct the aspect ratio", "En-boy oranını düzelt"),
            "boot_logo" => self.tr(
                "Show the console's start logo",
                "Konsolun açılış logosunu göster",
            ),
            "onscreen_messages" => self.tr("Messages on screen", "Ekranda mesajlar"),
            "widescreen" => self.tr("Widescreen", "Geniş ekran"),
            _ => self.tr("Option", "Seçenek"),
        }
    }

    /// Sets an emulator's option (`None`: back to the default).
    fn emu_set_option(&mut self, engine: &str, option: &str, value: Option<String>) {
        let e = &mut self.global.emulators;
        if option == "fullscreen" {
            e.windowed.retain(|w| w != engine);
        }
        let map = e.options.entry(engine.to_string()).or_default();
        match value {
            Some(v) => {
                map.insert(option.to_string(), v);
            }
            None => {
                map.remove(option);
            }
        }
        if map.is_empty() {
            e.options.remove(engine);
        }
        let r = self.global.save();
        self.report(r);
    }

    /// Asks which versions an emulator offers (in the background).
    fn emu_load_versions(&mut self, id: String, cx: &mut Context<Self>) {
        if self.emu.offered.contains_key(&id) {
            return;
        }
        self.emu.offered.insert(id.clone(), Ok(Vec::new()));
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let key = id.clone();
            let result = bg
                .spawn(async move {
                    let m = emu::engine(&key).ok_or_else(|| "unknown".to_string())?;
                    install::releases(&m)
                        .map(|r| r.into_iter().map(|r| r.version).collect::<Vec<_>>())
                        .map_err(|e| format!("{e:#}"))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.emu.offered.insert(id, result);
                cx.notify();
            });
        })
        .detach();
    }

    /// An emulator's options and versions, under its row.
    fn render_emu_detail(&mut self, m: &emu::EngineManifest, cx: &mut Context<Self>) -> Div {
        let mut out = div()
            .flex()
            .flex_col()
            .gap_2()
            .ml_4()
            .pl_4()
            .border_l_2()
            .border_color(pal::accent().opacity(0.4));
        let values = emu::launch::option_values(m, &self.global.emulators);
        let os = emu::host::Os::current();
        for (ox, o) in m
            .options
            .iter()
            .filter(|o| o.visible_when.hosts.is_empty() || o.visible_when.hosts.contains(&os))
            .enumerate()
        {
            let engine = m.id.clone();
            let oid = o.id.clone();
            let control: gpui::AnyElement = match o.kind {
                emu::manifest::OptionKind::Toggle => {
                    let on = values.get(&o.id).and_then(|v| v.as_bool()).unwrap_or(false);
                    widgets::switch(on)
                        .id(gpui::SharedString::from(format!(
                            "emu-opt-{}-{}",
                            m.id, o.id
                        )))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.emu_set_option(&engine, &oid, Some((!on).to_string()));
                            cx.notify();
                        }))
                        .into_any_element()
                }
                emu::manifest::OptionKind::Choice => {
                    let now = values
                        .get(&o.id)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let mut chips = div().flex().flex_wrap().gap_1();
                    for (cx_ix, c) in o.choices.iter().enumerate() {
                        let (engine, oid, c2) = (engine.clone(), oid.clone(), c.clone());
                        let label = if c.is_empty() {
                            self.tr("Automatic", "Otomatik").to_string()
                        } else {
                            c.clone()
                        };
                        chips = chips.child(
                            widgets::chip(("emu-choice-opt", ox * 16 + cx_ix), label, *c == now)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.emu_set_option(&engine, &oid, Some(c2.clone()));
                                    cx.notify();
                                })),
                        );
                    }
                    chips.into_any_element()
                }
                emu::manifest::OptionKind::Text => div().into_any_element(),
            };
            out = out.child(widgets::setting_row(
                self.emu_option_name(&o.id),
                "",
                control,
            ));
        }
        // Versions Arkor downloaded: which one games use, and removing one.
        let installed = install::installed_versions(&m.id);
        if !installed.is_empty() || install::way(m) == install::Way::Download {
            let pinned = self.global.emulators.versions.get(&m.id).cloned();
            let mut chips = div().flex().flex_wrap().gap_1();
            let id = m.id.clone();
            chips = chips.child(
                widgets::chip(
                    "emu-ver-newest",
                    self.tr("The newest", "En yenisi"),
                    pinned.is_none(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.global.emulators.versions.remove(&id);
                    let r = this.global.save();
                    this.report(r);
                    cx.notify();
                })),
            );
            for (vx, v) in installed.iter().enumerate() {
                let (id, v2, v3) = (m.id.clone(), v.clone(), v.clone());
                let id2 = m.id.clone();
                chips = chips
                    .child(
                        widgets::chip(("emu-ver", vx), v.clone(), pinned.as_deref() == Some(v))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.global
                                    .emulators
                                    .versions
                                    .insert(id.clone(), v2.clone());
                                let r = this.global.save();
                                this.report(r);
                                cx.notify();
                            })),
                    )
                    .when(installed.len() > 1, |c| {
                        c.child(
                            style::button(("emu-ver-rm", vx), "", false)
                                .child(style::icon("trash-2", 12., pal::muted()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.global.emulators.versions.get(&id2) == Some(&v3) {
                                        this.global.emulators.versions.remove(&id2);
                                        let _ = this.global.save();
                                    }
                                    let r = install::remove_version(&id2, &v3);
                                    this.report(r);
                                    cx.notify();
                                })),
                        )
                    });
            }
            out = out.child(widgets::setting_row(
                self.tr("Version games use", "Oyunların kullandığı sürüm"),
                "",
                chips,
            ));
            if install::way(m) == install::Way::Download {
                let mut other = div().flex().flex_wrap().gap_1();
                match self.emu.offered.get(&m.id).cloned() {
                    None => {
                        let id = m.id.clone();
                        other = other.child(
                            style::button(
                                "emu-ver-list",
                                self.tr("Show versions", "Sürümleri göster"),
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.emu_load_versions(id.clone(), cx);
                                    cx.notify();
                                },
                            )),
                        );
                    }
                    Some(Ok(list)) if list.is_empty() => {
                        other = other.child(widgets::label(self.tr("Looking…", "Bakılıyor…")));
                    }
                    Some(Ok(list)) => {
                        for (vx, v) in list.iter().filter(|v| !installed.contains(v)).enumerate() {
                            let key = format!("emu:{}@{v}", m.id);
                            other = other.child(
                                widgets::chip(("emu-ver-get", vx), v.clone(), false).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.enqueue_download(&key, cx)
                                    }),
                                ),
                            );
                        }
                    }
                    Some(Err(e)) => other = other.child(widgets::label(e)),
                }
                out = out.child(widgets::setting_row(
                    self.tr("Install another version", "Başka bir sürüm kur"),
                    "",
                    other,
                ));
            }
        }
        out
    }

    /// The ROM part of the add page: ROM folders found, and choosing one.
    pub(super) fn render_add_roms(&mut self, cx: &mut Context<Self>) -> Div {
        self.emu_look(cx);
        let mut card = widgets::section(self.tr("Emulator games", "Emülatör oyunları"));
        card = card.child(widgets::label(self.tr(
            "Adds every game in a folder of ROMs (one folder per system, as EmuDeck and RetroDECK keep them, or any folder). The emulator each one needs is downloaded when it first starts.",
            "Bir ROM klasöründeki bütün oyunları ekler (EmuDeck ve RetroDECK'teki gibi her sistem için bir klasör, ya da herhangi bir klasör). Her birinin ihtiyaç duyduğu emülatör ilk açılışta indirilir.",
        )));
        for (ix, source) in self
            .emu
            .suggested
            .clone()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            let systems: Vec<String> = source
                .systems
                .iter()
                .filter_map(|(s, _)| emu::systems::get(s).map(|s| s.short.clone()))
                .collect();
            let root = source.root.clone();
            card = card.child(widgets::setting_row(
                source.root.display().to_string(),
                systems.join(", "),
                style::button(
                    ("rom-folder", ix),
                    self.tr("Add its games", "Oyunlarını ekle"),
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.emu_add_folder(root.clone(), cx))),
            ));
        }
        card.child(
            div().flex().gap_2().child(
                style::button(
                    "rom-pick",
                    self.tr("Choose a ROM folder…", "ROM klasörü seç…"),
                    true,
                )
                .when(self.emu.scanning, |b| b.opacity(0.5))
                .on_click(cx.listener(|this, _, _, cx| {
                    let title = this.tr("Choose a folder of games", "Oyun klasörü seç");
                    this.prompt_path(true, title, cx, |this, path, cx| {
                        this.emu_add_folder(path, cx)
                    });
                })),
            ),
        )
    }

    // ---- Console mode ----

    /// The add screen's ROM lines.
    pub(super) fn emu_add_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (ix, source) in self.emu.suggested.iter().flatten().enumerate() {
            let systems: Vec<String> = source
                .systems
                .iter()
                .filter_map(|(s, _)| emu::systems::get(s).map(|s| s.short.clone()))
                .collect();
            let mut row = Row::new(
                Kind::Line,
                vec![Item::line(
                    Act::EmuAddSuggested(ix),
                    "joystick",
                    source.root.display().to_string(),
                    Value::Text(systems.join(", ")),
                )],
            );
            if ix == 0 {
                row.title = Some(self.tr("ROM folders", "ROM klasörleri").into());
            }
            rows.push(row);
        }
        rows
    }

    /// A ROM folder found earlier, chosen in console mode.
    pub(super) fn emu_add_suggested(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(root) = self
            .emu
            .suggested
            .as_ref()
            .and_then(|s| s.get(ix))
            .map(|s| s.root.clone())
        {
            self.emu_add_folder(root, cx);
        }
    }

    /// Where the console's folder picker starts for ROMs.
    pub(super) fn emu_start_folder(&self) -> Option<PathBuf> {
        self.emu
            .suggested
            .as_ref()
            .and_then(|s| s.first())
            .map(|s| s.root.clone())
            .or_else(::dirs::home_dir)
    }

    /// Console settings → Emulators.
    pub(super) fn emulator_rows(&self) -> Vec<Row> {
        let line = |act: Act, icon: &'static str, label: &str, value: Value| {
            Row::new(Kind::Line, vec![Item::line(act, icon, label, value)])
        };
        let mut rows = vec![line(
            Act::Folder(FolderFor::Bios),
            "cpu",
            self.tr("BIOS folder", "BIOS klasörü"),
            Value::Text(emu::bios_dir(&self.global.emulators).display().to_string()),
        )];
        for (ix, m) in emu::engines().iter().enumerate() {
            let installed = !discovery::find(m).is_empty();
            let act = if installed && emu::open::can_open(&m.id) {
                Act::EmuOpen(m.id.clone())
            } else if installed {
                Act::Nothing
            } else if install::can_get(m) {
                Act::Queue(format!("emu:{}", m.id))
            } else if let Some(package) = install::distro_package(&m.id) {
                Act::InstallTools(vec![package.to_string()])
            } else {
                Act::Nothing
            };
            let mut row = line(act, "joystick", &m.name, Value::Text(state_of(self, m)));
            row.items[0].sub = self.emu_systems_short(m);
            if ix == 0 {
                row.title = Some(self.tr("Emulators", "Emülatörler").into());
            }
            rows.push(row);
        }
        rows
    }

    /// The emulator choices of an emulator game in console mode: (label, value).
    pub(super) fn emu_game_choices(&self, entry: &LibraryGame) -> Vec<(String, String)> {
        let Some((system, _)) = Self::emu_parts(entry) else {
            return Vec::new();
        };
        std::iter::once((
            self.tr("System default", "Sistemin varsayılanı")
                .to_string(),
            String::new(),
        ))
        .chain(
            emu::choices_for(system)
                .into_iter()
                .map(|c| (c.label(), c.to_string())),
        )
        .collect()
    }
}

fn self_text(lib: &Library, en: &'static str, tr: &'static str) -> String {
    lib.tr(en, tr).to_string()
}

/// "Open" for the BIOS folder (made when missing).
fn bios_button(lib: &Library, dir: PathBuf) -> gpui::Stateful<Div> {
    style::button("bios-open", lib.tr("Open", "Aç"), false)
        .child(style::icon("folder-open", 13., pal::muted()))
        .on_click(move |_, _, _| {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
        })
}

impl EmuUi {
    /// Looks for ROM folders again next time.
    pub(super) fn forget_folders(&mut self) {
        self.suggested = None;
    }
}
