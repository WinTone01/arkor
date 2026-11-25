//! Where a game's saves are, in its Wine prefix.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;

use super::tree::{Filter, in_case_of};

/// One folder of saves that is kept in the cloud.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// The store's name for it (Epic has one place per game).
    pub name: String,
    pub path: PathBuf,
    pub filter: Filter,
}

/// The folders save places are named by.
pub struct Folders {
    pub prefix: PathBuf,
    pub install: PathBuf,
    pub account: String,
}

impl Folders {
    /// The Windows user's folder in the prefix.
    fn user(&self) -> PathBuf {
        let root = if self.prefix.join("pfx/drive_c").is_dir() {
            self.prefix.join("pfx")
        } else {
            self.prefix.clone()
        };
        let users = root.join("drive_c/users");
        // Proton's user; prefixes of plain Wine have the Linux user's name.
        let wine = std::env::var("USER").ok().map(|u| users.join(u));
        let proton = users.join("steamuser");
        match wine {
            Some(w) if w.is_dir() && !proton.is_dir() => w,
            _ => proton,
        }
    }

    /// What a variable stands for.
    fn variable(&self, name: &str) -> Option<PathBuf> {
        let user = self.user();
        Some(match name.to_lowercase().as_str() {
            "{installdir}" | "<?install?>" => self.install.clone(),
            "{epicid}" => {
                // From the login: one plain name.
                let plain = !self.account.is_empty()
                    && self.account.chars().all(|c| c.is_ascii_alphanumeric());
                plain.then(|| PathBuf::from(&self.account))?
            }
            "{appdata}" | "<?application_data_local?>" | "%localappdata%" => {
                user.join("AppData/Local")
            }
            "<?application_data_local_low?>" => user.join("AppData/LocalLow"),
            "<?application_data_roaming?>" | "%appdata%" => user.join("AppData/Roaming"),
            "{userdir}" | "<?documents?>" => user.join("Documents"),
            "{userprofile}" | "%userprofile%" => user,
            "{usersavedgames}" | "<?saved_games?>" => user.join("Saved Games"),
            _ => return None,
        })
    }

    /// The folder a store's text names. Only one inside the prefix or the game's folder, and
    /// not one that holds more than one game's files.
    pub fn resolve(&self, text: &str, filter: &Filter) -> anyhow::Result<PathBuf> {
        let refuse = || anyhow::anyhow!("the store's save folder is not usable: {text}");
        let text = text.replace('\\', "/");
        let mut parts = text.split('/').filter(|p| !p.is_empty() && *p != ".");
        let first = parts.next().ok_or_else(refuse)?;
        let mut path = self.variable(first).filter(|p| p.is_absolute());
        for part in parts {
            let base = path.as_mut().ok_or_else(refuse)?;
            if part == ".." {
                base.pop();
            } else if part.contains(['{', '<', '%']) {
                let value = self.variable(part).ok_or_else(refuse)?;
                anyhow::ensure!(value.is_relative(), refuse());
                base.push(value);
            } else {
                base.push(part);
            }
        }
        let path = path.ok_or_else(refuse)?;
        let plain = path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
        let user = self.user();
        let base = [&user, &self.install]
            .into_iter()
            .find(|b| !b.as_os_str().is_empty() && path.starts_with(b) && path != **b)
            .ok_or_else(refuse)?;
        let shared = [
            "AppData",
            "AppData/Local",
            "AppData/LocalLow",
            "AppData/Roaming",
            "Documents",
            "Documents/My Games",
            "Saved Games",
            "Desktop",
        ]
        .iter()
        .any(|d| path == user.join(d));
        // "Documents" itself with `*.sav` would be fine; all of it would not.
        anyhow::ensure!(plain && (!shared || !filter.include.is_empty()), refuse());
        let rel = path.strip_prefix(base).expect("checked above");
        Ok(base.join(in_case_of(base, rel)))
    }
}

/// The place in Epic's description of a game (`metadata/<app>.json`), if it has saves in the
/// cloud.
pub fn epic(metadata: &Value, folders: &Folders) -> anyhow::Result<Option<Place>> {
    let attributes = &metadata["metadata"]["customAttributes"];
    let value = |name: &str| attributes[name]["value"].as_str().unwrap_or_default();
    let folder = value("CloudSaveFolder");
    if folder.is_empty() {
        return Ok(None);
    }
    let filter = Filter::new(value("CloudIncludeList"), value("CloudExcludeList"));
    Ok(Some(Place {
        name: "saves".into(),
        path: folders.resolve(folder, &filter)?,
        filter,
    }))
}

#[derive(Debug, Deserialize)]
struct Location {
    name: String,
    location: String,
}

/// The places in GOG's description of a game.
pub fn gog(config: &Value, client: &str, folders: &Folders) -> anyhow::Result<Vec<Place>> {
    let storage = &config["content"]["Windows"]["cloudStorage"];
    if storage["enabled"] != true {
        return Ok(Vec::new());
    }
    let mut list: Vec<Location> =
        serde_json::from_value(storage["locations"].clone()).unwrap_or_default();
    if list.is_empty() {
        list.push(Location {
            name: "__default".into(),
            location: format!(
                "%LocalAppData%/GOG.com/Galaxy/Applications/{client}/Storage/Shared/Files"
            ),
        });
    }
    list.into_iter()
        .map(|l| {
            // Part of a web address and of a folder name.
            anyhow::ensure!(
                !l.name.is_empty()
                    && l.name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_- .".contains(c))
                    && !l.name.starts_with(['.', '-']),
                "unexpected name of a save folder: {}",
                l.name
            );
            Ok(Place {
                path: folders.resolve(&l.location, &Filter::default())?,
                name: l.name,
                filter: Filter::default(),
            })
        })
        .collect()
}

/// A GOG game's client (its name at GOG's services) and that client's public key, when known.
pub fn gog_client(
    stores: &Path,
    install: &Path,
    id: &str,
) -> anyhow::Result<(String, Option<String>)> {
    let read = |p: &Path| -> Option<BTreeMap<String, Value>> {
        serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
    };
    let manifest = read(&stores.join("gog/heroic_gogdl/manifests").join(id)).unwrap_or_default();
    let info = read(&install.join(format!("goggame-{id}.info"))).unwrap_or_default();
    let text = |m: &BTreeMap<String, Value>, key: &str| {
        m.get(key)
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(str::to_string)
    };
    let client = text(&manifest, "clientId")
        .or_else(|| text(&info, "clientId"))
        .context("the game has no cloud saves")?;
    Ok((client, text(&manifest, "clientSecret")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders(tmp: &Path) -> Folders {
        std::fs::create_dir_all(tmp.join("pfx/drive_c/users/steamuser/AppData")).unwrap();
        Folders {
            prefix: tmp.to_path_buf(),
            install: tmp.join("games/Thief"),
            account: "0123abc".into(),
        }
    }

    #[test]
    fn epic_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let f = folders(tmp.path());
        let user = tmp.path().join("pfx/drive_c/users/steamuser");
        let at = |t: &str| f.resolve(t, &Filter::default());
        assert_eq!(
            at("{AppData}/../LocalLow/Take IT Studio!/Electrician/{EpicID}/").unwrap(),
            user.join("AppData/LocalLow/Take IT Studio!/Electrician/0123abc")
        );
        assert_eq!(
            at("{UserDir}\\My Games\\Fallout3\\Saves\\").unwrap(),
            user.join("Documents/My Games/Fallout3/Saves")
        );
        assert_eq!(
            at("{UserProfile}/Documents/Cat Quest II/").unwrap(),
            user.join("Documents/Cat Quest II")
        );
        assert_eq!(
            at("{InstallDir}/ThiefGame/SaveData/").unwrap(),
            tmp.path().join("games/Thief/ThiefGame/SaveData")
        );
        for bad in [
            "",
            "{InstallDir}",
            "{InstallDir}/",
            "{AppData}",
            "{AppData}/..",
            "{UserDir}/",
            "{UserProfile}",
            "{AppData}/../../../../../../etc",
            "{InstallDir}/../other",
            "{Unknown}/x",
            "C:/Windows",
            "/etc",
            "{AppData}/Game/{Unknown}",
        ] {
            assert!(at(bad).is_err(), "{bad} passed: {:?}", at(bad));
        }
        // A folder many games share, with the files named.
        assert!(
            f.resolve("{UserDir}/My Games", &Filter::new("ours.sav", ""))
                .is_ok()
        );
    }

    #[test]
    fn folders_are_found_in_their_own_case() {
        let tmp = tempfile::tempdir().unwrap();
        let f = folders(tmp.path());
        let real = tmp
            .path()
            .join("pfx/drive_c/users/steamuser/AppData/Roaming/KF2/Saves");
        std::fs::create_dir_all(&real).unwrap();
        assert_eq!(
            f.resolve("{appdata}/../roaming/kf2/saves", &Filter::default())
                .unwrap(),
            real
        );
    }

    #[test]
    fn epic_and_gog_descriptions() {
        let tmp = tempfile::tempdir().unwrap();
        let f = folders(tmp.path());
        let v = serde_json::json!({"metadata": {"customAttributes": {
            "CloudSaveFolder": {"value": "{AppData}/Sifu/Saved"},
            "CloudIncludeList": {"value": "*.sav"}}}});
        let p = epic(&v, &f).unwrap().unwrap();
        assert!(p.path.ends_with("AppData/Local/Sifu/Saved"));
        assert_eq!(p.filter, Filter::new("*.sav", ""));
        assert_eq!(epic(&serde_json::json!({}), &f).unwrap(), None);

        let v = serde_json::json!({"content": {"Windows": {"cloudStorage":
            {"enabled": true, "locations": [
                {"name": "saves", "location": "<?DOCUMENTS?>/Game/Saves"},
                {"name": "conf", "location": "<?INSTALL?>/conf"}]}}}});
        let p = gog(&v, "5115", &f).unwrap();
        assert_eq!(p.len(), 2);
        assert!(p[1].path.ends_with("games/Thief/conf"));
        let v = serde_json::json!({"content": {"Windows": {"cloudStorage":
            {"enabled": true, "locations": []}}}});
        let p = gog(&v, "5115", &f).unwrap();
        assert_eq!(p[0].name, "__default");
        assert!(
            p[0].path
                .ends_with("AppData/Local/GOG.com/Galaxy/Applications/5115/Storage/Shared/Files")
        );
        assert!(gog(&serde_json::json!({}), "5115", &f).unwrap().is_empty());
        let v = serde_json::json!({"content": {"Windows": {"cloudStorage":
            {"enabled": true, "locations": [{"name": "../x", "location": "<?INSTALL?>/c"}]}}}});
        assert!(gog(&v, "5115", &f).is_err());
    }
}
