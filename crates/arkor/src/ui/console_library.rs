//! The library from console mode: adding games (found in other launchers, from a Wine
//! prefix, a program chosen on a disk, a launcher to install) and arranging them
//! (favourites, name, category, hidden, cover, sort order, removal).

use std::path::{Path, PathBuf};

use arkor_core::library::{GameKind, LibraryGame, NewGame};
use arkor_launch::covers::{self, SgdbImage};
use arkor_launch::launchers;
use arkor_stores::{Detected, prefix};
use gpui::Context;

use super::Library;
use super::Sort;
use super::console::{
    Act, Art, Filter, FolderFor, Item, Kind, Look, Opt, Overlay, Row, Screen, Value,
};
use super::console_settings::GameOpt;
use super::synth::Cue;

/// A cover of SteamGridDB's, and its small picture when it was fetched.
type Cover = (SgdbImage, Option<PathBuf>);

#[derive(Default)]
pub(super) struct LibState {
    /// Games of other launchers, once looked for.
    found: Option<Vec<Detected>>,
    looking: bool,
    /// The programs of a prefix being imported.
    prefix: Option<PathBuf>,
    candidates: Vec<prefix::Candidate>,
    /// The library's order.
    sort: Sort,
    /// SteamGridDB's covers for a game, by its entry.
    covers: Option<(String, Result<Vec<Cover>, String>)>,
}

/// Files in `dir` a game can be started with.
pub(super) fn programs(dir: &Path) -> Vec<String> {
    let mut list: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            [".exe", ".bat", ".lnk", ".sh", ".appimage", ".x86_64"]
                .iter()
                .any(|x| name.ends_with(x))
                || executable(&e.path())
                || arkor_launch::emu::games::looks_like_rom(&name)
        })
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    list.sort_by_key(|n| n.to_lowercase());
    list
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

/// A program as a library game: Windows' run through Proton, a game an emulator plays
/// through its emulator, others as they are.
fn new_game(path: &Path) -> NewGame {
    if let Some(new) = arkor_launch::emu::games::new_game_for(path) {
        return new;
    }
    let name = prefix::suggested_name(path);
    let lower = path.to_string_lossy().to_lowercase();
    let windows = [".exe", ".bat", ".lnk", ".msi"]
        .iter()
        .any(|x| lower.ends_with(x));
    let mut new = NewGame::windows(name, path.to_path_buf());
    if !windows {
        new.kind = GameKind::Linux;
    }
    new
}

impl Library {
    /// The library sorted as chosen, favourites first.
    pub(super) fn console_sorted(&self) -> Vec<&LibraryGame> {
        let mut games: Vec<&LibraryGame> =
            self.library.games.iter().filter(|g| !g.hidden).collect();
        games.sort_by_key(|g| g.name.to_lowercase());
        match self.console.lib.sort {
            Sort::Name => {}
            Sort::Recent => games.sort_by_key(|g| std::cmp::Reverse(self.playtime(&g.game_id()).1)),
            Sort::Playtime => {
                games.sort_by_key(|g| std::cmp::Reverse(self.playtime(&g.game_id()).0))
            }
        }
        games.sort_by_key(|g| !g.favorite);
        games
    }

    /// The library screen's extra choices: favourites, hidden games, the order.
    pub(super) fn library_pills(&self) -> Vec<Item> {
        let f = &self.console.filter;
        let mut pills = Vec::new();
        if self.library.games.iter().any(|g| g.favorite) {
            pills.push(Item::pill(
                Act::Filter(Filter::Favorites),
                "star",
                self.tr("Favourites", "Favoriler"),
                *f == Filter::Favorites,
            ));
        }
        if self.library.games.iter().any(|g| g.hidden) {
            pills.push(Item::pill(
                Act::Filter(Filter::Hidden),
                "eye-off",
                self.tr("Hidden", "Gizliler"),
                *f == Filter::Hidden,
            ));
        }
        let order = match self.console.lib.sort {
            Sort::Name => self.tr("A–Z", "A–Z"),
            Sort::Recent => self.tr("Last played", "Son oynanan"),
            Sort::Playtime => self.tr("Most played", "En çok oynanan"),
        };
        pills.push(Item::pill(Act::SortLibrary, "arrow-down-up", order, false));
        pills.push(Item::pill(
            Act::Screen(Screen::AddGames),
            "plus",
            self.tr("Add games", "Oyun ekle"),
            false,
        ));
        pills
    }

    /// Looks at other launchers (once per opening).
    pub(super) fn add_opened(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || self.console.lib.looking {
            return;
        }
        self.console.lib.looking = true;
        self.emu_look(cx);
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let (found, _) = bg.spawn(async { arkor_stores::detect() }).await;
            let _ = this.update(cx, |this, cx| {
                this.console.lib.looking = false;
                this.console.lib.found = Some(found);
                cx.notify();
            });
        })
        .detach();
    }

    /// The found games that are not in the library yet.
    fn not_added(&self) -> Vec<(usize, &Detected)> {
        self.console
            .lib
            .found
            .iter()
            .flatten()
            .enumerate()
            .filter(|(_, d)| d.new.is_ok() && !self.library.has_origin(&d.game.id.to_string()))
            .collect()
    }

    pub(super) fn add_rows(&self) -> Vec<Row> {
        let line = |act: Act, icon: &'static str, label: &str, value: Value| {
            Row::new(Kind::Line, vec![Item::line(act, icon, label, value)])
        };
        let mut rows = vec![Row::new(
            Kind::Pills,
            vec![
                Item::main(
                    Act::Folder(FolderFor::GameFile),
                    "file-plus",
                    self.tr("Choose a program", "Program seç"),
                ),
                Item::pill(
                    Act::Folder(FolderFor::Roms),
                    "joystick",
                    self.tr("A ROM folder", "ROM klasörü"),
                    false,
                ),
                Item::pill(
                    Act::Folder(FolderFor::ImportPrefix),
                    "folder-input",
                    self.tr("From a Wine prefix", "Wine prefix'ten"),
                    false,
                ),
                Item::pill(
                    Act::AddScan,
                    "refresh-cw",
                    self.tr("Look again", "Yeniden ara"),
                    false,
                ),
            ],
        )];
        // A prefix that was chosen: its programs.
        if let Some(dir) = &self.console.lib.prefix {
            let mut first = true;
            for (n, c) in self.console.lib.candidates.iter().enumerate() {
                let name = c
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut row = line(
                    Act::AddCandidate(n),
                    "file-play",
                    &name,
                    Value::Text(super::console_library::folder_of(&c.path)),
                );
                if std::mem::take(&mut first) {
                    row.title = Some(format!(
                        "{} · {}",
                        self.tr("In the prefix", "Prefix'te"),
                        dir.display()
                    ));
                }
                rows.push(row);
            }
            if first {
                let mut row = line(
                    Act::Nothing,
                    "info",
                    self.tr("No games were found in it.", "İçinde oyun bulunamadı."),
                    Value::None,
                );
                row.title = Some(self.tr("In the prefix", "Prefix'te").into());
                rows.push(row);
            }
        }
        rows.extend(self.emu_add_rows());
        let found = self.not_added();
        let mut first = true;
        for (n, d) in &found {
            let mut row = line(
                Act::AddFound(*n),
                "plus",
                &d.game.name,
                Value::Text(self.store_name(&d.game.id)),
            );
            if std::mem::take(&mut first) {
                row.title = Some(
                    self.tr("On this computer", "Bu bilgisayarda bulunanlar")
                        .into(),
                );
            }
            rows.push(row);
        }
        if first {
            let mut row = line(
                Act::Nothing,
                if self.console.lib.looking {
                    "loader-circle"
                } else {
                    "info"
                },
                if self.console.lib.looking {
                    self.tr(
                        "Looking in other launchers…",
                        "Diğer başlatıcılarda aranıyor…",
                    )
                } else {
                    self.tr(
                        "Every game of other launchers is in the library.",
                        "Diğer başlatıcıların tüm oyunları kütüphanede.",
                    )
                },
                Value::None,
            );
            row.title = Some(
                self.tr("On this computer", "Bu bilgisayarda bulunanlar")
                    .into(),
            );
            rows.push(row);
        }
        for (n, l) in launchers::LAUNCHERS.iter().enumerate() {
            let mut row = line(Act::AddLauncher(l.id), "download", l.name, Value::None);
            if n == 0 {
                row.title = Some(self.tr("Install a launcher", "Başlatıcı kur").into());
            }
            rows.push(row);
        }
        rows
    }

    fn store_name(&self, id: &arkor_core::GameId) -> String {
        use arkor_core::Store;
        match id.store {
            Store::Steam => "Steam",
            Store::Epic => "Epic",
            Store::Gog => "GOG",
            Store::Amazon => "Amazon",
            Store::Heroic => "Heroic",
            Store::Lutris => "Lutris",
            Store::Faugus => "Faugus",
            _ => "",
        }
        .to_string()
    }

    /// The programs of a chosen prefix.
    pub(super) fn import_prefix(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(root) = prefix::resolve(&dir) else {
            self.status = Some(
                self.tr(
                    "That is no Wine prefix (it has no drive_c).",
                    "Bu bir Wine prefix'i değil (drive_c yok).",
                )
                .into(),
            );
            return cx.notify();
        };
        self.console.lib.prefix = Some(root.clone());
        self.console.lib.candidates.clear();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let scan = root.clone();
            let found = bg
                .spawn(async move { prefix::find_executables(&scan) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.console.lib.prefix.as_ref() == Some(&root) {
                    this.console.lib.candidates = found;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Adds a game and says so; its cover is looked for.
    fn console_add(&mut self, new: NewGame, cx: &mut Context<Self>) {
        let name = new.name.clone();
        if let Some(id) = self.add_entry(new, cx) {
            self.auto_cover(&id.id, cx);
            self.cue(Cue::Done);
            self.status = Some(format!("{} {name}", self.tr("Added:", "Eklendi:")).into());
        }
    }

    // ---- A game's own things ----

    fn edit_menu(&mut self, entry: &str) {
        let Some(g) = self.library.get(entry).cloned() else {
            return;
        };
        let t = |en, tr| self.tr(en, tr).to_string();
        let mut options = vec![
            (
                if g.favorite {
                    t("Not a favourite", "Favorilerden çıkar")
                } else {
                    t("A favourite", "Favorilere ekle")
                },
                Act::Favorite(entry.into()),
                false,
            ),
            (
                t("Rename", "Adını değiştir"),
                Act::Opt(Opt::Game(GameOpt::Name)),
                false,
            ),
            (
                t("Category", "Kategori"),
                Act::PickCategory(entry.into()),
                false,
            ),
            (
                if g.hidden {
                    t("Show in the library", "Kütüphanede göster")
                } else {
                    t("Hide in the library", "Kütüphanede gizle")
                },
                Act::HideGame(entry.into()),
                false,
            ),
        ];
        if !self.global.steamgriddb_key.trim().is_empty() {
            options.push((
                t("Change its cover", "Kapağını değiştir"),
                Act::Screen(Screen::Covers(entry.into())),
                false,
            ));
        }
        options.push((
            t("Remove from the library", "Kütüphaneden kaldır"),
            Act::AskRemoveGame(entry.into()),
            false,
        ));
        options.push((t("Close", "Kapat"), Act::Close, false));
        self.cue(Cue::Open);
        self.console_overlay(Overlay::Pick(g.name, options));
    }

    fn category_menu(&mut self, entry: &str) {
        let now = self.library.get(entry).and_then(|g| g.category.clone());
        let mut options = vec![(
            self.tr("None", "Yok").to_string(),
            Act::Pick(Opt::Game(GameOpt::Category), String::new()),
            now.is_none(),
        )];
        for c in self.categories() {
            let on = now.as_ref() == Some(&c);
            options.push((c.clone(), Act::Pick(Opt::Game(GameOpt::Category), c), on));
        }
        options.push((
            self.tr("A new one…", "Yeni kategori…").to_string(),
            Act::Opt(Opt::Game(GameOpt::NewCategory)),
            false,
        ));
        self.console_overlay(Overlay::Pick(
            self.tr("Category", "Kategori").into(),
            options,
        ));
    }

    /// What was typed for a game: its name, or a new category.
    pub(super) fn library_text(&mut self, o: GameOpt, text: &str) {
        let Some(entry) = self.entry().map(|g| g.id.clone()) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        match o {
            GameOpt::Name => {
                if let Err(e) = self.library.rename(&entry, text) {
                    self.status = Some(format!("{e:#}").into());
                    return;
                }
            }
            GameOpt::NewCategory => {
                if let Some(g) = self.entry_mut() {
                    g.category = Some(text.to_string());
                }
            }
            _ => return,
        }
        self.save_library();
        self.reload_games();
    }

    pub(super) fn library_text_of(&self, o: GameOpt) -> Option<String> {
        match o {
            GameOpt::Name => self.entry().map(|g| g.name.clone()),
            GameOpt::NewCategory => Some(String::new()),
            _ => None,
        }
    }

    // ---- Covers ----

    /// SteamGridDB's covers of the game.
    pub(super) fn covers_opened(&mut self, entry: &str, cx: &mut Context<Self>) {
        let key = self.global.steamgriddb_key.clone();
        let Some(name) = self.library.get(entry).map(|g| g.name.clone()) else {
            return;
        };
        if cfg!(test)
            || self
                .console
                .lib
                .covers
                .as_ref()
                .is_some_and(|(e, _)| e == entry)
        {
            return;
        }
        self.console.lib.covers = Some((entry.to_string(), Ok(Vec::new())));
        let of = entry.to_string();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let found = bg
                .spawn(async move {
                    let game = covers::sgdb_search(&name, &key)?
                        .into_iter()
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("SteamGridDB does not know this game"))?;
                    let list = covers::sgdb_covers(game.id, false, &key)?;
                    Ok(list
                        .into_iter()
                        .take(18)
                        .map(|i| {
                            let thumb = covers::sgdb_thumb(&i).ok();
                            (i, thumb)
                        })
                        .collect())
                })
                .await
                .map_err(|e: anyhow::Error| format!("{e:#}"));
            let _ = this.update(cx, |this, cx| {
                this.console.lib.covers = Some((of, found));
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn cover_rows(&self, entry: &str) -> Vec<Row> {
        let line = |act: Act, icon: &'static str, label: &str| {
            Row::new(Kind::Line, vec![Item::line(act, icon, label, Value::None)])
        };
        let mut rows = vec![line(
            Act::AutoCover(entry.into()),
            "wand-sparkles",
            self.tr("Find one by itself", "Kendiliğinden bul"),
        )];
        match &self.console.lib.covers {
            Some((e, Ok(list))) if e == entry && list.is_empty() => {
                rows.push(line(
                    Act::Nothing,
                    "loader-circle",
                    self.tr("Looking…", "Bakılıyor…"),
                ));
            }
            Some((e, Ok(list))) if e == entry => {
                let items = list
                    .iter()
                    .enumerate()
                    .filter_map(|(n, (_, thumb))| {
                        Some(Item {
                            act: Act::SetCover(n),
                            label: String::new(),
                            sub: String::new(),
                            look: Look::Cover(Art::File(thumb.clone()?, false)),
                            progress: None,
                            mark: None,
                        })
                    })
                    .collect();
                rows.extend(self.grid(items));
            }
            Some((e, Err(why))) if e == entry => {
                let mut row = line(
                    Act::Nothing,
                    "circle-alert",
                    self.tr("No covers", "Kapak yok"),
                );
                row.items[0].sub = why.clone();
                rows.push(row);
            }
            _ => {}
        }
        rows
    }

    // ---- What their lines do ----

    pub(super) fn library_act(&mut self, act: Act, cx: &mut Context<Self>) {
        match act {
            Act::AddScan => {
                self.console.lib.found = None;
                self.emu.forget_folders();
                self.add_opened(cx);
            }
            Act::AddFound(n) => {
                let new = self
                    .console
                    .lib
                    .found
                    .as_ref()
                    .and_then(|f| f.get(n))
                    .and_then(|d| d.new.clone().ok());
                if let Some(new) = new {
                    self.console_add(new, cx);
                }
            }
            Act::AddCandidate(n) => {
                let Some(c) = self.console.lib.candidates.get(n).cloned() else {
                    return;
                };
                let mut new = NewGame::windows(prefix::suggested_name(&c.path), c.path.clone());
                new.prefix = self.console.lib.prefix.clone();
                self.console_add(new, cx);
            }
            Act::AddFile(path) => {
                self.console.overlay = None;
                self.console_add(new_game(&path), cx);
            }
            Act::AddLauncher(id) => self.install_launcher(id, cx),
            Act::EditGame(entry) => self.edit_menu(&entry),
            Act::Favorite(entry) => {
                self.console.overlay = None;
                if let Some(g) = self.library.get_mut(&entry) {
                    g.favorite = !g.favorite;
                }
                self.save_library();
            }
            Act::HideGame(entry) => {
                self.console.overlay = None;
                if let Some(g) = self.library.get_mut(&entry) {
                    g.hidden = !g.hidden;
                }
                self.save_library();
                self.apply_filter();
            }
            Act::PickCategory(entry) => self.category_menu(&entry),
            Act::AskRemoveGame(entry) => {
                let Some(name) = self.library.get(&entry).map(|g| g.name.clone()) else {
                    return;
                };
                let text = format!(
                    "{name}: {}",
                    self.tr(
                        "remove it from the library? Its files and saves stay.",
                        "kütüphaneden kaldırılsın mı? Dosyaları ve kayıtları kalır."
                    )
                );
                let yes = self.tr("Remove", "Kaldır").to_string();
                self.console_overlay(Overlay::Ask(text, yes, Act::RemoveGame(entry)));
            }
            Act::RemoveGame(entry) => {
                self.console.overlay = None;
                self.emu_note_removed(&entry);
                self.library.remove(&entry);
                let r = self.library.save();
                self.report(r);
                self.reload_games();
                self.console_open(Screen::Library, cx);
            }
            Act::SetCover(n) => {
                let Some((entry, Ok(list))) = &self.console.lib.covers else {
                    return;
                };
                let (entry, image) = (entry.clone(), list.get(n).map(|(i, _)| i.clone()));
                let Some(image) = image else { return };
                let bg = cx.background_executor().clone();
                cx.spawn(async move |this, cx| {
                    let of = entry.clone();
                    let result = bg
                        .spawn(async move { covers::set_from_sgdb(&of, &image) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        match result {
                            Ok(_) => {
                                this.refresh_cover(&entry);
                                this.cue(Cue::Done);
                                if matches!(this.console.screen(), Screen::Covers(_)) {
                                    this.console_back(cx);
                                }
                            }
                            Err(e) => this.status = Some(format!("{e:#}").into()),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            Act::AutoCover(entry) => self.auto_cover(&entry, cx),
            Act::SortLibrary => {
                self.console.lib.sort = match self.console.lib.sort {
                    Sort::Name => Sort::Recent,
                    Sort::Recent => Sort::Playtime,
                    Sort::Playtime => Sort::Name,
                };
            }
            _ => {}
        }
        cx.notify();
    }
}

/// The folder a program is in, by its name.
pub(super) fn folder_of(path: &Path) -> String {
    path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_found_and_told_apart() {
        let tmp = tempfile::tempdir().unwrap();
        for f in ["Game.exe", "readme.txt", "start.sh", ".hidden.exe"] {
            std::fs::write(tmp.path().join(f), "").unwrap();
        }
        std::fs::create_dir(tmp.path().join("data.exe.d")).unwrap();
        assert_eq!(programs(tmp.path()), ["Game.exe", "start.sh"]);
        assert_eq!(
            new_game(&tmp.path().join("Game.exe")).kind,
            GameKind::Windows
        );
        assert_eq!(new_game(&tmp.path().join("start.sh")).kind, GameKind::Linux);
    }
}
