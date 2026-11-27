//! Arkor's games in Sunshine (or Apollo, which uses Sunshine's folder on Linux), so Moonlight
//! lists and starts them: each library game becomes an application of Sunshine's
//! `apps.json` whose command is `arkor launch <id>`.
//!
//! Only Arkor's own entries change (they are known by their command); the user's other
//! applications stay as they are. Before the first change the file is copied to
//! `apps.json.arkor-backup`. Sunshine reads the file when it starts, so a running Sunshine
//! user service is restarted after a change.

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::library::Library;
use serde_json::{Value, json};

/// Sunshine's folder on this computer, and whether it is the Flatpak's.
pub fn config() -> Option<(PathBuf, bool)> {
    let home = dirs::home_dir()?;
    let native = dirs::config_dir()?.join("sunshine");
    let flatpak = home.join(".var/app/dev.lizardbyte.app.Sunshine/config/sunshine");
    [(native, false), (flatpak, true)]
        .into_iter()
        .find(|(d, _)| d.join("apps.json").is_file() || d.join("sunshine.conf").is_file())
}

/// Whether an application of `apps.json` is one Arkor wrote.
fn ours(app: &Value) -> bool {
    app["cmd"]
        .as_str()
        .is_some_and(|c| c.contains("arkor") && c.contains(" launch local:"))
}

/// `apps.json` with Arkor's games (`games`: id, name, cover) in place of its earlier ones.
pub fn plan(
    current: &Value,
    games: &[(String, String, Option<PathBuf>)],
    exe: &Path,
    flatpak: bool,
) -> Value {
    let mut doc = if current.is_object() {
        current.clone()
    } else {
        json!({"env": {}, "apps": []})
    };
    let mut apps: Vec<Value> = doc["apps"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !ours(a))
        .collect();
    let exe = exe.display().to_string();
    for (id, name, cover) in games {
        let cmd = if flatpak {
            format!("flatpak-spawn --host \"{exe}\" launch local:{id}")
        } else {
            format!("\"{exe}\" launch local:{id}")
        };
        let mut app = json!({
            "name": name,
            "cmd": cmd,
            "auto-detach": "true",
            "wait-all": "true",
            "exit-timeout": "5",
        });
        // Sunshine shows PNG pictures only.
        if let Some(c) = cover
            .as_ref()
            .filter(|c| c.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        {
            app["image-path"] = json!(c.display().to_string());
        }
        apps.push(app);
    }
    doc["apps"] = Value::Array(apps);
    doc
}

/// Writes Arkor's games into Sunshine's list (or takes them out, with `on` false); how many
/// it holds now. Blocking.
pub fn sync(on: bool) -> anyhow::Result<usize> {
    let (dir, flatpak) = config().context("Sunshine is not set up on this computer")?;
    let path = dir.join("apps.json");
    let current: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    let games: Vec<(String, String, Option<PathBuf>)> = if on {
        Library::load()?
            .games
            .iter()
            .filter(|g| !g.hidden)
            .map(|g| (g.id.clone(), g.name.clone(), crate::covers::cover_for(g)))
            .collect()
    } else {
        Vec::new()
    };
    let exe = std::env::current_exe().context("finding Arkor's program")?;
    let next = plan(&current, &games, &exe, flatpak);
    if next == current {
        return Ok(games.len());
    }
    let backup = dir.join("apps.json.arkor-backup");
    if path.is_file() && !backup.exists() {
        std::fs::copy(&path, &backup)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&next)?)
        .with_context(|| format!("writing {}", path.display()))?;
    restart();
    Ok(games.len())
}

/// Restarts a running Sunshine user service, so it reads the list again.
fn restart() {
    for unit in ["sunshine", "app-dev.lizardbyte.app.Sunshine"] {
        let active = std::process::Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", unit])
            .status()
            .is_ok_and(|s| s.success());
        if active {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "restart", unit])
                .status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_users_apps_and_replaces_arkors() {
        let current = json!({"env": {"PATH": "x"}, "apps": [
            {"name": "Desktop", "image-path": "desktop.png"},
            {"name": "Old", "cmd": "\"/usr/bin/arkor\" launch local:old"}
        ]});
        let games = vec![
            (
                "hades".to_string(),
                "Hades".to_string(),
                Some(PathBuf::from("/c/hades.png")),
            ),
            (
                "doom".to_string(),
                "DOOM".to_string(),
                Some(PathBuf::from("/c/doom.jpg")),
            ),
        ];
        let next = plan(&current, &games, Path::new("/usr/bin/arkor"), false);
        let apps = next["apps"].as_array().unwrap();
        assert_eq!(apps.len(), 3);
        assert_eq!(apps[0]["name"], "Desktop");
        assert_eq!(apps[1]["cmd"], "\"/usr/bin/arkor\" launch local:hades");
        assert_eq!(apps[1]["image-path"], "/c/hades.png");
        assert!(
            apps[2].get("image-path").is_none(),
            "Sunshine shows PNG only"
        );
        assert_eq!(next["env"]["PATH"], "x");
        let flat = plan(&json!(null), &games[..1], Path::new("/a/arkor"), true);
        assert!(
            flat["apps"][0]["cmd"]
                .as_str()
                .unwrap()
                .starts_with("flatpak-spawn --host")
        );
        let removed = plan(&next, &[], Path::new("/usr/bin/arkor"), false);
        assert_eq!(removed["apps"].as_array().unwrap().len(), 1);
    }
}
