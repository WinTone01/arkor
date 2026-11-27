//! Achievements kept in files on this computer, read without running anything:
//!
//! - Goldberg's Steam emulator (and its fork, gbe_fork), beside a Windows game: the list in
//!   `steam_settings/achievements.json`, what was earned in the prefix's
//!   `AppData/Roaming/GSE Saves/<app id>/achievements.json` (or `Goldberg SteamEmu Saves`).
//! - RPCS3 (PlayStation 3): `dev_hdd0/home/<user>/trophy/<comm id>/`, the list in
//!   `TROPCONF.SFM` and what is unlocked in `TROPUSR.DAT`.
//! - shadPS4 (PlayStation 4): the pack in `<user>/trophy/<comm id>/Xml/TROP.XML` and each
//!   console user's copy with the unlocks, `<user>/home/<id>/trophy/<comm id>.xml` (older
//!   versions: `game_data/<title>/TrophyFiles/trophy00/Xml/TROP.XML`).

use std::path::{Path, PathBuf};

use arkor_core::library::{GameKind, LibraryGame};

pub use crate::shops::gog::Achievement;

/// Where the achievements came from, and the list.
pub fn find(game: &LibraryGame, prefixes_dir: &Path) -> Option<(&'static str, Vec<Achievement>)> {
    match &game.kind {
        GameKind::Windows => goldberg(game, prefixes_dir).map(|a| ("Goldberg", a)),
        GameKind::Emulated { system, .. } if system == "ps3" => {
            rpcs3(&game.exe, &game.name).map(|a| ("RPCS3", a))
        }
        GameKind::Emulated { system, .. } if system == "ps4" => {
            shadps4(&game.exe, &game.name).map(|a| ("shadPS4", a))
        }
        _ => None,
    }
}

// ---- Goldberg ----

/// `steam_settings` folders in and under `dir` (two levels).
fn steam_settings(dir: &Path, depth: u32) -> Option<PathBuf> {
    let here = dir.join("steam_settings");
    if here.join("achievements.json").is_file() {
        return Some(here);
    }
    if depth == 0 {
        return None;
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .find_map(|p| steam_settings(&p, depth - 1))
}

fn text_of(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(m) => m
            .get("english")
            .or_else(|| m.values().next())
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn goldberg_list(defs: &serde_json::Value, earned: &serde_json::Value) -> Vec<Achievement> {
    defs.as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let api = d["name"].as_str()?;
            let hidden = match &d["hidden"] {
                serde_json::Value::String(s) => s == "1",
                serde_json::Value::Number(n) => n.as_u64() == Some(1),
                serde_json::Value::Bool(b) => *b,
                _ => false,
            };
            Some(Achievement {
                name: Some(text_of(&d["displayName"]))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| api.to_string()),
                description: text_of(&d["description"]),
                unlocked: earned[api]["earned"].as_bool().unwrap_or(false),
                rarity: d["percent"]
                    .as_f64()
                    .or_else(|| d["percent"].as_str().and_then(|s| s.parse().ok()))
                    .map(|p| (p * 10.).round() as u32)
                    .unwrap_or(0),
                hidden,
            })
        })
        .collect()
}

fn goldberg(game: &LibraryGame, prefixes_dir: &Path) -> Option<Vec<Achievement>> {
    let dir = game.exe.parent()?;
    let settings =
        steam_settings(dir, 2).or_else(|| dir.parent().and_then(|p| steam_settings(p, 0)))?;
    let defs: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(settings.join("achievements.json")).ok()?)
            .ok()?;
    let appid = [
        settings.join("steam_appid.txt"),
        settings.parent()?.join("steam_appid.txt"),
    ]
    .iter()
    .find_map(|f| std::fs::read_to_string(f).ok())
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());
    let prefix = game.effective_prefix(prefixes_dir);
    let earned = appid
        .and_then(|appid| {
            std::fs::read_dir(prefix.join("drive_c/users"))
                .ok()?
                .flatten()
                .flat_map(|u| {
                    ["GSE Saves", "Goldberg SteamEmu Saves"].map(|s| {
                        u.path()
                            .join("AppData/Roaming")
                            .join(s)
                            .join(&appid)
                            .join("achievements.json")
                    })
                })
                .find_map(|f| std::fs::read_to_string(f).ok())
        })
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null);
    let list = goldberg_list(&defs, &earned);
    (!list.is_empty()).then_some(list)
}

// ---- PlayStation trophy lists (XML) ----

/// The trophies of a `TROPCONF.SFM` / `TROP.XML`: (id, name, detail, hidden, unlocked by the
/// list itself), and the list's title.
fn trophy_xml(xml: &str) -> (String, Vec<(u32, String, String, bool, bool)>) {
    let title = xml
        .split("<title-name>")
        .nth(1)
        .and_then(|r| r.split("</title-name>").next())
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut out = Vec::new();
    for part in xml.split("<trophy ").skip(1) {
        let Some((attrs, body)) = part.split_once('>') else {
            continue;
        };
        let attr = |k: &str| {
            attrs
                .split(&format!("{k}=\""))
                .nth(1)
                .and_then(|r| r.split('"').next())
                .unwrap_or_default()
        };
        let tag = |t: &str| {
            body.split(&format!("<{t}>"))
                .nth(1)
                .and_then(|r| r.split(&format!("</{t}>")).next())
                .unwrap_or_default()
                .replace("&amp;", "&")
                .replace("&apos;", "'")
                .replace("&quot;", "\"")
                .trim()
                .to_string()
        };
        let Ok(id) = attr("id").parse::<u32>() else {
            continue;
        };
        out.push((
            id,
            tag("name"),
            tag("detail"),
            attr("hidden") == "yes",
            matches!(attr("unlockstate"), "true" | "1" | "yes"),
        ));
    }
    (title, out)
}

fn same_title(a: &str, b: &str) -> bool {
    let (a, b) = (
        arkor_core::library::title_key(a),
        arkor_core::library::title_key(b),
    );
    !a.is_empty() && a == b
}

/// Comm ids (`NPWR01234_00`) named in `bytes`.
fn comm_ids(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while let Some(at) = text[i..].find("NPWR") {
        let start = i + at;
        let id = &text[start..(start + 12).min(text.len())];
        let ok = id.len() == 12
            && id[4..9].bytes().all(|c| c.is_ascii_digit())
            && b[start + 9] == b'_'
            && id[10..].bytes().all(|c| c.is_ascii_digit());
        if ok && !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
        i = start + 4;
    }
    out
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

// ---- RPCS3 ----

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The unlocked trophy ids of a `TROPUSR.DAT` (RPCS3's `TROPUSR.h`: table 6 has one
/// 0x60-byte entry per trophy, its id at 0x10 and its state at 0x14).
fn tropusr_unlocked(b: &[u8]) -> Option<Vec<u32>> {
    if be_u32(b, 0)? != 0x818F_54AD {
        return None;
    }
    let tables = be_u32(b, 8)?.min(64) as usize;
    let mut out = Vec::new();
    for t in 0..tables {
        let at = 0x30 + t * 0x20;
        if be_u32(b, at)? != 6 {
            continue;
        }
        let count = be_u32(b, at + 0x0C)?.min(4096) as usize;
        let offset = u64::from_be_bytes(b.get(at + 0x10..at + 0x18)?.try_into().ok()?) as usize;
        for i in 0..count {
            let e = offset.checked_add(i * 0x60)?;
            if be_u32(b, e + 0x14)? == 1 {
                out.push(be_u32(b, e + 0x10)?);
            }
        }
    }
    Some(out)
}

fn rpcs3(rom: &Path, title: &str) -> Option<Vec<Achievement>> {
    let home = dirs::home_dir()?;
    let roots = [
        home.join(".config/rpcs3"),
        home.join(".var/app/net.rpcs3.RPCS3/config/rpcs3"),
    ];
    // The game names its pack in PS3_GAME/TROPDIR.
    let mut wanted = Vec::new();
    for dir in rom.ancestors().take(4) {
        for d in subdirs(&dir.join("PS3_GAME/TROPDIR")) {
            if let Some(n) = d.file_name() {
                wanted.push(n.to_string_lossy().into_owned());
            }
        }
    }
    for root in roots {
        for user in subdirs(&root.join("dev_hdd0/home")) {
            for pack in subdirs(&user.join("trophy")) {
                let Ok(conf) = std::fs::read_to_string(pack.join("TROPCONF.SFM")) else {
                    continue;
                };
                let (list_title, trophies) = trophy_xml(&conf);
                let name = pack.file_name().map(|n| n.to_string_lossy().into_owned());
                let named = name.is_some_and(|n| wanted.contains(&n));
                if !named && !same_title(&list_title, title) {
                    continue;
                }
                let unlocked = std::fs::read(pack.join("TROPUSR.DAT"))
                    .ok()
                    .and_then(|b| tropusr_unlocked(&b))
                    .unwrap_or_default();
                return Some(
                    trophies
                        .into_iter()
                        .map(|(id, name, detail, hidden, _)| Achievement {
                            name,
                            description: detail,
                            unlocked: unlocked.contains(&id),
                            rarity: 0,
                            hidden,
                        })
                        .collect(),
                );
            }
        }
    }
    None
}

// ---- shadPS4 ----

fn shadps4(rom: &Path, title: &str) -> Option<Vec<Achievement>> {
    let home = dirs::home_dir()?;
    let users = [
        dirs::data_dir()?.join("shadPS4"),
        home.join(".var/app/net.shadps4.shadPS4/data/shadPS4"),
    ];
    let game_dir = if rom.is_dir() { rom } else { rom.parent()? };
    let wanted = std::fs::read(game_dir.join("sce_sys/npbind.dat"))
        .map(|b| comm_ids(&b))
        .unwrap_or_default();
    let to_list = |xml: &str| -> Vec<Achievement> {
        trophy_xml(xml)
            .1
            .into_iter()
            .map(|(_, name, detail, hidden, unlocked)| Achievement {
                name,
                description: detail,
                unlocked,
                rarity: 0,
                hidden,
            })
            .collect()
    };
    for user in users.iter().filter(|u| u.is_dir()) {
        // The pack, and each console user's copy with the unlocks.
        for pack in subdirs(&user.join("trophy")) {
            let Some(id) = pack.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let Ok(list) = std::fs::read_to_string(pack.join("Xml/TROP.XML")) else {
                continue;
            };
            if !wanted.contains(&id) && !same_title(&trophy_xml(&list).0, title) {
                continue;
            }
            let copy = subdirs(&user.join("home"))
                .into_iter()
                .map(|h| h.join("trophy").join(format!("{id}.xml")))
                .find_map(|f| std::fs::read_to_string(f).ok());
            return Some(to_list(copy.as_deref().unwrap_or(&list)));
        }
        // Older versions: one list with the unlocks per game.
        for g in subdirs(&user.join("game_data")) {
            let Ok(xml) = std::fs::read_to_string(g.join("TrophyFiles/trophy00/Xml/TROP.XML"))
            else {
                continue;
            };
            let ids = comm_ids(xml.as_bytes());
            if ids.iter().any(|i| wanted.contains(i)) || same_title(&trophy_xml(&xml).0, title) {
                return Some(to_list(&xml));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"<trophyconf version="1.1"><npcommid>NPWR10000_00</npcommid>
<title-name>Bloodborne</title-name>
<trophy id="000" hidden="no" ttype="P" pid="-1"><name>Bloodborne</name><detail>All trophies.</detail></trophy>
<trophy id="001" hidden="yes" ttype="B" pid="000" unlockstate="true" timestamp="1700000000"><name>Yharnam</name><detail>Arrive.</detail></trophy>
</trophyconf>"#;

    #[test]
    fn reads_trophy_lists_and_comm_ids() {
        let (title, t) = trophy_xml(LIST);
        assert_eq!(title, "Bloodborne");
        assert_eq!(t.len(), 2);
        assert_eq!(t[1], (1, "Yharnam".into(), "Arrive.".into(), true, true));
        assert_eq!(
            comm_ids(b"\x00\x01NPWR10000_00\x00NPWR12345_01"),
            ["NPWR10000_00", "NPWR12345_01"]
        );
    }

    #[test]
    fn reads_tropusr() {
        // Header (0x30), one table header of type 6 with 2 entries at 0x50.
        let mut b = vec![0u8; 0x50 + 2 * 0x60];
        b[0..4].copy_from_slice(&0x818F_54ADu32.to_be_bytes());
        b[8..12].copy_from_slice(&1u32.to_be_bytes());
        b[0x30..0x34].copy_from_slice(&6u32.to_be_bytes());
        b[0x3C..0x40].copy_from_slice(&2u32.to_be_bytes());
        b[0x40..0x48].copy_from_slice(&0x50u64.to_be_bytes());
        for (i, (id, state)) in [(0u32, 0u32), (1, 1)].into_iter().enumerate() {
            let e = 0x50 + i * 0x60;
            b[e + 0x10..e + 0x14].copy_from_slice(&id.to_be_bytes());
            b[e + 0x14..e + 0x18].copy_from_slice(&state.to_be_bytes());
        }
        assert_eq!(tropusr_unlocked(&b), Some(vec![1]));
        assert_eq!(tropusr_unlocked(b"nope"), None);
    }

    #[test]
    fn reads_goldberg() {
        let defs = serde_json::json!([
            {"name": "ACH_1", "displayName": {"english": "First steps"}, "description": "Start.", "hidden": "0", "percent": "54.5"},
            {"name": "ACH_2", "displayName": "Secret", "description": "", "hidden": "1"}
        ]);
        let earned = serde_json::json!({"ACH_1": {"earned": true, "earned_time": 1700000000}});
        let l = goldberg_list(&defs, &earned);
        assert_eq!(l.len(), 2);
        assert!(l[0].unlocked && l[0].name == "First steps" && l[0].rarity == 545);
        assert!(!l[1].unlocked && l[1].hidden);
    }
}
