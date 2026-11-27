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
        if keys.is_empty() || cfg!(test) {
            return false;
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

    /// Adds the games in a ROM folder to the library (read in the background).
    pub(super) fn emu_add_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        if self.emu.scanning {
            return;
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
        self.watch_progress(cx);
        self.status = Some(format!("{} {package}…", self.tr("Installing", "Kuruluyor:")).into());
        cx.notify();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg
                .spawn(async move {
                    let _job = arkor_launch::progress::job("system-tools", package);
                    arkor_launch::tools::install_packages(&[package])
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok(()) => this.tr("Installed.", "Kuruldu.").into(),
                    Err(e) => format!("{e:#}").into(),
                });
                cx.notify();
            });
        })
        .detach();
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

    /// The Emulators section of the settings page.
    pub(super) fn render_emulators(&mut self, cx: &mut Context<Self>) -> Div {
        let mut card = widgets::section(self.tr("Emulators", "Emülatörler"));
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
            if ours {
                actions = actions.child(
                    style::button(("emu-remove", ix), self.tr("Remove", "Kaldır"), false)
                        .on_click(cx.listener(move |this, _, _, cx| this.emu_remove(&id, cx))),
                );
            }
            card = card
                .child(widgets::divider())
                .child(widgets::setting_row(title, hint, actions));
        }
        card
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
            let act = if installed {
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
