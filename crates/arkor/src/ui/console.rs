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
use arkor_launch::{steam_api, steam_control, steam_downloads};
use gpui::{Context, KeyBinding, Window, actions};

use super::console_sound::Cue;
use super::console_steam::SteamPage;
use super::console_tools::{AfterDisk, NeedsPassword};
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
    /// What Steam has about a game.
    Steam(u32, SteamPage),
    /// A library game's saves.
    Saves(String),
    /// A library game's mods.
    Mods(String),
    /// Why a library game did not start.
    Trouble(String),
    /// The quick menu over a running game.
    Quick(String),
    /// Trying a gamepad's buttons.
    PadTest,
    /// Adding games: found ones, from a prefix, a program, a launcher.
    AddGames,
    /// Choosing a game's cover.
    Covers(String),
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
    Favorites,
    Hidden,
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
    /// Steam's downloads are controlled through its developer port.
    SteamControl,
    /// The pad's middle button brings Steam forward.
    GuideSteam,
    /// The computer's own settings (see `console_system`).
    Wifi,
    Bluetooth,
    Volume,
    Muted,
    SoundOutput,
    DisplayMode(String),
    DisplayScale(String),
    Hdr(String),
    Vrr(String),
    Primary(String),
    PowerProfile,
    /// The quick menu's: the game's performance overlay, and its frame limit.
    HudShown,
    FpsLimit,
    /// Whose button marks are shown.
    PadGlyphs,
    /// The screen darkens, the computer sleeps, after so many minutes without input.
    DimAfter,
    SleepAfter,
    /// Steam's library folder a game is installed in.
    SteamFolder,
    /// Console mode's own sounds, and its start-up animation.
    ConsoleVolume,
    ConsoleIntro,
    StartHere,
    Autostart,
    /// Games started from console mode full screen in gamescope.
    FullScreen,
    /// Saves follow the stores' clouds.
    CloudSaves,
    /// Clips of games: on, how long, with sound.
    Clips,
    ClipLength,
    ClipSound,
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
    /// What the login window's message offers.
    LoginAgain,
    LoginBrowser,
    LoginInstall,
    LoginClose,
    SeeDownloads,
    /// Steam updates the game; it is not started afterwards.
    DoNotStart,
    /// A download Steam has not begun or paused.
    SteamDownload(u32),
    /// What Steam began to install is called off; what it paused goes on.
    CallOff(u32),
    SteamGoOn(u32),
    SteamPause(u32),
    /// Its place in Steam's queue.
    SteamPlace(u32, usize),
    /// A game to another of Steam's folders: where to, done, stopped.
    SteamMoveAsk(u32),
    SteamMove(u32, i32),
    SteamMoveMenu,
    SteamMoveCancel,
    /// What a launch asked is answered, or the launch is called off.
    SteamAnswer(u32, String),
    SteamCancelAction(u32),
    /// The licence a launch, an installation waits for.
    SteamLicence(u32),
    Licence(bool),
    ShowSteamWindow,
    /// A game's saves: follow the cloud, now, the question, its answer (this computer's?),
    /// its answer for later.
    SavesToggle,
    SavesSync,
    SavesAsk,
    SavesKeep(String, bool),
    SavesLater(String),
    /// Copies of them.
    SavesCopy,
    SavesAskRestore(String),
    SavesRestore(String),
    /// Copies of any game's saves (Ludusavi): one now; after playing on or off.
    SavesBackup,
    SavesBackupsToggle,
    /// A game's mods: get Amethyst, read them again, use mods for the game, switch one,
    /// its menu (X), move it, the question before removing it and its answer, the Nexus
    /// Mods account, the update check, a FOMOD option picked (step, group, option) and the
    /// answer to the installer (install or not).
    ModsGet,
    ModsReload,
    ModsLink,
    ModsToggle(String),
    /// Mods in the game folder only while the game runs: on or off.
    ModsWhilePlaying,
    ModsMenu(String),
    ModsMove(String, bool),
    ModsAskRemove(String),
    ModsRemove(String),
    ModsSignIn,
    ModsSignOut,
    ModsUpdates,
    ModsPick(String, String, String),
    ModsAnswer(bool),
    /// Arkor's mod browser at a page; the game's mod loader fetched.
    ModsBrowse(String),
    ModsGetLoader,
    /// Networks and Bluetooth devices: look for them, and one of them; a screen setting is kept.
    WifiScan,
    WifiNet(String),
    WifiConnect(String),
    WifiDisconnect(String),
    WifiForget(String),
    BtScan,
    BtDevice(String),
    BtPair(String),
    BtConnect(String),
    BtDisconnect(String),
    BtForget(String),
    DisplayKeep,
    /// The quick menu: back to the game, to Arkor, a screenshot.
    QuickBack,
    QuickHome,
    QuickShot,
    /// Saves a clip of the game.
    QuickClip,
    /// The library: adding games, and a game's own things.
    AddScan,
    AddFound(usize),
    /// A ROM folder found on the computer: its games go into the library.
    EmuAddSuggested(usize),
    /// Opens an emulator on its own (to set its controls up).
    EmuOpen(String),
    AddCandidate(usize),
    AddFile(PathBuf),
    AddLauncher(&'static str),
    EditGame(String),
    Favorite(String),
    HideGame(String),
    PickCategory(String),
    AskRemoveGame(String),
    RemoveGame(String),
    SetCover(usize),
    AutoCover(String),
    SortLibrary,
    /// Why a game did not start: one of a problem's fixes, the log, the settings as before.
    Fix(usize, usize),
    TroubleLog,
    TroubleUndo,
    SteamDlc(u32, u32, bool),
    WorkshopMenu(u32, String),
    SteamWorkshop(u32, String, bool),
    AskUnsubscribe(u32, String, String),
    Unsubscribe(u32, String),
    ShotMenu(usize),
    ShotShow(PathBuf),
    AskShotDelete(usize),
    ShotDelete(usize),
    /// Steam is controlled through its developer port from now on, or no longer.
    SteamControl(bool),
    /// Before Steam's other downloads.
    SteamFirst(u32),
    /// What can be done with a download of the queue, and its place in it.
    DownloadMenu(String),
    MoveDownload(String, bool),
    FirstDownload(String),
    /// Arkor's own update: look, ask, install.
    CheckArkor,
    /// What is new in it.
    ArkorNotes,
    AskArkor,
    UpdateArkor,
    ShowSteam,
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
    /// A mounted disk: what can be done with it.
    DiskMenu(String),
    Unmount(String),
    /// The games folder is chosen from here on.
    FolderAt(PathBuf),

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
    /// A picture on the computer, and whether it is a wide one.
    File(PathBuf, bool),
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
    /// A long text (up and down move it) and what to answer.
    Read(String, String, Vec<(String, Act)>),
    /// A picture over the whole screen.
    Picture(PathBuf),
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
    /// A game's program, to add it.
    GameFile,
    /// A Wine prefix whose programs are added.
    ImportPrefix,
    /// A new library of Steam's.
    SteamLibrary,
    /// A folder of ROMs whose games are added.
    Roms,
    /// The folder of the user's BIOS files.
    Bios,
}

/// The picture behind the screen.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Behind {
    pub(super) path: PathBuf,
    pub(super) size: cover_art::Size,
}

/// The choice of a new library folder for Steam.
pub(super) const NEW_FOLDER: &str = "new";

/// A key no keyboard has, pressed to make the pointer go (see [`hide_pointer`]).
pub(super) const POINTER_KEY: &str = "f35";

/// Hides the mouse pointer until the mouse moves. The toolkit does that when something is
/// typed and has no other way to ask for it.
fn hide_pointer(window: &mut Window, cx: &mut gpui::App) {
    if cfg!(test) || !cx.is_cursor_visible() || !window.is_window_active() {
        return;
    }
    // Later: whoever calls this is being updated right now.
    window.defer(cx, |window, cx| {
        let key = gpui::KeyDownEvent {
            keystroke: gpui::Keystroke {
                modifiers: Default::default(),
                key: POINTER_KEY.into(),
                key_char: Some(String::new()),
            },
            is_held: false,
            prefer_character_input: false,
        };
        window.dispatch_event(gpui::PlatformInput::KeyDown(key), cx);
    });
}

/// Starts Steam without its window, so it goes on with its downloads (not in tests).
fn let_steam_update() -> anyhow::Result<bool> {
    if cfg!(test) {
        return Ok(true);
    }
    steam_downloads::let_steam_update()
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
    /// Steam starts it: its app id. Steam's own windows may come before the game's.
    pub(super) steam: Option<String>,
    /// Steam has started the game itself.
    pub(super) came: bool,
    /// Steam updates it first.
    pub(super) updating: bool,
}

impl Launch {
    /// Steam was asked, the game is not there yet.
    pub(super) fn waits_for_steam(&self) -> bool {
        self.steam.is_some() && !self.came
    }

    /// So long that Steam may be asking something.
    pub(super) fn steam_may_ask(&self) -> bool {
        self.waits_for_steam() && !self.updating && self.at.elapsed() > Duration::from_secs(20)
    }
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
    /// The note at the bottom and when it came (it goes by itself).
    pub(super) said: Option<(gpui::SharedString, Instant)>,
    /// Where the bars of downloads are drawn (they glide).
    pub(super) bars: HashMap<String, f32>,
    /// What Steam has about a game, as it was loaded.
    pub(super) steam_page: Option<super::console_steam::Loaded>,
    pub(super) steam_folders: Vec<arkor_launch::steam_api::Folder>,
    /// The question of Steam's that was put last.
    pub(super) asked: Option<String>,
    /// The licence a launch waits for.
    pub(super) licence: Option<(u32, Vec<arkor_launch::steam_api::Eula>)>,
    /// How far a long text was moved.
    pub(super) read_at: f32,
    /// Arkor's own update was looked for in this session.
    pub(super) looked: bool,
    /// When Steam was opened to log in.
    pub(super) steam_wanted: Option<Instant>,
    /// Steam's windows were asked for.
    pub(super) steam_shown: Option<Instant>,
    /// A game that starts when Steam has updated it: which, its app id, since when.
    pub(super) after_update: Option<(String, u32, Instant)>,
    /// The Steam window guard runs.
    guarding: bool,
    /// Steam was told what the pad's middle button does, this session.
    pub(super) guide_told: bool,
    /// The computer's network, Bluetooth, sound, screens and power.
    pub(super) sys: super::console_system::SysState,
    /// The game the quick menu is about.
    pub(super) quick: super::console_quick::QuickState,
    /// What console mode knows for adding and arranging games (see `console_library`).
    pub(super) lib: super::console_library::LibState,
    /// The screen at rest, keeping the desktop awake, words about events (`console_idle`).
    pub(super) idle: super::console_idle::IdleState,
    /// The gamepads' state as console mode knows it (see `console_pads`).
    pub(super) pads: super::console_pads::PadsState,
    /// When the mouse was moved last, and whether that is being watched.
    pointer: Option<Instant>,
    pointer_watched: bool,
    /// Arkor's window was opened full screen: it has no other size yet.
    pub(super) opened_full: bool,
    /// Capitals, and the symbols page, on the keyboard.
    pub(super) shift: bool,
    pub(super) symbols: bool,
    /// The administrator password being typed, and what it is for.
    pub(super) secret: Option<(super::console_tools::NeedsPassword, String)>,
    /// The disks, when they were looked for, and whether one is being mounted.
    pub(super) drives: Vec<arkor_launch::admin::Drive>,
    pub(super) drives_at: Option<Instant>,
    /// All the room of the system's disk, and what is free.
    pub(super) system_space: Option<(u64, u64)>,
    pub(super) mounting: bool,
    /// The start-up animation, while it plays.
    pub(super) intro: Option<super::console_intro::ConsoleIntro>,
    /// It covers the console (which comes in when it opens).
    pub(super) intro_covers: bool,
    /// When the edge of a row was last heard.
    edge_at: Option<Instant>,
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
        self.on && (self.unsettled || self.intro.is_some())
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
        let job = match &g.kind {
            arkor_core::library::GameKind::Steam { appid } => self.shop_job(Shop::Steam, appid),
            _ => shop.as_ref().and_then(|(s, i)| self.shop_job(*s, i)),
        };
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
            mark: running.then_some("play").or(g.favorite.then_some("star")),
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
            Screen::Steam(app, page) => self.steam_page_rows(app, page),
            Screen::Saves(id) => self.saves_rows(&id),
            Screen::Mods(id) => self.mods_rows(&id),
            Screen::Quick(id) => self.quick_rows(&id),
            Screen::PadTest => self.pad_test_rows(),
            Screen::AddGames => self.add_rows(),
            Screen::Covers(id) => self.cover_rows(&id),
            Screen::Trouble(id) => self.trouble_rows(&id),
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
        let favorites: Vec<&LibraryGame> = self
            .console_all()
            .into_iter()
            .filter(|g| g.favorite)
            .collect();
        if !favorites.is_empty() {
            rows.push(Row::titled(
                self.tr("Favourites", "Favoriler"),
                Kind::Shelf,
                shelf(&favorites),
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
                        Act::Screen(Screen::AddGames),
                        "plus",
                        self.tr("Add games", "Oyun ekle"),
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

    pub(super) fn grid(&self, items: Vec<Item>) -> Vec<Row> {
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
        pills.extend(self.library_pills());
        for c in self.categories() {
            let filter = Filter::Category(c.clone());
            let on = *f == filter;
            pills.push(Item::pill(Act::Filter(filter), "folder", c, on));
        }
        let games = match f {
            Filter::Recent => self.console_recent(),
            Filter::Hidden => self.library.games.iter().filter(|g| g.hidden).collect(),
            _ => self.console_sorted(),
        };
        let needle = self.console.query.trim().to_lowercase();
        let items = games
            .into_iter()
            .filter(|g| match f {
                Filter::Category(c) => g.category.as_ref() == Some(c),
                Filter::Favorites => g.favorite,
                _ => true,
            })
            .filter(|g| needle.is_empty() || arkor_core::fuzzy::matches(&needle, &g.name))
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

    /// A Steam game's download by its queue key.
    fn steam_item(&self, key: &str) -> Option<steam_downloads::Item> {
        let (shop, id, _) = shops::parse_key(key)?;
        (shop == Shop::Steam).then_some(())?;
        let id: u32 = id.parse().ok()?;
        steam_downloads::items().into_iter().find(|i| i.appid == id)
    }

    /// What Steam does with a download, and what it is.
    pub(super) fn steam_step(&self, i: &steam_downloads::Item) -> &'static str {
        use steam_downloads::Step;
        match i.step {
            Step::Off => self.tr("Steam is closed", "Steam kapalı"),
            Step::Waiting => self.tr("Waiting in Steam", "Steam'de sırada"),
            Step::Paused => self.tr("Paused in Steam", "Steam'de duraklatıldı"),
            Step::Preparing => self.tr("Preparing", "Hazırlanıyor"),
            Step::Downloading => self.tr("Downloading", "İndiriliyor"),
            Step::Installing => self.tr("Installing", "Kuruluyor"),
        }
    }

    /// A line for something Steam downloads, or has waiting.
    fn steam_row(&self, i: &steam_downloads::Item) -> Row {
        use steam_downloads::Step;
        let what = if i.update {
            self.tr("Update", "Güncelleme")
        } else {
            self.tr("Installation", "Kurulum")
        };
        let value = if i.running && i.total > 0 {
            self.percent(i.done as f32 / i.total as f32)
        } else {
            self.steam_step(i).to_string()
        };
        let icon = match i.step {
            Step::Preparing | Step::Downloading | Step::Installing => "download",
            Step::Waiting => "clock",
            Step::Paused | Step::Off => "play",
        };
        let mut item = Item::line(
            Act::SteamDownload(i.appid),
            icon,
            i.name.clone(),
            Value::Text(value),
        );
        let mut sub = vec![format!("Steam · {what}")];
        if i.running && i.step != Step::Downloading {
            sub.push(self.steam_step(i).into());
        }
        if i.total > 0 {
            sub.push(format!(
                "{} / {}",
                progress::human_bytes(i.done),
                progress::human_bytes(i.total)
            ));
        }
        if i.speed > 1. {
            sub.push(format!("{}/s", progress::human_bytes(i.speed as u64)));
        }
        item.sub = sub.join(" · ");
        item.progress = (i.total > 0).then(|| i.done as f32 / i.total as f32);
        if let Look::Line { art, .. } = &mut item.look {
            *art = Some(Art::Shop(Shop::Steam, i.appid.to_string()));
        }
        Row::new(Kind::Line, vec![item])
    }

    fn download_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let art =
            |key: &str| shops::parse_key(key).map(|(shop, id, _)| Art::Shop(shop, id.to_string()));
        let active = progress::list();
        let steam = steam_downloads::items();
        rows.extend(self.steam_move_row());
        // What goes on now: Arkor's own, then Steam's.
        for e in &active {
            if self.steam_item(&e.key).is_some() {
                continue;
            }
            let mut item = Item::line(
                Act::DownloadMenu(e.key.clone()),
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
        rows.extend(
            steam
                .iter()
                .filter(|i| i.running)
                .map(|i| self.steam_row(i)),
        );
        // Then what waits, in its order: Steam's queue, Arkor's queue.
        rows.extend(
            steam
                .iter()
                .filter(|i| !i.running)
                .map(|i| self.steam_row(i)),
        );
        for i in downloads::items() {
            if active.iter().any(|e| e.key == i.key) || self.steam_item(&i.key).is_some() {
                continue;
            }
            let (icon, text) = match &i.state {
                State::Queued => ("clock", self.tr("Waiting", "Bekliyor").to_string()),
                State::Paused => ("play", self.tr("Paused", "Duraklatıldı").to_string()),
                State::Failed(_) => ("rotate-ccw", self.tr("Failed", "Başarısız").to_string()),
            };
            let mut item = Item::line(
                Act::DownloadMenu(i.key.clone()),
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

    /// One of Steam's downloads was chosen.
    fn console_steam_download(&mut self, appid: u32) {
        use steam_downloads::Step;
        let items = steam_downloads::items();
        let Some(i) = items.iter().find(|i| i.appid == appid).cloned() else {
            return;
        };
        let mut options = Vec::new();
        let control = steam_control::on();
        // Paused by itself, it only goes on in Steam (or with Steam under control).
        if matches!(i.step, Step::Paused | Step::Off)
            && (control || steam_control::wanted() || !i.held)
        {
            options.push((self.tr("Go on", "Sürdür"), Act::SteamGoOn(appid)));
        }
        if control {
            if i.running || i.step == Step::Waiting {
                options.push((self.tr("Pause", "Duraklat"), Act::SteamPause(appid)));
            }
            // Its place among those that are not paused by themselves.
            let queue: Vec<u32> = items.iter().filter(|q| !q.held).map(|q| q.appid).collect();
            if let Some(at) = queue.iter().position(|q| *q == appid) {
                if at > 0 {
                    options.push((
                        self.tr("Download it first", "Önce bunu indir"),
                        Act::SteamPlace(appid, 0),
                    ));
                }
                if at > 1 {
                    options.push((
                        self.tr("Move up", "Yukarı taşı"),
                        Act::SteamPlace(appid, at - 1),
                    ));
                }
                if at + 1 < queue.len() {
                    options.push((
                        self.tr("Move down", "Aşağı taşı"),
                        Act::SteamPlace(appid, at + 1),
                    ));
                }
            }
        } else if !i.running && !i.held && items.len() > 1 && items[0].appid != appid {
            // Not the one that comes first anyway.
            options.push((
                self.tr(
                    "Download it first (Steam restarts)",
                    "Önce bunu indir (Steam yeniden başlar)",
                ),
                Act::SteamFirst(appid),
            ));
        }
        if !i.update {
            options.push((
                self.tr("Call the installation off", "Kurulumu iptal et"),
                Act::CallOff(appid),
            ));
        }
        options.push((
            self.tr("Open Steam's downloads", "Steam'in indirmelerini aç"),
            Act::ShowSteam,
        ));
        options.push((self.tr("Close", "Kapat"), Act::Close));
        let title = format!("{} · {}", i.name, self.steam_step(&i));
        let options = options
            .into_iter()
            .map(|(t, a)| (t.to_string(), a, false))
            .collect();
        self.console_overlay(Overlay::Pick(title, options));
    }

    /// One of Arkor's downloads was chosen.
    fn console_download_menu(&mut self, key: &str) {
        let items = downloads::items();
        let item = items.iter().find(|i| i.key == key);
        let running = progress::running(key);
        let label = item
            .map(|i| i.label.clone())
            .or_else(|| {
                progress::list()
                    .into_iter()
                    .find(|e| e.key == key)
                    .map(|e| e.label)
            })
            .unwrap_or_default();
        let mut options = Vec::new();
        if let Some(i) = item {
            options.push(match &i.state {
                State::Queued => (self.tr("Pause", "Duraklat"), Act::Download(key.into())),
                State::Paused => (self.tr("Go on", "Sürdür"), Act::Download(key.into())),
                State::Failed(_) => (
                    self.tr("Try again", "Yeniden dene"),
                    Act::Download(key.into()),
                ),
            });
            // Its place among those that wait.
            let waiting: Vec<&str> = items
                .iter()
                .filter(|i| !progress::running(&i.key) && self.steam_item(&i.key).is_none())
                .map(|i| i.key.as_str())
                .collect();
            if !running && let Some(at) = waiting.iter().position(|k| *k == key) {
                if at > 0 {
                    options.push((
                        self.tr("To the front", "En öne al"),
                        Act::FirstDownload(key.into()),
                    ));
                    options.push((
                        self.tr("Move up", "Yukarı taşı"),
                        Act::MoveDownload(key.into(), true),
                    ));
                }
                if at + 1 < waiting.len() {
                    options.push((
                        self.tr("Move down", "Aşağı taşı"),
                        Act::MoveDownload(key.into(), false),
                    ));
                }
            }
            options.push((
                self.tr("Cancel it", "İptal et"),
                Act::CancelDownload(key.into()),
            ));
        }
        options.push((self.tr("Close", "Kapat"), Act::Close));
        let options = options
            .into_iter()
            .map(|(t, a)| (t.to_string(), a, false))
            .collect();
        self.console_overlay(Overlay::Pick(label, options));
    }

    pub(super) fn game_rows(&self, id: &str) -> Vec<Row> {
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
        pills.push(Item::pill(
            Act::EditGame(g.id.clone()),
            "pencil",
            self.tr("Arrange", "Düzenle"),
            false,
        ));
        pills.extend(
            self.saves_pill()
                .filter(|_| self.entry().is_some_and(|e| e.id == g.id)),
        );
        pills.push(Item::pill(
            Act::Screen(Screen::Mods(g.id.clone())),
            "puzzle",
            self.tr("Mods", "Modlar"),
            false,
        ));
        if let Some((shop, id)) = &shop {
            pills.push(match self.removing_pill(*shop, id) {
                Some(pill) => pill,
                None => Item::pill(
                    Act::AskUninstall(*shop, id.clone()),
                    "trash",
                    self.tr("Uninstall", "Kaldır"),
                    false,
                ),
            });
        }
        let mut rows = vec![Row::new(Kind::Pills, pills)];
        if let GameKind::Steam { appid } = &g.kind {
            rows.extend(appid.parse().ok().and_then(|a| self.steam_pills(a)));
        }
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

    /// The button of a game that is being removed.
    fn removing_pill(&self, shop: Shop, id: &str) -> Option<Item> {
        self.shops
            .removing
            .contains(&(shop, id.to_string()))
            .then(|| {
                let text = if self.shops.is_installed(shop, id) {
                    self.tr("Uninstalling…", "Kaldırılıyor…")
                } else {
                    self.tr("Calling it off…", "İptal ediliyor…")
                };
                Item::pill(Act::Nothing, "loader-circle", text, false)
            })
    }

    fn shop_game_rows(&self, shop: Shop, id: &str) -> Vec<Row> {
        if let Some(pill) = self.removing_pill(shop, id) {
            return vec![Row::new(Kind::Pills, vec![pill])];
        }
        // Steam installs it, or has stopped in the middle of it.
        if shop == Shop::Steam
            && let Some(i) = self.steam_item(&shops::key(shop, id)).filter(|i| !i.update)
        {
            use steam_downloads::Step;
            let mut pills = Vec::new();
            if matches!(i.step, Step::Paused | Step::Off) {
                let (act, text) = if i.held && !steam_control::wanted() {
                    (
                        Act::ShowSteam,
                        self.tr("Paused in Steam: open it", "Steam'de duraklatıldı: aç"),
                    )
                } else {
                    (
                        Act::SteamGoOn(i.appid),
                        self.tr("Go on installing", "Kurulumu sürdür"),
                    )
                };
                pills.push(Item::main(act, "play", text));
            } else if steam_control::on() {
                pills.push(Item::main(
                    Act::SteamPause(i.appid),
                    "pause",
                    self.tr("Pause", "Duraklat"),
                ));
            }
            pills.push(Item::pill(
                Act::CallOff(i.appid),
                "x",
                self.tr("Call the installation off", "Kurulumu iptal et"),
                false,
            ));
            return vec![Row::new(Kind::Pills, pills), self.steam_row(&i)];
        }
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
            let mut rows = vec![Row::new(Kind::Pills, pills)];
            if shop == Shop::Steam {
                rows.extend(id.parse().ok().and_then(|a| self.steam_pills(a)));
            }
            return rows;
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
        if steam && !self.console.steam_folders.is_empty() {
            rows.push(line(
                Act::Opt(Opt::SteamFolder),
                "hard-drive",
                self.tr("Install on", "Kurulacağı yer"),
                self.opt_value(&Opt::SteamFolder),
            ));
        }
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

    /// What can be done while a store's login is under way.
    pub(super) fn login_rows(&self) -> Vec<Row> {
        use super::shops::LoginMode;
        let mut items = Vec::new();
        if let Some(l) = &self.shops.login {
            let waiting = l.busy || (l.error.is_none() && l.mode != LoginMode::Closed);
            if l.no_webkit {
                items.push(Item::main(
                    Act::LoginInstall,
                    "download",
                    self.tr("Install it", "Kur"),
                ));
            }
            if !waiting && l.mode != LoginMode::Browser {
                items.push(if l.no_webkit {
                    Item::pill(
                        Act::LoginAgain,
                        "rotate-ccw",
                        self.tr("Try again", "Yeniden dene"),
                        false,
                    )
                } else {
                    Item::main(
                        Act::LoginAgain,
                        "log-in",
                        self.tr("Open it again", "Yeniden aç"),
                    )
                });
                // Leaves console mode's reach: only offered when nothing else is left.
                if l.no_webkit {
                    items.push(Item::pill(
                        Act::LoginBrowser,
                        "external-link",
                        self.tr("Use my browser", "Tarayıcımı kullan"),
                        false,
                    ));
                }
            }
            items.push(Item::pill(
                Act::LoginClose,
                "x",
                if waiting {
                    self.tr("Cancel", "Vazgeç")
                } else {
                    self.tr("Close", "Kapat")
                },
                false,
            ));
        }
        vec![Row::new(Kind::Pills, items)]
    }

    /// The rows of what is over the screen.
    pub(super) fn overlay_rows(&self, overlay: &Overlay) -> Vec<Row> {
        let line = |act, icon, label: &str| {
            Row::new(Kind::Line, vec![Item::line(act, icon, label, Value::None)])
        };
        match overlay {
            Overlay::Menu => {
                let mut rows = vec![line(Act::Close, "arrow-left", self.tr("Go on", "Devam et"))];
                if self.quick_target().is_some() {
                    rows.push(line(
                        Act::QuickBack,
                        "play",
                        self.tr("Back to the game", "Oyuna dön"),
                    ));
                }
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
            Overlay::Folder(what, dir) => {
                let file = *what == FolderFor::GameFile;
                let mut rows = vec![Row::new(
                    Kind::Pills,
                    vec![
                        Item::main(
                            if file { Act::Nothing } else { Act::FolderPick },
                            if file { "file" } else { "check" },
                            if file {
                                self.tr("Choose the game's program", "Oyunun programını seç")
                            } else {
                                self.tr("Choose this folder", "Bu klasörü seç")
                            },
                        ),
                        Item::pill(
                            Act::FolderUp,
                            "arrow-up",
                            self.tr("Up", "Üst klasör"),
                            false,
                        ),
                        Item::pill(Act::Close, "x", self.tr("Cancel", "Vazgeç"), false),
                    ],
                )];
                rows.extend(self.drive_rows());
                for name in self.subfolders(dir) {
                    rows.push(line(Act::FolderOpen(dir.join(&name)), "folder", &name));
                }
                if file {
                    for name in super::console_library::programs(dir) {
                        rows.push(line(Act::AddFile(dir.join(&name)), "file-play", &name));
                    }
                }
                rows
            }
            Overlay::Failed(id, _, _) => vec![Row::new(
                Kind::Pills,
                vec![
                    Item::main(
                        Act::Screen(Screen::Trouble(id.clone())),
                        "stethoscope",
                        self.tr("Why?", "Neden açılmadı?"),
                    ),
                    Item::pill(Act::Close, "x", self.tr("Close", "Kapat"), false),
                ],
            )],
            Overlay::Ask(_, yes, act) => vec![Row::new(
                Kind::Pills,
                vec![
                    Item::pill(Act::Close, "x", self.tr("Cancel", "Vazgeç"), false),
                    Item::main(act.clone(), "check", yes.clone()),
                ],
            )],
            Overlay::Read(_, _, options) => vec![Row::new(
                Kind::Pills,
                options
                    .iter()
                    .enumerate()
                    .map(|(n, (text, act))| match n {
                        0 => Item::main(act.clone(), "check", text.clone()),
                        _ => Item::pill(act.clone(), "x", text.clone(), false),
                    })
                    .collect(),
            )],
            Overlay::Picture(_) => vec![Row::new(
                Kind::Pills,
                vec![Item::pill(
                    Act::Close,
                    "x",
                    self.tr("Close", "Kapat"),
                    false,
                )],
            )],
            Overlay::Pick(_, options) => {
                // Things to do, or one of several to choose.
                let choice = options.iter().any(|(_, _, on)| *on);
                options
                    .iter()
                    .map(|(label, act, on)| {
                        let icon = match (choice, on) {
                            (false, _) => "chevron-right",
                            (true, true) => "circle-check",
                            (true, false) => "circle",
                        };
                        line(act.clone(), icon, label)
                    })
                    .collect()
            }
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
            Some("tools") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::Tools),
            ],
            Some("add") => vec![Screen::Library, Screen::AddGames],
            Some("gamepads") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::Controllers),
            ],
            Some("padtest") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::Controllers),
                Screen::PadTest,
            ],
            Some(c @ ("network" | "bluetooth" | "sound" | "screen" | "power")) => {
                use super::console_settings::AppCat;
                let cat = match c {
                    "network" => AppCat::Network,
                    "bluetooth" => AppCat::Bluetooth,
                    "sound" => AppCat::Sound,
                    "screen" => AppCat::Display,
                    _ => AppCat::Power,
                };
                vec![Screen::Settings, Screen::SettingsCat(cat)]
            }
            Some("updates") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::Arkor),
            ],
            Some("general") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::General),
            ],
            Some("games") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::Games),
            ],
            Some("emulators") => vec![
                Screen::Settings,
                Screen::SettingsCat(super::console_settings::AppCat::Emulators),
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
                Some(("trouble", id)) => vec![
                    Screen::Library,
                    Screen::Game(id.into()),
                    Screen::Trouble(id.into()),
                ],
                Some(("saves", id)) => vec![
                    Screen::Library,
                    Screen::Game(id.into()),
                    Screen::Saves(id.into()),
                ],
                Some(("mods", id)) => vec![
                    Screen::Library,
                    Screen::Game(id.into()),
                    Screen::Mods(id.into()),
                ],
                Some(("login", shop)) => {
                    self.shops.on_open = Some(("login".into(), shop.into()));
                    vec![Screen::Stores]
                }
                Some(("settings", rest)) => {
                    use super::console_settings::GameCat;
                    // `settings/<id>/<category>` opens one of the game's categories.
                    let (id, cat) = match rest.split_once('/') {
                        Some((id, c)) => (
                            id,
                            match c {
                                "smooth" | "performance" => Some(GameCat::Smooth),
                                "picture" => Some(GameCat::Picture),
                                "overlay" | "mangohud" => Some(GameCat::Overlay),
                                "compatibility" | "proton" => Some(GameCat::Compatibility),
                                "starting" | "launch" => Some(GameCat::Starting),
                                "game" => Some(GameCat::Game),
                                _ => None,
                            },
                        ),
                        None => (rest, None),
                    };
                    let mut v = vec![
                        Screen::Library,
                        Screen::Game(id.into()),
                        Screen::GameSettings(id.into()),
                    ];
                    v.extend(cat.map(|c| Screen::GameCat(id.into(), c)));
                    v
                }
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
            s @ (Screen::SettingsCat(_) | Screen::Settings) => self.console_open(s, cx),
            Screen::Game(id)
            | Screen::GameSettings(id)
            | Screen::GameCat(id, _)
            | Screen::Saves(id)
            | Screen::Mods(id)
            | Screen::Trouble(id) => {
                if let Some(ix) = self.games.iter().position(|g| g.id.id == id) {
                    self.select(Selection::Game(ix), cx);
                }
                if matches!(self.console.screen(), Screen::Mods(_)) {
                    self.mods_opened(cx);
                }
                if matches!(self.console.screen(), Screen::Saves(_)) {
                    self.saves_local_lookup(cx);
                }
                if matches!(self.console.screen(), Screen::Trouble(_))
                    && let Some(id) = self.selected_id()
                {
                    self.trouble_load(&id, cx);
                }
            }
            Screen::ShopGame(shop, id) if self.shops.game.is_none() => {
                self.shop_open_game(shop, &id, cx)
            }
            _ => {}
        }
        self.shops_opened(cx);
        self.guard_steam(cx);
        self.console_look_for_arkor(cx);
    }

    /// A game is gone: so are its screens.
    pub(super) fn console_left_by(&mut self, (shop, id): &super::shops::ShopGame) {
        if !self.console.on {
            return;
        }
        let entry = self.shop_entry(*shop, id);
        let of_it = |s: &Screen| match s {
            Screen::ShopGame(sh, i) => sh == shop && i == id,
            Screen::Game(e)
            | Screen::GameSettings(e)
            | Screen::GameCat(e, _)
            | Screen::Saves(e)
            | Screen::Mods(e)
            | Screen::Quick(e)
            | Screen::Covers(e)
            | Screen::Trouble(e) => entry.as_deref() == Some(e.as_str()),
            _ => false,
        };
        if self.console.stack.iter().any(of_it) {
            self.console.stack.truncate(1);
            self.console.entered = Some(Instant::now());
            self.shops.game = None;
        }
    }

    /// A newer Arkor is looked for once a session (not when that is switched off).
    fn console_look_for_arkor(&mut self, cx: &mut Context<Self>) {
        let never = self.global.update_interval() == arkor_core::profile::UpdateInterval::Never;
        if !cfg!(test) && !never && !std::mem::replace(&mut self.console.looked, true) {
            self.check_app_update(true, cx);
        }
    }

    /// The note at the bottom goes after some seconds.
    pub(super) fn console_note(&mut self, cx: &mut Context<Self>) {
        let now = self.status.clone();
        let Some(text) = now else {
            self.console.said = None;
            return;
        };
        if self.console.said.as_ref().is_some_and(|(t, _)| *t == text) {
            return;
        }
        self.console.said = Some((text.clone(), Instant::now()));
        let timer = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            timer.timer(Duration::from_secs(7)).await;
            let _ = this.update(cx, |this, cx| {
                let old = this.console.said.as_ref().is_some_and(|(t, at)| {
                    *t == text && at.elapsed() > Duration::from_millis(6500)
                });
                if this.console.on && old && this.status.as_ref() == Some(&text) {
                    this.status = None;
                    this.console.said = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn toggle_console(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let on = !self.console.on;
        self.set_console(on, cx);
        // Fullscreen follows the mode.
        if window.is_fullscreen() != on {
            window.toggle_fullscreen();
        }
        if !on {
            self.fit_window(window, cx);
        }
    }

    /// Out of full screen: a window that was never anything else has no size to go back to
    /// and would stay as large as the screen, partly below the panel. It is maximised then,
    /// which leaves the panel its room.
    fn fit_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.console.opened_full) {
            window.zoom_window();
            return;
        }
        let Some(screen) = window.display(cx).map(|d| d.bounds().size) else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            let _ = this.update_in(cx, |this, window, _| {
                let size = window.viewport_size();
                let covers =
                    size.width >= screen.width && size.height >= screen.height - gpui::px(60.);
                if !this.console.on && covers && !window.is_fullscreen() && !window.is_maximized() {
                    window.zoom_window();
                }
            });
        })
        .detach();
    }

    /// The mouse was moved: its pointer is there again.
    pub(super) fn pointer_moved(&mut self) {
        self.console.pointer = Some(Instant::now());
        self.idle_input();
    }

    /// The pointer goes when the mouse rests, and at once when a gamepad is used.
    pub(super) fn watch_pointer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.console.pointer_watched, true) {
            return;
        }
        self.console.pointer = Some(Instant::now());
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let go_on = this.update_in(cx, |this, window, cx| {
                    if !this.console.on {
                        this.console.pointer_watched = false;
                        return false;
                    }
                    let rests = this
                        .console
                        .pointer
                        .is_none_or(|t| t.elapsed() > Duration::from_millis(2500));
                    if rests {
                        hide_pointer(window, cx);
                    }
                    this.idle_tick(window, cx);
                    true
                });
                if !go_on.unwrap_or(false) {
                    return;
                }
            }
        })
        .detach();
    }

    /// Whether a game of Steam's runs or starts.
    fn steam_plays(&self) -> bool {
        let steam = |id: &str| {
            self.library
                .get(id)
                .is_some_and(|g| matches!(g.kind, GameKind::Steam { .. }))
        };
        self.running
            .keys()
            .any(|k| k.split_once(':').is_none_or(|(_, id)| steam(id)))
            || self
                .console
                .launching
                .as_ref()
                .is_some_and(|l| l.steam.is_some() || steam(&l.id))
    }

    /// While console mode is on: Steam is told what the pad's middle button does, and (KDE)
    /// its own windows are put away whenever no game runs, so nothing of Steam stays over
    /// the console.
    fn guard_steam(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || self.console.guarding {
            return;
        }
        self.console.guarding = true;
        cx.on_app_quit(|_, cx| {
            cx.background_executor()
                .spawn(async { super::desk::unguard() })
        })
        .detach();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let mut on = false;
            loop {
                let go = this.update(cx, |this, _| {
                    // A game Steam has not started yet is no reason to let its windows out
                    // (they would push Arkor aside).
                    let waits = this
                        .console
                        .launching
                        .as_ref()
                        .is_some_and(|l| l.waits_for_steam());
                    // Other games have no use for Steam's windows either (the pad's middle
                    // button would put Steam over them).
                    let playing = match waits {
                        true => this.running.len() > 1,
                        false => this.steam_plays(),
                    };
                    let shown = this
                        .console
                        .steam_shown
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(180));
                    let login = this.shops.login.is_some();
                    // Steam is being logged in to: its window is wanted.
                    let wanted = this
                        .console
                        .steam_wanted
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(300))
                        && !this.shops.logged_in(Shop::Steam);
                    (this.console.on, !playing && !login && !wanted && !shown)
                });
                // Steam is told once what the pad's middle button does.
                let tell = this
                    .update(cx, |this, _| {
                        let due = this.console.on
                            && !this.console.guide_told
                            && arkor_launch::steam_control::on();
                        this.console.guide_told |= due;
                        due.then_some(this.global.guide_opens_steam)
                    })
                    .ok()
                    .flatten();
                if let Some(on) = tell {
                    let told = bg
                        .spawn(async move { arkor_launch::steam_api::set_guide_focus(on) })
                        .await;
                    if told.is_err() {
                        let _ = this.update(cx, |this, _| this.console.guide_told = false);
                    }
                }
                let (console, guard) = go.unwrap_or((false, false));
                let guard = console && guard && super::desk::guards_steam();
                if guard != on {
                    on = guard;
                    bg.spawn(async move {
                        if on {
                            super::desk::guard()
                        } else {
                            super::desk::unguard()
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
        if self.console.on && !on {
            self.cue(Cue::Bye);
            self.console.intro = None;
        }
        self.console.on = on;
        // Leave any focused text field so the keys reach the console.
        self.refocus = true;
        self.console.overlay = None;
        self.console.launching = None;
        self.console.entered = Some(Instant::now());
        self.shops.game = None;
        if on {
            // The desktop's start-up animation, if it still plays, gives way to the console's.
            self.intro = None;
            super::console_sound::warm();
            self.start_console_intro();
            self.guard_steam(cx);
            self.console_look_for_arkor(cx);
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
            Screen::SettingsCat(
                super::console_settings::AppCat::System | super::console_settings::AppCat::Tools,
            ) => {
                if self.system.info.is_none() {
                    self.scan_system(cx);
                }
            }
            Screen::Game(id) | Screen::GameSettings(id) | Screen::GameCat(id, _) => {
                if let Some(ix) = self.games.iter().position(|g| g.id.id == *id) {
                    self.select(Selection::Game(ix), cx);
                }
                self.emu_game_opened(id, cx);
                self.console.focus.remove(&screen);
            }
            Screen::ShopGame(shop, id) => {
                self.console.focus.remove(&screen);
                self.shop_open_game(*shop, id, cx);
                if *shop == Shop::Steam {
                    self.steam_look_folders(cx);
                }
            }
            Screen::SettingsCat(c) if super::console_system::sys_cat(*c) => {
                self.sys_opened(*c, cx);
            }
            // Its list says how they stand.
            Screen::Settings => {
                use super::console_settings::AppCat;
                for c in [AppCat::Network, AppCat::Sound, AppCat::Power] {
                    self.sys_load(c, cx);
                }
            }
            Screen::AddGames => self.add_opened(cx),
            Screen::Covers(id) => self.covers_opened(id, cx),
            Screen::Saves(_) => self.saves_local_lookup(cx),
            Screen::Mods(id) => {
                self.console.focus.remove(&screen);
                if let Some(ix) = self.games.iter().position(|g| g.id.id == *id)
                    && self.selected_id().is_none_or(|g| g.id != *id)
                {
                    self.select(Selection::Game(ix), cx);
                }
                self.mods_opened(cx);
            }
            Screen::Trouble(id) => {
                self.console.focus.remove(&screen);
                if let Some(ix) = self.games.iter().position(|g| g.id.id == *id) {
                    if self.selected_id().is_none_or(|g| g.id != *id) {
                        self.select(Selection::Game(ix), cx);
                    }
                    let id = self.games[ix].id.clone();
                    self.trouble_load(&id, cx);
                }
            }
            Screen::Steam(app, page) => {
                self.console.focus.remove(&screen);
                self.steam_page_load(*app, *page, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn console_back(&mut self, cx: &mut Context<Self>) {
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
        // A screen setting that is not kept goes back at once.
        if matches!(
            &self.console.overlay,
            Some(Overlay::Ask(_, _, Act::DisplayKeep))
        ) {
            self.sys_revert(cx);
            return;
        }
        if let Some(Overlay::Folder(what, dir)) = self.console.overlay.clone()
            && let Some(up) = dir.parent()
        {
            self.console_overlay(Overlay::Folder(what, up.to_path_buf()));
            cx.notify();
            return;
        }
        // Leaving a licence is no answer but "no".
        if let Some(Overlay::Read(_, _, options)) = &self.console.overlay
            && let Some((_, Act::SteamCancelAction(id))) = options.last().cloned()
        {
            return self.steam_cancel_action(id, cx);
        }
        if matches!(&self.console.overlay, Some(Overlay::Read(..))) {
            arkor_launch::steam_api::answer_licence(false);
        }
        if let Some(Overlay::Pick(_, options)) = &self.console.overlay
            && let Some((_, Act::SteamCancelAction(id), _)) = options.last().cloned()
        {
            return self.steam_cancel_action(id, cx);
        }
        if self.console.overlay.take().is_some() {
            self.console.edit = None;
            // No password, no disk: back to the folders it was wanted for.
            if let Some((NeedsPassword::Mount(_, then), _)) = self.console.secret.take() {
                self.console_disk_not_mounted(&then);
            }
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
        // The mod browser, like the login window below, is the console's while it is open.
        if self.console.on
            && let Some(pad) = self.mods.browser.as_ref().and_then(|b| b.pad.clone())
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
        // A login that ended may leave the focus nowhere: its message still answers.
        let login = self.console.on && self.shops.login.is_some() && self.running.is_empty();
        if !window.is_window_active() && !login {
            if nav == Nav::Toggle {
                // Over a game: the quick menu. Else Arkor comes forward.
                let quick = self.console.on && self.quick_open(window, cx);
                if !quick && self.running.is_empty() {
                    window.activate_window();
                }
            }
            return;
        }
        if nav == Nav::Toggle {
            if self.console.on && matches!(self.console.screen(), Screen::Quick(_)) {
                self.quick_back(window, cx);
            } else if self.console.on {
                self.nav(Nav::Menu, window, cx);
            } else {
                self.toggle_console(window, cx);
            }
            return;
        }
        if self.console.on {
            self.console.pad = true;
            hide_pointer(window, cx);
        }
        self.nav(nav, window, cx);
    }

    /// One step of navigation, from a gamepad or the keyboard.
    pub(super) fn nav(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        if !self.console.on {
            return;
        }
        // A dark screen only wakes.
        if self.idle_input() {
            return cx.notify();
        }
        // The start-up animation ends with the first input.
        self.skip_intro(cx);
        if self.skip_console_intro(cx) {
            return;
        }
        // Trying the buttons: they do nothing else; B twice leaves.
        if self.console.screen() == Screen::PadTest && self.console.overlay.is_none() {
            if nav == Nav::Back && self.pad_test_leave() {
                self.console_back(cx);
            }
            return;
        }
        // B in the quick menu: back to the game.
        if nav == Nav::Back
            && self.console.overlay.is_none()
            && matches!(self.console.screen(), Screen::Quick(_))
        {
            return self.quick_back(window, cx);
        }
        if self.shops.login.is_some() {
            let rows = self.login_rows();
            let focus = &mut self.console.over_focus;
            focus.clamp(&rows);
            match nav {
                Nav::Left | Nav::Up => {
                    let moved = focus.step(&rows, -1, 0);
                    self.cue_step(moved, 0.);
                }
                Nav::Right | Nav::Down => {
                    let moved = focus.step(&rows, 1, 0);
                    self.cue_step(moved, 0.);
                }
                Nav::Accept => {
                    let act = rows[0].items.get(focus.col()).map(|i| i.act.clone());
                    if let Some(act) = act {
                        self.console_do(act, window, cx);
                    }
                }
                Nav::Back => {
                    self.cue(Cue::Back);
                    self.console_back(cx)
                }
                _ => {}
            }
            cx.notify();
            return;
        }
        // Starting or playing: only the buttons of that screen.
        if let Some(l) = self.console.launching.clone()
            && !l.dismissed
        {
            // Waiting for its saves: the game starts without them.
            if self.saves.starting(&l.id) {
                if nav == Nav::Back {
                    self.saves_skip();
                }
                return;
            }
            match nav {
                Nav::Back | Nav::Accept => self.console_back(cx),
                Nav::Search if l.steam_may_ask() => self.console_show_steam(None, cx),
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
            self.cue(Cue::Turn);
            cx.notify();
            return;
        }
        // A long text moves.
        if dy != 0 && matches!(over, Some(Overlay::Read(..))) {
            self.console.read_at = (self.console.read_at + dy as f32).max(0.);
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
            let moved = focus.step(&rows, dx, dy);
            let (row, col) = (focus.row, focus.col());
            if moved {
                if over.is_none() {
                    self.console.left = Some(before);
                }
                self.console.moved = Some(Instant::now());
            }
            // Heard from where it is.
            let wide = rows.get(row).map_or(1, |r| r.items.len().max(1)) as f32;
            let pan = (col as f32 + 0.5) / wide - 0.5;
            self.cue_step(moved, pan);
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
            Nav::Back => {
                self.cue(Cue::Back);
                self.console_back(cx)
            }
            Nav::Extra if keyboard => self.console_do(Act::Backspace, window, cx),
            Nav::Extra if matches!(over, Some(Overlay::Folder(..))) => {
                self.console_do(Act::FolderPick, window, cx)
            }
            Nav::Search if keyboard => self.console_do(Act::Key(' '), window, cx),
            Nav::Menu if keyboard => self.console_do(Act::Done, window, cx),
            Nav::Menu => {
                if matches!(over, Some(Overlay::Menu | Overlay::Folder(..))) {
                    self.cue(Cue::Back);
                    self.console.overlay = None;
                } else if over.is_none() {
                    self.cue(Cue::Open);
                    self.console_overlay(Overlay::Menu);
                }
            }
            _ if over.is_some() => {}
            Nav::PrevTab => {
                self.cue(Cue::Tab);
                self.step_tab(-1, cx)
            }
            Nav::NextTab => {
                self.cue(Cue::Tab);
                self.step_tab(1, cx)
            }
            Nav::Search => {
                self.cue(Cue::Open);
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
            Act::Download(key) | Act::DownloadMenu(key)
                if downloads::items().iter().any(|i| i.key == *key) =>
            {
                Some(Act::CancelDownload(key.clone()))
            }
            Act::ModsToggle(name) => Some(Act::ModsMenu(name.clone())),
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

    /// Starts a game; a disk it needs is mounted first.
    pub(super) fn console_play(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(ix) = self.games.iter().position(|g| g.id.to_string() == id) else {
            return;
        };
        if let Some(drive) = self.game_disk(&self.games[ix].id) {
            self.status = Some(
                self.tr(
                    "The game is on a disk that has to be mounted first.",
                    "Oyun, önce bağlanması gereken bir diskte.",
                )
                .into(),
            );
            return self.console_mount_drive(drive, AfterDisk::Play(id.to_string()), cx);
        }
        let steam = match self.library.get(&self.games[ix].id.id).map(|g| &g.kind) {
            Some(arkor_core::library::GameKind::Steam { appid }) => Some(appid.clone()),
            _ => None,
        };
        // Steam would update it first, behind a window of its own: said here instead.
        if let Some(appid) = steam.as_ref().and_then(|a| a.parse::<u32>().ok())
            && self
                .console
                .after_update
                .as_ref()
                .is_none_or(|(i, ..)| i != id)
            && !cfg!(test)
            && let Some(item) = steam_downloads::pending(appid)
        {
            return self.console_update_first(id.to_string(), item, cx);
        }
        self.console.after_update.take_if(|(i, ..)| i == id);
        self.console.steam_shown = None;
        self.cue(Cue::Launch);
        self.select(Selection::Game(ix), cx);
        let entry = self.games[ix].id.id.clone();
        // The same screen goes on when its saves were looked at.
        let waited = self
            .console
            .launching
            .as_ref()
            .is_some_and(|l| l.id == entry);
        if !waited {
            self.console_launch_screen(&entry);
        }
        if let Some(l) = self.console.launching.as_mut() {
            l.at = Instant::now();
            l.steam = steam;
        }
        self.play(cx);
        if !waited {
            self.launch_over_soon(cx);
        }
    }

    /// The screen of a game that starts.
    pub(super) fn console_launch_screen(&mut self, entry: &str) {
        self.console.launching = Some(Launch {
            id: entry.to_string(),
            at: Instant::now(),
            playing: false,
            dismissed: false,
            steam: None,
            came: false,
            updating: false,
        });
    }

    /// A game Steam has to update first: Steam does it in the background, the game starts
    /// when it is done.
    pub(super) fn console_update_first(
        &mut self,
        id: String,
        item: steam_downloads::Item,
        cx: &mut Context<Self>,
    ) {
        self.console.after_update = Some((id, item.appid, Instant::now()));
        self.console_say_update(&item.name, item.left(), true);
        // Steam updates by itself when it starts.
        // (A Steam that was closed needs a moment to begin.)
        let patience = match let_steam_update() {
            Ok(true) => 40,
            Ok(false) => 8,
            Err(e) => {
                self.status = Some(format!("{e:#}").into());
                0
            }
        };
        self.watch_progress(cx);
        cx.notify();
        let timer = cx.background_executor().clone();
        let appid = item.appid;
        cx.spawn(async move |this, cx| {
            loop {
                timer.timer(Duration::from_millis(1500)).await;
                let state = timer
                    .spawn(async move { steam_downloads::pending(appid) })
                    .await;
                let go_on = this.update(cx, |this, cx| {
                    let Some((id, waiting, since)) = this.console.after_update.clone() else {
                        return false;
                    };
                    if waiting != appid || !this.console.on {
                        return false;
                    }
                    if state.is_some() {
                        // Steam does not go on by itself (paused, or told to update only when
                        // the game starts): starting the game makes it.
                        let idle = since.elapsed() > Duration::from_secs(patience)
                            && steam_downloads::items()
                                .iter()
                                .any(|i| i.appid == appid && !i.running);
                        if !idle {
                            return true;
                        }
                    }
                    // Over another game nothing starts by itself.
                    if this.running.is_empty() {
                        this.console.overlay = None;
                        this.console_play(&id, cx);
                    } else {
                        this.console.after_update = None;
                        this.status =
                            Some(this.tr("The update is ready.", "Güncelleme hazır.").into());
                    }
                    cx.notify();
                    false
                });
                if !matches!(go_on, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// Says that Steam updates a game. `ours`: Arkor starts it afterwards (else Steam does).
    fn console_say_update(&mut self, name: &str, left: u64, ours: bool) {
        let size = match left {
            0 => String::new(),
            n => format!(" ({})", progress::human_bytes(n)),
        };
        let title = match self.global.language {
            arkor_core::Language::Tr => {
                format!("{name} güncelleniyor{size}. Bitince oyun kendiliğinden başlar.")
            }
            arkor_core::Language::En => {
                format!("{name} is being updated{size}. The game starts when it is done.")
            }
        };
        let mut options = vec![
            (
                self.tr("Go on in Arkor", "Arkor'da devam et").to_string(),
                Act::Close,
                false,
            ),
            (
                self.tr("Show the downloads", "İndirilenler'i göster")
                    .to_string(),
                Act::SeeDownloads,
                false,
            ),
        ];
        if ours {
            options.push((
                self.tr(
                    "Only update, do not start it",
                    "Yalnızca güncelle, başlatma",
                )
                .to_string(),
                Act::DoNotStart,
                false,
            ));
        }
        self.cue(Cue::Open);
        self.console_overlay(Overlay::Pick(title, options));
    }

    /// Brings Steam's windows to the front, for what only they can answer.
    pub(super) fn console_show_steam(
        &mut self,
        page: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        self.console.steam_shown = Some(Instant::now());
        self.console.overlay = None;
        let bg = cx.background_executor().clone();
        bg.spawn(async move {
            if !cfg!(test) {
                super::desk::show_steam();
                if let Some(page) = page {
                    let _ = arkor_launch::steam_link::open_url(page);
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// How loud console mode's sounds are, 0…1.
    pub(super) fn console_volume(&self) -> f32 {
        if !self.global.appearance.sounds {
            return 0.;
        }
        f32::from(self.global.console_volume.unwrap_or(70).min(100)) / 100.
    }

    /// One of console mode's sounds. Silent while a game has the screen.
    pub(super) fn cue(&self, cue: Cue) {
        self.cue_at(cue, 0.);
    }

    fn cue_at(&self, cue: Cue, pan: f32) {
        if self.console.on {
            super::console_sound::play(cue, self.console_volume(), pan);
        }
    }

    /// The focus moved, or there was nothing further that way (said once, not for as long
    /// as the direction is held).
    fn cue_step(&mut self, moved: bool, pan: f32) {
        if moved {
            self.console.edge_at = None;
            self.cue_at(Cue::Move, pan);
            return;
        }
        let now = Instant::now();
        let again = self
            .console
            .edge_at
            .is_none_or(|t| now - t > Duration::from_millis(350));
        self.console.edge_at = Some(now);
        if again {
            self.cue(Cue::Edge);
        }
    }

    /// What choosing `act` sounds like.
    fn act_cue(&self, act: &Act) -> Option<Cue> {
        Some(match act {
            Act::Nothing => Cue::Edge,
            Act::Key(_) | Act::Shift | Act::Symbols => Cue::Key,
            Act::Backspace => Cue::Erase,
            // These come again as `Play`, or say how they went themselves.
            Act::Play(_) | Act::ShopPlay(..) | Act::Desktop | Act::DesktopPage(_) => return None,
            Act::Close | Act::LoginClose | Act::FolderUp | Act::ClearSearch => Cue::Back,
            Act::Search | Act::AskQuit | Act::AskPower(_) | Act::Folder(_) => Cue::Open,
            Act::AskUninstall(..) | Act::AskRemoveTool(_) | Act::ToolMenu(_) => Cue::Open,
            Act::DiskMenu(_) | Act::FolderAt(_) | Act::SteamDownload(_) => Cue::Open,
            Act::DoNotStart | Act::CallOff(_) => Cue::Back,
            Act::DownloadMenu(_) | Act::AskArkor | Act::ArkorNotes => Cue::Open,
            Act::SteamMoveAsk(_) | Act::SteamMoveMenu | Act::WorkshopMenu(..) => Cue::Open,
            Act::ShotMenu(_) | Act::AskShotDelete(_) | Act::AskUnsubscribe(..) => Cue::Open,
            Act::SteamDlc(_, _, on) => {
                if *on {
                    Cue::On
                } else {
                    Cue::Off
                }
            }
            Act::GameLanguage => Cue::Open,
            Act::ShopTab(_) | Act::Filter(_) | Act::ProtonVariant(_) => Cue::Tab,
            Act::Opt(o) => match self.opt_value(o) {
                Value::Switch(true) => Cue::Off,
                Value::Switch(false) => Cue::On,
                _ => Cue::Open,
            },
            _ => Cue::Accept,
        })
    }

    pub(super) fn console_do(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(cue) = self.act_cue(&act) {
            self.cue(cue);
        }
        match act {
            Act::Nothing => {}
            Act::Screen(s) => self.console_open(s, cx),
            Act::Close => self.console.overlay = None,
            Act::Play(id) => self.console_play(&id, cx),
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
                self.console.over_focus = Focus::default();
                self.shop_login(shop, cx);
            }
            Act::LoginAgain => {
                self.console.over_focus = Focus::default();
                match self
                    .shops
                    .login
                    .as_ref()
                    .map(|l| (l.shop, l.login.is_some()))
                {
                    Some((_, true)) => self.shop_login_window(cx),
                    // The store could not be asked: from the beginning.
                    Some((shop, false)) => self.shop_login(shop, cx),
                    None => {}
                }
            }
            Act::SeeDownloads => {
                self.console.overlay = None;
                self.console_open(Screen::Downloads, cx);
            }
            Act::DoNotStart => {
                self.console.overlay = None;
                self.console.after_update = None;
            }
            Act::SteamDownload(appid) => self.console_steam_download(appid),
            Act::CallOff(appid) => {
                self.console.overlay = None;
                let key = (Shop::Steam, appid.to_string());
                if self.shops.removing.insert(key) {
                    self.steam_do(
                        move || {
                            let done = shops::steam_call_off(appid);
                            steam_downloads::look_again();
                            done
                        },
                        move |this| {
                            let key = (Shop::Steam, appid.to_string());
                            this.shops.removing.remove(&key);
                            Some(this.tr("Called off.", "İptal edildi."))
                        },
                        cx,
                    );
                }
            }
            Act::SteamGoOn(appid) => {
                self.console.overlay = None;
                if !steam_control::on() {
                    self.status = Some(
                        self.tr(
                            "Steam starts in the background and goes on.",
                            "Steam arka planda açılıyor ve devam edecek.",
                        )
                        .into(),
                    );
                }
                self.steam_do(move || shops::steam_go_on(appid), |_| None, cx);
            }
            Act::SteamPause(appid) => {
                self.console.overlay = None;
                self.steam_do(move || steam_control::pause(appid), |_| None, cx);
            }
            Act::SteamPlace(appid, place) => {
                self.console.overlay = None;
                self.steam_do(move || steam_control::place(appid, place), |_| None, cx);
            }
            Act::Fix(p, n) => self.trouble_choose(p, n, cx),
            Act::WifiScan
            | Act::WifiNet(_)
            | Act::WifiConnect(_)
            | Act::WifiDisconnect(_)
            | Act::WifiForget(_)
            | Act::BtScan
            | Act::BtDevice(_)
            | Act::BtPair(_)
            | Act::BtConnect(_)
            | Act::BtDisconnect(_)
            | Act::BtForget(_)
            | Act::DisplayKeep => self.sys_act(act, cx),
            Act::QuickBack | Act::QuickHome | Act::QuickShot | Act::QuickClip => {
                self.quick_act(act, window, cx)
            }
            Act::EmuAddSuggested(ix) => self.emu_add_suggested(ix, cx),
            Act::EmuOpen(id) => self.emu_open(&id, cx),
            Act::AddScan
            | Act::AddFound(_)
            | Act::AddCandidate(_)
            | Act::AddFile(_)
            | Act::AddLauncher(_)
            | Act::EditGame(_)
            | Act::Favorite(_)
            | Act::HideGame(_)
            | Act::PickCategory(_)
            | Act::AskRemoveGame(_)
            | Act::RemoveGame(_)
            | Act::SetCover(_)
            | Act::AutoCover(_)
            | Act::SortLibrary => self.library_act(act, cx),
            Act::TroubleLog => self.trouble_log(),
            Act::TroubleUndo => self.trouble_undo(cx),
            Act::SavesToggle
            | Act::SavesSync
            | Act::SavesAsk
            | Act::SavesKeep(..)
            | Act::SavesLater(_)
            | Act::SavesCopy
            | Act::SavesAskRestore(_)
            | Act::SavesRestore(_)
            | Act::SavesBackup
            | Act::SavesBackupsToggle => self.saves_do(act, cx),
            Act::ModsGet
            | Act::ModsReload
            | Act::ModsLink
            | Act::ModsToggle(_)
            | Act::ModsWhilePlaying
            | Act::ModsMenu(_)
            | Act::ModsMove(..)
            | Act::ModsAskRemove(_)
            | Act::ModsRemove(_)
            | Act::ModsSignIn
            | Act::ModsSignOut
            | Act::ModsUpdates
            | Act::ModsPick(..)
            | Act::ModsAnswer(_)
            | Act::ModsBrowse(_)
            | Act::ModsGetLoader => self.mods_act(act, cx),
            Act::SteamMoveAsk(app) => self.steam_move_ask(app, cx),
            Act::SteamMove(app, folder) => self.steam_move(app, folder, cx),
            Act::SteamMoveMenu => self.steam_move_menu(),
            Act::SteamMoveCancel => {
                self.console.overlay = None;
                self.steam_do(steam_api::cancel_move, |_| None, cx);
            }
            Act::SteamAnswer(id, what) => self.steam_answer(id, what, cx),
            Act::SteamCancelAction(id) => self.steam_cancel_action(id, cx),
            Act::SteamLicence(id) => self.steam_accept(id, cx),
            Act::Licence(yes) => {
                self.steam_answered();
                steam_api::answer_licence(yes);
            }
            Act::ShowSteamWindow => {
                // Answered there.
                steam_api::answer_licence(false);
                self.console_show_steam(None, cx);
            }
            Act::SteamDlc(app, dlc, on) => self.steam_set_dlc(app, dlc, on, cx),
            Act::WorkshopMenu(app, id) => self.workshop_menu(app, &id),
            Act::SteamWorkshop(app, id, on) => self.steam_set_workshop(app, id, on, cx),
            Act::AskUnsubscribe(app, id, title) => self.ask_unsubscribe(app, id, &title),
            Act::Unsubscribe(app, id) => self.steam_unsubscribe(app, id, cx),
            Act::ShotMenu(ix) => self.shot_menu(ix),
            Act::ShotShow(path) => self.console_overlay(Overlay::Picture(path)),
            Act::AskShotDelete(ix) => self.ask_shot_delete(ix),
            Act::ShotDelete(ix) => self.shot_delete(ix, cx),
            Act::SteamControl(on) => {
                self.console.overlay = None;
                self.set_steam_control(on, cx);
            }
            Act::SteamFirst(appid) => {
                self.console.overlay = None;
                self.status = Some(self.tr("Steam restarts…", "Steam yeniden başlıyor…").into());
                self.steam_do(
                    move || shops::steam_first(appid),
                    |this| Some(this.tr("It comes first now.", "Artık ilk sırada.")),
                    cx,
                );
            }
            Act::DownloadMenu(key) => self.console_download_menu(&key),
            Act::MoveDownload(key, up) => {
                self.console.overlay = None;
                downloads::shift(&key, up);
            }
            Act::FirstDownload(key) => {
                self.console.overlay = None;
                downloads::move_to_front(&key);
            }
            Act::CheckArkor => self.check_app_update(false, cx),
            Act::ArkorNotes => {
                if let Some(r) = &self.app_update.available {
                    let title = format!("Arkor {}", r.version);
                    let text = super::console_tools::plain_notes(&r.notes);
                    let options = vec![
                        (self.tr("Update", "Güncelle").to_string(), Act::AskArkor),
                        (self.tr("Close", "Kapat").to_string(), Act::Close),
                    ];
                    self.console.read_at = 0.;
                    self.console_overlay(Overlay::Read(title, text, options));
                }
            }
            Act::AskArkor => {
                if let Some(r) = &self.app_update.available {
                    let text = format!(
                        "Arkor {} ({}): {}",
                        r.version,
                        progress::human_bytes(r.size),
                        self.tr(
                            "download and install it? Arkor restarts; a running game goes on.",
                            "indirilip kurulsun mu? Arkor yeniden başlar; açık oyun çalışmaya devam eder."
                        )
                    );
                    let yes = self.tr("Update", "Güncelle").to_string();
                    self.console_overlay(Overlay::Ask(text, yes, Act::UpdateArkor));
                }
            }
            Act::UpdateArkor => {
                self.console.overlay = None;
                self.install_app_update(cx);
                self.watch_progress(cx);
            }
            Act::ShowSteam => self.console_show_steam(Some("steam://nav/downloads"), cx),
            Act::LoginBrowser => self.shop_login_browser(cx),
            Act::LoginInstall => {
                self.close_shop_windows(cx);
                self.console_install_tools(vec![super::shops::WEBKIT.into()]);
            }
            Act::LoginClose => {
                self.close_shop_windows(cx);
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
                    FolderFor::GameFile => dirs::home_dir(),
                    FolderFor::ImportPrefix => Some(self.global.prefixes_dir()),
                    FolderFor::Roms => self.emu_start_folder(),
                    FolderFor::Bios => Some(arkor_launch::emu::bios_dir(&self.global.emulators)),
                    // Other disks are mounted there.
                    FolderFor::SteamLibrary => self
                        .shops
                        .game
                        .as_ref()
                        .and_then(|g| g.steam_new.clone())
                        .or_else(|| {
                            let user = std::env::var("USER").ok()?;
                            Some(PathBuf::from("/run/media").join(user)).filter(|p| p.is_dir())
                        }),
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
                        FolderFor::GameFile => {}
                        FolderFor::ImportPrefix => self.import_prefix(dir, cx),
                        FolderFor::Roms => self.emu_add_folder(dir, cx),
                        FolderFor::Bios => {
                            self.global.emulators.bios_dir = Some(dir);
                            let r = self.global.save();
                            self.report(r);
                        }
                        FolderFor::SteamLibrary => {
                            if let Some(g) = self.shops.game.as_mut() {
                                g.steam_new = Some(dir);
                                g.steam_folder = None;
                            }
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
                let then = match &self.console.overlay {
                    Some(Overlay::Folder(what, _)) => AfterDisk::Folder(*what),
                    _ => AfterDisk::Stay,
                };
                self.console_mount(&path, then, cx);
            }
            Act::DiskMenu(path) => self.console_disk_menu(&path),
            Act::Unmount(path) => self.console_unmount(&path, cx),
            Act::FolderAt(path) => self.console_overlay(Overlay::Folder(FolderFor::Games, path)),
            Act::InstallAnyway => {
                self.console.overlay = None;
                self.shop_install(cx);
            }
            Act::Opt(o) => self.console_opt(o, cx),
            Act::Pick(Opt::SteamFolder, value) if value == NEW_FOLDER => {
                self.console.overlay = None;
                self.console_do(Act::Folder(FolderFor::SteamLibrary), window, cx);
            }
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
                // Its screens go with it, when it is gone.
                self.console.overlay = None;
                self.shop_uninstall(shop, &id, cx);
            }
            Act::Download(key) => {
                self.console.overlay = None;
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
            Act::CancelDownload(key) => {
                self.console.overlay = None;
                // Steam has it, not only Arkor's queue.
                match self.steam_item(&key).filter(|i| !i.update) {
                    Some(i) => self.console_do(Act::CallOff(i.appid), window, cx),
                    None => downloads::cancel(&key),
                }
            }
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
            // Asked first: what it opens is said.
            Opt::SteamControl if !self.global.steam_control => {
                let text = self.steam_control_text().to_string();
                let yes = self.tr("Switch it on", "Aç").to_string();
                self.console_overlay(Overlay::Ask(text, yes, Act::SteamControl(true)));
            }
            Opt::SteamControl => self.set_steam_control(false, cx),
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
            Opt::Game(GameOpt::Anr) => {
                if self.tools.busy.is_some() || arkor_launch::progress::running("anr") {
                } else if self.tools.anr_manifest.is_some() {
                    self.anr_job("remove", cx);
                } else if self.tools.anr_downloaded.is_some() {
                    self.anr_job("install", cx);
                } else {
                    self.enqueue_download("anr", cx);
                }
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
    pub(super) fn launch_over_soon(&mut self, cx: &mut Context<Self>) {
        let timer = cx.background_executor().clone();
        let steam = self
            .console
            .launching
            .as_ref()
            .and_then(|l| l.steam.clone());
        cx.spawn(async move |this, cx| {
            loop {
                timer.timer(Duration::from_millis(400)).await;
                let waits = this
                    .update(cx, |this, _| {
                        let l = this.console.launching.as_ref();
                        l.is_some_and(|l| l.waits_for_steam() && l.steam == steam)
                    })
                    .unwrap_or(false);
                // Where Steam is with it.
                if waits && let Some(appid) = steam.clone() {
                    let (came, update) = timer
                        .spawn(async move {
                            let came = !arkor_launch::steam_link::game_pids(&appid).is_empty();
                            let update = match appid.parse() {
                                Ok(id) if !came => steam_downloads::pending(id),
                                _ => None,
                            };
                            (came, update)
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| this.steam_launch_step(came, update, cx));
                }
                let go_on = this.update(cx, |this, cx| {
                    let Some(l) = &this.console.launching else {
                        return false;
                    };
                    let id = l.id.clone();
                    let running = this
                        .running
                        .keys()
                        .any(|k| k.split_once(':').is_some_and(|(_, i)| i == id));
                    if !running && !this.saves.starting(&id) {
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

    /// Steam started the game, or turned out to update it first.
    pub(super) fn steam_launch_step(
        &mut self,
        came: bool,
        update: Option<steam_downloads::Item>,
        cx: &mut Context<Self>,
    ) {
        let Some(l) = self.console.launching.as_mut() else {
            return;
        };
        if came {
            l.came = true;
            // The screen it left comes back.
            if std::mem::take(&mut l.updating) {
                l.dismissed = false;
                l.at = Instant::now();
                self.console.overlay = None;
            }
            cx.notify();
        } else if let Some(item) = update
            && !l.updating
        {
            l.updating = true;
            l.dismissed = true;
            self.console_say_update(&item.name, item.left(), false);
            self.watch_progress(cx);
            cx.notify();
        } else if l.steam_may_ask() {
            cx.notify();
        }
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
        self.cue(Cue::Error);
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
            Art::File(path, true) => Some(wide(path.clone())),
            Art::File(..) => None,
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
