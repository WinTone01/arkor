//! The Steam account's games, read from what the Steam client keeps on disk: the library's
//! picture cache (one folder per game in the library) and `appcache/appinfo.vdf` (names,
//! kinds, install folders). No login of Arkor's own and no Web API key are needed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::steam::{library_folders, steam_roots};
use crate::vdf;

/// What appinfo.vdf says about an app.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppInfo {
    pub name: String,
    /// `game`, `application`, `tool`, `demo`, `dlc`, … (lowercase).
    pub kind: String,
    pub installdir: String,
}

/// A small reader of Valve's binary key-values.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.at)?;
        self.at += 1;
        Some(b)
    }
    fn u32(&mut self) -> Option<u32> {
        let b = self.data.get(self.at..self.at + 4)?;
        self.at += 4;
        Some(u32::from_le_bytes(b.try_into().ok()?))
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        (self.at + n <= self.data.len()).then(|| self.at += n)
    }
    fn cstr(&mut self) -> Option<String> {
        let rest = self.data.get(self.at..)?;
        let end = rest.iter().position(|b| *b == 0)?;
        let s = String::from_utf8_lossy(&rest[..end]).into_owned();
        self.at += end + 1;
        Some(s)
    }
}

/// Key-values as nested maps; only strings are kept (enough for names and folders).
#[derive(Debug, Default)]
struct Kv(HashMap<String, KvValue>);

#[derive(Debug)]
enum KvValue {
    Map(Kv),
    Str(String),
    Other,
}

impl Kv {
    fn get(&self, path: &[&str]) -> Option<&str> {
        let (last, first) = path.split_last()?;
        let mut map = self;
        for k in first {
            match map.0.get(*k)? {
                KvValue::Map(m) => map = m,
                _ => return None,
            }
        }
        match map.0.get(*last)? {
            KvValue::Str(s) => Some(s),
            _ => None,
        }
    }
}

fn read_kv(r: &mut Reader, strings: Option<&[String]>, depth: usize) -> Option<Kv> {
    if depth > 64 {
        return None;
    }
    let mut out = Kv::default();
    loop {
        let kind = r.u8()?;
        if kind == 0x08 {
            return Some(out);
        }
        let key = match strings {
            Some(table) => table.get(r.u32()? as usize)?.to_lowercase(),
            None => r.cstr()?.to_lowercase(),
        };
        let value = match kind {
            0x00 => KvValue::Map(read_kv(r, strings, depth + 1)?),
            0x01 => KvValue::Str(r.cstr()?),
            0x02 | 0x03 | 0x04 | 0x06 => {
                r.skip(4)?;
                KvValue::Other
            }
            0x07 | 0x0A => {
                r.skip(8)?;
                KvValue::Other
            }
            _ => return None,
        };
        out.0.insert(key, value);
    }
}

/// Reads appinfo.vdf (formats 28 and 29) for the apps in `wanted` (all when empty).
pub fn parse_appinfo(data: &[u8], wanted: &HashSet<u32>) -> anyhow::Result<HashMap<u32, AppInfo>> {
    let mut r = Reader { data, at: 0 };
    let magic = r.u32().context("empty appinfo")?;
    r.u32().context("short appinfo")?;
    let strings: Option<Vec<String>> = match magic {
        0x0756_4429 => {
            let offset = u64::from_le_bytes(
                data.get(8..16)
                    .context("short appinfo")?
                    .try_into()
                    .context("short appinfo")?,
            ) as usize;
            r.at = 16;
            let mut t = Reader { data, at: offset };
            let count = t.u32().context("bad string table")? as usize;
            let mut table = Vec::with_capacity(count.min(1 << 20));
            for _ in 0..count {
                table.push(t.cstr().context("bad string table")?);
            }
            Some(table)
        }
        0x0756_4428 => None,
        other => anyhow::bail!("unknown appinfo format {other:#x}"),
    };
    let mut out = HashMap::new();
    while let Some(appid) = r.u32() {
        if appid == 0 {
            break;
        }
        let size = r.u32().context("cut appinfo")? as usize;
        let start = r.at;
        let end = start + size;
        if end > data.len() {
            break;
        }
        if wanted.is_empty() || wanted.contains(&appid) {
            let mut e = Reader {
                data: &data[..end],
                at: start + 60,
            };
            // The root holds one map, "appinfo".
            if e.u8() == Some(0x00) {
                let _ = match &strings {
                    Some(_) => e.u32().map(|_| ()),
                    None => e.cstr().map(|_| ()),
                };
                if let Some(kv) = read_kv(&mut e, strings.as_deref(), 0) {
                    out.insert(
                        appid,
                        AppInfo {
                            name: kv.get(&["common", "name"]).unwrap_or_default().to_string(),
                            kind: kv
                                .get(&["common", "type"])
                                .unwrap_or_default()
                                .to_lowercase(),
                            installdir: kv
                                .get(&["config", "installdir"])
                                .unwrap_or_default()
                                .to_string(),
                        },
                    );
                }
            }
        }
        r.at = end;
    }
    Ok(out)
}

/// The Steam folder in use (the first one found).
pub fn root() -> Option<PathBuf> {
    steam_roots().into_iter().next()
}

/// The account Steam logs in with: (SteamID64, its name), from `config/loginusers.vdf`.
pub fn account() -> Option<(String, String)> {
    let text = std::fs::read_to_string(root()?.join("config/loginusers.vdf")).ok()?;
    let doc = vdf::parse(&text).ok()?;
    let users = doc.obj("users")?;
    let mut best: Option<(String, String, u64)> = None;
    for (id, v) in &users.0 {
        if let vdf::Value::Obj(u) = v {
            let recent = u.str("mostrecent") == Some("1");
            let time: u64 = u.str("timestamp").and_then(|t| t.parse().ok()).unwrap_or(0);
            let rank = if recent { u64::MAX } else { time };
            if best.as_ref().is_none_or(|b| rank > b.2) {
                best = Some((
                    id.clone(),
                    u.str("personaname").unwrap_or_default().to_string(),
                    rank,
                ));
            }
        }
    }
    best.map(|(id, name, _)| (id, name))
}

/// The games in the account's library: the apps Steam keeps library pictures for, that
/// appinfo calls games. Sorted by name.
pub fn games() -> anyhow::Result<Vec<(u32, AppInfo)>> {
    let root = root().context("Steam is not installed")?;
    let ids: HashSet<u32> = std::fs::read_dir(root.join("appcache/librarycache"))
        .context("Steam has no library cache yet (open Steam once)")?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();
    let data = std::fs::read(root.join("appcache/appinfo.vdf")).context("reading appinfo.vdf")?;
    let infos = parse_appinfo(&data, &ids)?;
    let mut out: Vec<(u32, AppInfo)> = infos
        .into_iter()
        .filter(|(id, i)| i.kind == "game" && !i.name.is_empty() && !is_runtime(*id, &i.name))
        .collect();
    out.sort_by_key(|(_, i)| i.name.to_lowercase());
    Ok(out)
}

fn is_runtime(appid: u32, name: &str) -> bool {
    [228980, 1070560, 1391110, 1628350, 4183110].contains(&appid)
        || name.starts_with("Proton")
        || name.starts_with("Steam Linux Runtime")
}

/// What an installation stands at, from its `appmanifest_<id>.acf`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Progress {
    pub state: u32,
    pub done: u64,
    pub total: u64,
    /// What is put together of the files so far, and all of it.
    pub staged: u64,
    pub to_stage: u64,
}

impl Progress {
    /// Installed, but Steam wants to update it before it is played.
    pub fn needs_update(&self) -> bool {
        self.state & 4 != 0 && self.state & 2 != 0
    }

    /// The user paused it: Steam leaves it alone until it is told to go on.
    pub fn held(&self) -> bool {
        self.state & 512 != 0
    }

    /// Something is left to do: an installation or an update.
    pub fn unfinished(&self) -> bool {
        self.state & 2 != 0
    }

    /// Fully installed and up to date.
    pub fn installed(&self) -> bool {
        self.state & 4 != 0 && self.state & 2 == 0
    }
}

/// The manifest of `appid` in any library folder.
pub fn manifest(appid: u32) -> Option<PathBuf> {
    let root = root()?;
    library_folders(&root)
        .ok()?
        .into_iter()
        .map(|l| l.join(format!("steamapps/appmanifest_{appid}.acf")))
        .find(|p| p.is_file())
}

/// Whether another game's manifest beside `manifest` names the same install folder.
pub fn folder_shared(manifest: &Path, installdir: &str) -> bool {
    let Some(dir) = manifest.parent() else {
        return true;
    };
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let path = e.path();
            path != manifest
                && path.extension().is_some_and(|x| x == "acf")
                && std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| vdf::parse(&t).ok())
                    .and_then(|d| Some(d.obj("appstate")?.str("installdir")?.to_string()))
                    .is_some_and(|d| d.eq_ignore_ascii_case(installdir))
        })
}

/// One app's `appmanifest_<id>.acf`.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub appid: u32,
    pub name: String,
    pub path: PathBuf,
    /// The name of its folder in the library's `common` folder.
    pub folder: String,
    pub progress: Progress,
    /// Proton, a runtime: part of Steam, not a game.
    pub runtime: bool,
}

pub fn read_manifest(path: PathBuf) -> Option<Manifest> {
    let text = std::fs::read_to_string(&path).ok()?;
    let doc = vdf::parse(&text).ok()?;
    let app = doc.obj("appstate")?;
    let num = |k: &str| app.str(k).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let appid = num("appid") as u32;
    let name = app.str("name").unwrap_or_default().to_string();
    Some(Manifest {
        appid,
        runtime: is_runtime(appid, &name),
        name,
        path,
        folder: app.str("installdir").unwrap_or_default().to_string(),
        progress: Progress {
            state: num("stateflags") as u32,
            done: num("bytesdownloaded"),
            total: num("bytestodownload"),
            staged: num("bytesstaged"),
            to_stage: num("bytestostage"),
        },
    })
}

/// Every app in Steam's library folders.
pub fn manifests() -> Vec<Manifest> {
    let folders = root()
        .and_then(|r| library_folders(&r).ok())
        .unwrap_or_default();
    let mut out: Vec<Manifest> = Vec::new();
    for folder in folders {
        let files = std::fs::read_dir(folder.join("steamapps"));
        for e in files.into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("appmanifest_")
                && name.ends_with(".acf")
                && let Some(m) = read_manifest(e.path())
                && !out.iter().any(|o| o.appid == m.appid)
            {
                out.push(m);
            }
        }
    }
    out
}

pub fn progress(appid: u32) -> Option<Progress> {
    read_manifest(manifest(appid)?).map(|m| m.progress)
}

/// A manifest that tells Steam, when it starts, to install the app into `library`
/// (Steam then downloads it without asking). Written only when none exists.
pub fn write_install_manifest(
    library: &Path,
    appid: u32,
    info: &AppInfo,
    owner: &str,
) -> anyhow::Result<PathBuf> {
    let dir = library.join("steamapps");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("appmanifest_{appid}.acf"));
    anyhow::ensure!(!path.exists(), "Steam already knows this game");
    let installdir = if info.installdir.is_empty() {
        info.name.clone()
    } else {
        info.installdir.clone()
    };
    let clean = |s: &str| s.replace(['"', '\\'], "");
    let text = format!(
        "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"Universe\"\t\t\"1\"\n\t\"name\"\t\t\"{}\"\n\t\"StateFlags\"\t\t\"1026\"\n\t\"installdir\"\t\t\"{}\"\n\t\"LastUpdated\"\t\t\"0\"\n\t\"SizeOnDisk\"\t\t\"0\"\n\t\"buildid\"\t\t\"0\"\n\t\"LastOwner\"\t\t\"{}\"\n\t\"BytesToDownload\"\t\t\"0\"\n\t\"BytesDownloaded\"\t\t\"0\"\n\t\"AutoUpdateBehavior\"\t\t\"0\"\n\t\"AllowOtherDownloadsWhileRunning\"\t\t\"0\"\n\t\"ScheduledAutoUpdate\"\t\t\"0\"\n}}\n",
        clean(&info.name),
        clean(&installdir),
        clean(owner),
    );
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A v29 appinfo with one app, built by hand.
    fn sample() -> Vec<u8> {
        let strings = ["appinfo", "common", "name", "type", "config", "installdir"];
        let mut kv = Vec::new();
        let key = |kv: &mut Vec<u8>, i: u32| kv.extend(i.to_le_bytes());
        kv.push(0x00);
        key(&mut kv, 0);
        kv.push(0x00);
        key(&mut kv, 1);
        kv.push(0x01);
        key(&mut kv, 2);
        kv.extend(b"Portal 2\0");
        kv.push(0x01);
        key(&mut kv, 3);
        kv.extend(b"Game\0");
        kv.push(0x02);
        key(&mut kv, 5);
        kv.extend(7u32.to_le_bytes());
        kv.push(0x08);
        kv.push(0x00);
        key(&mut kv, 4);
        kv.push(0x01);
        key(&mut kv, 5);
        kv.extend(b"Portal 2\0");
        kv.push(0x08);
        kv.push(0x08);
        kv.push(0x08);
        let mut entry = vec![0u8; 60];
        entry.extend(&kv);
        let mut out = Vec::new();
        out.extend(0x0756_4429u32.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend(0u64.to_le_bytes());
        out.extend(620u32.to_le_bytes());
        out.extend((entry.len() as u32).to_le_bytes());
        out.extend(&entry);
        out.extend(0u32.to_le_bytes());
        let table = out.len() as u64;
        out[8..16].copy_from_slice(&table.to_le_bytes());
        out.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            out.extend(s.as_bytes());
            out.push(0);
        }
        out
    }

    #[test]
    fn reads_names_kinds_and_folders() {
        let infos = parse_appinfo(&sample(), &HashSet::new()).unwrap();
        let p = &infos[&620];
        assert_eq!(
            (p.name.as_str(), p.kind.as_str(), p.installdir.as_str()),
            ("Portal 2", "game", "Portal 2")
        );
        assert!(parse_appinfo(&sample(), &[1u32].into()).unwrap().is_empty());
    }

    #[test]
    fn writes_a_manifest_once() {
        let tmp = tempfile::tempdir().unwrap();
        let info = AppInfo {
            name: "Portal \"2\"".into(),
            kind: "game".into(),
            installdir: String::new(),
        };
        let p = write_install_manifest(tmp.path(), 620, &info, "765").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("\"StateFlags\"\t\t\"1026\"") && text.contains("\"Portal 2\""));
        assert!(write_install_manifest(tmp.path(), 620, &info, "765").is_err());
    }

    /// Reads the real Steam folder (nothing is written).
    #[test]
    #[ignore]
    fn reads_this_computers_library() {
        let games = games().unwrap();
        println!(
            "{} games, e.g. {:?}",
            games.len(),
            games.iter().take(5).collect::<Vec<_>>()
        );
        println!("account known: {}", account().is_some());
    }
}
