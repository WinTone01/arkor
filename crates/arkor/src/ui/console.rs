//! Console mode: the whole of Arkor with a gamepad, for the couch. Everything on a screen is
//! rows of items; the focus moves over them, and choosing an item does its action. Keyboard
//! and mouse work, too. How it looks is in `console_view`, gamepads are in `gamepad`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use arkor_core::library::{GameKind, LibraryGame};
use arkor_launch::downloads::{self, State};
use arkor_launch::progress;
use arkor_launch::shops::{self, Shop};
use gpui::{Context, KeyBinding, Window, actions};

pub(super) use super::gamepad::Nav;
use super::{Library, Selection, cover_art, shops_page};

actions!(
    console,
    [
        ToggleConsole,
        NavUp,
        NavDown,
        NavLeft,
        NavRight,
        NavAccept,
        NavBack,
        NavExtra,
        NavSearch,
        NavPrevTab,
        NavNextTab,
        NavPageUp,
        NavPageDown,
        NavMenu
    ]
);

pub(super) const CONTEXT: &str = "Console";

pub fn bind_keys(cx: &mut gpui::App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("f11", ToggleConsole, None),
        KeyBinding::new("up", NavUp, c),
        KeyBinding::new("down", NavDown, c),
        KeyBinding::new("left", NavLeft, c),
        KeyBinding::new("right", NavRight, c),
        KeyBinding::new("enter", NavAccept, c),
        KeyBinding::new("escape", NavBack, c),
        KeyBinding::new("tab", NavNextTab, c),
        KeyBinding::new("shift-tab", NavPrevTab, c),
        KeyBinding::new("pageup", NavPageUp, c),
        KeyBinding::new("pagedown", NavPageDown, c),
        KeyBinding::new("delete", NavExtra, c),
        KeyBinding::new("ctrl-f", NavSearch, c),
        KeyBinding::new("f10", NavMenu, c),
    ]);
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Screen {
    Home,
    Library,
    Stores,
    Downloads,
    Settings,
    /// A library game, by its id.
    Game(String),
    /// A store game.
    ShopGame(Shop, String),
    /// A library game's settings: their categories.
    GameSettings(String),
    /// One category of a game's settings.
    GameCat(String, super::console_settings::GameCat),
    /// One category of Arkor's settings.
    SettingsCat(super::console_settings::AppCat),
}

impl Screen {
    pub(super) const TABS: [Screen; 5] = [
        Screen::Home,
        Screen::Library,
        Screen::Stores,
        Screen::Downloads,
        Screen::Settings,
    ];
}

/// Which games the library screen shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum Filter {
    #[default]
    All,
    Recent,
    Category(String),
}

/// A switch or choice on the settings or a game's screen.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Opt {
    /// One of the quick switches of the game page.
    Quick(usize),
    Language,
    Theme,
    Accent,
    InstallProton,
    InstallMenu,
    InstallDesktop,
    Tray,
    Updates,
    DefaultProton,
    Motion,
    Sounds,
    StartHere,
    Autostart,
    /// Games started from console mode full screen in gamescope.
    FullScreen,
    GlobalHud,
    /// One of a game's settings.
    Game(super::console_settings::GameOpt),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Act {
    Screen(Screen),
    Play(String),
    Stop(String),
    Filter(Filter),
    ShopTab(Option<Shop>),
    OnlyInstalled,
    Search,
    ClearSearch,
    Login(Shop),
    Shift,
    Symbols,
    Queue(String),
    ToolMenu(String),
    AskRemoveTool(String),
    RemoveTool(String),
    CheckUpdates,
    UpdateAll,
    ProtonVariant(usize),
    InstallRelease(String),
    InstallTools(Vec<String>),
    Mount(String),
    Folder(FolderFor),
    FolderOpen(PathBuf),
    FolderUp,
    FolderPick,
    InstallAnyway,
    Logout(Shop),
    Opt(Opt),
    /// Pick this value of an option (in the picker).
    Pick(Opt, String),
    Platform(bool),
    GameLanguage,
    PickLanguage(String),
    Dlc(String),
    Install,
    AskUninstall(Shop, String),
    Uninstall(Shop, String),
    Repair(Shop, String),
    Update(Shop, String),
    ShopPlay(Shop, String),
    /// A queued download: pause, go on or try again.
    Download(String),
    CancelDownload(String),
    Desktop,
    /// The game's page in the desktop interface.
    DesktopPage(String),
    AskQuit,
    Quit,
    AskPower(Power),
    Power(Power),
    Close,
    /// Keys of the on-screen keyboard.
    Key(char),
    Backspace,
    Done,
    /// Something to read, not to choose.
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Power {
    Suspend,
    Restart,
    Off,
}

impl Power {
    fn verb(self) -> &'static str {
        match self {
            Power::Suspend => "suspend",
            Power::Restart => "reboot",
            Power::Off => "poweroff",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Art {
    Game(String),
    Shop(Shop, String),
}

/// How an item is drawn.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Look {
    Cover(Art),
    /// A rounded button; `on` is the chosen one of its row.
    Pill {
        icon: &'static str,
        on: bool,
        main: bool,
    },
    /// A line across the screen with its value at the end.
    Line {
        icon: &'static str,
        value: Value,
        art: Option<Art>,
    },
    /// A key of the keyboard (and how many it is wide).
    Key(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Value {
    None,
    Switch(bool),
    Text(String),
    /// One of several: left and right turn it.
    Choice(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Item {
    pub(super) act: Act,
    pub(super) label: String,
    pub(super) sub: String,
    pub(super) look: Look,
    pub(super) progress: Option<f32>,
    /// Running or installed: a small mark on the cover.
    pub(super) mark: Option<&'static str>,
}

impl Item {
    pub(super) fn pill(act: Act, icon: &'static str, label: impl Into<String>, on: bool) -> Self {
        Item {
            act,
            label: label.into(),
            sub: String::new(),
            look: Look::Pill {
                icon,
                on,
                main: false,
            },
            progress: None,
            mark: None,
        }
    }

    pub(super) fn main(act: Act, icon: &'static str, label: impl Into<String>) -> Self {
        let mut i = Item::pill(act, icon, label, false);
        i.look = Look::Pill {
            icon,
            on: false,
            main: true,
        };
        i
    }

    pub(super) fn line(
        act: Act,
        icon: &'static str,
        label: impl Into<String>,
        value: Value,
    ) -> Self {
        Item {
            act,
            label: label.into(),
            sub: String::new(),
            look: Look::Line {
                icon,
                value,
                art: None,
            },
            progress: None,
            mark: None,
        }
    }

    fn key(act: Act, label: impl Into<String>, wide: f32) -> Self {
        Item {
            act,
            label: label.into(),
            sub: String::new(),
            look: Look::Key(wide),
            progress: None,
            mark: None,
        }
    }
}

/// How a row is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// Covers that scroll sideways.
    Shelf,
    /// One line of a grid of covers.
    Grid,
    Pills,
    Line,
    Keys,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Row {
    pub(super) title: Option<String>,
    pub(super) kind: Kind,
    pub(super) items: Vec<Item>,
}

impl Row {
    pub(super) fn new(kind: Kind, items: Vec<Item>) -> Self {
        Row {
            title: None,
            kind,
            items,
        }
    }

    fn titled(title: impl Into<String>, kind: Kind, items: Vec<Item>) -> Self {
        Row {
            title: Some(title.into()),
            kind,
            items,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Focus {
    pub(super) row: usize,
    /// Where each row was left.
    cols: HashMap<usize, usize>,
    /// Eased scroll positions: the screen's, and each shelf's.
    pub(super) scroll: f32,
    pub(super) shelves: HashMap<usize, f32>,
}

impl Focus {
    pub(super) fn col(&self) -> usize {
        self.cols.get(&self.row).copied().unwrap_or(0)
    }

    /// Keeps the focus on something that exists.
    fn clamp(&mut self, rows: &[Row]) {
        self.row = self.row.min(rows.len().saturating_sub(1));
        if let Some(r) = rows.get(self.row) {
            let col = self.col().min(r.items.len().saturating_sub(1));
            self.cols.insert(self.row, col);
        }
    }

    pub(super) fn set(&mut self, row: usize, col: usize) {
        self.row = row;
        self.cols.insert(row, col);
    }

    /// Moves; `true` when the focus is elsewhere now.
    pub(super) fn step(&mut self, rows: &[Row], dx: i32, dy: i32) -> bool {
        self.clamp(rows);
        let Some(here) = rows.get(self.row) else {
            return false;
        };
        let before = (self.row, self.col());
        if dx != 0 {
            let last = here.items.len().saturating_sub(1) as i32;
            let col = (before.1 as i32 + dx).clamp(0, last);
            self.cols.insert(self.row, col as usize);
        }
        if dy != 0 {
            let row = (before.0 as i32 + dy).clamp(0, rows.len() as i32 - 1) as usize;
            if row != before.0 {
                let there = &rows[row];
                // In a grid (or on the keyboard) the column stays; elsewhere a row
                // remembers its own, or starts at what is chosen in it.
                let col = match (here.kind, there.kind) {
                    (Kind::Grid, Kind::Grid) => before.1,
                    (Kind::Keys, Kind::Keys) => {
                        before.1 * there.items.len() / here.items.len().max(1)
                    }
                    _ => self.cols.get(&row).copied().unwrap_or_else(|| {
                        there
                            .items
                            .iter()
                            .position(|i| matches!(i.look, Look::Pill { on: true, .. }))
                            .unwrap_or(0)
                    }),
                };
                self.set(row, col.min(there.items.len().saturating_sub(1)));
            }
        }
        before != (self.row, self.col())
    }
}

/// Something over the screen.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Overlay {
    Menu,
    Keyboard,
    /// A question: what to say, and what happens on yes.
    Ask(String, String, Act),
    /// Choosing a folder: what for, and where it is now.
    Folder(FolderFor, PathBuf),
    /// A game did not start: which, and the end of its log.
    Failed(String, String, Vec<String>),
    /// A choice: its title and (label, action, chosen).
    Pick(String, Vec<(String, Act, bool)>),
}

/// What a folder is chosen for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum FolderFor {
    /// Where a store game is installed.
    Install,
    /// A store game's Wine prefix.
    Prefix,
    /// Where store games go by default.
    Games,
}

/// The picture behind the screen.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Behind {
    pub(super) path: PathBuf,
    pub(super) size: cover_art::Size,
}

/// A game started from console mode.
#[derive(Debug, Clone)]
pub(super) struct Launch {
    /// The library entry.
    pub(super) id: String,
    pub(super) at: Instant,
    /// Its window has come (Arkor was left): the game is on.
    pub(super) playing: bool,
    /// Back in Arkor while it runs, and the "playing" screen was put away.
    pub(super) dismissed: bool,
}

#[derive(Default)]
pub(super) struct Console {
    pub(super) on: bool,
    /// The sections, with what was opened from them on top.
    stack: Vec<Screen>,
    focus: HashMap<Screen, Focus>,
    pub(super) overlay: Option<Overlay>,
    pub(super) over_focus: Focus,
    pub(super) filter: Filter,
    /// Searching in the library or the stores.
    pub(super) query: String,
    /// The last input came from a gamepad: the hints show its buttons.
    pub(super) pad: bool,
    /// When the focus moved last, and from where (its item shrinks back).
    pub(super) moved: Option<Instant>,
    pub(super) left: Option<(usize, usize)>,
    /// When the screen changed last (it fades in).
    pub(super) entered: Option<Instant>,
    /// What is behind the screen, what was before, and since when.
    pub(super) behind: Option<Behind>,
    pub(super) behind_before: Option<Behind>,
    pub(super) behind_at: Option<Instant>,
    /// A game was started: its id and when.
    pub(super) launching: Option<Launch>,
    /// Columns of the grids, from the window's width.
    pub(super) cols: usize,
    /// When it was drawn last, for the easing.
    pub(super) drawn: Option<Instant>,
    /// Something is still moving: draw again.
    pub(super) unsettled: bool,
    /// The keyboard was typed on with real keys: Enter ends it.
    typed: bool,
    /// What is in focus and since when (its picture is looked up when the focus rests).
    pub(super) behind_art: Option<(Art, Instant)>,
    /// A text of the game's settings on the keyboard (else it types the search).
    pub(super) edit: Option<(super::console_settings::GameOpt, String)>,
    /// When Steam was opened to log in.
    pub(super) steam_wanted: Option<Instant>,
    /// The Steam window guard runs.
    guarding: bool,
    /// Capitals, and the symbols page, on the keyboard.
    pub(super) shift: bool,
    pub(super) symbols: bool,
    /// The administrator password being typed, and what it is for.
    pub(super) secret: Option<(super::console_tools::NeedsPassword, String)>,
    /// The disks, when they were looked for, and whether one is being mounted.
    pub(super) drives: Vec<arkor_launch::admin::Drive>,
    pub(super) drives_at: Option<Instant>,
    pub(super) mounting: bool,
    /// The folders of the folder being looked at.
    pub(super) folders: std::cell::RefCell<Option<(PathBuf, Vec<String>)>>,
    /// The screen in device pixels (games started from console mode fill it).
    pub(super) screen_px: (u32, u32),
    /// A game ended: bring Arkor's window to the front again.
    pub(super) come_back: bool,
    /// SteamGridDB's wide pictures of library games, by library id.
    pub(super) game_heroes: HashMap<String, Option<PathBuf>>,
    /// Steam's wide pictures, looked up once.
    heroes: HashMap<String, Option<PathBuf>>,
    /// When the window's leftovers were last cleared.
    pub(super) trimmed: Option<Instant>,
}

impl Console {
    pub(super) fn screen(&self) -> Screen {
        self.stack.last().cloned().unwrap_or(Screen::Home)
    }

    /// The section the screen belongs to.
    pub(super) fn tab(&self) -> Screen {
        self.stack.first().cloned().unwrap_or(Screen::Home)
    }

    pub(super) fn focus(&self) -> Focus {
        self.focus.get(&self.screen()).cloned().unwrap_or_default()
    }

    pub(super) fn focus_mut(&mut self) -> &mut Focus {
        let screen = self.screen();
        self.focus.entry(screen).or_default()
    }

    /// Draw again soon: something moves by itself.
    pub(super) fn moving(&self) -> bool {
        self.on && self.unsettled
    }
}

const KEYS: [&str; 4] = ["1234567890", "qwertyuıopğü", "asdfghjklşi", "zxcvbnmöç."];
const KEYS_EN: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm-.:"];
/// The keyboard's symbols (passwords and e-mail addresses need them).
const SYMBOLS: [&str; 4] = ["1234567890", "@#$%&*-_+=", "!?()[]{}/\\", ".,:;'\"<>|~"];

impl Library {
    // ---- What is on the screen ----

    fn game_item(&self, g: &LibraryGame) -> Item {
        let id = g.game_id().to_string();
        let running = self.running.contains_key(&id);
        let shop = self.entry_shop(g);
        let job = shop.as_ref().and_then(|(s, i)| self.shop_job(*s, i));
        let (played, _) = self.playtime(&g.game_id());
        Item {
            act: Act::Screen(Screen::Game(g.id.clone())),
            label: g.name.clone(),
            sub: if played > 0 {
                arkor_core::stats::format_duration(played)
            } else {
                self.kind_label(g).to_string()
            },
            look: Look::Cover(Art::Game(g.id.clone())),
            progress: job.and_then(|(_, f)| f),
            mark: running.then_some("play"),
        }
    }

    fn shop_item(&self, shop: Shop, ix: usize) -> Option<Item> {
        let g = self.shops.owned(shop).get(ix)?;
        let installed = self.shops.is_installed(shop, &g.id);
        let job = self.shop_job(shop, &g.id);
        Some(Item {
            act: Act::Screen(Screen::ShopGame(shop, g.id.clone())),
            label: g.title.clone(),
            sub: match &job {
                Some((_, Some(f))) => self.percent(*f),
                Some((_, None)) => self.tr("In queue", "Sırada").into(),
                None => shop.name().into(),
            },
            look: Look::Cover(Art::Shop(shop, g.id.clone())),
            progress: job.and_then(|(_, f)| f),
            mark: installed.then_some("check"),
        })
    }

    /// Library entries by when they were played last, the never played ones left out.
    fn console_recent(&self) -> Vec<&LibraryGame> {
        let mut games: Vec<(u64, &LibraryGame)> = self
            .library
            .games
            .iter()
            .filter(|g| !g.hidden)
            .filter_map(|g| self.playtime(&g.game_id()).1.map(|t| (t, g)))
            .collect();
        games.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
        games.into_iter().map(|(_, g)| g).collect()
    }

    fn console_all(&self) -> Vec<&LibraryGame> {
        let mut games: Vec<&LibraryGame> =
            self.library.games.iter().filter(|g| !g.hidden).collect();
        games.sort_by_key(|g| g.name.to_lowercase());
        games
    }

    /// The rows of the screen.
    pub(super) fn console_rows(&self) -> Vec<Row> {
        match self.console.screen() {
            Screen::Home => self.home_rows(),
            Screen::Library => self.library_rows(),
            Screen::Stores => self.store_rows(),
            Screen::Downloads => self.download_rows(),
            Screen::Settings => self.setting_rows(),
            Screen::Game(id) => self.game_rows(&id),
            Screen::ShopGame(shop, id) => self.shop_game_rows(shop, &id),
            Screen::GameSettings(id) => self.game_setting_rows(&id),
            Screen::GameCat(id, c) => {
                if self.entry().is_some_and(|e| e.id == id) {
                    self.game_cat_rows(c)
                } else {
                    Vec::new()
                }
            }
            Screen::SettingsCat(c) => self.app_cat_rows(c),
        }
    }

    /// "18%", "%18" in Turkish.
    pub(super) fn percent(&self, share: f32) -> String {
        self.t().percent((share * 100.) as u32)
    }

    fn home_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut recent = self.console_recent();
        // What runs comes first.
        recent.sort_by_key(|g| !self.running.contains_key(&g.game_id().to_string()));
        let shelf = |games: &[&LibraryGame]| games.iter().map(|g| self.game_item(g)).collect();
        if !recent.is_empty() {
            rows.push(Row::titled(
                self.tr("Continue playing", "Oynamaya devam et"),
                Kind::Shelf,
                shelf(&recent[..recent.len().min(12)]),
            ));
        }
        let all = self.console_all();
        if !all.is_empty() {
            rows.push(Row::titled(
                self.tr("Library", "Kütüphane"),
                Kind::Shelf,
                shelf(&all[..all.len().min(40)]),
            ));
        }
        for shop in Shop::ALL {
            let items: Vec<Item> = (0..self.shops.owned(shop).len())
                .filter(|ix| {
                    let g = &self.shops.owned(shop)[*ix];
                    !self.shops.is_installed(shop, &g.id)
                })
                .take(30)
                .filter_map(|ix| self.shop_item(shop, ix))
                .collect();
            if !items.is_empty() {
                rows.push(Row::titled(
                    format!(
                        "{} {}",
                        self.tr("Install from", "Kurulabilir:"),
                        shop.name()
                    ),
                    Kind::Shelf,
                    items,
                ));
            }
        }
        if rows.is_empty() {
            rows.push(Row::new(
                Kind::Pills,
                vec![
                    Item::main(
                        Act::Screen(Screen::Stores),
                        "store",
                        self.tr("Stores", "Mağazalar"),
                    ),
                    Item::pill(
                        Act::Desktop,
                        "monitor",
                        self.tr("Add games on the desktop", "Masaüstünde oyun ekle"),
                        false,
                    ),
                ],
            ));
        }
        rows
    }

    fn search_item(&self) -> Item {
        let q = self.console.query.trim();
        if q.is_empty() {
            Item::pill(Act::Search, "search", self.tr("Search", "Ara"), false)
        } else {
            Item::pill(Act::ClearSearch, "x", format!("“{q}”"), true)
        }
    }

    fn grid(&self, items: Vec<Item>) -> Vec<Row> {
        let cols = self.console.cols.max(1);
        items
            .chunks(cols)
            .map(|c| Row::new(Kind::Grid, c.to_vec()))
            .collect()
    }

    fn library_rows(&self) -> Vec<Row> {
        let f = &self.console.filter;
        let mut pills = vec![
            self.search_item(),
            Item::pill(
                Act::Filter(Filter::All),
                "layout-grid",
                self.tr("All", "Tümü"),
                *f == Filter::All,
            ),
            Item::pill(
                Act::Filter(Filter::Recent),
                "clock",
                self.tr("Recent", "Son oynanan"),
                *f == Filter::Recent,
            ),
        ];
        for c in self.categories() {
            let filter = Filter::Category(c.clone());
            let on = *f == filter;
            pills.push(Item::pill(Act::Filter(filter), "folder", c, on));
        }
        let games = match f {
            Filter::Recent => self.console_recent(),
            _ => self.console_all(),
        };
        let needle = self.console.query.trim().to_lowercase();
        let items = games
            .into_iter()
            .filter(|g| match f {
                Filter::Category(c) => g.category.as_ref() == Some(c),
                _ => true,
            })
            .filter(|g| needle.is_empty() || g.name.to_lowercase().contains(&needle))
            .map(|g| self.game_item(g))
            .collect();
        let mut rows = vec![Row::new(Kind::Pills, pills)];
        rows.extend(self.grid(items));
        rows
    }

    fn store_rows(&self) -> Vec<Row> {
        let tab = self.shops.tab;
        let mut pills = vec![
            self.search_item(),
            Item::pill(
                Act::ShopTab(None),
                "layout-grid",
                self.tr("All", "Tümü"),
                tab.is_none(),
            ),
        ];
        for shop in Shop::ALL {
            pills.push(Item::pill(
                Act::ShopTab(Some(shop)),
                shops_page::shop_icon(shop),
                shop.name(),
                tab == Some(shop),
            ));
        }
        pills.push(Item::pill(
            Act::OnlyInstalled,
            "hard-drive",
            self.tr("Installed", "Kurulu"),
            self.shops.only_installed,
        ));
        let mut rows = vec![Row::new(Kind::Pills, pills)];
        let logins: Vec<Item> = Shop::ALL
            .into_iter()
            .filter(|s| tab.is_none_or(|t| t == *s))
            .filter(|s| {
                !self.shops.logged_in(*s)
                    || self
                        .shops
                        .errors
                        .get(s)
                        .is_some_and(|e| super::shops::login_ended(e))
            })
            .map(|s| {
                Item::pill(
                    Act::Login(s),
                    "log-in",
                    format!("{} · {}", s.name(), self.tr("log in", "giriş yap")),
                    false,
                )
            })
            .collect();
        if !logins.is_empty() {
            rows.push(Row::new(Kind::Pills, logins));
        }
        let items = self
            .shops
            .shown(tab)
            .into_iter()
            .filter_map(|(s, ix)| self.shop_item(s, ix))
            .collect();
        rows.extend(self.grid(items));
        rows
    }

    fn download_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let art =
            |key: &str| shops::parse_key(key).map(|(shop, id, _)| Art::Shop(shop, id.to_string()));
        let active = progress::list();
        for e in &active {
            let mut item = Item::line(
                Act::Download(e.key.clone()),
                "download",
                e.label.clone(),
                Value::Text(match e.fraction() {
                    Some(f) => self.percent(f),
                    None => String::new(),
                }),
            );
            item.sub = match e.total {
                Some(t) => format!(
                    "{} / {} · {}/s",
                    progress::human_bytes(e.done),
                    progress::human_bytes(t),
                    progress::human_bytes(e.speed as u64)
                ),
                None => self.tr("Preparing", "Hazırlanıyor").into(),
            };
            item.progress = Some(e.fraction().unwrap_or(0.));
            if let Look::Line { art: a, .. } = &mut item.look {
                *a = art(&e.key);
            }
            rows.push(Row::new(Kind::Line, vec![item]));
        }
        for i in downloads::items() {
            if active.iter().any(|e| e.key == i.key) {
                continue;
            }
            let (icon, text) = match &i.state {
                State::Queued => ("clock", self.tr("Waiting", "Bekliyor").to_string()),
                State::Paused => ("play", self.tr("Paused", "Duraklatıldı").to_string()),
                State::Failed(_) => ("rotate-ccw", self.tr("Failed", "Başarısız").to_string()),
            };
            let mut item = Item::line(
                Act::Download(i.key.clone()),
                icon,
                i.label.clone(),
                Value::Text(text),
            );
            if let State::Failed(e) = &i.state {
                item.sub = self.shop_error(e);
            }
            if let Look::Line { art: a, .. } = &mut item.look {
                *a = art(&i.key);
            }
            rows.push(Row::new(Kind::Line, vec![item]));
        }
        if let Some(first) = rows.first_mut() {
            first.title = Some(self.tr("Downloads", "İndirilenler").into());
        }
        let now = arkor_core::stats::now();
        for (n, f) in downloads::history().into_iter().take(8).enumerate() {
            let mut item = Item::line(
                Act::Nothing,
                if f.ok { "circle-check" } else { "circle-x" },
                f.label.clone(),
                Value::Text(super::game_page::ago(
                    now.saturating_sub(f.at),
                    self.global.language,
                )),
            );
            if !f.ok {
                item.sub = f.report.clone();
            }
            if let Look::Line { art: a, .. } = &mut item.look {
                *a = art(&f.key);
            }
            let mut row = Row::new(Kind::Line, vec![item]);
            if n == 0 {
                row.title = Some(self.tr("Completed", "Tamamlananlar").into());
            }
            rows.push(row);
        }
        rows.extend(self.console_tool_rows());
        rows
    }

    fn game_rows(&self, id: &str) -> Vec<Row> {
        let Some(g) = self.library.get(id).cloned() else {
            return Vec::new();
        };
        let gid = g.game_id().to_string();
        let mut pills = vec![if self.running.contains_key(&gid) {
            Item::main(Act::Stop(gid.clone()), "square", self.tr("Stop", "Durdur"))
        } else {
            Item::main(Act::Play(gid.clone()), "play", self.tr("Play", "Oyna"))
        }];
        let shop = self.entry_shop(&g);
        if let Some((shop, id)) = &shop {
            match self.shop_job(*shop, id) {
                Some((key, f)) => pills.push(Item::pill(
                    Act::Download(key),
                    "download",
                    match f {
                        Some(f) => self.percent(f),
                        None => self.tr("In queue", "Sırada").into(),
                    },
                    true,
                )),
                None if self.shop_update(*shop, id) => pills.push(Item::pill(
                    Act::Update(*shop, id.clone()),
                    "circle-arrow-up",
                    self.tr("Update", "Güncelle"),
                    true,
                )),
                None => pills.push(Item::pill(
                    Act::Repair(*shop, id.clone()),
                    "shield-check",
                    self.tr("Check files", "Dosyaları denetle"),
                    false,
                )),
            }
        }
        pills.push(Item::pill(
            Act::Screen(Screen::GameSettings(g.id.clone())),
            "sliders-horizontal",
            self.tr("Settings", "Ayarlar"),
            false,
        ));
        if let Some((shop, id)) = &shop {
            pills.push(Item::pill(
                Act::AskUninstall(*shop, id.clone()),
                "trash",
                self.tr("Uninstall", "Kaldır"),
                false,
            ));
        }
        let mut rows = vec![Row::new(Kind::Pills, pills)];
        // The switches need the game's profile, which is the selected game's.
        if self.entry().is_some_and(|e| e.id == g.id) {
            if matches!(
                g.kind,
                GameKind::Windows | GameKind::Epic { .. } | GameKind::Amazon { .. }
            ) {
                let proton = match &self.profile.proton {
                    Some(p) => p.clone(),
                    None => self.tr("Automatic", "Otomatik").to_string(),
                };
                let _ = proton;
                rows.push(Row::new(
                    Kind::Line,
                    vec![Item::line(
                        Act::Opt(Opt::Game(super::console_settings::GameOpt::Proton)),
                        "wine",
                        "Proton",
                        self.opt_value(&Opt::Game(super::console_settings::GameOpt::Proton)),
                    )],
                ));
            }
            for (ix, (key, _, _)) in super::features::QUICK.into_iter().enumerate() {
                let (label, icon, on) = self.quick_label(key);
                rows.push(Row::new(
                    Kind::Line,
                    vec![Item::line(
                        Act::Opt(Opt::Quick(ix)),
                        icon,
                        label,
                        Value::Switch(on),
                    )],
                ));
            }
        }
        rows
    }

    fn shop_game_rows(&self, shop: Shop, id: &str) -> Vec<Row> {
        if let Some((key, fraction)) = self.shop_job(shop, id) {
            let mut item = Item::line(
                Act::Download(key.clone()),
                "download",
                match fraction {
                    Some(_) => self.tr("Downloading", "İndiriliyor"),
                    None => self.tr("In queue", "Sırada"),
                },
                Value::Text(fraction.map_or(String::new(), |f| self.percent(f))),
            );
            item.progress = fraction;
            return vec![
                Row::new(
                    Kind::Pills,
                    vec![
                        Item::main(
                            Act::Download(key.clone()),
                            "pause",
                            self.tr("Pause / go on", "Duraklat / sürdür"),
                        ),
                        Item::pill(
                            Act::CancelDownload(key),
                            "x",
                            self.tr("Cancel", "İptal et"),
                            false,
                        ),
                    ],
                ),
                Row::new(Kind::Line, vec![item]),
            ];
        }
        let sid = id.to_string();
        if self.shops.removing.contains(&(shop, sid.clone())) {
            return vec![Row::new(
                Kind::Line,
                vec![Item::line(
                    Act::Nothing,
                    "trash",
                    self.tr("Uninstalling…", "Kaldırılıyor…"),
                    Value::None,
                )],
            )];
        }
        if self.shops.is_installed(shop, id) {
            let mut pills = vec![Item::main(
                Act::ShopPlay(shop, sid.clone()),
                "play",
                self.tr("Play", "Oyna"),
            )];
            if self.shop_update(shop, id) {
                pills.push(Item::pill(
                    Act::Update(shop, sid.clone()),
                    "circle-arrow-up",
                    self.tr("Update", "Güncelle"),
                    true,
                ));
            }
            pills.extend([
                Item::pill(
                    Act::Repair(shop, sid.clone()),
                    "shield-check",
                    self.tr("Check files", "Dosyaları denetle"),
                    false,
                ),
                Item::pill(
                    Act::AskUninstall(shop, sid),
                    "trash",
                    self.tr("Uninstall", "Kaldır"),
                    false,
                ),
            ]);
            return vec![Row::new(Kind::Pills, pills)];
        }
        let Some(g) = self
            .shops
            .game
            .as_ref()
            .filter(|g| g.shop == shop && g.game.id == id)
        else {
            return Vec::new();
        };
        let mut pills = Vec::new();
        if !self.shops.logged_in(shop) {
            pills.push(Item::main(
                Act::Login(shop),
                "log-in",
                self.tr("Log in", "Giriş yap"),
            ));
        } else {
            let text = match &g.info {
                // No need to wait for the store: the usual choices are used.
                None => self.tr("Install", "Kur").into(),
                Some(Err(e)) => self.shop_error(e),
                Some(Ok(info)) => match (g.missing_space(), info.download) {
                    (Some((need, free)), _) => format!(
                        "{}: {} / {}",
                        self.tr("Not enough space", "Yeterli yer yok"),
                        progress::human_bytes(free),
                        progress::human_bytes(need)
                    ),
                    (None, Some(n)) => format!(
                        "{} · {}",
                        self.tr("Install", "Kur"),
                        progress::human_bytes(n)
                    ),
                    (None, None) => self.tr("Install", "Kur").into(),
                },
            };
            if shops_page::can_install(g) {
                pills.push(Item::main(Act::Install, "download", text));
            } else {
                let icon = if g.info.is_none() {
                    "clock"
                } else {
                    "triangle-alert"
                };
                pills.push(Item::pill(Act::Nothing, icon, text, false));
            }
        }
        let mut rows = vec![Row::new(Kind::Pills, pills)];
        let line = |act, icon, label: &str, value| {
            Row::new(Kind::Line, vec![Item::line(act, icon, label, value)])
        };
        if let Some(c) = &g.compat {
            rows.extend(self.compat_rows(c));
        }
        if g.game.linux && g.game.windows {
            rows.push(line(
                Act::Platform(!g.linux),
                "monitor",
                self.tr("Version", "Sürüm"),
                Value::Text(if g.linux { "Linux" } else { "Windows" }.into()),
            ));
        }
        if let Some(Ok(info)) = &g.info {
            if info.languages.len() > 1 {
                rows.push(line(
                    Act::GameLanguage,
                    "languages",
                    self.tr("Language", "Dil"),
                    Value::Text(shops_page::language_name(&g.language)),
                ));
            }
            for (n, d) in info.dlcs.iter().enumerate() {
                let mut row = line(
                    Act::Dlc(d.id.clone()),
                    "package-plus",
                    &d.title,
                    Value::Switch(g.dlcs.contains(&d.id)),
                );
                if n == 0 {
                    row.title = Some("DLC".into());
                }
                rows.push(row);
            }
        }
        let steam = shop == Shop::Steam;
        if !g.linux && !steam {
            let mut proton = line(
                Act::Opt(Opt::InstallProton),
                "wine",
                "Proton",
                self.opt_value(&Opt::InstallProton),
            );
            proton.title = Some(self.tr("How it runs", "Nasıl çalışacak").into());
            rows.push(proton);
            rows.push(line(
                Act::Folder(FolderFor::Prefix),
                "folder-cog",
                "Wine prefix",
                Value::Text(match &g.prefix {
                    Some(p) => p.display().to_string(),
                    None => self.tr("Its own", "Kendi prefix'i").into(),
                }),
            ));
        }
        let mut menu = line(
            Act::Opt(Opt::InstallMenu),
            "layout-grid",
            self.tr("In the app menu", "Uygulama menüsünde"),
            Value::Switch(g.menu),
        );
        menu.title = Some(
            self.tr("Shortcuts and folder", "Kısayollar ve klasör")
                .into(),
        );
        rows.push(menu);
        rows.push(line(
            Act::Opt(Opt::InstallDesktop),
            "monitor",
            self.tr("On the desktop", "Masaüstünde"),
            Value::Switch(g.desktop),
        ));
        if !steam {
            rows.push(line(
                Act::Folder(FolderFor::Install),
                "folder",
                self.tr("Folder", "Klasör"),
                Value::Text(g.dir.display().to_string()),
            ));
        }
        rows
    }

    /// What is known about a game on Linux, as lines (with a warning when it will not work).
    pub(super) fn compat_rows(&self, c: &arkor_launch::compat::Compat) -> Vec<Row> {
        use arkor_launch::compat::{AntiCheat, Deck};
        let mut parts = Vec::new();
        if let Some(a) = c.anticheat {
            parts.push(match a {
                AntiCheat::Supported | AntiCheat::Running => self
                    .tr("anti-cheat works", "hile koruması çalışıyor")
                    .to_string(),
                AntiCheat::Planned => self
                    .tr("anti-cheat planned", "hile koruması planlanıyor")
                    .into(),
                AntiCheat::Broken => self.tr("anti-cheat broken", "hile koruması bozuk").into(),
                AntiCheat::Denied => self
                    .tr(
                        "anti-cheat refuses Linux",
                        "hile koruması Linux'u reddediyor",
                    )
                    .into(),
            });
        }
        if let Some(t) = &c.protondb {
            parts.push(format!("ProtonDB {t}"));
        }
        if let Some(d) = c.deck {
            parts.push(
                match d {
                    Deck::Verified => self.tr("Deck verified", "Deck doğrulanmış"),
                    Deck::Playable => self.tr("Deck playable", "Deck oynanabilir"),
                    Deck::Unsupported => self.tr("Deck unsupported", "Deck desteklenmiyor"),
                }
                .to_string(),
            );
        }
        if parts.is_empty() {
            return Vec::new();
        }
        let mut item = Item::line(
            Act::Nothing,
            if c.warns() {
                "triangle-alert"
            } else {
                "shield-check"
            },
            self.tr("On Linux", "Linux'ta"),
            Value::Text(parts.join(" · ")),
        );
        if c.warns() {
            item.sub = self
                .tr(
                    "Online play will not work, and there is a risk of a ban.",
                    "Çevrim içi oyun çalışmaz ve ban riski var.",
                )
                .into();
        }
        vec![Row::new(Kind::Line, vec![item])]
    }

    /// Folders in `dir` to go into (hidden ones left out), sorted.
    fn subfolders(&self, dir: &std::path::Path) -> Vec<String> {
        let mut cache = self.console.folders.borrow_mut();
        if let Some((d, list)) = cache.as_ref()
            && d == dir
        {
            return list.clone();
        }
        let mut list: Vec<String> = std::fs::read_dir(dir)
            .map(|r| {
                r.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| !n.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default();
        list.sort_by_key(|n| n.to_lowercase());
        *cache = Some((dir.to_path_buf(), list.clone()));
        list
    }

    /// The rows of what is over the screen.
    pub(super) fn overlay_rows(&self, overlay: &Overlay) -> Vec<Row> {
        let line = |act, icon, label: &str| {
            Row::new(Kind::Line, vec![Item::line(act, icon, label, Value::None)])
        };
        match overlay {
            Overlay::Menu => {
                let mut rows = vec![line(Act::Close, "arrow-left", self.tr("Go on", "Devam et"))];
                let mut running: Vec<&String> = self.running.keys().collect();
                running.sort();
                for id in running {
                    let name = self
                        .games
                        .iter()
                        .find(|g| g.id.to_string() == *id)
                        .map_or(id.as_str(), |g| g.name.as_str());
                    rows.push(line(
                        Act::Stop(id.clone()),
                        "square",
                        &format!("{}: {name}", self.tr("Stop", "Durdur")),
                    ));
                }
                rows.push(line(
                    Act::Desktop,
                    "monitor",
                    self.tr("Leave console mode", "Konsol modundan çık"),
                ));
                rows.push(line(
                    Act::AskPower(Power::Suspend),
                    "moon",
                    self.tr("Suspend", "Uyku"),
                ));
                rows.push(line(
                    Act::AskPower(Power::Restart),
                    "rotate-cw",
                    self.tr("Restart the computer", "Bilgisayarı yeniden başlat"),
                ));
                rows.push(line(
                    Act::AskPower(Power::Off),
                    "power",
                    self.tr("Shut down", "Bilgisayarı kapat"),
                ));
                rows.push(line(
                    Act::AskQuit,
                    "log-out",
                    self.tr("Quit Arkor", "Arkor'dan çık"),
                ));
                rows
            }
            Overlay::Folder(_, dir) => {
                let mut rows = vec![Row::new(
                    Kind::Pills,
                    vec![
                        Item::main(
                            Act::FolderPick,
                            "check",
                            self.tr("Choose this folder", "Bu klasörü seç"),
                        ),
                        Item::pill(
                            Act::FolderUp,
                            "arrow-up",
                            self.tr("Up", "Üst klasör"),
                            false,
                        ),
                    ],
                )];
                rows.extend(self.drive_rows());
                for name in self.subfolders(dir) {
                    rows.push(line(Act::FolderOpen(dir.join(&name)), "folder", &name));
                }
                rows
            }
            Overlay::Failed(id, _, _) => vec![Row::new(
                Kind::Pills,
                vec![
                    Item::main(Act::Close, "check", self.tr("OK", "Tamam")),
                    Item::pill(
                        Act::Screen(Screen::GameSettings(id.clone())),
                        "sliders-horizontal",
                        self.tr("Its settings", "Ayarlarına git"),
                        false,
                    ),
                ],
            )],
            Overlay::Ask(_, yes, act) => vec![Row::new(
                Kind::Pills,
                vec![
                    Item::pill(Act::Close, "x", self.tr("Cancel", "Vazgeç"), false),
                    Item::main(act.clone(), "check", yes.clone()),
                ],
            )],
            Overlay::Pick(_, options) => options
                .iter()
                .map(|(label, act, on)| {
                    line(
                        act.clone(),
                        if *on { "circle-check" } else { "circle" },
                        label,
                    )
                })
                .collect(),
            Overlay::Keyboard => {
                let keys = match self.global.language {
                    arkor_core::Language::Tr => KEYS,
                    arkor_core::Language::En => KEYS_EN,
                };
                let keys = if self.console.symbols { SYMBOLS } else { keys };
                let upper = self.console.shift && !self.console.symbols;
                let turkish = self.global.language == arkor_core::Language::Tr;
                let mut rows: Vec<Row> = keys
                    .iter()
                    .map(|line| {
                        Row::new(
                            Kind::Keys,
                            line.chars()
                                .map(|c| {
                                    // Turkish capitals: i → İ, ı → I.
                                    let c = match (upper, turkish, c) {
                                        (true, true, 'i') => 'İ',
                                        (true, true, 'ı') => 'I',
                                        (true, _, c) => c.to_uppercase().next().unwrap_or(c),
                                        (false, _, c) => c,
                                    };
                                    Item::key(Act::Key(c), c.to_string(), 1.)
                                })
                                .collect(),
                        )
                    })
                    .collect();
                rows.push(Row::new(
                    Kind::Keys,
                    vec![
                        Item::key(Act::Shift, if upper { "⇪" } else { "⇧" }, 1.5),
                        Item::key(
                            Act::Symbols,
                            if self.console.symbols { "abc" } else { "?123" },
                            1.5,
                        ),
                        Item::key(Act::Key(' '), self.tr("Space", "Boşluk"), 3.),
                        Item::key(Act::Backspace, "⌫", 1.5),
                        Item::key(Act::Done, self.tr("Done", "Tamam"), 2.5),
                    ],
                ));
                rows
            }
        }
    }

    /// The screen's focus; on the first visit it goes to the games, not to the buttons.
    pub(super) fn console_focus(&mut self, rows: &[Row]) -> &mut Focus {
        let screen = self.console.screen();
        let start = rows.iter().position(|r| r.kind == Kind::Grid).unwrap_or(0);
        self.console.focus.entry(screen).or_insert_with(|| Focus {
            row: start,
            ..Focus::default()
        })
    }

    /// What has the focus on the screen (not on what is over it).
    pub(super) fn focused_item(&self, rows: &[Row]) -> Option<Item> {
        let f = self.console.focus();
        let row = rows.get(f.row.min(rows.len().saturating_sub(1)))?;
        row.items
            .get(f.col().min(row.items.len().saturating_sub(1)))
            .cloned()
    }

    // ---- Moving about ----

    /// Opened with console mode, on a screen (`console#library`, `console#game/<id>`).
    pub(super) fn console_start(&mut self, screen: Option<&str>) {
        self.console.on = true;
        let stack = match screen {
            Some("library") => vec![Screen::Library],
            Some("stores") => vec![Screen::Stores],
            Some("downloads") => vec![Screen::Downloads],
            Some("settings") => vec![Screen::Settings],
            Some("proton") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::Proton),
            ],
            Some("system") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::System),
            ],
            Some("general") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::General),
            ],
            Some("folder") => {
                let home = dirs::home_dir().unwrap_or_default();
                self.console_overlay(Overlay::Folder(FolderFor::Games, home));
                vec![Screen::Settings]
            }
            Some("menu") => {
                self.console_overlay(Overlay::Menu);
                vec![Screen::Home]
            }
            Some("search") => {
                self.console_overlay(Overlay::Keyboard);
                vec![Screen::Library]
            }
            Some(s) => match s.split_once('/') {
                Some(("game", id)) => vec![Screen::Library, Screen::Game(id.into())],
                Some(("settings", id)) => vec![
                    Screen::Library,
                    Screen::Game(id.into()),
                    Screen::GameSettings(id.into()),
                ],
                Some((shop, id)) if Shop::from_key(shop).is_some() => vec![
                    Screen::Stores,
                    Screen::ShopGame(Shop::from_key(shop).expect("checked"), id.into()),
                ],
                _ => vec![Screen::Home],
            },
            None => vec![Screen::Home],
        };
        self.console.stack = stack;
    }

    /// What a screen that was opened at the start still needs.
    pub(super) fn console_first(&mut self, cx: &mut Context<Self>) {
        match self.console.screen() {
            s @ Screen::SettingsCat(_) => self.console_open(s, cx),
            Screen::Game(id) | Screen::GameSettings(id) => {
                if let Some(ix) = self.games.iter().position(|g| g.id.id == id) {
                    self.select(Selection::Game(ix), cx);
                }
            }
            Screen::ShopGame(shop, id) if self.shops.game.is_none() => {
                self.shop_open_game(shop, &id, cx)
            }
            _ => {}
        }
        self.shops_opened(cx);
        self.guard_steam(cx);
    }

    pub(super) fn toggle_console(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let on = !self.console.on;
        self.set_console(on, cx);
        // Fullscreen follows the mode.
        if window.is_fullscreen() != on {
            window.toggle_fullscreen();
        }
    }

    /// While console mode is on (KDE): Steam's own windows are put away whenever no game
    /// runs, so nothing of Steam stays over the console.
    fn guard_steam(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || self.console.guarding || !super::kwin::available() {
            return;
        }
        self.console.guarding = true;
        cx.on_app_quit(|_, cx| {
            cx.background_executor()
                .spawn(async { super::kwin::unguard() })
        })
        .detach();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let mut on = false;
            loop {
                let go = this.update(cx, |this, _| {
                    let playing = !this.running.is_empty() || this.console.launching.is_some();
                    let login = this.shops.login.is_some();
                    // Steam is being logged in to: its window is wanted.
                    let wanted = this
                        .console
                        .steam_wanted
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(300))
                        && !this.shops.logged_in(Shop::Steam);
                    (this.console.on, !playing && !login && !wanted)
                });
                let (console, guard) = go.unwrap_or((false, false));
                let guard = console && guard;
                if guard != on {
                    on = guard;
                    bg.spawn(async move {
                        if on {
                            super::kwin::guard()
                        } else {
                            super::kwin::unguard()
                        }
                    })
                    .await;
                }
                let over = |this: &mut Self, _: &mut Context<Self>| {
                    this.console.guarding = this.console.on;
                    !this.console.on
                };
                // Turned on again while the guard was taken down: go on.
                if !console && this.update(cx, over).unwrap_or(true) {
                    return;
                }
                bg.timer(Duration::from_millis(700)).await;
            }
        })
        .detach();
    }

    pub(super) fn set_console(&mut self, on: bool, cx: &mut Context<Self>) {
        self.console.on = on;
        // Leave any focused text field so the keys reach the console.
        self.refocus = true;
        self.console.overlay = None;
        self.console.launching = None;
        self.console.entered = Some(Instant::now());
        self.shops.game = None;
        if on {
            self.guard_steam(cx);
            if self.console.stack.is_empty() {
                self.console.stack = vec![Screen::Home];
            }
            self.shops_opened(cx);
        }
        cx.notify();
    }

    /// Opens a screen: a section, or something on top of the one that is open.
    pub(super) fn console_open(&mut self, screen: Screen, cx: &mut Context<Self>) {
        let same = self.console.screen() == screen;
        if Screen::TABS.contains(&screen) {
            self.console.stack = vec![screen.clone()];
        } else if !same {
            self.console.stack.push(screen.clone());
        }
        if !same {
            self.console.entered = Some(Instant::now());
            self.console.left = None;
        }
        match &screen {
            Screen::Stores => self.shops_opened(cx),
            Screen::SettingsCat(super::console_settings::AppCat::Proton) => {
                let v = self.protons.variant;
                if !self.protons.releases.contains_key(v.label()) && !self.protons.loading {
                    self.load_releases(v, cx);
                }
            }
            Screen::SettingsCat(super::console_settings::AppCat::System) => {
                if self.system.info.is_none() {
                    self.scan_system(cx);
                }
            }
            Screen::Game(id) | Screen::GameSettings(id) | Screen::GameCat(id, _) => {
                if let Some(ix) = self.games.iter().position(|g| g.id.id == *id) {
                    self.select(Selection::Game(ix), cx);
                }
                self.console.focus.remove(&screen);
            }
            Screen::ShopGame(shop, id) => {
                self.console.focus.remove(&screen);
                self.shop_open_game(*shop, id, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn console_back(&mut self, cx: &mut Context<Self>) {
        if self.shops.login.is_some() {
            self.close_shop_windows(cx);
            return;
        }
        if let Some(l) = self.console.launching.as_mut()
            && !l.dismissed
        {
            l.dismissed = true;
            cx.notify();
            return;
        }
        if let Some(Overlay::Folder(what, dir)) = self.console.overlay.clone()
            && let Some(up) = dir.parent()
        {
            self.console_overlay(Overlay::Folder(what, up.to_path_buf()));
            cx.notify();
            return;
        }
        if self.console.overlay.take().is_some() {
            self.console.edit = None;
            self.console.secret = None;
            cx.notify();
            return;
        }
        if self.console.stack.len() > 1 {
            if matches!(self.console.screen(), Screen::ShopGame(..)) {
                self.shops.game = None;
            }
            self.console.stack.pop();
            self.console.entered = Some(Instant::now());
            // Back on a game's screen: its profile is the selected one again.
            if let Screen::Game(id) = self.console.screen()
                && let Some(ix) = self.games.iter().position(|g| g.id.id == id)
            {
                self.select(Selection::Game(ix), cx);
            }
        } else if self.console.screen() != Screen::Home {
            self.console_open(Screen::Home, cx);
        } else {
            self.console_overlay(Overlay::Menu);
        }
        cx.notify();
    }

    pub(super) fn console_overlay(&mut self, overlay: Overlay) {
        let mut focus = Focus::default();
        // A choice starts at what is chosen; the keyboard in its middle.
        match &overlay {
            Overlay::Pick(_, options) => {
                focus.row = options.iter().position(|(_, _, on)| *on).unwrap_or(0);
            }
            Overlay::Keyboard => focus.set(1, 4),
            _ => {}
        }
        self.console.over_focus = focus;
        self.console.typed = false;
        self.console.overlay = Some(overlay);
        self.console.moved = Some(Instant::now());
    }

    fn step_tab(&mut self, delta: i32, cx: &mut Context<Self>) {
        let tabs = Screen::TABS;
        let at = tabs
            .iter()
            .position(|t| *t == self.console.tab())
            .unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(tabs.len() as i32) as usize;
        self.shops.game = None;
        self.console_open(tabs[next].clone(), cx);
    }

    /// Input from a gamepad. It only counts while Arkor's window is in use: a game that
    /// runs must not be steering Arkor, too.
    pub(super) fn pad(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        // The login window is the console's for now: its buttons go there.
        if self.console.on
            && let Some(l) = &self.shops.login
            && let super::shops::LoginMode::Window(_) = l.mode
            && let Some(pad) = &l.pad
        {
            let word = match nav {
                Nav::Up => "up",
                Nav::Down => "down",
                Nav::Left => "left",
                Nav::Right => "right",
                Nav::Accept => "accept",
                Nav::Back => "back",
                Nav::Extra => "extra",
                Nav::Search => "search",
                Nav::Menu | Nav::Toggle => "menu",
                _ => return,
            };
            if let Ok(mut stdin) = pad.lock() {
                use std::io::Write;
                let _ = writeln!(stdin, "{word}");
            }
            return;
        }
        if !window.is_window_active() {
            if nav == Nav::Toggle && self.running.is_empty() {
                window.activate_window();
            }
            return;
        }
        if nav == Nav::Toggle {
            if self.console.on {
                self.nav(Nav::Menu, window, cx);
            } else {
                self.toggle_console(window, cx);
            }
            return;
        }
        if self.console.on && !self.console.pad {
            self.console.pad = true;
        }
        self.nav(nav, window, cx);
    }

    /// One step of navigation, from a gamepad or the keyboard.
    pub(super) fn nav(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        if !self.console.on {
            return;
        }
        // The start-up animation ends with the first input.
        self.skip_intro(cx);
        if self.shops.login.is_some() {
            if nav == Nav::Back {
                self.console_back(cx);
            }
            return;
        }
        // Starting or playing: only the buttons of that screen.
        if let Some(l) = self.console.launching.clone()
            && !l.dismissed
        {
            match nav {
                Nav::Back | Nav::Accept => self.console_back(cx),
                Nav::Extra if l.playing => {
                    if let Some(g) = self.library.get(&l.id) {
                        let gid = g.game_id().to_string();
                        self.console_do(Act::Stop(gid), window, cx);
                    }
                }
                _ => {}
            }
            return;
        }
        let over = self.console.overlay.clone();
        let rows = match &over {
            Some(o) => self.overlay_rows(o),
            None => self.console_rows(),
        };
        let (dx, dy) = match nav {
            Nav::Left => (-1, 0),
            Nav::Right => (1, 0),
            Nav::Up => (0, -1),
            Nav::Down => (0, 1),
            Nav::PageUp => (0, -3),
            Nav::PageDown => (0, 3),
            _ => (0, 0),
        };
        // Left and right turn a choice.
        if dy == 0
            && dx != 0
            && over.is_none()
            && let Some(Item {
                act: Act::Opt(o), ..
            }) = self.focused_item(&rows)
            && self.opt_turn(&o, dx)
        {
            cx.notify();
            return;
        }
        if (dx, dy) != (0, 0) {
            self.console.typed = false;
            let focus = match over {
                Some(_) => &mut self.console.over_focus,
                None => self.console_focus(&rows),
            };
            focus.clamp(&rows);
            let before = (focus.row, focus.col());
            if focus.step(&rows, dx, dy) {
                if over.is_none() {
                    self.console.left = Some(before);
                }
                self.console.moved = Some(Instant::now());
            }
            cx.notify();
            return;
        }
        let keyboard = over == Some(Overlay::Keyboard);
        match nav {
            Nav::Accept if keyboard && self.console.typed => self.console_do(Act::Done, window, cx),
            Nav::Accept => {
                let focus = match over {
                    Some(_) => &mut self.console.over_focus,
                    None => self.console_focus(&rows),
                };
                focus.clamp(&rows);
                let (row, col) = (focus.row, focus.col());
                if let Some(item) = rows.get(row).and_then(|r| r.items.get(col)) {
                    self.console_do(item.act.clone(), window, cx);
                }
            }
            Nav::Back => self.console_back(cx),
            Nav::Extra if keyboard => self.console_do(Act::Backspace, window, cx),
            Nav::Extra if matches!(over, Some(Overlay::Folder(..))) => {
                self.console_do(Act::FolderPick, window, cx)
            }
            Nav::Search if keyboard => self.console_do(Act::Key(' '), window, cx),
            Nav::Menu if keyboard => self.console_do(Act::Done, window, cx),
            Nav::Menu => {
                if over == Some(Overlay::Menu) {
                    self.console.overlay = None;
                } else if over.is_none() {
                    self.console_overlay(Overlay::Menu);
                }
            }
            _ if over.is_some() => {}
            Nav::PrevTab => self.step_tab(-1, cx),
            Nav::NextTab => self.step_tab(1, cx),
            Nav::Search => {
                if !matches!(self.console.screen(), Screen::Library | Screen::Stores) {
                    self.console_open(Screen::Library, cx);
                }
                self.console_overlay(Overlay::Keyboard);
            }
            Nav::Extra => {
                if let Some(act) = self.focused_item(&rows).and_then(|i| self.extra(&i)) {
                    self.console_do(act, window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// The second action of an item (X): a game plays at once, a download is cancelled.
    pub(super) fn extra(&self, item: &Item) -> Option<Act> {
        match &item.act {
            Act::Screen(Screen::Game(id)) => {
                let gid = self.library.get(id)?.game_id().to_string();
                Some(if self.running.contains_key(&gid) {
                    Act::Stop(gid)
                } else {
                    Act::Play(gid)
                })
            }
            Act::Screen(Screen::ShopGame(shop, id)) if self.shops.is_installed(*shop, id) => {
                Some(Act::ShopPlay(*shop, id.clone()))
            }
            Act::Download(key) => Some(Act::CancelDownload(key.clone())),
            _ => None,
        }
    }

    /// What the keyboard writes into: a text being edited, or the search.
    fn typing(&mut self) -> &mut String {
        if let Some((_, text)) = &mut self.console.secret {
            return text;
        }
        match &mut self.console.edit {
            Some((_, text)) => text,
            None => &mut self.console.query,
        }
    }

    fn typed_into(&mut self, cx: &mut Context<Self>) {
        if self.console.edit.is_none() && self.console.secret.is_none() {
            self.search_changed(cx);
        } else {
            cx.notify();
        }
    }

    /// A key typed on a real keyboard, for the search. `true` when it was used.
    pub(super) fn console_type(
        &mut self,
        key: &str,
        text: Option<&str>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.console.on || self.console.overlay != Some(Overlay::Keyboard) {
            return false;
        }
        if key == "backspace" {
            self.typing().pop();
        } else if let Some(t) = text.filter(|t| t.chars().all(|c| !c.is_control())) {
            self.typing().push_str(t);
        } else {
            return false;
        }
        self.console.typed = true;
        self.typed_into(cx);
        true
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        self.shops.query = self.console.query.clone();
        // The desktop page's field shows the same search.
        self.fields.remove("shop-search");
        let focus = self.console.focus_mut();
        focus.set(0, 0);
        focus.scroll = 0.;
        cx.notify();
    }

    // ---- Doing ----

    pub(super) fn console_do(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        match act {
            Act::Nothing => {}
            Act::Screen(s) => self.console_open(s, cx),
            Act::Close => self.console.overlay = None,
            Act::Play(id) => {
                if let Some(ix) = self.games.iter().position(|g| g.id.to_string() == id) {
                    self.select(Selection::Game(ix), cx);
                    self.console.launching = Some(Launch {
                        id: self.games[ix].id.id.clone(),
                        at: Instant::now(),
                        playing: false,
                        dismissed: false,
                    });
                    self.play(cx);
                    self.launch_over_soon(cx);
                }
            }
            Act::ShopPlay(shop, id) => {
                self.shop_show_in_library(shop, &id, cx);
                if let Some(entry) = self.shop_entry(shop, &id)
                    && let Some(g) = self.library.get(&entry)
                {
                    let gid = g.game_id().to_string();
                    self.console_do(Act::Play(gid), window, cx);
                }
            }
            Act::Stop(id) => {
                self.console.overlay = None;
                self.console.launching = None;
                if let Some(ix) = self.games.iter().position(|g| g.id.to_string() == id) {
                    self.select(Selection::Game(ix), cx);
                    self.stop(cx);
                }
            }
            Act::Filter(f) => {
                self.console.filter = f;
                self.console.focus_mut().scroll = 0.;
            }
            Act::ShopTab(t) => {
                self.shops.tab = t;
                self.console.focus_mut().scroll = 0.;
            }
            Act::OnlyInstalled => self.shops.only_installed = !self.shops.only_installed,
            Act::Search => self.console_overlay(Overlay::Keyboard),
            Act::ClearSearch => {
                self.console.query.clear();
                self.search_changed(cx);
            }
            Act::Login(shop) => {
                self.console.overlay = None;
                self.shop_login(shop, cx);
            }
            Act::Logout(shop) => self.shop_logout(shop, cx),
            Act::Folder(what) => {
                let start = match what {
                    FolderFor::Install => self.shops.game.as_ref().map(|g| g.dir.clone()),
                    FolderFor::Prefix => self
                        .shops
                        .game
                        .as_ref()
                        .and_then(|g| g.prefix.clone())
                        .or_else(|| Some(self.global.prefixes_dir())),
                    FolderFor::Games => Some(self.global.games_dir()),
                }
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default());
                // The nearest folder that exists.
                let start = start
                    .ancestors()
                    .find(|p| p.is_dir())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/"));
                self.console_overlay(Overlay::Folder(what, start));
            }
            Act::FolderOpen(path) => {
                if let Some(Overlay::Folder(what, _)) = self.console.overlay.clone() {
                    self.console_overlay(Overlay::Folder(what, path));
                }
            }
            Act::FolderUp => {
                if let Some(Overlay::Folder(what, dir)) = self.console.overlay.clone()
                    && let Some(up) = dir.parent()
                {
                    self.console_overlay(Overlay::Folder(what, up.to_path_buf()));
                }
            }
            Act::FolderPick => {
                if let Some(Overlay::Folder(what, dir)) = self.console.overlay.take() {
                    match what {
                        FolderFor::Install => {
                            if let Some(g) = self.shops.game.as_mut() {
                                g.dir = dir;
                                g.error = None;
                            }
                        }
                        FolderFor::Prefix => {
                            if let Some(g) = self.shops.game.as_mut() {
                                g.prefix = Some(dir);
                            }
                        }
                        FolderFor::Games => {
                            self.global.games_dir = Some(dir);
                            let r = self.global.save();
                            self.report(r);
                        }
                    }
                }
            }
            Act::Shift => self.console.shift = !self.console.shift,
            Act::Symbols => self.console.symbols = !self.console.symbols,
            Act::Queue(key) => {
                self.console.overlay = None;
                self.enqueue_download(&key, cx);
            }
            Act::ToolMenu(id) => self.console_tool_menu(&id),
            Act::AskRemoveTool(id) => self.console_ask_remove_tool(&id),
            Act::RemoveTool(id) => {
                self.console.overlay = None;
                self.remove_installed(&id, cx);
            }
            Act::CheckUpdates => self.check_tool_updates(cx),
            Act::UpdateAll => self.downloads_update_all(cx),
            Act::ProtonVariant(i) => {
                if let Some(v) = arkor_launch::proton_dl::Variant::ALL.get(i).copied() {
                    self.protons.variant = v;
                    if !self.protons.releases.contains_key(v.label()) {
                        self.load_releases(v, cx);
                    }
                }
            }
            Act::InstallRelease(tag) => self.console_install_release(&tag, cx),
            Act::InstallTools(packages) => self.console_install_tools(packages),
            Act::Mount(path) => {
                let what = match &self.console.overlay {
                    Some(Overlay::Folder(what, _)) => *what,
                    _ => FolderFor::Games,
                };
                self.console_mount(&path, what, cx);
            }
            Act::InstallAnyway => {
                self.console.overlay = None;
                self.shop_install(cx);
            }
            Act::Opt(o) => self.console_opt(o, cx),
            Act::Pick(o, value) => {
                self.console.overlay = None;
                self.opt_set(&o, &value);
            }
            Act::Platform(linux) => self.shop_set_platform(linux, cx),
            Act::GameLanguage => {
                if let Some(g) = &self.shops.game
                    && let Some(Ok(info)) = &g.info
                {
                    let options = info
                        .languages
                        .iter()
                        .map(|l| {
                            (
                                shops_page::language_name(l),
                                Act::PickLanguage(l.clone()),
                                *l == g.language,
                            )
                        })
                        .collect();
                    let title = self.tr("Language", "Dil").to_string();
                    self.console_overlay(Overlay::Pick(title, options));
                }
            }
            Act::PickLanguage(l) => {
                self.console.overlay = None;
                if let Some(g) = self.shops.game.as_mut() {
                    g.language = l;
                }
            }
            Act::Dlc(id) => {
                if let Some(g) = self.shops.game.as_mut()
                    && !g.dlcs.remove(&id)
                {
                    g.dlcs.insert(id);
                }
            }
            Act::Install
                if self
                    .shops
                    .game
                    .as_ref()
                    .is_some_and(|g| g.compat.as_ref().is_some_and(|c| c.warns())) =>
            {
                let text = self
                    .tr(
                        "Its anti-cheat does not let it run on Linux: online play will not work, and there is a risk of a ban. Install it anyway?",
                        "Hile koruması Linux'ta çalışmasına izin vermiyor: çevrim içi oyun çalışmaz ve ban riski var. Yine de kurulsun mu?",
                    )
                    .to_string();
                let yes = self.tr("Install anyway", "Yine de kur").to_string();
                self.console_overlay(Overlay::Ask(text, yes, Act::InstallAnyway));
            }
            Act::Install => {
                if self
                    .shops
                    .game
                    .as_ref()
                    .is_some_and(shops_page::can_install)
                {
                    self.shop_install(cx);
                }
            }
            Act::Repair(shop, id) => self.shop_repair(shop, &id, cx),
            Act::Update(shop, id) => downloads::enqueue(&shops::key(shop, &id)),
            Act::AskUninstall(shop, id) => {
                let name = self
                    .shops
                    .installed(shop, &id)
                    .map_or(id.clone(), |g| g.title.clone());
                let text = format!(
                    "{name}: {}",
                    self.tr(
                        "delete the game's files? Saves in its prefix stay.",
                        "oyunun dosyaları silinsin mi? Prefix'indeki kayıtlar kalır."
                    )
                );
                let yes = self.tr("Uninstall", "Kaldır").to_string();
                self.console_overlay(Overlay::Ask(text, yes, Act::Uninstall(shop, id)));
            }
            Act::Uninstall(shop, id) => {
                self.console.overlay = None;
                // Its screens go with it.
                if self.console.stack.len() > 1 {
                    self.console.stack.truncate(1);
                }
                self.shop_uninstall(shop, &id, cx);
            }
            Act::Download(key) => {
                let state = downloads::items()
                    .into_iter()
                    .find(|i| i.key == key)
                    .map(|i| i.state);
                match state {
                    Some(State::Queued) => downloads::pause(&key),
                    Some(State::Paused | State::Failed(_)) => downloads::resume(&key),
                    None => {}
                }
            }
            Act::CancelDownload(key) => downloads::cancel(&key),
            Act::Desktop => self.toggle_console(window, cx),
            Act::DesktopPage(id) => {
                self.toggle_console(window, cx);
                if let Ok(id) = id.parse() {
                    self.select_game(&id, cx);
                }
            }
            Act::AskQuit => {
                let text = self.tr("Quit Arkor?", "Arkor'dan çıkılsın mı?").to_string();
                let yes = self.tr("Quit", "Çık").to_string();
                self.console_overlay(Overlay::Ask(text, yes, Act::Quit));
            }
            Act::Quit => cx.quit(),
            Act::AskPower(p) => {
                let (text, yes) = match p {
                    Power::Suspend => (
                        self.tr("Suspend the computer?", "Bilgisayar uyutulsun mu?"),
                        self.tr("Suspend", "Uyut"),
                    ),
                    Power::Restart => (
                        self.tr(
                            "Restart the computer?",
                            "Bilgisayar yeniden başlatılsın mı?",
                        ),
                        self.tr("Restart", "Yeniden başlat"),
                    ),
                    Power::Off => (
                        self.tr("Shut the computer down?", "Bilgisayar kapatılsın mı?"),
                        self.tr("Shut down", "Kapat"),
                    ),
                };
                self.console_overlay(Overlay::Ask(text.into(), yes.into(), Act::Power(p)));
            }
            Act::Power(p) => {
                self.console.overlay = None;
                if !cfg!(test) {
                    let _ = std::process::Command::new("systemctl")
                        .arg(p.verb())
                        .spawn();
                }
            }
            Act::Key(c) => {
                self.typing().push(c);
                self.typed_into(cx);
            }
            Act::Backspace => {
                self.typing().pop();
                self.typed_into(cx);
            }
            Act::Done if self.console.secret.is_some() => {
                self.console.overlay = None;
                if let Some((why, password)) = self.console.secret.take() {
                    self.console_with_password(why, password, cx);
                }
            }
            Act::Done if self.console.edit.is_some() => {
                self.console.overlay = None;
                if let Some((o, text)) = self.console.edit.take() {
                    self.game_opt_write(o, &text);
                }
            }
            Act::Done => {
                self.console.overlay = None;
                // On to the results.
                let rows = self.console_rows();
                if let Some(row) = rows.iter().position(|r| r.kind == Kind::Grid) {
                    self.console.focus_mut().set(row, 0);
                }
            }
        }
        cx.notify();
    }

    fn console_opt(&mut self, opt: Opt, cx: &mut Context<Self>) {
        use super::console_settings::GameOpt;
        match &opt {
            Opt::Quick(ix) => {
                if let Some((_, _, toggle)) = super::features::QUICK.get(*ix) {
                    toggle(self);
                    self.save();
                }
            }
            Opt::Game(o @ (GameOpt::Winecfg | GameOpt::Winetricks)) => {
                let (tool, label) = if *o == GameOpt::Winecfg {
                    (arkor_launch::start::PrefixTool::Winecfg, "winecfg")
                } else {
                    (
                        arkor_launch::start::PrefixTool::Winetricks(Vec::new()),
                        "winetricks",
                    )
                };
                self.run_tool(tool, label.into(), cx);
            }
            Opt::Game(o) if o.is_text() => {
                let text = self.game_opt_text(*o);
                self.console.edit = Some((*o, text));
                self.console_overlay(Overlay::Keyboard);
            }
            _ => match self.opt_choices(&opt) {
                Some(choices) => {
                    let current = self.opt_key(&opt);
                    let options = choices
                        .into_iter()
                        .map(|(label, key)| {
                            let on = key == current;
                            (label, Act::Pick(opt.clone(), key), on)
                        })
                        .collect();
                    let title = self
                        .focused_item(&self.console_rows())
                        .map(|i| i.label)
                        .unwrap_or_default();
                    self.console_overlay(Overlay::Pick(title, options));
                }
                None => self.opt_toggle(&opt),
            },
        }
        cx.notify();
    }

    /// The black screen stays while the game runs (its window lies on top of it); when the
    /// game ends, or does not start, console mode is back.
    fn launch_over_soon(&mut self, cx: &mut Context<Self>) {
        let timer = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                timer.timer(Duration::from_millis(400)).await;
                let go_on = this.update(cx, |this, cx| {
                    let Some(l) = &this.console.launching else {
                        return false;
                    };
                    let id = l.id.clone();
                    let running = this
                        .running
                        .keys()
                        .any(|k| k.split_once(':').is_some_and(|(_, i)| i == id));
                    if !running {
                        this.console.launching = None;
                        cx.notify();
                        return false;
                    }
                    true
                });
                if !matches!(go_on, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// A game ended badly: the console says so over everything, with the end of its log.
    pub(super) fn console_failed(
        &mut self,
        id: &arkor_core::GameId,
        name: &str,
        why: &str,
        log: &std::path::Path,
    ) {
        let mut tail = super::game_page::log_tail(log, 8);
        if tail.is_empty() {
            tail.push(why.to_string());
        }
        self.console.launching = None;
        self.console_overlay(Overlay::Failed(id.id.clone(), name.to_string(), tail));
    }

    // ---- What is behind the screen ----

    /// The picture that goes with an item.
    pub(super) fn console_behind(&mut self, art: &Art, cx: &mut Context<Self>) -> Option<Behind> {
        use cover_art::Size;
        let wide = |path| Behind {
            path,
            size: Size::Wide,
        };
        let blur = |path| Behind {
            path,
            size: Size::Blur,
        };
        match art {
            Art::Game(id) => {
                let g = self.library.get(id)?.clone();
                if let GameKind::Steam { appid } = &g.kind
                    && let Some(p) = self.steam_hero(appid)
                {
                    return Some(wide(p));
                }
                if let Some((shop, sid)) = g.origin.as_deref().and_then(shops::from_origin) {
                    let sid = sid.to_string();
                    if let Some(p) = self.shop_wide(shop, &sid, cx) {
                        return Some(wide(p));
                    }
                }
                if let Some(p) = self.game_hero(&g, cx) {
                    return Some(wide(p));
                }
                // No wide picture anywhere: the cover as a poster.
                self.covers.get(id).cloned().flatten().map(|path| Behind {
                    path,
                    size: Size::Cover,
                })
            }
            Art::Shop(shop, id) => match self.shop_wide(*shop, id, cx) {
                Some(p) => Some(wide(p)),
                None => self.shop_cover(*shop, id, cx).map(blur),
            },
        }
    }

    /// A wide picture from SteamGridDB for a game no store gives one for; asked for once.
    fn game_hero(&mut self, g: &LibraryGame, cx: &mut Context<Self>) -> Option<PathBuf> {
        if let Some(known) = self.console.game_heroes.get(&g.id) {
            return known.clone();
        }
        let found = arkor_launch::covers::hero(&g.id);
        self.console.game_heroes.insert(g.id.clone(), found.clone());
        let key = self.global.steamgriddb_key.clone();
        if found.is_none()
            && !cfg!(test)
            && !key.trim().is_empty()
            && !arkor_launch::covers::hero_missing(&g.id)
        {
            let game = g.clone();
            let bg = cx.background_executor().clone();
            cx.spawn(async move |this, cx| {
                let id = game.id.clone();
                let got = bg
                    .spawn(async move { arkor_launch::covers::fetch_hero(&game, &key) })
                    .await;
                if let Ok(Some(path)) = got {
                    let _ = this.update(cx, |this, cx| {
                        this.console.game_heroes.insert(id, Some(path));
                        cx.notify();
                    });
                }
            })
            .detach();
        }
        found
    }

    pub(super) fn steam_hero(&mut self, appid: &str) -> Option<PathBuf> {
        if let Some(known) = self.console.heroes.get(appid) {
            return known.clone();
        }
        let found = arkor_launch::covers::steam_hero(appid);
        self.console.heroes.insert(appid.to_string(), found.clone());
        found
    }
}
