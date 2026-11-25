//! Why a game did not start, and what helps.
//!
//! The last launch's log, the game's files, its prefix, its settings and this computer are
//! looked at; what is found becomes [`Problem`]s, the likeliest first, each with [`Fix`]es
//! that take one press. Nothing is changed by looking.
//!
//! Wine says little by itself, so a game can also be started once with everything written
//! down ("detail"), or once without anything of Arkor's around it ("plain"): see [`Trial`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::library::{GameKind, Library, LibraryGame};
use arkor_core::{GameId, Global, HudMode, LsfgMode, Profile, paths};

use crate::components::Latest;
use crate::optiscaler::{Manifest, Mod};
use crate::shops::{self, Shop};
use crate::{admin, compat, tools};

/// What is wrong.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cause {
    /// The game's files are on a disk that is not there.
    DriveGone(String),
    FilesGone,
    NoSpace,
    /// The prefix is on a file system Windows uses.
    PrefixDisk(String),
    ProtonGone(String),
    NoProton,
    /// The store's login ended.
    Login(Shop),
    Outdated,
    /// A command from the game's settings failed.
    PreLaunch,
    /// A program the settings put in front of the game could not be started.
    Wrapper(String),
    NotExecutable,
    /// Left-overs of another Wine run in the prefix.
    WineVersion,
    Gamescope,
    /// This computer has no Vulkan driver (`true`: only the 32-bit one is missing).
    NoVulkan(bool),
    Vulkan,
    DeviceLost,
    /// A Windows component the game needs.
    Dll(String),
    DotNet,
    Media,
    SharedLibrary(String),
    AntiCheat,
    /// The makers keep Linux out, or it does not work yet.
    AntiCheatBlocked,
    SteamNeeded,
    Runtime,
    Namespaces,
    OpenFiles,
    NoNetwork,
    /// ProtonDB calls it broken.
    Borked,
    /// Mods Arkor put into the game's folder.
    Mods(Vec<&'static str>),
    Crash,
    Unknown,
}

/// What can be done with one press.
#[derive(Debug, Clone, PartialEq)]
pub enum Fix {
    /// Start once with everything written down; look again afterwards.
    Detail,
    /// Start once without anything of Arkor's around the game.
    Plain,
    /// Use this Proton build from now on.
    Proton(String),
    /// Proton as Arkor chooses it.
    ProtonAuto,
    /// Draw Direct3D with OpenGL.
    WineD3d,
    GamescopeOff,
    ClearPreLaunch,
    ClearLaunchOptions,
    /// Windows components, installed into the prefix by winetricks.
    Verbs(Vec<&'static str>),
    AntiCheat,
    Runtime,
    /// Ends what is left in the prefix.
    EndPrefix,
    /// Lets Wine set the prefix up again (keeps what is in it).
    UpdatePrefix,
    MakeExecutable(PathBuf),
    Repair(Shop, String),
    Update(Shop, String),
    Login(Shop),
    Packages(Vec<&'static str>),
    Mount,
    RemoveMod(Mod),
    /// Run the Linux game in this Steam Linux Runtime (`None`: without one).
    LinuxRuntime(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub cause: Cause,
    /// The line of the log that tells.
    pub line: Option<String>,
    pub fixes: Vec<Fix>,
}

/// How the last start ended, when it is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ended {
    pub code: Option<i32>,
    pub secs: u64,
}

/// What is looked at.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// The launch log, and the detailed one when it was written.
    pub log: String,
    pub detail: Option<String>,
    pub steam: bool,
    pub windows: bool,
    /// The store it came from and its id there.
    pub shop: Option<(Shop, String)>,
    pub exe: PathBuf,
    pub exe_there: bool,
    pub drive_gone: Option<String>,
    /// The game is a 32-bit Windows program.
    pub bits32: bool,
    /// Windows components that are installed in the prefix.
    pub verbs: BTreeSet<String>,
    /// Not marked as a program (Linux games).
    pub not_executable: bool,
    pub prefix_disk: Option<String>,
    pub free: Option<u64>,
    /// The build the settings name, and whether it is there.
    pub proton: Option<(String, bool)>,
    pub protons: Vec<String>,
    pub vulkan: bool,
    /// `None`: cannot be told on this system.
    pub vulkan32: Option<bool>,
    pub gpus: tools::Gpus,
    pub gamescope: bool,
    /// Something of Arkor's is around the game (overlay, layers, options).
    pub extras: bool,
    pub wined3d: bool,
    pub pre_launch: bool,
    pub launch_options: bool,
    pub mods: Vec<Mod>,
    pub compat: Option<compat::Compat>,
    pub logged_in: bool,
}

/// The least room a prefix needs to be set up.
const ROOM: u64 = 400 * 1024 * 1024;

/// The part of a log that is read: its beginning and its end.
const PART: usize = 1_500_000;

fn read_log(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut head = Vec::new();
    let _ = (&mut f).take(PART as u64).read_to_end(&mut head);
    if len > 2 * PART as u64 && f.seek(SeekFrom::End(-(PART as i64))).is_ok() {
        head.push(b'\n');
    }
    let _ = f.read_to_end(&mut head);
    String::from_utf8_lossy(&head).into_owned()
}

/// Where a detailed start writes its log.
pub fn detail_dir(id: &GameId) -> anyhow::Result<PathBuf> {
    let log = paths::launch_log(id)?;
    let dir = log.parent().context("no log folder")?;
    Ok(dir.join("detail").join(id.file_stem()))
}

impl Facts {
    /// Looks at everything. Blocking (files only).
    pub fn gather(id: &GameId) -> anyhow::Result<Facts> {
        let library = Library::load()?;
        let game = library.get(&id.id).context("not in the library")?;
        let global = Global::load_or_import()?;
        let profile = Profile::load(id).unwrap_or_default();
        Ok(Self::of(id, game, &global, &profile))
    }

    fn of(id: &GameId, game: &LibraryGame, global: &Global, profile: &Profile) -> Facts {
        let windows = game.needs_prefix();
        let shop = shop_of(game);
        let prefix = game.effective_prefix(&global.prefixes_dir());
        let log_path = paths::launch_log(id).ok();
        let log = log_path.as_deref().map(read_log).unwrap_or_default();
        // Only one that is not older than the last start.
        let detail = detail_dir(id).ok().and_then(|d| {
            let newest = std::fs::read_dir(d)
                .ok()?
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .max()?;
            let last = log_path
                .as_ref()
                .and_then(|p| std::fs::metadata(p).ok()?.modified().ok());
            let same = last.is_none_or(|l| newest.0 + std::time::Duration::from_secs(60) >= l);
            same.then(|| read_log(&newest.1))
        });
        let files = [game.exe.clone(), prefix.clone()];
        let gone = files
            .iter()
            .any(|p| admin::disk_folder(p).is_some() && !p.exists());
        let drive_gone = gone
            .then(|| admin::missing_disk(&files, &admin::drives()))
            .flatten()
            .map(|d| match d.disk.label.is_empty() {
                true => d.disk.uuid.clone(),
                false => d.disk.label.clone(),
            });
        let proton = profile
            .proton
            .clone()
            .or_else(|| global.defaults.proton.clone())
            .map(|name| {
                let there = match Latest::from_value(&name) {
                    // It is downloaded when the game starts.
                    Some(l) => l.path().is_ok_and(|p| p.join("proton").is_file()),
                    None => tools::resolve_proton(&name).is_some(),
                };
                (name, there)
            });
        let steam = matches!(game.kind, GameKind::Steam { .. });
        Facts {
            log,
            detail,
            steam,
            windows,
            exe_there: steam || game.exe.exists(),
            bits32: windows && game.exe.is_file() && pe_is_32(&game.exe),
            not_executable: matches!(game.kind, GameKind::Linux) && !executable(&game.exe),
            verbs: installed_verbs(&prefix),
            exe: game.exe.clone(),
            drive_gone,
            prefix_disk: windows.then(|| foreign_disk(&prefix)).flatten(),
            free: shops::free_space(if windows { &prefix } else { &game.exe }),
            proton: proton.filter(|_| windows),
            protons: tools::installed_protons()
                .into_iter()
                .map(|(n, _)| n)
                .collect(),
            vulkan: has_icd(),
            vulkan32: vulkan32(),
            gpus: tools::gpus(),
            gamescope: profile.gamescope.enabled,
            extras: profile.gamescope.enabled
                || profile.effective_hud(global).is_some()
                || profile.lsfg.mode == LsfgMode::On
                || profile.vkbasalt.enabled
                || profile.layers != Default::default()
                || !profile.tweaks.is_empty()
                || !profile.env.is_empty()
                || !profile.launch_options.trim().is_empty()
                || !profile.pre_launch.trim().is_empty()
                || profile.extra_app.path.is_some(),
            wined3d: profile.tweaks.contains("wined3d"),
            pre_launch: !profile.pre_launch.trim().is_empty(),
            launch_options: !profile.launch_options.trim().is_empty(),
            mods: Mod::ALL
                .into_iter()
                .filter(|m| Manifest::load_mod(*m, id).ok().flatten().is_some())
                .collect(),
            compat: shop.as_ref().and_then(|(shop, app)| {
                compat::cached(&compat::Lookup {
                    name: game.name.clone(),
                    platform: shop.key().into(),
                    id: app.clone(),
                    epic_namespace: None,
                })
            }),
            logged_in: shop.as_ref().is_none_or(|(s, _)| shops::logged_in(*s)),
            shop,
        }
    }
}

/// The store a library game was installed from, by Arkor.
fn shop_of(game: &LibraryGame) -> Option<(Shop, String)> {
    let (shop, app) = match &game.kind {
        GameKind::Steam { appid } => (Shop::Steam, appid.clone()),
        GameKind::Epic { app_name } => (Shop::Epic, app_name.clone()),
        GameKind::Amazon { id } => (Shop::Amazon, id.clone()),
        _ => {
            let (shop, app) = shops::from_origin(game.origin.as_deref()?)?;
            (shop, app.to_string())
        }
    };
    (shop == Shop::Steam || shops::installed(shop).iter().any(|g| g.id == app))
        .then_some((shop, app))
}

/// What winetricks installed in a prefix.
fn installed_verbs(prefix: &Path) -> BTreeSet<String> {
    [
        prefix.join("winetricks.log"),
        prefix.join("pfx/winetricks.log"),
    ]
    .iter()
    .filter_map(|p| std::fs::read_to_string(p).ok())
    .flat_map(|t| t.lines().map(|l| l.trim().to_string()).collect::<Vec<_>>())
    .collect()
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map_or(true, |m| m.permissions().mode() & 0o111 != 0)
}

/// Whether a Windows program is a 32-bit one (from its header).
fn pe_is_32(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let machine = || -> std::io::Result<u16> {
        let mut f = std::fs::File::open(path)?;
        let mut head = [0u8; 64];
        f.read_exact(&mut head)?;
        if &head[..2] != b"MZ" {
            return Ok(0);
        }
        let at = u32::from_le_bytes(head[60..64].try_into().expect("four bytes"));
        f.seek(SeekFrom::Start(at.into()))?;
        let mut pe = [0u8; 6];
        f.read_exact(&mut pe)?;
        Ok(match &pe[..4] {
            b"PE\0\0" => u16::from_le_bytes([pe[4], pe[5]]),
            _ => 0,
        })
    };
    machine().is_ok_and(|m| m == 0x014c)
}

/// The file system `path` is (or will be) on, when a prefix does not work there.
fn foreign_disk(path: &Path) -> Option<String> {
    use std::os::unix::ffi::OsStrExt;
    let existing = path.ancestors().find(|p| p.exists())?;
    let c = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    // SAFETY: statfs fills the zeroed struct for a valid path.
    let kind = unsafe {
        let mut st: libc::statfs = std::mem::zeroed();
        if libc::statfs(c.as_ptr(), &mut st) != 0 {
            return None;
        }
        st.f_type as i64
    };
    match kind {
        0x7366_746e | 0x5346_544e => Some("NTFS".into()),
        0x2011_bab0 => Some("exFAT".into()),
        0x4d44 => Some("FAT".into()),
        _ => None,
    }
}

fn has_icd() -> bool {
    ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"]
        .into_iter()
        .any(|d| std::fs::read_dir(d).is_ok_and(|mut e| e.next().is_some()))
}

/// Whether a 32-bit Vulkan driver is there, where that can be told (Arch's layout).
fn vulkan32() -> Option<bool> {
    let dir = Path::new("/usr/lib32");
    if !dir.is_dir() {
        return None;
    }
    let found = std::fs::read_dir(dir).ok()?.flatten().any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        (name.starts_with("libvulkan_") && !name.starts_with("libvulkan_lvp"))
            || name.starts_with("libGLX_nvidia.so")
    });
    Some(found)
}

// ---- Reading logs ----

/// The Windows component that brings a DLL.
fn verb(dll: &str) -> Option<&'static str> {
    let d = dll.to_lowercase();
    Some(match d.as_str() {
        _ if [
            "vcruntime140",
            "msvcp140",
            "concrt140",
            "vcomp140",
            "mfc140",
            "ucrtbase",
        ]
        .iter()
        .any(|p| d.starts_with(p)) =>
        {
            "vcrun2022"
        }
        _ if ["msvcr120", "msvcp120", "mfc120", "vcomp120"]
            .iter()
            .any(|p| d.starts_with(p)) =>
        {
            "vcrun2013"
        }
        _ if ["msvcr110", "msvcp110", "mfc110"]
            .iter()
            .any(|p| d.starts_with(p)) =>
        {
            "vcrun2012"
        }
        _ if ["msvcr100", "msvcp100", "mfc100"]
            .iter()
            .any(|p| d.starts_with(p)) =>
        {
            "vcrun2010"
        }
        _ if ["msvcr90", "msvcp90", "mfc90"]
            .iter()
            .any(|p| d.starts_with(p)) =>
        {
            "vcrun2008"
        }
        _ if d.starts_with("d3dx9_") => "d3dx9",
        _ if d.starts_with("d3dx10_") => "d3dx10",
        _ if d.starts_with("d3dx11_") => "d3dx11_43",
        _ if d.starts_with("d3dcompiler_47") => "d3dcompiler_47",
        _ if d.starts_with("d3dcompiler_43") => "d3dcompiler_43",
        _ if d.starts_with("xinput1_3") || d.starts_with("xinput9_1_0") => "xinput",
        _ if d.starts_with("xactengine") || d.starts_with("xaudio2_") => "xact",
        _ if d.starts_with("openal32") => "openal",
        _ if d.starts_with("physxloader") || d.starts_with("physx3") => "physx",
        _ if d.starts_with("mscoree") || d.starts_with("clr.") => "dotnet48",
        _ => return None,
    })
}

/// What a line of a log tells, if anything. Only what is a reason for a game not to start:
/// the logs of games that run well are full of "error" and "failed", too.
fn read_line(line: &str, detail: bool) -> Option<Cause> {
    let l = line.to_lowercase();
    let has = |p: &str| l.contains(p);
    // "err:module:import_dll Library MSVCP140.dll (which is needed by L"…") not found"
    if has("err:module:import_dll library ") && has("not found") {
        let dll = l
            .split("import_dll library ")
            .nth(1)?
            .split([' ', '('])
            .next()?
            .to_string();
        return Some(if dll.starts_with("mscoree") {
            Cause::DotNet
        } else {
            Cause::Dll(dll)
        });
    }
    if has("wine mono is not installed") || has("this application requires .net framework") {
        return Some(Cause::DotNet);
    }
    if has("no space left on device") || has("disk quota exceeded") {
        return Some(Cause::NoSpace);
    }
    if has("wine client error") && has("version mismatch")
        || has("wineserver") && has("version mismatch")
    {
        return Some(Cause::WineVersion);
    }
    if has("vk_error_incompatible_driver")
        || has("failed to create vulkan instance")
        || has("dxvk: no adapters found")
        || has("no vulkan capable")
        || (has("required vulkan extension") && has("not supported"))
        || has("vkd3d_create_instance: failed")
        || has("d3d11createdevice: failed to create d3d11 device")
        || has("failed to create d3d12 device")
    {
        return Some(Cause::Vulkan);
    }
    if has("vk_error_device_lost")
        || has("dxgi_error_device_removed")
        || has("vk_error_out_of_device_memory")
    {
        return Some(Cause::DeviceLost);
    }
    if has("pre-launch command failed") {
        return Some(Cause::PreLaunch);
    }
    if let Some(rest) = l
        .split("failed to start `")
        .nth(1)
        .or_else(|| l.split("failed to exec `").nth(1))
        && let Some(name) = rest.split('`').next()
    {
        return Some(match name {
            n if n.contains("umu") => Cause::Runtime,
            n => Cause::Wrapper(n.rsplit('/').next().unwrap_or(n).to_string()),
        });
    }
    if has("arkor: proton `")
        && has("is not installed")
        && let Some(name) = line.split('`').nth(1)
    {
        return Some(Cause::ProtonGone(name.to_string()));
    }
    if has("not found (is its drive mounted?)") {
        return Some(Cause::FilesGone);
    }
    if has("[gamescope]")
        && (has("failed to initialize")
            || has("failed to create backend")
            || has("no physical device")
            || has("failed to load x11")
            || has("failed to create vulkan"))
        || has("gamescope: error while loading")
    {
        return Some(Cause::Gamescope);
    }
    if has("not logged in to epic") || has("login failed! cannot continue") {
        return Some(Cause::Login(Shop::Epic));
    }
    if has("game is out of date") {
        return Some(Cause::Outdated);
    }
    if has("is not currently installed") || has("game is not installed") {
        return Some(Cause::FilesGone);
    }
    if has("bwrap:")
        && (has("permission denied")
            || has("operation not permitted")
            || has("no permissions to create"))
    {
        return Some(Cause::Namespaces);
    }
    if has("pressure-vessel") && (has(": e: ") || has("[error]"))
        || has("steam linux runtime not installed")
        || has("umu-run not found")
    {
        return Some(Cause::Runtime);
    }
    if has("too many open files") {
        return Some(Cause::OpenFiles);
    }
    if (has("easyanticheat") || has("easy anti-cheat") || has("battleye"))
        && (has("launch error")
            || has("failed to initialize")
            || has("not installed")
            || has("untrusted system file")
            || has("failed to start"))
    {
        return Some(Cause::AntiCheat);
    }
    if has("steamapi_init() failed")
        || has("unable to initialize steam api")
        || has("steam must be running")
        || has("steam is not running")
    {
        return Some(Cause::SteamNeeded);
    }
    if has("error while loading shared libraries:")
        && let Some(lib) = l.split("shared libraries:").nth(1)
    {
        return Some(Cause::SharedLibrary(
            lib.split(':').next().unwrap_or(lib).trim().to_string(),
        ));
    }
    if has("err:") && (has("winegstreamer") || has("mfplat") || has("wmvcore")) {
        return Some(Cause::Media);
    }
    if has("failed to resolve") || has("temporary failure in name resolution") {
        return Some(Cause::NoNetwork);
    }
    // Wine's own crash report; with everything written down, games catch some themselves.
    if !detail
        && (has("unhandled page fault")
            || has("unhandled exception")
            || has("segmentation fault")
            || has("core dumped"))
        || detail && has("unhandled") && has("starting debugger")
    {
        return Some(Cause::Crash);
    }
    None
}

// ---- Telling what is wrong ----

/// What is wrong with a game, the likeliest first. There is always something to try.
pub fn problems(f: &Facts, ended: Option<Ended>) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();
    let mut add = |cause: Cause, line: Option<&str>, fixes: Vec<Fix>| {
        if !out.iter().any(|p| p.cause == cause) {
            out.push(Problem {
                cause,
                line: line.map(|l| short(l.trim())),
                fixes,
            });
        }
    };
    let repair = |f: &Facts| -> Vec<Fix> {
        f.shop
            .iter()
            .map(|(s, id)| Fix::Repair(*s, id.clone()))
            .collect()
    };

    // What is plainly so, whatever the log says.
    if let Some(disk) = &f.drive_gone {
        add(Cause::DriveGone(disk.clone()), None, vec![Fix::Mount]);
    } else if !f.exe_there {
        add(Cause::FilesGone, None, repair(f));
    }
    if f.free.is_some_and(|b| b < ROOM) {
        add(Cause::NoSpace, None, Vec::new());
    }
    if let Some(kind) = &f.prefix_disk {
        add(Cause::PrefixDisk(kind.clone()), None, Vec::new());
    }
    if f.not_executable {
        add(
            Cause::NotExecutable,
            None,
            vec![Fix::MakeExecutable(f.exe.clone())],
        );
    }
    if f.windows
        && let Some((name, false)) = &f.proton
        && Latest::from_value(name).is_none()
    {
        add(Cause::ProtonGone(name.clone()), None, protons(f, true));
    }
    if f.windows && f.protons.is_empty() && f.proton.as_ref().is_none_or(|(_, there)| !there) {
        add(Cause::NoProton, None, protons(f, true));
    }
    if !f.logged_in
        && let Some((shop, _)) = &f.shop
        && matches!(shop, Shop::Epic | Shop::Amazon)
    {
        add(Cause::Login(*shop), None, vec![Fix::Login(*shop)]);
    }
    if !f.vulkan {
        add(Cause::NoVulkan(false), None, packages(f, false));
    }

    // What the logs tell, in the order it happened.
    let lines = f
        .log
        .lines()
        .map(|l| (l, false))
        .chain(f.detail.iter().flat_map(|d| d.lines()).map(|l| (l, true)));
    let mut dlls = BTreeSet::new();
    for (line, detail) in lines {
        let Some(cause) = read_line(line, detail) else {
            continue;
        };
        let fixes = match &cause {
            Cause::Dll(dll) => {
                // Games look for some they can do without: each once, those that can be
                // installed and are not yet.
                let new = verb(dll).is_some_and(|v| !f.verbs.contains(v));
                if !new || !dlls.insert(verb(dll)) {
                    continue;
                }
                verb(dll).map(|v| Fix::Verbs(vec![v])).into_iter().collect()
            }
            Cause::DotNet => vec![Fix::Verbs(vec!["dotnet48"])],
            Cause::NoSpace => Vec::new(),
            Cause::WineVersion => vec![Fix::EndPrefix, Fix::UpdatePrefix],
            Cause::Vulkan => {
                let mut v = Vec::new();
                if f.bits32 && f.vulkan32 == Some(false) {
                    v.extend(packages(f, true));
                }
                if f.windows && !f.wined3d {
                    v.push(Fix::WineD3d);
                }
                v.extend(protons(f, false).into_iter().take(2));
                v
            }
            Cause::DeviceLost => plain(f),
            Cause::PreLaunch => vec![Fix::ClearPreLaunch],
            Cause::Wrapper(name) if name.contains("gamescope") => {
                vec![Fix::GamescopeOff, Fix::Packages(vec!["gamescope"])]
            }
            Cause::Wrapper(_) if f.launch_options => {
                let mut v = plain(f);
                v.push(Fix::ClearLaunchOptions);
                v
            }
            Cause::Wrapper(_) => plain(f),
            Cause::ProtonGone(_) => protons(f, true),
            Cause::FilesGone if f.drive_gone.is_some() => continue,
            Cause::FilesGone => repair(f),
            Cause::Gamescope if !f.gamescope => Vec::new(),
            Cause::Gamescope => vec![Fix::GamescopeOff],
            Cause::Login(s) => vec![Fix::Login(*s)],
            Cause::Outdated => f
                .shop
                .iter()
                .map(|(s, id)| Fix::Update(*s, id.clone()))
                .collect(),
            Cause::Runtime => vec![Fix::Runtime],
            Cause::AntiCheat => {
                let mut v = vec![Fix::AntiCheat];
                v.extend(repair(f));
                v
            }
            Cause::SharedLibrary(_) if !f.windows => vec![
                Fix::LinuxRuntime("umu-steamrt4"),
                Fix::LinuxRuntime("umu-sniper"),
                Fix::LinuxRuntime("umu-scout"),
            ],
            Cause::SharedLibrary(_) => vec![Fix::Runtime],
            Cause::Media => protons(f, false)
                .into_iter()
                .filter(|p| matches!(p, Fix::Proton(n) if n.contains("GE")))
                .collect(),
            _ => Vec::new(),
        };
        add(cause, Some(line), fixes);
    }

    if let Some(c) = &f.compat {
        if matches!(
            c.anticheat,
            Some(compat::AntiCheat::Broken | compat::AntiCheat::Denied)
        ) {
            add(Cause::AntiCheatBlocked, None, Vec::new());
        }
        if c.protondb.as_deref() == Some("borked") {
            add(Cause::Borked, None, protons(f, false));
        }
    }
    if f.bits32 && f.vulkan32 == Some(false) && f.windows {
        add(Cause::NoVulkan(true), None, packages(f, true));
    }
    if !f.mods.is_empty() {
        add(
            Cause::Mods(f.mods.iter().map(|m| m.name()).collect()),
            None,
            f.mods.iter().map(|m| Fix::RemoveMod(*m)).collect(),
        );
    }

    // What always can be tried.
    let mut tries = Vec::new();
    if f.windows && !f.steam && f.detail.is_none() {
        tries.push(Fix::Detail);
    }
    tries.extend(plain(f));
    if f.windows {
        tries.extend(protons(f, false));
        if !f.wined3d {
            tries.push(Fix::WineD3d);
        }
        tries.push(Fix::EndPrefix);
        tries.push(Fix::UpdatePrefix);
    }
    tries.extend(repair(f));
    // A crash alone says little: its tries are the general ones.
    let crashed = out.iter().any(|p| p.cause == Cause::Crash)
        || ended.is_some_and(|e| matches!(e.code, Some(139 | 134 | 136)));
    out.sort_by_key(|p| p.cause.rank());
    let mut given: Vec<Fix> = Vec::new();
    for p in &mut out {
        p.fixes.retain(|x| !given.contains(x));
        given.extend(p.fixes.iter().cloned());
    }
    out.retain(|p| p.cause != Cause::Crash);
    tries.retain(|x| !given.contains(x));
    let line = last_error(&f.log).map(|l| short(&l));
    out.push(Problem {
        cause: if crashed {
            Cause::Crash
        } else {
            Cause::Unknown
        },
        line: if out.is_empty() { line } else { None },
        fixes: tries,
    });
    out
}

impl Cause {
    /// Plain reasons first, hints last.
    fn rank(&self) -> u8 {
        match self {
            Cause::DriveGone(_) | Cause::FilesGone => 0,
            Cause::NoSpace | Cause::PrefixDisk(_) | Cause::NotExecutable => 1,
            Cause::NoProton | Cause::ProtonGone(_) | Cause::Login(_) | Cause::Outdated => 2,
            Cause::NoVulkan(false) | Cause::PreLaunch | Cause::Wrapper(_) | Cause::Gamescope => 3,
            Cause::WineVersion | Cause::Namespaces | Cause::Runtime => 4,
            Cause::AntiCheatBlocked => 5,
            Cause::Vulkan | Cause::Dll(_) | Cause::DotNet | Cause::SharedLibrary(_) => 6,
            Cause::AntiCheat | Cause::SteamNeeded | Cause::DeviceLost | Cause::OpenFiles => 7,
            Cause::Media | Cause::NoVulkan(true) | Cause::Borked => 8,
            Cause::NoNetwork | Cause::Mods(_) => 9,
            Cause::Crash | Cause::Unknown => 10,
        }
    }
}

fn plain(f: &Facts) -> Vec<Fix> {
    if f.extras {
        vec![Fix::Plain]
    } else {
        Vec::new()
    }
}

/// Other builds to try: the kept-newest ones first, then one that is installed.
fn protons(f: &Facts, auto: bool) -> Vec<Fix> {
    if !f.windows || f.steam {
        return Vec::new();
    }
    let now = f.proton.as_ref().map(|(n, _)| n.as_str());
    let mut out = Vec::new();
    if auto && now.is_some() && !f.protons.is_empty() {
        out.push(Fix::ProtonAuto);
    }
    let names = [Latest::Ge, Latest::CachyOs]
        .map(|l| l.dir_name().to_string())
        .into_iter()
        .chain(f.protons.iter().cloned());
    for name in names {
        if Some(name.as_str()) != now && !out.contains(&Fix::Proton(name.clone())) && out.len() < 3
        {
            out.push(Fix::Proton(name));
        }
    }
    out
}

/// Vulkan drivers for the graphics cards here (Arch's names).
fn packages(f: &Facts, only32: bool) -> Vec<Fix> {
    if crate::start::find_in_path("pacman").is_none() {
        return Vec::new();
    }
    let mut list = Vec::new();
    for (there, p64, p32) in [
        (f.gpus.amd, "vulkan-radeon", "lib32-vulkan-radeon"),
        (f.gpus.intel, "vulkan-intel", "lib32-vulkan-intel"),
        (f.gpus.nvidia, "nvidia-utils", "lib32-nvidia-utils"),
    ] {
        if there {
            if !only32 {
                list.push(p64);
            }
            list.push(p32);
        }
    }
    if list.is_empty() {
        return Vec::new();
    }
    vec![Fix::Packages(list)]
}

/// The last line of a log that sounds like an error.
fn last_error(log: &str) -> Option<String> {
    log.lines()
        .rev()
        .take(80)
        .map(str::trim)
        .find(|l| {
            let l = l.to_lowercase();
            l.starts_with("arkor:") || l.starts_with("error") || l.contains(" error:")
        })
        .map(str::to_string)
}

fn short(line: &str) -> String {
    // Colours of terminals.
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if !c.is_control() {
            out.push(c);
        }
    }
    if out.chars().count() > 180 {
        out = out.chars().take(179).collect::<String>() + "…";
    }
    out
}

// ---- Starting once in another way ----

/// How one start differs from the settings. Passed on in the environment, so it reaches
/// `arkor run` behind a store's client, too.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Trial {
    /// Nothing of Arkor's around the game.
    pub plain: bool,
    /// Proton writes everything down.
    pub detail: bool,
}

pub const TRIAL_ENV: &str = "ARKOR_TRIAL";

impl Trial {
    pub fn text(self) -> Option<&'static str> {
        match (self.plain, self.detail) {
            (true, _) => Some("plain"),
            (_, true) => Some("detail"),
            _ => None,
        }
    }

    pub(crate) fn from_env() -> Trial {
        let v = std::env::var(TRIAL_ENV).unwrap_or_default();
        Trial {
            plain: v == "plain",
            detail: v == "detail",
        }
    }

    /// Changes the settings this start runs with (nothing is saved).
    pub(crate) fn apply(self, id: &GameId, profile: &mut Profile, global: &mut Global) {
        if self.plain {
            let keep = std::mem::take(profile);
            *profile = Profile {
                proton: keep.proton,
                umu_id: keep.umu_id,
                linux_runtime: keep.linux_runtime,
                steam: keep.steam,
                cloud_saves: keep.cloud_saves,
                ..Profile::default()
            };
            profile.mangohud.mode = HudMode::Off;
            profile.lsfg.mode = LsfgMode::Off;
            global.mangohud.enabled = false;
            global.env.clear();
        }
        if self.detail
            && let Ok(dir) = detail_dir(id)
        {
            // Old ones would be read instead.
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if e.path().extension().is_some_and(|x| x == "log") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
            let _ = std::fs::create_dir_all(&dir);
            profile.env.insert("PROTON_LOG".into(), "1".into());
            // Wine's errors without what it only notes: small, and as fast as ever.
            profile
                .env
                .insert("WINEDEBUG".into(), "+timestamp,fixme-all".into());
            profile
                .env
                .insert("PROTON_LOG_DIR".into(), dir.display().to_string());
        }
    }
}

// ---- Doing it ----

/// Does what needs no settings changed (those are the caller's). Blocking.
pub fn apply(id: &GameId, fix: &Fix) -> anyhow::Result<()> {
    match fix {
        Fix::Verbs(verbs) => {
            // umu runs winetricks unattended.
            let words = verbs.iter().map(|v| v.to_string()).collect();
            let status =
                crate::start::run_prefix_tool(id, &crate::start::PrefixTool::Winetricks(words))?;
            let (prefix, _) = crate::start::game_wine_env(id)?;
            let there = installed_verbs(&prefix);
            // Its exit code is no help: umu ends with 1 when it is there already.
            anyhow::ensure!(
                verbs.iter().all(|v| there.contains(*v)),
                "winetricks ended with {status}"
            );
        }
        Fix::AntiCheat => {
            crate::components::ensure_anticheat(true)?;
        }
        Fix::Runtime => {
            crate::components::ensure_umu(true)?;
            crate::components::ensure_runtime(true)?;
        }
        Fix::EndPrefix => crate::start::kill_prefix(id)?,
        Fix::UpdatePrefix => crate::start::update_prefix(id)?,
        Fix::MakeExecutable(path) => {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)?.permissions().mode();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o111))?;
        }
        Fix::RemoveMod(m) => crate::optiscaler::uninstall_mod(*m, id)?,
        _ => anyhow::bail!("not done here"),
    }
    Ok(())
}

/// Changes the settings as a fix says; `false` when it is not about settings.
pub fn change(profile: &mut Profile, fix: &Fix) -> bool {
    match fix {
        Fix::Proton(name) => profile.proton = Some(name.clone()),
        Fix::ProtonAuto => profile.proton = None,
        Fix::WineD3d => {
            profile.tweaks.insert("wined3d".into());
        }
        Fix::GamescopeOff => profile.gamescope.enabled = false,
        Fix::ClearPreLaunch => profile.pre_launch.clear(),
        Fix::ClearLaunchOptions => profile.launch_options.clear(),
        Fix::LinuxRuntime(r) => profile.linux_runtime = Some((*r).to_string()),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(log: &str) -> Facts {
        Facts {
            log: log.into(),
            windows: true,
            exe_there: true,
            vulkan: true,
            vulkan32: Some(true),
            logged_in: true,
            protons: vec!["proton-cachyos-slr".into()],
            ..Facts::default()
        }
    }

    fn causes(f: &Facts) -> Vec<Cause> {
        problems(f, None).into_iter().map(|p| p.cause).collect()
    }

    #[test]
    fn a_healthy_log_tells_nothing() {
        let log = "[gamescope] [[0;31mError[0m] [0;37mvulkan:[0m vkGetPhysicalDeviceFormatProperties2 returned zero modifiers for DRM format 0x38344241 (VkResult: 0)\n\
            INFO: Failed to find bus name com.steampowered.App251 (retry 1)\n\
            (lsfg-vk) [INFO]: Hooked new Vulkan device creation\n\
            [2026-11-22 22:38:49] [MANGOHUD] [info] [blacklist.cpp:86] process 'explorer.exe' is blacklisted\n\
            XALIA WARNING: Unable to release element hwnd-10108\n";
        assert_eq!(causes(&facts(log)), [Cause::Unknown]);
    }

    #[test]
    fn reads_what_logs_say() {
        let cases: &[(&str, Cause)] = &[
            (
                r#"0024:err:module:import_dll Library MSVCP140.dll (which is needed by L"Z:\\g\\a.exe") not found"#,
                Cause::Dll("msvcp140.dll".into()),
            ),
            (
                r#"err:module:import_dll Library mscoree.dll (which is needed by L"x") not found"#,
                Cause::DotNet,
            ),
            (
                "err:   DxvkInstance: vkCreateInstance: VK_ERROR_INCOMPATIBLE_DRIVER",
                Cause::Vulkan,
            ),
            (
                "warn:  DXVK: No adapters found. Please check your Vulkan setup",
                Cause::Vulkan,
            ),
            (
                "wine client error:0: version mismatch 862/864.",
                Cause::WineVersion,
            ),
            (
                "cp: error writing 'x': No space left on device",
                Cause::NoSpace,
            ),
            (
                "Error: pre-launch command failed (exit status: 1)",
                Cause::PreLaunch,
            ),
            (
                "Error: failed to start `gamescope`",
                Cause::Wrapper("gamescope".into()),
            ),
            (
                "arkor: Proton `GE-Proton9-1` is not installed, using the default",
                Cause::ProtonGone("GE-Proton9-1".into()),
            ),
            (
                "Error: /mnt/x/a.exe not found (is its drive mounted?)",
                Cause::FilesGone,
            ),
            (
                "bwrap: setting up uid map: Permission denied",
                Cause::Namespaces,
            ),
            (
                "[cli] ERROR: Game is out of date, please update or launch with update check skipping!",
                Cause::Outdated,
            ),
            (
                "Error: not logged in to Epic: log in on Arkor's Stores page",
                Cause::Login(Shop::Epic),
            ),
            (
                "[S_API FAIL] SteamAPI_Init() failed; unable to locate a running instance",
                Cause::SteamNeeded,
            ),
            (
                "./game: error while loading shared libraries: libssl.so.1.1: cannot open shared object file",
                Cause::SharedLibrary("libssl.so.1.1".into()),
            ),
        ];
        for (line, cause) in cases {
            assert_eq!(read_line(line, false).as_ref(), Some(cause), "{line}");
        }
        let p = problems(&facts(cases[0].0), None);
        assert_eq!(p[0].fixes, [Fix::Verbs(vec!["vcrun2022"])]);
        assert!(p[0].line.as_ref().unwrap().contains("MSVCP140"));
    }

    #[test]
    fn plain_reasons_come_first_and_fixes_once() {
        let mut f = facts("wine: Unhandled page fault on read access to 0000\n");
        f.exe_there = false;
        f.shop = Some((Shop::Gog, "7".into()));
        f.extras = true;
        let p = problems(&f, None);
        assert_eq!(p[0].cause, Cause::FilesGone);
        assert_eq!(p[0].fixes, [Fix::Repair(Shop::Gog, "7".into())]);
        assert_eq!(p.last().unwrap().cause, Cause::Crash);
        let last = &p.last().unwrap().fixes;
        assert_eq!(last[..2], [Fix::Detail, Fix::Plain]);
        assert!(!last.contains(&Fix::Repair(Shop::Gog, "7".into())));
        // Ended by a signal: a crash, too.
        let p = problems(
            &facts(""),
            Some(Ended {
                code: Some(139),
                secs: 3,
            }),
        );
        assert_eq!(p[0].cause, Cause::Crash);
    }

    #[test]
    fn many_missing_parts_are_one_installation() {
        let log = [
            "msvcp140.dll",
            "vcruntime140.dll",
            "vcruntime140_1.dll",
            "d3dx9_43.dll",
            "steam_api_x.dll",
        ]
        .map(|d| {
            format!("err:module:import_dll Library {d} (which is needed by L\"x\") not found\n")
        })
        .concat();
        let p = problems(&facts(&log), None);
        let verbs: Vec<&Fix> = p
            .iter()
            .flat_map(|p| &p.fixes)
            .filter(|x| matches!(x, Fix::Verbs(_)))
            .collect();
        assert_eq!(
            verbs,
            [&Fix::Verbs(vec!["vcrun2022"]), &Fix::Verbs(vec!["d3dx9"])]
        );
    }

    #[test]
    fn missing_proton() {
        let mut f = facts("");
        f.proton = Some(("GE-Proton9-1".into(), false));
        let p = problems(&f, None);
        assert_eq!(p[0].cause, Cause::ProtonGone("GE-Proton9-1".into()));
        assert_eq!(p[0].fixes[0], Fix::ProtonAuto);
        // One that is fetched when the game starts is not missing.
        f.proton = Some((Latest::Ge.dir_name().into(), false));
        assert_eq!(causes(&f), [Cause::Unknown]);
    }

    #[test]
    fn fixes_change_the_settings() {
        let mut p = Profile::default();
        assert!(change(&mut p, &Fix::WineD3d));
        assert!(p.tweaks.contains("wined3d"));
        assert!(arkor_core::tweaks::TWEAKS.iter().any(|t| t.id == "wined3d"));
        assert!(change(&mut p, &Fix::Proton("X".into())));
        assert_eq!(p.proton.as_deref(), Some("X"));
        assert!(!change(&mut p, &Fix::EndPrefix));
    }

    #[test]
    fn a_plain_start_leaves_everything_out() {
        let id: GameId = "local:x".parse().unwrap();
        let mut p = Profile {
            proton: Some("X".into()),
            launch_options: "foo %command%".into(),
            ..Profile::default()
        };
        p.gamescope.enabled = true;
        let mut g = Global::default();
        g.mangohud.enabled = true;
        g.env.insert("A".into(), "1".into());
        Trial {
            plain: true,
            detail: false,
        }
        .apply(&id, &mut p, &mut g);
        assert_eq!(p.proton.as_deref(), Some("X"));
        assert!(!p.gamescope.enabled && p.launch_options.is_empty() && g.env.is_empty());
        assert!(p.effective_hud(&g).is_none());
    }

    #[test]
    fn program_headers() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = |machine: u16| {
            let mut b = vec![0u8; 64];
            b[..2].copy_from_slice(b"MZ");
            b[60] = 64;
            b.extend(b"PE\0\0");
            b.extend(machine.to_le_bytes());
            let p = tmp.path().join(format!("{machine}.exe"));
            std::fs::write(&p, b).unwrap();
            p
        };
        assert!(pe_is_32(&exe(0x014c)));
        assert!(!pe_is_32(&exe(0x8664)));
        std::fs::write(tmp.path().join("t"), "text").unwrap();
        assert!(!pe_is_32(&tmp.path().join("t")));
        assert_eq!(short("\u{1b}[0;31mError\u{1b}[0m x"), "Error x");
    }
}
