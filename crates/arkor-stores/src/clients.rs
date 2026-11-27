//! Games installed inside the store launchers Arkor puts in prefixes of their own
//! (Battle.net, the EA app, Ubisoft Connect, the Rockstar Games Launcher). Each launcher in
//! the library is a Windows program in its prefix; what it installed is read from the
//! prefix's files without running anything, and each game becomes a library entry that
//! starts the launcher with the arguments that open that game.
//!
//! - Battle.net: the Agent's `product.db` (protobuf).
//! - EA app: `__Installer/installerdata.xml` in each game folder.
//! - Ubisoft Connect: `Ubisoft\Launcher\Installs` in the registry.
//! - Rockstar: `Rockstar Games` and the uninstall keys in the registry.
//!
//! Ids are `<launcher>/<launcher's library id>/<game id>`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::library::{GameKind, Library, LibraryGame};
use arkor_core::{Game, GameId, Store};

use crate::target::ExeTarget;

/// A launcher: its id and its program inside the prefix.
struct Client {
    id: &'static str,
    exe: &'static str,
}

const CLIENTS: [Client; 4] = [
    Client {
        id: "battle",
        exe: "drive_c/Program Files (x86)/Battle.net/Battle.net.exe",
    },
    Client {
        id: "ea",
        exe: "drive_c/Program Files/Electronic Arts/EA Desktop/EA Desktop/EALauncher.exe",
    },
    Client {
        id: "ubisoft",
        exe: "drive_c/Program Files (x86)/Ubisoft/Ubisoft Game Launcher/UbisoftConnect.exe",
    },
    Client {
        id: "rockstar",
        exe: "drive_c/Program Files/Rockstar Games/Launcher/Launcher.exe",
    },
];

/// A game a launcher installed.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientGame {
    pub id: String,
    pub title: String,
    pub folder: Option<PathBuf>,
    /// What the launcher's program is given to open the game.
    pub args: Vec<String>,
}

// ---- Wine's registry files ----

/// A key of a Wine `.reg` file and its string values.
#[derive(Debug, Default)]
struct RegKey {
    path: String,
    values: Vec<(String, String)>,
}

impl RegKey {
    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The last part of its path.
    fn name(&self) -> &str {
        self.path.rsplit('\\').next().unwrap_or_default()
    }

    /// Whether it is directly under `parent`.
    fn under(&self, parent: &str) -> bool {
        self.path.len() > parent.len() + 1
            && self.path[..parent.len()].eq_ignore_ascii_case(parent)
            && self.path.as_bytes()[parent.len()] == b'\\'
            && !self.path[parent.len() + 1..].contains('\\')
    }
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('x') => {
                let hex: String = std::iter::from_fn(|| chars.next_if(char::is_ascii_hexdigit))
                    .take(4)
                    .collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some('n') => out.push('\n'),
            Some('0') => {}
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// The keys of a Wine registry file with their string values.
fn parse_reg(text: &str) -> Vec<RegKey> {
    let mut keys: Vec<RegKey> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix('[') {
            let Some(end) = rest.rfind(']') else { continue };
            keys.push(RegKey {
                path: unescape(&rest[..end]),
                values: Vec::new(),
            });
        } else if let (Some(key), Some(rest)) = (keys.last_mut(), line.strip_prefix('"')) {
            let Some((name, value)) = rest.split_once("\"=") else {
                continue;
            };
            if let Some(v) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                key.values.push((unescape(name), unescape(v)));
            }
        }
    }
    keys
}

fn machine_keys(prefix: &Path) -> Vec<RegKey> {
    std::fs::read(prefix.join("system.reg"))
        .map(|b| parse_reg(&String::from_utf8_lossy(&b)))
        .unwrap_or_default()
}

const UNINSTALL: [&str; 2] = [
    "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "Software\\Wow6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];

// ---- Windows paths inside a prefix ----

/// `rel` under `root`, matching each part without regard to case (Windows does not care).
fn join_ci(root: &Path, rel: &str) -> PathBuf {
    let mut at = root.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|p| !p.is_empty()) {
        let exact = at.join(part);
        if exact.exists() {
            at = exact;
            continue;
        }
        let found = std::fs::read_dir(&at).ok().and_then(|rd| {
            rd.flatten()
                .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))
                .map(|e| e.path())
        });
        at = found.unwrap_or(exact);
    }
    at
}

/// A Windows path (`C:\Games\X` or `C:/Games/X`) as a path inside the prefix.
fn local_path(prefix: &Path, windows: &str) -> Option<PathBuf> {
    let w = windows.trim().trim_matches('"');
    let (drive, rest) = w.split_once(':')?;
    let drive = drive.chars().next()?.to_ascii_lowercase();
    Some(join_ci(&prefix.join(format!("drive_{drive}")), rest))
}

fn last_part(windows: &str) -> Option<String> {
    windows
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty() && !s.ends_with(':'))
        .map(str::to_string)
}

fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

// ---- Battle.net ----

/// Fields of a protobuf message: (number, varint or bytes).
fn pb_fields(mut b: &[u8]) -> Vec<(u64, Result<u64, &[u8]>)> {
    fn varint(b: &mut &[u8]) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let (&byte, rest) = b.split_first()?;
            *b = rest;
            v |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    let mut out = Vec::new();
    while !b.is_empty() {
        let Some(tag) = varint(&mut b) else { break };
        match tag & 7 {
            0 => match varint(&mut b) {
                Some(v) => out.push((tag >> 3, Ok(v))),
                None => break,
            },
            2 => {
                let Some(len) = varint(&mut b).and_then(|l| usize::try_from(l).ok()) else {
                    break;
                };
                if len > b.len() {
                    break;
                }
                out.push((tag >> 3, Err(&b[..len])));
                b = &b[len..];
            }
            1 if b.len() >= 8 => b = &b[8..],
            5 if b.len() >= 4 => b = &b[4..],
            _ => break,
        }
    }
    out
}

fn pb_str(v: &Result<u64, &[u8]>) -> Option<String> {
    v.as_ref()
        .err()
        .and_then(|b| std::str::from_utf8(b).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Codes Battle.net's `--exec="launch <code>"` takes for games it knows by uid.
const BATTLE_KNOWN: &[(&str, &str, &str)] = &[
    ("wow", "WoW", "World of Warcraft"),
    ("wow_classic", "WoWC", "World of Warcraft Classic"),
    ("prometheus", "Pro", "Overwatch 2"),
    ("fenris", "Fen", "Diablo IV"),
    ("diablo3", "D3", "Diablo III"),
    ("osi", "OSI", "Diablo II: Resurrected"),
    ("hs_beta", "WTCG", "Hearthstone"),
    ("heroes", "Hero", "Heroes of the Storm"),
    ("s2", "S2", "StarCraft II"),
    ("s1", "S1", "StarCraft: Remastered"),
    ("w3", "W3", "Warcraft III: Reforged"),
    ("rtro", "RTRO", "Blizzard Arcade Collection"),
    ("viper", "VIPR", "Call of Duty: Black Ops 4"),
    ("odin", "ODIN", "Call of Duty: Modern Warfare"),
    ("zeus", "ZEUS", "Call of Duty: Black Ops Cold War"),
    ("fore", "FORE", "Call of Duty: Vanguard"),
    ("auks", "AUKS", "Call of Duty"),
    ("pinta", "PNTA", "Call of Duty: Modern Warfare III"),
    ("aris", "ARIS", "DOOM: The Dark Ages"),
];

fn battle_games(prefix: &Path) -> Vec<ClientGame> {
    let Ok(db) = std::fs::read(join_ci(
        prefix,
        "drive_c/ProgramData/Battle.net/Agent/product.db",
    )) else {
        return Vec::new();
    };
    battle_from_db(prefix, &db)
}

fn battle_from_db(prefix: &Path, db: &[u8]) -> Vec<ClientGame> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (n, v) in pb_fields(db) {
        let (1, Err(install)) = (n, v) else { continue };
        let (mut uid, mut code, mut path, mut installed) =
            (String::new(), String::new(), None, None);
        for (n, v) in pb_fields(install) {
            match n {
                1 => uid = pb_str(&v).unwrap_or_default(),
                2 => code = pb_str(&v).unwrap_or_default(),
                3 => {
                    if let Err(settings) = v {
                        path = pb_fields(settings)
                            .iter()
                            .find(|(n, _)| *n == 1)
                            .and_then(|(_, v)| pb_str(v));
                    }
                }
                4 => {
                    if let Err(state) = v
                        && let Some((_, Err(base))) =
                            pb_fields(state).into_iter().find(|(n, _)| *n == 1)
                    {
                        installed = pb_fields(base)
                            .into_iter()
                            .find(|(n, _)| *n == 1)
                            .and_then(|(_, v)| v.ok())
                            .map(|f| f != 0);
                    }
                }
                _ => {}
            }
        }
        let id = if uid.is_empty() { code.clone() } else { uid };
        let not_game = |s: &str| {
            ["agent", "bna", "battle.net"]
                .iter()
                .any(|b| s.eq_ignore_ascii_case(b))
        };
        if id.is_empty() || not_game(&id) || not_game(&code) {
            continue;
        }
        if installed == Some(false) || (installed.is_none() && path.is_none()) {
            continue;
        }
        let known = BATTLE_KNOWN
            .iter()
            .find(|(u, ..)| u.eq_ignore_ascii_case(&id) || u.eq_ignore_ascii_case(&code));
        let (launch, title) = match known {
            Some((_, c, t)) => (c.to_string(), t.to_string()),
            None => {
                let c = if code.is_empty() {
                    id.clone()
                } else {
                    code.clone()
                };
                let t = path
                    .as_deref()
                    .and_then(last_part)
                    .unwrap_or_else(|| c.clone());
                (c, t)
            }
        };
        let folder = path.as_deref().and_then(|p| local_path(prefix, p));
        if folder.as_ref().is_some_and(|d| !d.is_dir()) || !seen.insert(id.to_lowercase()) {
            continue;
        }
        out.push(ClientGame {
            id,
            title,
            folder,
            args: vec![format!("--exec=launch {launch}")],
        });
    }
    out
}

// ---- Ubisoft Connect ----

fn ubisoft_games(prefix: &Path) -> Vec<ClientGame> {
    ubisoft_from_keys(prefix, &machine_keys(prefix))
}

fn ubisoft_from_keys(prefix: &Path, keys: &[RegKey]) -> Vec<ClientGame> {
    const INSTALLS: [&str; 2] = [
        "Software\\Wow6432Node\\Ubisoft\\Launcher\\Installs",
        "Software\\Ubisoft\\Launcher\\Installs",
    ];
    let title_of = |id: &str| {
        let name = format!("Uplay Install {id}");
        keys.iter()
            .filter(|k| {
                UNINSTALL.iter().any(|u| k.under(u)) && k.name().eq_ignore_ascii_case(&name)
            })
            .find_map(|k| k.get("DisplayName").map(str::to_string))
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for key in keys.iter().filter(|k| INSTALLS.iter().any(|i| k.under(i))) {
        let id = key.name().trim().to_string();
        let Some(dir) = key.get("InstallDir") else {
            continue;
        };
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let folder = local_path(prefix, dir);
        if let Some(f) = &folder {
            // Missing, or still downloading (the state file's first byte is 0x0A when done).
            let state = std::fs::read(join_ci(f, "uplay_install.state")).ok();
            if !f.is_dir() || state.is_some_and(|s| s.first().is_some_and(|b| *b != 0x0A)) {
                continue;
            }
        }
        let title = title_of(&id)
            .or_else(|| last_part(dir))
            .unwrap_or_else(|| id.clone());
        out.push(ClientGame {
            args: vec![format!("uplay://launch/{id}/0")],
            id,
            title,
            folder,
        });
    }
    out
}

// ---- Rockstar ----

fn rockstar_games(prefix: &Path) -> Vec<ClientGame> {
    rockstar_from_keys(prefix, &machine_keys(prefix))
}

fn rockstar_from_keys(prefix: &Path, keys: &[RegKey]) -> Vec<ClientGame> {
    const GAMES: [&str; 2] = [
        "Software\\Wow6432Node\\Rockstar Games",
        "Software\\Rockstar Games",
    ];
    let not_game = |s: &str| {
        ["launcher", "social club", "rockstar games social club"]
            .iter()
            .any(|n| s.eq_ignore_ascii_case(n))
    };
    let mut found: Vec<(String, Option<String>, String)> = Vec::new();
    // Uninstall keys: `Launcher.exe … -uninstall=<titleId>`.
    for k in keys.iter().filter(|k| UNINSTALL.iter().any(|u| k.under(u))) {
        let (Some(cmd), Some(dir)) = (k.get("UninstallString"), k.get("InstallLocation")) else {
            continue;
        };
        let lower = cmd.to_ascii_lowercase();
        if !lower.contains("launcher.exe") {
            continue;
        }
        if let Some(at) = lower.rfind("-uninstall=") {
            let id: String = cmd[at + 11..]
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '"')
                .collect();
            if !id.is_empty() && !not_game(&id) {
                found.push((
                    id,
                    k.get("DisplayName").map(str::to_string),
                    dir.to_string(),
                ));
            }
        }
    }
    // Each game's own key with `InstallFolder`.
    for k in keys.iter().filter(|k| GAMES.iter().any(|g| k.under(g))) {
        if not_game(k.name()) {
            continue;
        }
        if let Some(dir) = k.get("InstallFolder").or_else(|| k.get("InstallLocation")) {
            found.push((
                k.name().to_string(),
                Some(k.name().to_string()),
                dir.to_string(),
            ));
        }
    }
    let mut folders = HashSet::new();
    let mut ids = HashSet::new();
    let mut out = Vec::new();
    for (id, title, dir) in found {
        let win = dir
            .trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_string();
        if win.is_empty() || !folders.insert(win.to_lowercase()) || !ids.insert(id.to_lowercase()) {
            continue;
        }
        let folder = local_path(prefix, &win);
        if folder.as_ref().is_some_and(|d| !d.is_dir()) {
            continue;
        }
        let title = title
            .or_else(|| last_part(&win))
            .unwrap_or_else(|| id.clone());
        out.push(ClientGame {
            id,
            title,
            folder,
            args: vec!["-launchTitleInFolder".into(), win],
        });
    }
    out
}

// ---- EA app ----

/// Text of the elements named `tag` (with their attributes).
fn xml_elements<'a>(xml: &'a str, tag: &str) -> Vec<(&'a str, String)> {
    let mut out = Vec::new();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        if !after.starts_with(['>', ' ', '\t', '\r', '\n']) {
            rest = after;
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        let attrs = &after[..gt];
        let body = &after[gt + 1..];
        let Some(end) = body.find(&close) else { break };
        let text = body[..end]
            .trim()
            .trim_start_matches("<![CDATA[")
            .trim_end_matches("]]>")
            .replace("&amp;", "&")
            .replace("&apos;", "'")
            .replace("&quot;", "\"")
            .trim()
            .to_string();
        out.push((attrs, text));
        rest = &body[end..];
    }
    out
}

fn ea_installer(xml: &str) -> (Vec<String>, Option<String>) {
    let mut ids: Vec<String> = Vec::new();
    for (_, id) in xml_elements(xml, "contentID") {
        let usable = !id.is_empty()
            && !id.contains(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '&' | ','));
        if usable && !ids.contains(&id) {
            ids.push(id);
        }
    }
    let english = |a: &str| a.replace('-', "_").to_ascii_lowercase().contains("en_us");
    let titles = xml_elements(xml, "gameTitle");
    let title = titles
        .iter()
        .find(|(a, t)| english(a) && !t.is_empty())
        .or_else(|| titles.iter().find(|(_, t)| !t.is_empty()))
        .map(|(_, t)| t.clone());
    (ids, title)
}

fn ea_games(prefix: &Path) -> Vec<ClientGame> {
    let mut candidates: Vec<(PathBuf, Option<String>)> =
        subfolders(&join_ci(prefix, "drive_c/Program Files/EA Games"))
            .into_iter()
            .map(|f| (f, None))
            .collect();
    // Other library folders (`DownloadInPlaceDir` in its ini files).
    let mut inis = vec![join_ci(
        prefix,
        "drive_c/ProgramData/EA Desktop/machine.ini",
    )];
    for user in subfolders(&prefix.join("drive_c/users")) {
        let dir = join_ci(&user, "AppData/Local/Electronic Arts/EA Desktop");
        inis.extend(
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("user_") && n.ends_with(".ini"))
                }),
        );
    }
    for text in inis.iter().filter_map(|i| std::fs::read_to_string(i).ok()) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=')
                && k.trim()
                    .to_ascii_lowercase()
                    .ends_with("downloadinplacedir")
                && let Some(root) = local_path(prefix, v)
            {
                candidates.extend(subfolders(&root).into_iter().map(|f| (f, None)));
            }
        }
    }
    // Uninstall keys the EA installer wrote.
    for k in machine_keys(prefix)
        .iter()
        .filter(|k| UNINSTALL.iter().any(|u| k.under(u)))
        .filter(|k| {
            k.get("UninstallString")
                .is_some_and(|s| s.to_ascii_lowercase().contains("eainstaller"))
        })
    {
        if let Some(dir) = k.get("InstallLocation").and_then(|d| local_path(prefix, d)) {
            candidates.push((dir, k.get("DisplayName").map(str::to_string)));
        }
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (folder, display) in candidates {
        if !seen.insert(folder.to_string_lossy().to_lowercase()) {
            continue;
        }
        let Ok(xml) = std::fs::read_to_string(join_ci(&folder, "__Installer/installerdata.xml"))
        else {
            continue;
        };
        let (ids, title) = ea_installer(&xml);
        let Some(id) = ids.first().cloned() else {
            continue;
        };
        let Some(title) = title
            .or(display)
            .or_else(|| folder.file_name().map(|n| n.to_string_lossy().into_owned()))
        else {
            continue;
        };
        out.push(ClientGame {
            id,
            title,
            folder: Some(folder),
            args: vec![format!(
                "origin2://game/launch?offerIds={}&autoDownload=1",
                ids.join(",")
            )],
        });
    }
    out
}

// ---- The launchers in the library ----

/// The launchers in the library: (launcher id, its library entry, its prefix).
fn launchers(library: &Library, prefixes_dir: &Path) -> Vec<(&'static str, LibraryGame, PathBuf)> {
    library
        .games
        .iter()
        .filter(|g| g.kind == GameKind::Windows)
        .filter_map(|g| {
            let prefix = g.effective_prefix(prefixes_dir);
            let exe = g.exe.strip_prefix(&prefix).ok()?.to_string_lossy().to_lowercase();
            let client = CLIENTS.iter().find(|c| {
                exe == c.exe.to_lowercase()
                    // The EA app keeps its program in a versioned folder too.
                    || (c.id == "ea" && (exe.ends_with("ealauncher.exe") || exe.ends_with("eadesktop.exe")))
            })?;
            Some((client.id, g.clone(), prefix))
        })
        .collect()
}

/// What `client` installed in `prefix`.
pub fn client_games(client: &str, prefix: &Path) -> Vec<ClientGame> {
    match client {
        "battle" => battle_games(prefix),
        "ea" => ea_games(prefix),
        "ubisoft" => ubisoft_games(prefix),
        "rockstar" => rockstar_games(prefix),
        _ => Vec::new(),
    }
}

fn prefixes_dir() -> PathBuf {
    arkor_core::profile::Global::load_or_import()
        .map(|g| g.prefixes_dir())
        .unwrap_or_default()
}

/// Every game the launchers in the library installed.
pub fn games() -> (Vec<Game>, Vec<anyhow::Error>) {
    let library = match Library::load() {
        Ok(l) => l,
        Err(e) => return (Vec::new(), vec![e]),
    };
    let mut out = Vec::new();
    for (client, launcher, prefix) in launchers(&library, &prefixes_dir()) {
        for g in client_games(client, &prefix) {
            out.push(Game {
                id: GameId::new(Store::Client, format!("{client}/{}/{}", launcher.id, g.id)),
                name: g.title,
                install_dir: g.folder,
                size_on_disk: None,
            });
        }
    }
    (out, Vec::new())
}

/// How to start a launcher's game: the launcher's program, in its prefix, with the game's
/// arguments; the game's own folder is the working folder (so tools find the game).
pub fn launch_target(id: &str) -> anyhow::Result<ExeTarget> {
    let mut parts = id.splitn(3, '/');
    let (Some(client), Some(launcher_id), Some(game_id)) =
        (parts.next(), parts.next(), parts.next())
    else {
        anyhow::bail!("`{id}` is not a launcher game id");
    };
    let library = Library::load()?;
    let (_, launcher, prefix) = launchers(&library, &prefixes_dir())
        .into_iter()
        .find(|(c, l, _)| *c == client && l.id == launcher_id)
        .with_context(|| format!("the launcher of `{id}` is not in the library any more"))?;
    let game = client_games(client, &prefix)
        .into_iter()
        .find(|g| g.id == game_id)
        .with_context(|| format!("{game_id} is not installed in the launcher any more"))?;
    let mut t = ExeTarget::windows(launcher.exe.clone());
    t.args = game.args;
    if let Some(folder) = game.folder.filter(|f| f.is_dir()) {
        t.working_dir = folder;
    }
    t.prefix = Some(prefix);
    t.proton = launcher.source_proton.clone();
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(path: &Path, bytes: impl AsRef<[u8]>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    const REG: &str = r#"WINE REGISTRY Version 2

[Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Uplay Install 635] 1
"DisplayName"="Far Cry\x00ae 3"

[Software\\Wow6432Node\\Ubisoft\\Launcher\\Installs\\635] 1
"InstallDir"="C:/Program Files (x86)/Ubisoft/Ubisoft Game Launcher/games/Far Cry 3/"

[Software\\Wow6432Node\\Ubisoft\\Launcher\\Installs\\9] 1
"InstallDir"="C:/Program Files (x86)/Ubisoft/Ubisoft Game Launcher/games/Gone/"

[Software\\Wow6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Rockstar Games Launcher - GTA V] 1
"DisplayName"="Grand Theft Auto V"
"InstallLocation"="C:\\Program Files\\Rockstar Games\\Grand Theft Auto V"
"UninstallString"="\"C:\\Program Files\\Rockstar Games\\Launcher\\Launcher.exe\" -uninstall=gta5"
"#;

    #[test]
    fn reads_ubisoft_and_rockstar_from_the_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path();
        std::fs::create_dir_all(
            p.join("drive_c/Program Files (x86)/Ubisoft/Ubisoft Game Launcher/games/far cry 3"),
        )
        .unwrap();
        std::fs::create_dir_all(p.join("drive_c/Program Files/Rockstar Games/Grand Theft Auto V"))
            .unwrap();
        let keys = parse_reg(REG);
        let ubi = ubisoft_from_keys(p, &keys);
        assert_eq!(ubi.len(), 1, "a missing folder is left out: {ubi:?}");
        assert_eq!(ubi[0].title, "Far Cry® 3");
        assert_eq!(ubi[0].args, ["uplay://launch/635/0"]);
        assert!(
            ubi[0].folder.as_ref().unwrap().ends_with("far cry 3"),
            "case-insensitive"
        );
        let r = rockstar_from_keys(p, &keys);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "gta5");
        assert_eq!(r[0].title, "Grand Theft Auto V");
        assert_eq!(
            r[0].args,
            [
                "-launchTitleInFolder",
                "C:\\Program Files\\Rockstar Games\\Grand Theft Auto V"
            ]
        );
    }

    fn pb_bytes(n: u64, body: &[u8]) -> Vec<u8> {
        let mut out = vec![((n << 3) | 2) as u8, body.len() as u8];
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn reads_battle_net_product_db() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path();
        std::fs::create_dir_all(p.join("drive_c/Program Files (x86)/Diablo IV")).unwrap();
        let installed = pb_bytes(1, &[0x08, 0x01]);
        let state = pb_bytes(4, &pb_bytes(1, &[0x08, 0x01]));
        let mut d4 = pb_bytes(1, b"fenris");
        d4.extend(pb_bytes(2, b"Fen"));
        d4.extend(pb_bytes(
            3,
            &pb_bytes(1, b"C:/Program Files (x86)/Diablo IV"),
        ));
        d4.extend(state);
        let _ = installed;
        let mut agent = pb_bytes(1, b"agent");
        agent.extend(pb_bytes(2, b"agent"));
        let mut db = pb_bytes(1, &d4);
        db.extend(pb_bytes(1, &agent));
        let games = battle_from_db(p, &db);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Diablo IV");
        assert_eq!(games[0].args, ["--exec=launch Fen"]);
    }

    #[test]
    fn reads_ea_installer_data() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path();
        put(
            &p.join("drive_c/Program Files/EA Games/Dead Space/__Installer/installerdata.xml"),
            r#"<DiPManifest><contentIDs><contentID>1234567</contentID><contentID>Origin.OFR.50.0001</contentID></contentIDs>
            <gameTitles><gameTitle locale="en_US">Dead Space</gameTitle></gameTitles></DiPManifest>"#,
        );
        let games = ea_games(p);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Dead Space");
        assert_eq!(
            games[0].args,
            ["origin2://game/launch?offerIds=1234567,Origin.OFR.50.0001&autoDownload=1"]
        );
    }
}
