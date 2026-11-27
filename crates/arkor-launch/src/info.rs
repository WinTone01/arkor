//! What the game page shows about a game besides its settings: description, genres, how it
//! is played (single-player, co-op…), developer, release date, age ratings, screenshots and
//! trailers.
//!
//! From, in order:
//! - IGDB, when the user gave their own Twitch application (client id and secret);
//! - Steam's public store API (`appdetails`): a Steam game by its app id, any other game by
//!   the store search, only when a result has exactly the game's name (folded);
//! - VNDB's public API for visual novels, again only an exact title.
//!
//! A wrong match is worse than none, so nearest guesses are never taken. Answers are kept in
//! `~/.cache/arkor/info/` (30 days, a miss 7 days). Requests go through curl; IGDB's secret
//! is given on curl's standard input, never on a command line.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::Context;
use arkor_core::library::{GameKind, LibraryGame};
use serde::{Deserialize, Serialize};

const FOUND_TTL: u64 = 30 * 24 * 3600;
const MISSING_TTL: u64 = 7 * 24 * 3600;

/// A screenshot: its small picture and the full one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shot {
    pub thumb: String,
    pub full: String,
}

/// A trailer: its name, picture and where it plays (a web page or a stream).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trailer {
    pub name: String,
    pub thumb: String,
    pub url: String,
}

/// What is known about a game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameInfo {
    /// `Steam`, `IGDB`, `VNDB`.
    pub source: String,
    /// The source's page of the game.
    pub link: String,
    pub description: String,
    pub genres: Vec<String>,
    /// How it is played: `Single-player`, `Co-op`, `Online PvP`…
    pub modes: Vec<String>,
    pub developers: Vec<String>,
    /// As the source writes it (`24 Feb, 2017`, `2017-02-24`).
    pub release: String,
    /// `ESRB E10+`, `PEGI 12`, `USK 6`…
    pub ages: Vec<String>,
    pub shots: Vec<Shot>,
    pub trailers: Vec<Trailer>,
    /// Critics' score out of 100.
    pub score: Option<u32>,
    /// For a visual novel: how long it takes, in hours.
    pub hours: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Cached {
    at: u64,
    info: Option<GameInfo>,
}

fn dir() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("arkor/info"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What is kept about the game `id`, while it is fresh: `Some(None)` = looked up, nothing.
pub fn cached(id: &str) -> Option<Option<GameInfo>> {
    let path = dir()?.join(format!("{id}.json"));
    let c: Cached = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let ttl = if c.info.is_some() {
        FOUND_TTL
    } else {
        MISSING_TTL
    };
    (now().saturating_sub(c.at) < ttl).then_some(c.info)
}

/// Forgets what was kept about `id` (to look again).
pub fn forget(id: &str) {
    if let Some(d) = dir() {
        let _ = std::fs::remove_file(d.join(format!("{id}.json")));
    }
}

/// Letters and digits only, lower case, Roman numerals II–X as digits, trademark signs out:
/// "DOOM II: Hell on Earth" and "Doom 2 - Hell on Earth" compare equal.
pub fn fold(title: &str) -> String {
    let lower = title.to_lowercase();
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| match w {
            "ii" => "2",
            "iii" => "3",
            "iv" => "4",
            "v" => "5",
            "vi" => "6",
            "vii" => "7",
            "viii" => "8",
            "ix" => "9",
            "x" => "10",
            w => w,
        })
        .collect()
}

/// Runs curl with `config` (its settings, one per line) on standard input; the body.
fn curl(config: &str) -> anyhow::Result<String> {
    let mut child = Command::new("curl")
        .args(["-fsS", "--max-time", "20", "-A", "arkor", "-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running curl")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(config.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    anyhow::ensure!(
        out.status.success(),
        "request failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

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

fn get(url: &str) -> anyhow::Result<String> {
    curl(&format!("url = {}\n", quoted(url)))
}

/// Text with HTML tags taken out and its entities read.
fn plain(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.replace("<br>", "\n").replace("<br />", "\n").chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .trim()
        .to_string()
}

// ---- Steam ----

/// A Steam app of exactly this name, from the store search.
fn steam_search(name: &str) -> anyhow::Result<Option<u64>> {
    let body = get(&format!(
        "https://store.steampowered.com/api/storesearch/?term={}&cc=us&l=english",
        encode(name)
    ))?;
    let v: serde_json::Value = serde_json::from_str(&body)?;
    let wanted = fold(name);
    Ok(v["items"].as_array().and_then(|items| {
        items
            .iter()
            .find(|i| i["type"] == "app" && i["name"].as_str().is_some_and(|n| fold(n) == wanted))
            .and_then(|i| i["id"].as_u64())
    }))
}

/// Steam's age ratings as `ESRB E10+`, `USK 6`, `PEGI 12`.
fn steam_ages(ratings: &serde_json::Value) -> Vec<String> {
    let Some(map) = ratings.as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (org, r) in map {
        let Some(rating) = r["rating"].as_str() else {
            continue;
        };
        let name = match org.as_str() {
            "esrb" => "ESRB",
            "pegi" => "PEGI",
            "usk" | "steam_germany" => "USK",
            "cero" => "CERO",
            "dejus" => "ClassInd",
            "kgrb" => "GRAC",
            "oflc" => "ACB",
            _ => continue,
        };
        let rating = if name == "ESRB" {
            match rating {
                "e" => "E".into(),
                "e10" => "E10+".into(),
                "t" => "T".into(),
                "m" => "M".into(),
                "ao" => "AO".into(),
                other => other.to_uppercase(),
            }
        } else {
            rating.to_uppercase()
        };
        let label = format!("{name} {rating}");
        if !out.contains(&label) {
            out.push(label);
        }
    }
    out
}

/// Steam's store page data of `appid`.
pub fn steam(appid: u64) -> anyhow::Result<Option<GameInfo>> {
    let body = get(&format!(
        "https://store.steampowered.com/api/appdetails?appids={appid}&l=english"
    ))?;
    let v: serde_json::Value = serde_json::from_str(&body)?;
    let d = &v[appid.to_string()];
    if d["success"] != true {
        return Ok(None);
    }
    Ok(Some(steam_info(appid, &d["data"])))
}

pub(crate) fn steam_info(appid: u64, d: &serde_json::Value) -> GameInfo {
    let names = |v: &serde_json::Value, key: &str| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x[key].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let wanted_modes = [
        "Single-player",
        "Multi-player",
        "Co-op",
        "Online Co-op",
        "LAN Co-op",
        "Shared/Split Screen",
        "Shared/Split Screen Co-op",
        "Online PvP",
        "PvP",
    ];
    let modes = names(&d["categories"], "description")
        .into_iter()
        .filter(|m| wanted_modes.contains(&m.as_str()))
        .collect();
    let strings = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    GameInfo {
        source: "Steam".into(),
        link: format!("https://store.steampowered.com/app/{appid}"),
        description: plain(d["short_description"].as_str().unwrap_or_default()),
        genres: names(&d["genres"], "description"),
        modes,
        developers: strings(&d["developers"]),
        release: d["release_date"]["date"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        ages: steam_ages(&d["ratings"]),
        shots: d["screenshots"]
            .as_array()
            .map(|a| {
                a.iter()
                    .take(12)
                    .filter_map(|s| {
                        Some(Shot {
                            thumb: s["path_thumbnail"].as_str()?.to_string(),
                            full: s["path_full"].as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        trailers: d["movies"]
            .as_array()
            .map(|a| {
                a.iter()
                    .take(4)
                    .filter_map(|m| {
                        Some(Trailer {
                            name: m["name"].as_str()?.to_string(),
                            thumb: m["thumbnail"].as_str().unwrap_or_default().to_string(),
                            url: m["hls_h264"]
                                .as_str()
                                .or(m["mp4"]["max"].as_str())
                                .or(m["webm"]["max"].as_str())?
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        score: d["metacritic"]["score"].as_u64().map(|s| s as u32),
        hours: None,
    }
}

// ---- VNDB ----

/// A visual novel of exactly this title, from VNDB (descriptions of explicit images left
/// out, spoiler tags too).
pub fn vndb(name: &str) -> anyhow::Result<Option<GameInfo>> {
    let body = serde_json::json!({
        "filters": ["search", "=", name],
        "fields": "id,title,alttitle,released,length_minutes,rating,description,developers.name,screenshots.thumbnail,screenshots.url,screenshots.sexual,screenshots.violence,tags.name,tags.spoiler,tags.category",
        "results": 5
    });
    let answer = curl(&format!(
        "url = {}\nrequest = \"POST\"\nheader = \"Content-Type: application/json\"\ndata = {}\n",
        quoted("https://api.vndb.org/kana/vn"),
        quoted(&body.to_string())
    ))?;
    let v: serde_json::Value = serde_json::from_str(&answer)?;
    let wanted = fold(name);
    let Some(vn) = v["results"].as_array().and_then(|r| {
        r.iter().find(|vn| {
            [&vn["title"], &vn["alttitle"]]
                .iter()
                .any(|t| t.as_str().is_some_and(|t| fold(t) == wanted))
        })
    }) else {
        return Ok(None);
    };
    Ok(Some(vndb_info(vn)))
}

pub(crate) fn vndb_info(vn: &serde_json::Value) -> GameInfo {
    let id = vn["id"].as_str().unwrap_or_default();
    // VNDB's markup: [url=…]text[/url], [b]…[/b], [spoiler]…[/spoiler].
    let raw = vn["description"].as_str().unwrap_or_default();
    let mut desc = String::new();
    let mut depth = 0;
    let mut rest = raw;
    while let Some(open) = rest.find('[') {
        if depth == 0 {
            desc.push_str(&rest[..open]);
        }
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        let tag = &rest[open + 1..open + close];
        if tag == "spoiler" {
            depth += 1;
        } else if tag == "/spoiler" {
            depth = (depth - 1).max(0);
        }
        rest = &rest[open + close + 1..];
    }
    if depth == 0 {
        desc.push_str(rest);
    }
    GameInfo {
        source: "VNDB".into(),
        link: format!("https://vndb.org/{id}"),
        description: desc.trim().to_string(),
        genres: vn["tags"]
            .as_array()
            .map(|t| {
                t.iter()
                    .filter(|t| t["spoiler"].as_u64() == Some(0) && t["category"] == "cont")
                    .filter_map(|t| t["name"].as_str().map(str::to_string))
                    .take(6)
                    .collect()
            })
            .unwrap_or_default(),
        modes: vec!["Visual novel".into()],
        developers: vn["developers"]
            .as_array()
            .map(|d| {
                d.iter()
                    .filter_map(|d| d["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        release: vn["released"].as_str().unwrap_or_default().to_string(),
        ages: Vec::new(),
        // Only screenshots VNDB rates safe on both counts.
        shots: vn["screenshots"]
            .as_array()
            .map(|s| {
                s.iter()
                    .filter(|s| {
                        s["sexual"].as_f64().unwrap_or(2.0) < 0.5
                            && s["violence"].as_f64().unwrap_or(2.0) < 0.5
                    })
                    .take(12)
                    .filter_map(|s| {
                        Some(Shot {
                            thumb: s["thumbnail"].as_str()?.to_string(),
                            full: s["url"].as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        trailers: Vec::new(),
        score: vn["rating"].as_f64().map(|r| r.round() as u32),
        hours: vn["length_minutes"]
            .as_u64()
            .map(|m| ((m + 30) / 60) as u32),
    }
}

// ---- IGDB ----

/// The user's own Twitch application for IGDB.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct IgdbAccount {
    pub client_id: String,
    pub client_secret: String,
}

fn igdb_file() -> Option<PathBuf> {
    arkor_core::paths::config_dir()
        .ok()
        .map(|d| d.join("igdb.json"))
}

/// The saved IGDB application, if any.
pub fn igdb_account() -> Option<IgdbAccount> {
    let bytes = crate::shops::vault::read_secret(&igdb_file()?, "arkor-igdb")?;
    serde_json::from_slice(&bytes).ok()
}

/// Checks an IGDB application (asks Twitch for a token) and keeps it, sealed.
pub fn igdb_login(client_id: &str, client_secret: &str) -> anyhow::Result<()> {
    let account = IgdbAccount {
        client_id: client_id.trim().to_string(),
        client_secret: client_secret.trim().to_string(),
    };
    anyhow::ensure!(
        !account.client_id.is_empty() && !account.client_secret.is_empty(),
        "Enter the client ID and the client secret of your Twitch application"
    );
    igdb_token(&account).context("Twitch did not accept the client ID and secret")?;
    crate::shops::vault::write_secret(
        &igdb_file().context("no config folder")?,
        "arkor-igdb",
        &serde_json::to_vec(&account)?,
    )
}

pub fn igdb_logout() {
    if let Some(f) = igdb_file() {
        crate::shops::vault::remove_secret(&f);
    }
}

fn igdb_token(a: &IgdbAccount) -> anyhow::Result<String> {
    let body = curl(&format!(
        "url = {}\nrequest = \"POST\"\ndata = {}\n",
        quoted("https://id.twitch.tv/oauth2/token"),
        quoted(&format!(
            "client_id={}&client_secret={}&grant_type=client_credentials",
            encode(&a.client_id),
            encode(&a.client_secret)
        ))
    ))?;
    let v: serde_json::Value = serde_json::from_str(&body)?;
    v["access_token"]
        .as_str()
        .map(str::to_string)
        .context("no token from Twitch")
}

const IGDB_FIELDS: &str = "name,summary,first_release_date,genres.name,game_modes.name,involved_companies.company.name,involved_companies.developer,screenshots.image_id,videos.video_id,videos.name,url,age_ratings.organization.name,age_ratings.rating_category.rating,aggregated_rating";

/// The IGDB game of exactly this name.
pub fn igdb(account: &IgdbAccount, name: &str) -> anyhow::Result<Option<GameInfo>> {
    let token = igdb_token(account)?;
    let query = format!(
        "search \"{}\"; fields {IGDB_FIELDS}; limit 10;",
        name.replace('\\', "").replace('"', "")
    );
    let body = curl(&format!(
        "url = {}\nrequest = \"POST\"\nheader = {}\nheader = {}\ndata = {}\n",
        quoted("https://api.igdb.com/v4/games"),
        quoted(&format!("Client-ID: {}", account.client_id)),
        quoted(&format!("Authorization: Bearer {token}")),
        quoted(&query)
    ))?;
    let v: serde_json::Value = serde_json::from_str(&body)?;
    let wanted = fold(name);
    Ok(v.as_array()
        .and_then(|a| {
            a.iter()
                .find(|g| g["name"].as_str().is_some_and(|n| fold(n) == wanted))
        })
        .map(igdb_info))
}

pub(crate) fn igdb_info(g: &serde_json::Value) -> GameInfo {
    let names = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let release = g["first_release_date"]
        .as_i64()
        .map(|t| {
            let days = t.div_euclid(86_400);
            // Civil date from days since 1970 (Howard Hinnant's algorithm).
            let z = days + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = yoe + era * 400 + i64::from(m <= 2);
            format!("{y:04}-{m:02}-{d:02}")
        })
        .unwrap_or_default();
    let image = |id: &str, size: &str| {
        format!("https://images.igdb.com/igdb/image/upload/t_{size}/{id}.jpg")
    };
    GameInfo {
        source: "IGDB".into(),
        link: g["url"].as_str().unwrap_or_default().to_string(),
        description: g["summary"].as_str().unwrap_or_default().to_string(),
        genres: names(&g["genres"]),
        modes: names(&g["game_modes"]),
        developers: g["involved_companies"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|c| c["developer"] == true)
                    .filter_map(|c| c["company"]["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        release,
        ages: g["age_ratings"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|r| {
                        Some(format!(
                            "{} {}",
                            r["organization"]["name"].as_str()?,
                            r["rating_category"]["rating"].as_str()?
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        shots: g["screenshots"]
            .as_array()
            .map(|a| {
                a.iter()
                    .take(12)
                    .filter_map(|s| {
                        let id = s["image_id"].as_str()?;
                        Some(Shot {
                            thumb: image(id, "screenshot_med"),
                            full: image(id, "1080p"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        trailers: g["videos"]
            .as_array()
            .map(|a| {
                a.iter()
                    .take(4)
                    .filter_map(|v| {
                        let id = v["video_id"].as_str()?;
                        Some(Trailer {
                            name: v["name"].as_str().unwrap_or("Trailer").to_string(),
                            thumb: format!("https://img.youtube.com/vi/{id}/mqdefault.jpg"),
                            url: format!("https://www.youtube.com/watch?v={id}"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        score: g["aggregated_rating"].as_f64().map(|r| r.round() as u32),
        hours: None,
    }
}

/// The small pictures of `info`'s screenshots and trailers, downloaded into the cache
/// (`<cache>/info/<id>/`), for the game page: (picture file, full-size or trailer address).
/// Pictures already there are not fetched again. Blocking.
pub fn pictures(id: &str, info: &GameInfo) -> Vec<(PathBuf, String, bool)> {
    let Some(base) = dir().map(|d| d.join(id)) else {
        return Vec::new();
    };
    let _ = std::fs::create_dir_all(&base);
    let mut out = Vec::new();
    let all = info
        .trailers
        .iter()
        .map(|t| (t.thumb.as_str(), t.url.as_str(), true))
        .chain(
            info.shots
                .iter()
                .map(|s| (s.thumb.as_str(), s.full.as_str(), false)),
        );
    for (n, (thumb, target, trailer)) in all.take(10).enumerate() {
        if thumb.is_empty() {
            continue;
        }
        let file = base.join(format!("{n}.jpg"));
        if !file.is_file() && get_to(thumb, &file).is_err() {
            continue;
        }
        out.push((file, target.to_string(), trailer));
    }
    out
}

fn get_to(url: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let tmp = dest.with_extension("part");
    curl(&format!(
        "url = {}
output = {}
",
        quoted(url),
        quoted(&tmp.display().to_string())
    ))?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// Looks the game up (IGDB, Steam, VNDB) and keeps the answer. Blocking (a few requests).
pub fn lookup(game: &LibraryGame) -> anyhow::Result<Option<GameInfo>> {
    if let Some(c) = cached(&game.id) {
        return Ok(c);
    }
    let mut found = None;
    if let Some(account) = igdb_account() {
        found = igdb(&account, &game.name).unwrap_or_else(|e| {
            eprintln!("arkor: IGDB: {e:#}");
            None
        });
    }
    if found.is_none() {
        let appid = match &game.kind {
            GameKind::Steam { appid } => appid.parse().ok(),
            _ => steam_search(&game.name)?,
        };
        if let Some(appid) = appid {
            found = steam(appid)?;
        }
    }
    if found.is_none() {
        found = vndb(&game.name).unwrap_or(None);
    }
    if let Some(d) = dir() {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(
            d.join(format!("{}.json", game.id)),
            serde_json::to_vec(&Cached {
                at: now(),
                info: found.clone(),
            })?,
        );
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_titles() {
        assert_eq!(
            fold("DOOM II: Hell on Earth"),
            fold("Doom 2 - Hell on Earth")
        );
        assert_ne!(fold("Hollow Knight"), fold("Hollow Knight: Silksong"));
    }

    #[test]
    fn reads_steam_store_data() {
        let d = serde_json::json!({
            "short_description": "Forge your own path in &quot;Hallownest&quot;.<br>",
            "genres": [{"description": "Action"}, {"description": "Indie"}],
            "categories": [{"description": "Single-player"}, {"description": "Steam Trading Cards"}],
            "developers": ["Team Cherry"],
            "release_date": {"date": "24 Feb, 2017"},
            "ratings": {"esrb": {"rating": "e10"}, "steam_germany": {"rating": "6"}},
            "screenshots": [{"path_thumbnail": "t", "path_full": "f"}],
            "movies": [{"name": "Trailer", "thumbnail": "m", "hls_h264": "h"}],
            "metacritic": {"score": 87}
        });
        let i = steam_info(367520, &d);
        assert_eq!(i.description, "Forge your own path in \"Hallownest\".");
        assert_eq!(i.modes, ["Single-player"]);
        assert_eq!(i.ages, ["ESRB E10+", "USK 6"]);
        assert_eq!(i.trailers[0].url, "h");
        assert_eq!(i.score, Some(87));
    }

    #[test]
    fn reads_vndb_without_spoilers_or_unsafe_pictures() {
        let vn = serde_json::json!({
            "id": "v2002", "title": "Steins;Gate",
            "description": "A [b]story[/b]. [spoiler]Secret[/spoiler]End.",
            "tags": [{"name": "Time Travel", "spoiler": 0, "category": "cont"},
                     {"name": "Twist", "spoiler": 2, "category": "cont"}],
            "screenshots": [{"thumbnail": "a", "url": "A", "sexual": 0.0, "violence": 0.0},
                            {"thumbnail": "b", "url": "B", "sexual": 1.5, "violence": 0.0}],
            "length_minutes": 3000, "rating": 89.4
        });
        let i = vndb_info(&vn);
        assert!(!i.description.contains("Secret"));
        assert_eq!(i.genres, ["Time Travel"]);
        assert_eq!(i.shots.len(), 1);
        assert_eq!(i.hours, Some(50));
    }

    #[test]
    fn reads_igdb_games() {
        let g = serde_json::json!({
            "name": "Hades", "summary": "Defy the god of the dead.",
            "first_release_date": 1600387200,
            "genres": [{"name": "Role-playing (RPG)"}],
            "game_modes": [{"name": "Single player"}],
            "involved_companies": [{"developer": true, "company": {"name": "Supergiant Games"}},
                                   {"developer": false, "company": {"name": "Other"}}],
            "age_ratings": [{"organization": {"name": "PEGI"}, "rating_category": {"rating": "12"}}],
            "videos": [{"video_id": "abc", "name": "Launch"}]
        });
        let i = igdb_info(&g);
        assert_eq!(i.release, "2020-09-18");
        assert_eq!(i.developers, ["Supergiant Games"]);
        assert_eq!(i.ages, ["PEGI 12"]);
        assert_eq!(i.trailers[0].url, "https://www.youtube.com/watch?v=abc");
    }
}
