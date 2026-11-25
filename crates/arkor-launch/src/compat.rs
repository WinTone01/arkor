//! Whether a game runs on Linux, as Heroic tells it: its anti-cheat from AreWeAntiCheatYet,
//! its ProtonDB rating and Valve's Steam Deck verdict (the last two by its Steam app id,
//! which GOG's games database knows for games of other stores). Answers are cached.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const ANTICHEAT_URL: &str =
    "https://raw.githubusercontent.com/Starz0r/AreWeAntiCheatYet/HEAD/games.json";
const DAY: Duration = Duration::from_secs(86_400);

/// What AreWeAntiCheatYet says about a game's anti-cheat on Linux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AntiCheat {
    /// The developers made it work.
    Supported,
    /// Works, without the developers saying so.
    Running,
    /// Does not work (yet).
    Broken,
    /// The developers keep Linux out on purpose.
    Denied,
    Planned,
}

/// Valve's Steam Deck verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Deck {
    Verified,
    Playable,
    Unsupported,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Compat {
    pub anticheat: Option<AntiCheat>,
    /// The anti-cheats it uses, e.g. "Easy Anti-Cheat".
    pub anticheats: Vec<String>,
    /// ProtonDB's rating: "platinum", "gold", "silver", "bronze", "borked", "native".
    pub protondb: Option<String>,
    pub deck: Option<Deck>,
    pub steam_appid: Option<String>,
}

impl Compat {
    /// Multiplayer will not work, or the game not at all: worth a warning before installing.
    pub fn warns(&self) -> bool {
        matches!(self.anticheat, Some(AntiCheat::Broken | AntiCheat::Denied))
            || self.protondb.as_deref() == Some("borked")
    }
}

fn cache_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("no cache dir")?
        .join("arkor/compat");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn fresh(path: &std::path::Path, age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|a| a < age))
}

/// AreWeAntiCheatYet's list, fetched once a day.
fn anticheat_list() -> anyhow::Result<Vec<Value>> {
    let path = cache_dir()?.join("areweanticheatyet.json");
    if !fresh(&path, DAY) {
        let text = crate::tools::http_get(ANTICHEAT_URL)?;
        std::fs::write(&path, &text)?;
    }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text).context("unexpected AreWeAntiCheatYet list")
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The entry for a game: by its Epic namespace, its Steam app id, else its exact name.
pub(crate) fn find_anticheat<'a>(
    list: &'a [Value],
    epic_namespace: Option<&str>,
    steam_appid: Option<&str>,
    name: &str,
) -> Option<&'a Value> {
    let by_store = list.iter().find(|g| {
        let ids = &g["storeIds"];
        epic_namespace.is_some_and(|n| ids["epic"]["namespace"].as_str() == Some(n))
            || steam_appid.is_some_and(|a| ids["steam"].as_str() == Some(a))
    });
    by_store.or_else(|| {
        let wanted = squash(name);
        list.iter()
            .find(|g| g["name"].as_str().is_some_and(|n| squash(n) == wanted))
    })
}

fn anticheat_of(v: &Value) -> (Option<AntiCheat>, Vec<String>) {
    let status = match v["status"].as_str() {
        Some("Supported") => Some(AntiCheat::Supported),
        Some("Running") => Some(AntiCheat::Running),
        Some("Broken") => Some(AntiCheat::Broken),
        Some("Denied") => Some(AntiCheat::Denied),
        Some("Planned") => Some(AntiCheat::Planned),
        _ => None,
    };
    let names = v["anticheats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| n.as_str().map(str::to_string))
        .collect();
    (status, names)
}

/// The Steam app id GOG's games database knows for a game of `platform` (`epic`, `gog`).
fn steam_appid(platform: &str, id: &str) -> Option<String> {
    let safe: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c))
        .collect();
    let text = crate::tools::http_get_quick(&format!(
        "https://gamesdb.gog.com/platforms/{platform}/external_releases/{safe}"
    ))
    .ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v["game"]["releases"]
        .as_array()?
        .iter()
        .find(|r| r["platform_id"] == "steam")
        .and_then(|r| r["external_id"].as_str().map(str::to_string))
}

fn protondb(appid: &str) -> Option<String> {
    let text = crate::tools::http_get_quick(&format!(
        "https://www.protondb.com/api/v1/reports/summaries/{appid}.json"
    ))
    .ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v["tier"].as_str().map(str::to_string)
}

fn deck(appid: &str) -> Option<Deck> {
    let text = crate::tools::http_get_quick(&format!(
        "https://store.steampowered.com/saleaction/ajaxgetdeckappcompatibilityreport?nAppID={appid}"
    ))
    .ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    match v["results"]["resolved_category"].as_u64()? {
        3 => Some(Deck::Verified),
        2 => Some(Deck::Playable),
        1 => Some(Deck::Unsupported),
        _ => None,
    }
}

/// Where a game comes from, to look it up.
#[derive(Debug, Clone)]
pub struct Lookup {
    pub name: String,
    /// `epic`, `gog`, `amazon` or `steam`.
    pub platform: String,
    pub id: String,
    pub epic_namespace: Option<String>,
}

/// What is known about a game on Linux; cached for a week. Blocking (a few requests).
pub fn lookup(game: &Lookup) -> Compat {
    let safe: String = format!("{}-{}", game.platform, game.id)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let file = cache_dir().ok().map(|d| d.join(format!("{safe}.json")));
    if let Some(f) = &file
        && fresh(f, 7 * DAY)
        && let Ok(c) = std::fs::read_to_string(f)
            .map_err(anyhow::Error::from)
            .and_then(|t| Ok(serde_json::from_str::<Compat>(&t)?))
    {
        return c;
    }
    let appid = match game.platform.as_str() {
        "steam" => Some(game.id.clone()),
        "amazon" => None,
        p => steam_appid(p, &game.id),
    };
    let mut c = Compat {
        steam_appid: appid.clone(),
        ..Compat::default()
    };
    if let Ok(list) = anticheat_list()
        && let Some(v) = find_anticheat(
            &list,
            game.epic_namespace.as_deref(),
            appid.as_deref(),
            &game.name,
        )
    {
        (c.anticheat, c.anticheats) = anticheat_of(v);
    }
    if let Some(a) = &appid {
        c.protondb = protondb(a);
        c.deck = deck(a);
    }
    if let Some(f) = file {
        let _ = std::fs::write(f, serde_json::to_vec(&c).unwrap_or_default());
    }
    c
}

/// What was found before, without asking anyone. Reads a file only.
pub fn cached(game: &Lookup) -> Option<Compat> {
    let safe: String = format!("{}-{}", game.platform, game.id)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let text = std::fs::read_to_string(cache_dir().ok()?.join(format!("{safe}.json"))).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_games_by_store_or_name() {
        let list: Vec<Value> = serde_json::from_str(
            r#"[{"name":"Fortnite","status":"Denied","anticheats":["Easy Anti-Cheat"],
                 "storeIds":{"epic":{"namespace":"fn"}}},
                {"name":"Elden Ring","status":"Running","storeIds":{"steam":"1245620"}}]"#,
        )
        .unwrap();
        let f = find_anticheat(&list, Some("fn"), None, "x").unwrap();
        assert_eq!(anticheat_of(f).0, Some(AntiCheat::Denied));
        assert_eq!(anticheat_of(f).1, ["Easy Anti-Cheat"]);
        assert!(find_anticheat(&list, None, Some("1245620"), "x").is_some());
        assert!(find_anticheat(&list, None, None, "ELDEN RING™").is_some());
        assert!(find_anticheat(&list, None, None, "Elden-Ring").is_some());
        let c = Compat {
            anticheat: Some(AntiCheat::Denied),
            ..Compat::default()
        };
        assert!(c.warns());
    }

    /// Network.
    #[test]
    #[ignore]
    fn fortnite_is_denied() {
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: single test, ignored by default.
        unsafe { std::env::set_var("XDG_CACHE_HOME", tmp.path()) };
        let c = lookup(&Lookup {
            name: "Fortnite".into(),
            platform: "epic".into(),
            id: "Fortnite".into(),
            epic_namespace: Some("fn".into()),
        });
        println!("{c:?}");
        assert_eq!(c.anticheat, Some(AntiCheat::Denied));
        let w = lookup(&Lookup {
            name: "The Witcher".into(),
            platform: "gog".into(),
            id: "1207658924".into(),
            epic_namespace: None,
        });
        println!("{w:?}");
        assert_eq!(w.steam_appid.as_deref(), Some("20900"));
    }
}
