//! Shortcuts that start a game through Arkor from elsewhere: a `.desktop` entry in the app
//! menu (and optionally on the desktop), a non-Steam game in Steam's `shortcuts.vdf`, and the
//! plain command line for other launchers (Heroic, Lutris, scripts).

use std::path::{Path, PathBuf};

use anyhow::Context;
use arkor_core::GameId;

/// The command that launches `id` through Arkor.
pub fn launch_command(id: &GameId) -> String {
    format!(
        "{} launch {}",
        shell_quote(&arkor_exe().display().to_string()),
        shell_quote(&id.to_string())
    )
}

fn arkor_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("arkor"))
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:+@%".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

fn slug(id: &GameId) -> String {
    id.to_string()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

// ---- .desktop entries ----

fn applications_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no XDG data dir")?
        .join("applications"))
}

fn desktop_file_name(id: &GameId) -> String {
    format!("arkor-{}.desktop", slug(id))
}

/// Text of a desktop entry that launches `id`.
pub fn desktop_entry(id: &GameId, name: &str, icon: Option<&Path>) -> String {
    // Desktop entry Exec quoting: arguments with spaces in double quotes.
    let quote = |s: &str| {
        if s.contains([' ', '"', '\\', '`', '$']) {
            format!(
                "\"{}\"",
                s.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('`', "\\`")
                    .replace('$', "\\$")
            )
        } else {
            s.to_string()
        }
    };
    let mut text = format!(
        "[Desktop Entry]\nType=Application\nName={}\nComment=Play with Arkor\nExec={} launch {}\nCategories=Game;\nTerminal=false\n",
        name.replace('\n', " "),
        quote(&arkor_exe().display().to_string()),
        quote(&id.to_string()),
    );
    text.push_str(&format!(
        "Icon={}\n",
        icon.map_or_else(
            || "applications-games".to_string(),
            |p| p.display().to_string()
        )
    ));
    text
}

pub fn menu_entry_exists(id: &GameId) -> bool {
    applications_dir().is_ok_and(|d| d.join(desktop_file_name(id)).is_file())
}

/// Adds (or refreshes) the app-menu entry; with `on_desktop`, also a copy on the desktop.
pub fn add_menu_entry(
    id: &GameId,
    name: &str,
    icon: Option<&Path>,
    on_desktop: bool,
) -> anyhow::Result<PathBuf> {
    let dir = applications_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(desktop_file_name(id));
    let text = desktop_entry(id, name, icon);
    std::fs::write(&path, &text)?;
    if on_desktop {
        let desktop = dirs::desktop_dir().context("no desktop folder")?;
        std::fs::create_dir_all(&desktop)?;
        let copy = desktop.join(desktop_file_name(id));
        std::fs::write(&copy, &text)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755))?;
        // KDE / GNOME ask before running untrusted launchers; mark ours trusted when possible.
        let _ = std::process::Command::new("gio")
            .args(["set", "-t", "string"])
            .arg(&copy)
            .args(["metadata::trusted", "true"])
            .output();
    }
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&dir)
        .output();
    Ok(path)
}

/// Removes the menu entry and the desktop copy.
pub fn remove_menu_entry(id: &GameId) {
    if let Ok(dir) = applications_dir() {
        let _ = std::fs::remove_file(dir.join(desktop_file_name(id)));
    }
    if let Some(desktop) = dirs::desktop_dir() {
        let _ = std::fs::remove_file(desktop.join(desktop_file_name(id)));
    }
}

// ---- Steam non-Steam shortcuts (binary VDF) ----

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Map(Vec<(String, Vdf)>),
    Str(String),
    Int(u32),
    Float(f32),
    U64(u64),
}

impl Vdf {
    fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(items) => items
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Vdf::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Parses a binary VDF document (the root is an implicit map).
pub fn parse_vdf(data: &[u8]) -> anyhow::Result<Vdf> {
    let mut pos = 0;
    let map = parse_map(data, &mut pos)?;
    Ok(Vdf::Map(map))
}

fn read_cstr(data: &[u8], pos: &mut usize) -> anyhow::Result<String> {
    let end = data[*pos..]
        .iter()
        .position(|&b| b == 0)
        .context("unterminated string in VDF")?;
    let s = String::from_utf8_lossy(&data[*pos..*pos + end]).into_owned();
    *pos += end + 1;
    Ok(s)
}

fn take<const N: usize>(data: &[u8], pos: &mut usize) -> anyhow::Result<[u8; N]> {
    let bytes = data.get(*pos..*pos + N).context("truncated VDF")?;
    *pos += N;
    Ok(bytes.try_into().expect("length checked"))
}

fn parse_map(data: &[u8], pos: &mut usize) -> anyhow::Result<Vec<(String, Vdf)>> {
    let mut items = Vec::new();
    loop {
        let Some(&kind) = data.get(*pos) else {
            return Ok(items);
        };
        *pos += 1;
        if kind == 0x08 {
            return Ok(items);
        }
        let key = read_cstr(data, pos)?;
        let value = match kind {
            0x00 => Vdf::Map(parse_map(data, pos)?),
            0x01 => Vdf::Str(read_cstr(data, pos)?),
            0x02 => Vdf::Int(u32::from_le_bytes(take(data, pos)?)),
            0x03 => Vdf::Float(f32::from_le_bytes(take(data, pos)?)),
            0x07 => Vdf::U64(u64::from_le_bytes(take(data, pos)?)),
            other => anyhow::bail!("unsupported VDF type {other:#x}"),
        };
        items.push((key, value));
    }
}

pub fn write_vdf(root: &Vdf) -> Vec<u8> {
    let mut out = Vec::new();
    if let Vdf::Map(items) = root {
        write_map(items, &mut out);
    }
    out
}

fn write_map(items: &[(String, Vdf)], out: &mut Vec<u8>) {
    for (key, value) in items {
        let kind = match value {
            Vdf::Map(_) => 0x00,
            Vdf::Str(_) => 0x01,
            Vdf::Int(_) => 0x02,
            Vdf::Float(_) => 0x03,
            Vdf::U64(_) => 0x07,
        };
        out.push(kind);
        out.extend_from_slice(key.as_bytes());
        out.push(0);
        match value {
            Vdf::Map(inner) => write_map(inner, out),
            Vdf::Str(s) => {
                out.extend_from_slice(s.as_bytes());
                out.push(0);
            }
            Vdf::Int(i) => out.extend_from_slice(&i.to_le_bytes()),
            Vdf::Float(f) => out.extend_from_slice(&f.to_le_bytes()),
            Vdf::U64(u) => out.extend_from_slice(&u.to_le_bytes()),
        }
    }
    out.push(0x08);
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Steam's id for a non-Steam shortcut (what it shows in grid artwork file names).
pub fn shortcut_appid(exe: &str, name: &str) -> u32 {
    crc32(format!("{exe}{name}").as_bytes()) | 0x8000_0000
}

/// `shortcuts.vdf` files of every Steam account on this machine.
pub fn steam_shortcut_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for root in [".local/share/Steam", ".steam/steam"] {
        let Ok(users) = std::fs::read_dir(home.join(root).join("userdata")) else {
            continue;
        };
        for u in users.flatten() {
            let name = u.file_name().to_string_lossy().into_owned();
            if name == "0" || !name.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let file = u.path().join("config/shortcuts.vdf");
            let canon = std::fs::canonicalize(u.path()).unwrap_or(u.path());
            if !out.iter().any(|p: &PathBuf| {
                p.parent()
                    .and_then(Path::parent)
                    .is_some_and(|d| std::fs::canonicalize(d).unwrap_or(d.to_path_buf()) == canon)
            }) {
                out.push(file);
            }
        }
    }
    out
}

pub fn steam_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", "steam"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Adds (or updates) the game as a non-Steam shortcut in `file`. Returns its appid.
pub fn add_to_steam_file(
    file: &Path,
    id: &GameId,
    name: &str,
    icon: Option<&Path>,
) -> anyhow::Result<u32> {
    let mut root = match std::fs::read(file) {
        Ok(data) => parse_vdf(&data).with_context(|| format!("reading {}", file.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vdf::Map(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Vdf::Map(top) = &mut root else {
        unreachable!()
    };
    if !top.iter().any(|(k, _)| k.eq_ignore_ascii_case("shortcuts")) {
        top.push(("shortcuts".into(), Vdf::Map(Vec::new())));
    }
    let Some((_, Vdf::Map(list))) = top
        .iter_mut()
        .find(|(k, _)| k.eq_ignore_ascii_case("shortcuts"))
    else {
        anyhow::bail!("shortcuts.vdf has an unexpected layout");
    };
    let exe = format!("\"{}\"", arkor_exe().display());
    let options = format!("launch {}", shell_quote(&id.to_string()));
    let appid = shortcut_appid(&exe, name);
    let entry = Vdf::Map(vec![
        ("appid".into(), Vdf::Int(appid)),
        ("AppName".into(), Vdf::Str(name.into())),
        ("Exe".into(), Vdf::Str(exe)),
        (
            "StartDir".into(),
            Vdf::Str(format!(
                "\"{}\"",
                dirs::home_dir().unwrap_or_default().display()
            )),
        ),
        (
            "icon".into(),
            Vdf::Str(icon.map(|p| p.display().to_string()).unwrap_or_default()),
        ),
        ("ShortcutPath".into(), Vdf::Str(String::new())),
        ("LaunchOptions".into(), Vdf::Str(options.clone())),
        ("IsHidden".into(), Vdf::Int(0)),
        ("AllowDesktopConfig".into(), Vdf::Int(1)),
        ("AllowOverlay".into(), Vdf::Int(1)),
        ("OpenVR".into(), Vdf::Int(0)),
        ("Devkit".into(), Vdf::Int(0)),
        ("DevkitGameID".into(), Vdf::Str(String::new())),
        ("DevkitOverrideAppID".into(), Vdf::Int(0)),
        ("LastPlayTime".into(), Vdf::Int(0)),
        ("FlatpakAppID".into(), Vdf::Str(String::new())),
        (
            "tags".into(),
            Vdf::Map(vec![("0".into(), Vdf::Str("Arkor".into()))]),
        ),
    ]);
    // Same game added before (matched by its launch options): replace in place.
    if let Some((_, existing)) = list.iter_mut().find(|(_, v)| {
        v.str("LaunchOptions") == Some(options.as_str())
            && v.str("Exe").is_some_and(|e| e.contains("arkor"))
    }) {
        *existing = entry;
    } else {
        let next = list
            .iter()
            .filter_map(|(k, _)| k.parse::<u32>().ok())
            .max()
            .map_or(0, |m| m + 1);
        list.push((next.to_string(), entry));
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if file.exists() {
        let _ = std::fs::copy(file, file.with_extension("vdf.arkor-backup"));
    }
    std::fs::write(file, write_vdf(&root))?;
    Ok(appid)
}

/// Adds the game to every Steam account. Steam must be closed (it rewrites the file on exit).
pub fn add_to_steam(id: &GameId, name: &str, icon: Option<&Path>) -> anyhow::Result<usize> {
    anyhow::ensure!(
        !steam_running(),
        "close Steam first: it overwrites shortcuts.vdf when it exits"
    );
    let files = steam_shortcut_files();
    anyhow::ensure!(!files.is_empty(), "no Steam account found on this machine");
    for f in &files {
        add_to_steam_file(f, id, name, icon)?;
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_round_trip_and_update() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config/shortcuts.vdf");
        let id: GameId = "local:far-cry-6".parse().unwrap();
        let a = add_to_steam_file(&file, &id, "Far Cry 6", None).unwrap();
        assert!(a & 0x8000_0000 != 0);
        let other: GameId = "local:other".parse().unwrap();
        add_to_steam_file(&file, &other, "Other", None).unwrap();
        // Adding the same game again replaces it instead of duplicating.
        add_to_steam_file(&file, &id, "Far Cry 6", Some(Path::new("/c.png"))).unwrap();
        let data = std::fs::read(&file).unwrap();
        let root = parse_vdf(&data).unwrap();
        assert_eq!(write_vdf(&root), data, "writer is the parser's inverse");
        let Some(Vdf::Map(list)) = root.get("shortcuts") else {
            panic!()
        };
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].0, "0");
        assert_eq!(list[0].1.str("icon"), Some("/c.png"));
        assert_eq!(list[1].1.str("AppName"), Some("Other"));
        assert!(
            list[0]
                .1
                .str("LaunchOptions")
                .unwrap()
                .ends_with("local:far-cry-6")
        );
    }

    #[test]
    fn finds_steam_accounts_but_not_user_zero() {
        let _lock = crate::CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        for d in ["12345", "0", "notes"] {
            std::fs::create_dir_all(tmp.path().join(".local/share/Steam/userdata").join(d))
                .unwrap();
        }
        let old = std::env::var_os("HOME");
        // SAFETY: tests that change process-wide variables hold CONFIG_ENV_LOCK.
        unsafe { std::env::set_var("HOME", tmp.path()) };
        let files = steam_shortcut_files();
        match old {
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        assert_eq!(
            files,
            [tmp.path()
                .join(".local/share/Steam/userdata/12345/config/shortcuts.vdf")]
        );
    }

    /// Leaves a sample in `target/` so other VDF readers can check the format.
    #[test]
    fn writes_sample_for_external_check() {
        let out =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/arkor-shortcuts-sample.vdf");
        let _ = std::fs::remove_file(&out);
        add_to_steam_file(
            &out,
            &"local:far-cry-6".parse().unwrap(),
            "Far Cry 6",
            Some(Path::new("/covers/fc6.png")),
        )
        .unwrap();
        add_to_steam_file(&out, &"local:other".parse().unwrap(), "Other Game", None).unwrap();
        assert!(out.is_file());
    }

    #[test]
    fn crc_matches_reference() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn desktop_entry_quotes_paths() {
        let id: GameId = "local:a-b".parse().unwrap();
        let text = desktop_entry(&id, "A B", Some(Path::new("/x/y z.png")));
        assert!(text.contains("Name=A B\n"));
        assert!(text.contains(" launch local:a-b\n"));
        assert!(text.contains("Icon=/x/y z.png\n"));
    }
}
