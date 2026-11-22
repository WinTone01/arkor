//! umu-database (https://github.com/Open-Wine-Components/umu-database): maps games to the
//! `umu-<id>` GAMEID that makes Proton apply their protonfixes. Cached for a day.

use std::path::PathBuf;

use anyhow::Context;
use serde::Deserialize;

const API: &str = "https://umu.openwinecomponents.org/umu_api.php";
const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Entry {
    pub title: Option<String>,
    pub umu_id: String,
    pub store: Option<String>,
    pub notes: Option<String>,
}

fn cache_path() -> anyhow::Result<PathBuf> {
    Ok(dirs::cache_dir()
        .context("no cache dir")?
        .join("arkor/umu-database.json"))
}

/// The whole database, from cache when fresh. Blocking.
pub fn load() -> anyhow::Result<Vec<Entry>> {
    let path = cache_path()?;
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|age| age < MAX_AGE));
    let json = if fresh {
        std::fs::read_to_string(&path)?
    } else {
        let json = crate::tools::http_get(API)?;
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        std::fs::write(&path, &json)?;
        json
    };
    serde_json::from_str(&json).context("unexpected umu-database response")
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Entries whose title matches `name` (exact match first, then containment), one per umu id.
pub fn search<'a>(db: &'a [Entry], name: &str) -> Vec<&'a Entry> {
    let wanted = squash(name);
    if wanted.len() < 2 {
        return Vec::new();
    }
    let mut hits: Vec<(bool, &Entry)> = db
        .iter()
        .filter_map(|e| {
            let t = squash(e.title.as_deref()?);
            if t == wanted {
                Some((true, e))
            } else if t.contains(&wanted) || wanted.contains(&t) && t.len() >= 4 {
                Some((false, e))
            } else {
                None
            }
        })
        .collect();
    hits.sort_by_key(|(exact, e)| (!exact, e.title.clone()));
    let mut seen = Vec::new();
    hits.into_iter()
        .filter(|(_, e)| {
            let new = !seen.contains(&e.umu_id);
            seen.push(e.umu_id.clone());
            new
        })
        .map(|(_, e)| e)
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_prefers_exact_and_dedupes() {
        let db: Vec<Entry> = serde_json::from_str(
            r#"[{"title":"Far Cry 3","umu_id":"umu-220240","store":"ubisoft","notes":null},
                {"title":"Far Cry 3","umu_id":"umu-220240","store":"egs","notes":null},
                {"title":"Far Cry 3: Blood Dragon","umu_id":"umu-233270","store":"egs","notes":null},
                {"title":null,"umu_id":"umu-1","store":null,"notes":null}]"#,
        )
        .unwrap();
        let hits: Vec<_> = search(&db, "far cry 3")
            .iter()
            .map(|e| e.umu_id.as_str())
            .collect();
        assert_eq!(hits, ["umu-220240", "umu-233270"]);
        assert!(search(&db, "x").is_empty());
    }
}
