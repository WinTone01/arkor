//! RetroAchievements for emulator games (ported from Irori's `retroachievements` client and
//! its ROM library glue).
//!
//! - The account is a username and Web API key (`retroachievements.org/settings`), kept
//!   sealed (see `crate::shops::vault`); it only adds achievements.
//! - A game is matched by the hash RetroAchievements gives its file ([`hash`], the same
//!   one its emulators compute), against the console's games and their hashes. Hashes are
//!   cached by size and change time, console lists for a week, progress for 15 minutes.
//! - [`sign_in_emulators`] signs RetroArch, PCSX2, PPSSPP and Dolphin in with the token
//!   RetroAchievements gives emulators for the password (asked once, never kept).
//!
//! The key and the password never appear on a command line or in a message: requests go
//! through `curl` with its settings on standard input.

pub mod cheevos;
mod consoles;
mod disc;
pub mod hash;
pub mod model;

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Context;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub use consoles::{Console, console_for};
use model::{ConnectLogin, GameEntry, GameProgress, UserProfile};

pub const SITE: &str = "https://retroachievements.org";
/// Where a user finds their Web API key (signed in on the site).
pub const API_KEY_URL: &str = "https://retroachievements.org/settings";

const PROGRESS_MAX_AGE: u64 = 15 * 60;
const GAMES_MAX_AGE: u64 = 7 * 24 * 3600;

/// Whether `key` looks like a Web API key: 32 letters and digits.
pub fn is_api_key(key: &str) -> bool {
    let key = key.trim();
    key.len() == 32 && key.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The account achievements are read with.
#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    pub api_key: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

fn dir() -> PathBuf {
    super::dirs::cache().join("retroachievements")
}

fn account_file() -> PathBuf {
    super::dirs::custom()
        .parent()
        .map(|p| p.join("retroachievements.json"))
        .unwrap_or_else(|| dir().join("account.json"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The saved account, if any.
pub fn account() -> Option<Account> {
    let bytes = crate::shops::vault::read_secret(&account_file(), "arkor-retroachievements")?;
    serde_json::from_slice(&bytes).ok()
}

/// Runs curl with `config` (its settings, one per line) on standard input; the answer's
/// body and status.
fn curl(config: &str) -> anyhow::Result<(u16, Vec<u8>)> {
    let mut child = Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "60",
            "-A",
            "arkor",
            "-w",
            "\n%{http_code}",
            "-K",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("running curl")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(config.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    anyhow::ensure!(out.status.success(), "could not reach RetroAchievements");
    let text = out.stdout;
    let cut = text.iter().rposition(|b| *b == b'\n').unwrap_or(0);
    let status = String::from_utf8_lossy(&text[cut..])
        .trim()
        .parse()
        .unwrap_or(0);
    Ok((status, text[..cut].to_vec()))
}

/// A curl setting's value, quoted as its config files want.
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A Web API call.
fn web<T: DeserializeOwned>(
    method: &str,
    account: &Account,
    query: &[(&str, String)],
) -> anyhow::Result<T> {
    let mut url = format!(
        "{SITE}/API/{method}.php?y={}",
        encode(account.api_key.trim())
    );
    for (k, v) in query {
        url.push_str(&format!("&{k}={}", encode(v)));
    }
    let (status, body) = curl(&format!("url = {}\n", quoted(&url)))?;
    match status {
        200..=299 => {}
        401 | 403 => anyhow::bail!("RetroAchievements did not accept the Web API key"),
        429 => anyhow::bail!("RetroAchievements asks to slow down; try again in a minute"),
        s => anyhow::bail!("RetroAchievements answered {s}"),
    }
    serde_json::from_slice(&body)
        .with_context(|| format!("RetroAchievements sent an unexpected answer for {method}"))
}

/// Checks the username and key with RetroAchievements, then keeps them.
pub fn login(username: &str, api_key: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !username.trim().is_empty(),
        "Enter your RetroAchievements username"
    );
    anyhow::ensure!(
        is_api_key(api_key),
        "That is not a RetroAchievements Web API key: it has 32 letters and digits and is shown in the site's settings"
    );
    let account = Account {
        username: username.trim().to_string(),
        api_key: api_key.trim().to_string(),
    };
    let profile: UserProfile = web(
        "API_GetUserProfile",
        &account,
        &[("u", account.username.clone())],
    )?;
    anyhow::ensure!(
        !profile.user.is_empty(),
        "There is no user named {}",
        account.username
    );
    let account = Account {
        username: profile.user.clone(),
        ..account
    };
    crate::shops::vault::write_secret(
        &account_file(),
        "arkor-retroachievements",
        &serde_json::to_vec(&account)?,
    )?;
    Ok(profile.user)
}

/// Forgets the account and what was read with it.
pub fn logout() {
    crate::shops::vault::remove_secret(&account_file());
    let _ = std::fs::remove_dir_all(dir());
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct HashEntry {
    size: u64,
    modified: u64,
    hash: String,
}

/// The RetroAchievements hash of `file` for `system`, from the cache when unchanged.
fn hash_of(system: &str, file: &Path) -> Option<String> {
    let console = console_for(system)?;
    let meta = std::fs::metadata(file).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let cache = dir().join("hashes.json");
    let key = file.to_string_lossy().to_string();
    let mut all: HashMap<String, HashEntry> = super::read_json(&cache);
    if let Some(e) = all.get(&key)
        && e.size == meta.len()
        && e.modified == modified
    {
        return Some(e.hash.clone());
    }
    let hash = hash::hash_file(console.method, file).ok()?;
    all.insert(
        key,
        HashEntry {
            size: meta.len(),
            modified,
            hash: hash.clone(),
        },
    );
    let _ = super::write_json(&cache, &all);
    Some(hash)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct GameIndex {
    at: u64,
    games: BTreeMap<String, u64>,
}

/// The RetroAchievements game of a hash, from the console's list (read once a week).
fn game_id(account: &Account, console: u32, hash: &str) -> anyhow::Result<Option<u64>> {
    let path = dir().join(format!("games-{console}.json"));
    let cached: Option<GameIndex> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    if let Some(i) = cached
        .as_ref()
        .filter(|i| now().saturating_sub(i.at) < GAMES_MAX_AGE)
    {
        return Ok(i.games.get(hash).copied());
    }
    let list: Vec<GameEntry> = match web(
        "API_GetGameList",
        account,
        &[
            ("i", console.to_string()),
            ("f", "1".into()),
            ("h", "1".into()),
        ],
    ) {
        Ok(l) => l,
        Err(e) => return cached.map(|i| i.games.get(hash).copied()).ok_or(e),
    };
    let index = GameIndex {
        at: now(),
        games: model::hash_index(&list),
    };
    let found = index.games.get(hash).copied();
    let _ = super::write_json(&path, &index);
    Ok(found)
}

/// One achievement, as shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Achievement {
    pub title: String,
    pub description: String,
    pub points: u64,
    pub badge: String,
    pub unlocked: bool,
    /// Unlocked in hardcore mode.
    pub hardcore: bool,
    /// How many players have it, in per mille of the game's players.
    pub rarity: u64,
}

/// A game's achievements and its RetroAchievements page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameAchievements {
    pub game: u64,
    pub title: String,
    pub list: Vec<Achievement>,
}

impl GameAchievements {
    pub fn unlocked(&self) -> usize {
        self.list.iter().filter(|a| a.unlocked).count()
    }

    pub fn page(&self) -> String {
        format!("{SITE}/game/{}", self.game)
    }
}

fn map_progress(p: &GameProgress) -> GameAchievements {
    let players = p.num_distinct_players.max(1);
    GameAchievements {
        game: p.id,
        title: p.title.clone(),
        list: p
            .achievements
            .iter()
            .map(|a| Achievement {
                title: a.title.clone(),
                description: a.description.clone(),
                points: a.points,
                badge: a.badge_name.clone(),
                unlocked: a
                    .date_earned
                    .as_deref()
                    .is_some_and(|d| !d.trim().is_empty())
                    || a.date_earned_hardcore
                        .as_deref()
                        .is_some_and(|d| !d.trim().is_empty()),
                hardcore: a
                    .date_earned_hardcore
                    .as_deref()
                    .is_some_and(|d| !d.trim().is_empty()),
                rarity: a.num_awarded * 1000 / players,
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Progress {
    at: u64,
    of: GameAchievements,
}

/// Achievements of the game in `file` of `system`: `None` without an account, for a system
/// RetroAchievements does not cover, or a file it does not know. Blocking (hashing the
/// file, a request or two).
pub fn achievements(system: &str, file: &Path) -> anyhow::Result<Option<GameAchievements>> {
    let Some(account) = account() else {
        return Ok(None);
    };
    let Some(console) = console_for(system) else {
        return Ok(None);
    };
    let Some(hash) = hash_of(system, file) else {
        return Ok(None);
    };
    let cache = dir().join("progress").join(format!("{hash}.json"));
    let cached: Option<Progress> = std::fs::read_to_string(&cache)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    if let Some(c) = cached
        .as_ref()
        .filter(|c| now().saturating_sub(c.at) < PROGRESS_MAX_AGE)
    {
        return Ok(Some(c.of.clone()));
    }
    let Some(game) = game_id(&account, console.id, &hash)? else {
        return Ok(None);
    };
    let fetched: anyhow::Result<GameProgress> = web(
        "API_GetGameInfoAndUserProgress",
        &account,
        &[
            ("g", game.to_string()),
            ("u", account.username.clone()),
            ("a", "1".into()),
        ],
    );
    match fetched {
        Ok(p) => {
            let of = map_progress(&p);
            let _ = super::write_json(
                &cache,
                &Progress {
                    at: now(),
                    of: of.clone(),
                },
            );
            Ok(Some(of))
        }
        Err(e) => cached.map(|c| Some(c.of)).ok_or(e),
    }
}

/// The emulators' sign-in: the token RetroAchievements gives for the password. The password
/// goes in the request body, through curl's standard input.
fn connect_login(username: &str, password: &str) -> anyhow::Result<ConnectLogin> {
    let config = format!(
        "url = {}\ndata = {}\n",
        quoted(&format!("{SITE}/dorequest.php")),
        quoted(&format!(
            "r=login2&u={}&p={}",
            encode(username.trim()),
            encode(password)
        )),
    );
    let (status, body) = curl(&config)?;
    let login: ConnectLogin = serde_json::from_slice(&body)
        .map_err(|_| anyhow::anyhow!("RetroAchievements answered {status}"))?;
    if !login.success || login.token.is_empty() {
        anyhow::bail!(
            "{}",
            login
                .error
                .clone()
                .unwrap_or_else(|| "the username or password is wrong".into())
        );
    }
    Ok(login)
}

/// Signs the installed emulators that support it in to RetroAchievements with `password`
/// (used once for the emulators' token, never kept); each emulator's name and what
/// happened. Blocking.
pub fn sign_in_emulators(
    password: &str,
    hardcore: bool,
) -> anyhow::Result<Vec<(String, anyhow::Result<cheevos::Outcome>)>> {
    let account = account().context("Sign in to RetroAchievements first")?;
    let login = connect_login(&account.username, password)?;
    let login = cheevos::Login {
        username: login.user,
        token: login.token,
        hardcore,
    };
    let roots = cheevos::Roots::current();
    let mut out = Vec::new();
    for id in cheevos::SUPPORTED {
        let Some(m) = super::engine(id) else { continue };
        // RetroArch reads the file Arkor gives it, whichever RetroArch it is.
        let install = super::discovery::find(&m).into_iter().next();
        let install = match (install, id) {
            (Some(i), _) => i,
            (None, "retroarch") => super::discovery::Installation {
                engine: id.into(),
                origin: super::discovery::Origin::Managed,
                version: None,
                program: PathBuf::new(),
                flatpak: None,
                dir: None,
            },
            (None, _) => continue,
        };
        out.push((
            m.name.clone(),
            cheevos::sign_in(id, &install, &login, &roots),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_api_keys_and_quotes_curl_settings() {
        assert!(is_api_key("AbCdEf0123456789AbCdEf0123456789"));
        assert!(!is_api_key("short"));
        assert_eq!(quoted("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(encode("a b&c"), "a%20b%26c");
    }

    #[test]
    fn maps_progress() {
        let json = r#"{"ID": 7, "Title": "Game", "NumDistinctPlayers": 200,
            "Achievements": {"2": {"ID": 2, "Title": "B", "Points": 5, "DisplayOrder": 2, "NumAwarded": 50},
                             "1": {"ID": 1, "Title": "A", "Points": 10, "DisplayOrder": 1, "NumAwarded": 100,
                                   "DateEarned": "2024-01-01 10:00:00", "DateEarnedHardcore": "2024-01-01 10:00:00"}}}"#;
        let p: GameProgress = serde_json::from_str(json).unwrap();
        let g = map_progress(&p);
        assert_eq!(g.list[0].title, "A");
        assert!(g.list[0].unlocked && g.list[0].hardcore);
        assert_eq!(g.list[0].rarity, 500);
        assert!(!g.list[1].unlocked);
        assert_eq!(g.unlocked(), 1);
        assert_eq!(g.page(), "https://retroachievements.org/game/7");
    }
}
