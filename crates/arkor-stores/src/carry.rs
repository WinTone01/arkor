//! What another launcher knew about a game, carried over when the game is added to Arkor:
//! favourite, hidden, its category, play time and when it was last played.
//!
//! - Heroic: `store/config.json` (`games.favourites`, `games.hidden`,
//!   `games.customCategories`) and `store/timestamp.json` (`totalPlayed` in minutes,
//!   `lastPlayed`), for its Epic, GOG and sideloaded games.
//! - Lutris: `pga.db` (`playtime` in hours, `lastplayed`, its categories; `favorite` is one).
//! - Faugus Launcher: `games.json` (`playtime` in seconds, `favorite`, when it has them).
//!
//! Only read; nothing of the other launcher changes.

use std::path::PathBuf;

use arkor_core::{GameId, Store};
use serde_json::Value;

/// What another launcher knew.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Carried {
    pub favorite: bool,
    pub hidden: bool,
    pub category: Option<String>,
    pub playtime_secs: u64,
    /// Unix time.
    pub last_played: Option<u64>,
}

impl Carried {
    pub fn is_empty(&self) -> bool {
        *self == Carried::default()
    }
}

/// Seconds since 1970 of an RFC 3339 time (`2026-11-21T22:46:38.147Z`).
fn unix_of(stamp: &str) -> Option<u64> {
    let (date, time) = stamp.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.trim_end_matches('Z').split(':');
    let (h, mi) = (
        t.next()?.parse::<i64>().ok()?,
        t.next()?.parse::<i64>().ok()?,
    );
    let s = t.next()?.split('.').next()?.parse::<i64>().ok()?;
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + s).ok()
}

fn heroic_dirs() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    [
        ".config/heroic",
        ".var/app/com.heroicgameslauncher.hgl/config/heroic",
    ]
    .iter()
    .map(|d| home.join(d))
    .filter(|d| d.is_dir())
    .collect()
}

fn read_json(path: PathBuf) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// Heroic's knowledge of `app_name`, from its config (`config`) and play times (`times`).
pub(crate) fn from_heroic(config: &Value, times: &Value, app_name: &str) -> Carried {
    let games = &config["games"];
    let listed = |key: &str| {
        games[key].as_array().is_some_and(|a| {
            a.iter()
                .any(|g| g["appName"].as_str() == Some(app_name) || g.as_str() == Some(app_name))
        })
    };
    let category = games["customCategories"].as_object().and_then(|cats| {
        cats.iter()
            .find(|(_, ids)| {
                ids.as_array()
                    .is_some_and(|a| a.iter().any(|i| i.as_str() == Some(app_name)))
            })
            .map(|(name, _)| name.clone())
    });
    let t = &times[app_name];
    Carried {
        favorite: listed("favourites"),
        hidden: listed("hidden"),
        category,
        playtime_secs: t["totalPlayed"].as_u64().unwrap_or(0) * 60,
        last_played: t["lastPlayed"].as_str().and_then(unix_of),
    }
}

fn heroic(app_name: &str) -> Carried {
    for dir in heroic_dirs() {
        let c = from_heroic(
            &read_json(dir.join("store/config.json")),
            &read_json(dir.join("store/timestamp.json")),
            app_name,
        );
        if !c.is_empty() {
            return c;
        }
    }
    Carried::default()
}

fn lutris(id: i64) -> Carried {
    for dir in crate::lutris::data_dirs() {
        let Ok(conn) = rusqlite::Connection::open_with_flags(
            dir.join("pga.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) else {
            continue;
        };
        let Ok((hours, last)) = conn.query_row(
            "SELECT playtime, lastplayed FROM games WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, Option<f64>>(0)?, r.get::<_, Option<i64>>(1)?)),
        ) else {
            continue;
        };
        let cats: Vec<String> = conn
            .prepare(
                "SELECT c.name FROM categories c JOIN games_categories gc ON gc.category_id = c.id
                 WHERE gc.game_id = ?1",
            )
            .and_then(|mut s| {
                s.query_map([id], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap_or_default();
        return Carried {
            favorite: cats.iter().any(|c| c == "favorite"),
            hidden: cats.iter().any(|c| c == ".hidden"),
            category: cats
                .into_iter()
                .find(|c| c != "favorite" && !c.starts_with('.')),
            playtime_secs: hours.map_or(0, |h| (h * 3600.0) as u64),
            last_played: last.and_then(|t| u64::try_from(t).ok()).filter(|t| *t > 0),
        };
    }
    Carried::default()
}

fn faugus(gameid: &str) -> Carried {
    let Some(path) = dirs::data_dir().map(|d| d.join("faugus-launcher/games.json")) else {
        return Carried::default();
    };
    let v = read_json(path);
    let Some(e) = v
        .as_array()
        .and_then(|a| a.iter().find(|e| e["gameid"].as_str() == Some(gameid)))
    else {
        return Carried::default();
    };
    Carried {
        favorite: e["favorite"].as_bool().unwrap_or(false),
        hidden: false,
        category: None,
        playtime_secs: e["playtime"].as_u64().unwrap_or(0),
        last_played: None,
    }
}

/// Gives the library game `id` what its launcher knew (favourite, hidden, category, play
/// time); the library is saved by the caller. Blocking (reads the other launcher's files).
pub fn carry_over(library: &mut arkor_core::library::Library, id: &str) {
    let Some(g) = library.get_mut(id) else { return };
    let Some(origin) = g.origin.clone() else {
        return;
    };
    let c = carried(&origin);
    if c.is_empty() {
        return;
    }
    g.favorite |= c.favorite;
    g.hidden |= c.hidden;
    if g.category.is_none() {
        g.category = c.category.clone();
    }
    if let Err(e) = arkor_core::stats::Stats::seed(&g.game_id(), c.playtime_secs, c.last_played) {
        eprintln!("arkor: play time not carried over: {e:#}");
    }
}

/// What the launcher a detected game came from (`origin`, its id there) knew about it.
pub fn carried(origin: &str) -> Carried {
    let Ok(id) = origin.parse::<GameId>() else {
        return Carried::default();
    };
    match id.store {
        Store::Heroic | Store::Epic | Store::Gog | Store::Amazon => heroic(&id.id),
        Store::Lutris => id.id.parse().map(lutris).unwrap_or_default(),
        Store::Faugus => faugus(&id.id),
        _ => Carried::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_heroics_favourites_categories_and_times() {
        let config = json!({"games": {
            "favourites": [{"appName": "abc", "title": "A"}],
            "hidden": [{"appName": "zzz"}],
            "customCategories": {"Co-op": ["abc"], "Later": ["zzz"]}
        }});
        let times = json!({"abc": {"totalPlayed": 232, "lastPlayed": "2026-11-21T22:46:38.147Z"}});
        let c = from_heroic(&config, &times, "abc");
        assert!(c.favorite && !c.hidden);
        assert_eq!(c.category.as_deref(), Some("Co-op"));
        assert_eq!(c.playtime_secs, 232 * 60);
        assert_eq!(c.last_played, Some(1_795_301_198));
        assert!(from_heroic(&config, &times, "zzz").hidden);
        assert!(from_heroic(&config, &times, "none").is_empty());
    }
}
