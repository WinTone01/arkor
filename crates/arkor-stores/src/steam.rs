//! Steam: reads installed games straight from libraryfolders.vdf + appmanifest_*.acf.
//! No Steam API or running client needed.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::{Game, GameId, Store};

use crate::vdf;

/// Candidate Steam roots, native first. Flatpak Steam is listed but untested for now.
pub fn steam_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut roots: Vec<PathBuf> = [
        ".local/share/Steam",
        ".steam/steam",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    ]
    .iter()
    .map(|p| home.join(p))
    .filter(|p| p.join("steamapps").is_dir())
    .collect();
    // ~/.steam/steam is usually a symlink to ~/.local/share/Steam: dedupe by real path.
    let mut seen = Vec::new();
    roots.retain(|p| {
        let real = p.canonicalize().unwrap_or_else(|_| p.clone());
        let new = !seen.contains(&real);
        seen.push(real);
        new
    });
    roots
}

/// Library folders listed in `<root>/steamapps/libraryfolders.vdf` (includes the root itself).
pub fn library_folders(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let file = root.join("steamapps/libraryfolders.vdf");
    let text =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    let doc = vdf::parse(&text).with_context(|| format!("parsing {}", file.display()))?;
    let folders = doc
        .obj("libraryfolders")
        .context("libraryfolders.vdf has no `libraryfolders` block")?;
    let mut out: Vec<PathBuf> = folders
        .0
        .values()
        .filter_map(|v| match v {
            vdf::Value::Obj(o) => o.str("path").map(PathBuf::from),
            vdf::Value::Str(_) => None,
        })
        .collect();
    if !out.iter().any(|p| p == root) {
        out.insert(0, root.to_path_buf());
    }
    Ok(out)
}

/// Steam ships runtimes and compat tools as "apps"; they are not games.
fn is_tool(appid: &str, name: &str) -> bool {
    const TOOL_IDS: &[&str] = &[
        "228980",  // Steamworks Common Redistributables
        "1070560", // Steam Linux Runtime 1.0 (scout)
        "1391110", // Steam Linux Runtime 2.0 (soldier)
        "1628350", // Steam Linux Runtime 3.0 (sniper)
        "4183110", // Steam Linux Runtime 4.0
    ];
    TOOL_IDS.contains(&appid)
        || name.starts_with("Proton")
        || name.starts_with("Steam Linux Runtime")
        || name.starts_with("Steamworks")
}

fn parse_manifest(library: &Path, path: &Path) -> anyhow::Result<Option<Game>> {
    let text = std::fs::read_to_string(path)?;
    let doc = vdf::parse(&text)?;
    let app = doc.obj("appstate").context("no AppState block")?;
    let appid = app.str("appid").context("no appid")?;
    let name = app.str("name").unwrap_or(appid);
    if is_tool(appid, name) {
        return Ok(None);
    }
    Ok(Some(Game {
        id: GameId::new(Store::Steam, appid),
        name: name.to_string(),
        install_dir: app
            .str("installdir")
            .map(|d| library.join("steamapps/common").join(d)),
        size_on_disk: app.str("sizeondisk").and_then(|s| s.parse().ok()),
    }))
}

/// Install folder of any installed Steam app (tools and runtimes included).
pub fn app_dir(appid: &str) -> Option<PathBuf> {
    for root in steam_roots() {
        for library in library_folders(&root).unwrap_or_default() {
            let manifest = library
                .join("steamapps")
                .join(format!("appmanifest_{appid}.acf"));
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let Ok(doc) = vdf::parse(&text) else { continue };
            if let Some(dir) = doc.obj("appstate").and_then(|a| a.str("installdir")) {
                let path = library.join("steamapps/common").join(dir);
                if path.is_dir() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// All installed Steam games across every root and library.
/// Unreadable libraries (e.g. an unmounted drive) are skipped and reported in the second value.
pub fn installed_games() -> (Vec<Game>, Vec<anyhow::Error>) {
    let mut games = Vec::new();
    let mut errors = Vec::new();
    for root in steam_roots() {
        let libraries = match library_folders(&root) {
            Ok(l) => l,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        for library in libraries {
            let Ok(entries) = std::fs::read_dir(library.join("steamapps")) else {
                errors.push(anyhow::anyhow!(
                    "library not readable: {}",
                    library.display()
                ));
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let is_manifest = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"));
                if !is_manifest {
                    continue;
                }
                match parse_manifest(&library, &path) {
                    Ok(Some(game)) => games.push(game),
                    Ok(None) => {}
                    Err(e) => errors.push(e.context(format!("manifest {}", path.display()))),
                }
            }
        }
    }
    games.sort_by_key(|g| g.name.to_lowercase());
    games.dedup_by(|a, b| a.id == b.id);
    (games, errors)
}

/// Steam's own play time (minutes) and last-played time for an app, summed / maxed over all
/// local Steam users (`userdata/<id>/config/localconfig.vdf`).
pub fn playtime(appid: &str) -> Option<(u64, u64)> {
    let mut total = 0u64;
    let mut last = 0u64;
    let mut found = false;
    for root in steam_roots() {
        let Ok(users) = std::fs::read_dir(root.join("userdata")) else {
            continue;
        };
        for user in users.flatten() {
            let Ok(text) = std::fs::read_to_string(user.path().join("config/localconfig.vdf"))
            else {
                continue;
            };
            let Ok(doc) = vdf::parse(&text) else { continue };
            let app = doc
                .obj("UserLocalConfigStore")
                .and_then(|o| o.obj("Software"))
                .and_then(|o| o.obj("Valve"))
                .and_then(|o| o.obj("Steam"))
                .and_then(|o| o.obj("apps"))
                .and_then(|o| o.obj(appid));
            if let Some(app) = app {
                found = true;
                total += app
                    .str("Playtime")
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                last = last.max(
                    app.str("LastPlayed")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                );
            }
        }
    }
    found.then_some((total, last))
}
