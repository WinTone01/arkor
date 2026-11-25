//! GOG through gogdl (Heroic's GOG downloader) and GOG's own web interfaces.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Dlc, Info, Installed, Order, Owned, Shop, check, helper, plain_code, shop_dir};
use crate::tools;

/// GOG's login (the one its Galaxy client uses); it ends on an address with the code.
pub(super) const LOGIN_URL: &str = "https://auth.gog.com/auth?client_id=46899977096215655&redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient&response_type=code&layout=galaxy";

pub(super) fn find_code(text: &str) -> Option<String> {
    let text = text.trim();
    let code = if text.contains("on_login_success") {
        super::query_value(text, "code")?
    } else {
        text.to_string()
    };
    plain_code(&code, 40).then_some(code)
}

#[derive(Deserialize)]
struct Credentials {
    access_token: String,
    #[serde(default)]
    user_id: String,
}

/// The current login, renewed by gogdl when it ran out.
fn credentials() -> anyhow::Result<Credentials> {
    let mut cmd = helper::command(Shop::Gog)?;
    cmd.arg("auth");
    let out = helper::output(cmd)?;
    out.json()
        .map_err(|_| anyhow::anyhow!("the GOG login ended: log in again"))
}

pub(super) fn finish_login(code: &str) -> anyhow::Result<String> {
    anyhow::ensure!(plain_code(code, 40), "that is not a GOG login code");
    let mut cmd = helper::command(Shop::Gog)?;
    cmd.args(["auth", "--code", code]);
    let out = helper::output(cmd)?;
    let answer: Value = out.json()?;
    anyhow::ensure!(
        answer.get("error").is_none_or(|e| e == false),
        "GOG did not accept the code (codes work once, for a short time)"
    );
    Ok(account_name().unwrap_or_default())
}

fn account_name() -> anyhow::Result<String> {
    let c = credentials()?;
    let text = tools::http_get_bearer(
        &format!("https://users.gog.com/users/{}", c.user_id),
        &c.access_token,
    )?;
    let v: Value = serde_json::from_str(&text)?;
    Ok(v["username"].as_str().unwrap_or_default().to_string())
}

pub(super) fn logout() -> anyhow::Result<()> {
    // gogdl has nothing to say to GOG: the login is just removed (by the caller).
    let _ = std::fs::remove_file(shop_dir(Shop::Gog)?.join("auth.json"));
    Ok(())
}

pub(super) fn owned() -> anyhow::Result<Vec<Owned>> {
    let login = credentials()?;
    let mut out = Vec::new();
    let mut page = 1;
    loop {
        let text = tools::http_get_bearer(
            &format!("https://embed.gog.com/account/getFilteredProducts?mediaType=1&page={page}"),
            &login.access_token,
        )?;
        let v: Value = serde_json::from_str(&text).context("unexpected GOG library")?;
        out.extend(parse_page(&v));
        let pages = v["totalPages"].as_u64().unwrap_or(1);
        if page >= pages || page >= 200 {
            return Ok(out);
        }
        page += 1;
    }
}

/// One page of the account's games.
pub(super) fn parse_page(v: &Value) -> Vec<Owned> {
    v["products"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["isGame"] != false && p["isHidden"] != true)
        .filter_map(|p| {
            let works = |os: &str| p["worksOn"][os].as_bool().unwrap_or(false);
            let (windows, linux) = (works("Windows"), works("Linux"));
            (windows || linux).then_some(())?;
            Some(Owned {
                id: p["id"].as_u64()?.to_string(),
                title: p["title"].as_str()?.trim().to_string(),
                // The portrait picture is looked up game by game (see `covers`).
                cover: None,
                wide: p["image"]
                    .as_str()
                    .filter(|i| i.starts_with("//"))
                    .map(|i| format!("https:{i}.jpg")),
                linux,
                windows,
                namespace: None,
            })
        })
        .collect()
}

/// A GOG game's portrait picture, from GOG's public games database. Blocking.
pub(super) fn cover_url(id: &str) -> anyhow::Result<String> {
    art_url(id, "vertical_cover", "_glx_vertical_cover")
}

/// A GOG game's wide picture, the same way. Blocking.
pub(super) fn wide_url(id: &str) -> anyhow::Result<String> {
    art_url(id, "background", "_glx_bg_top_padding_7")
}

fn art_url(id: &str, kind: &str, formatter: &str) -> anyhow::Result<String> {
    let text = tools::http_get_quick(&format!(
        "https://gamesdb.gog.com/platforms/gog/external_releases/{id}"
    ))?;
    let v: Value = serde_json::from_str(&text)?;
    v["game"][kind]["url_format"]
        .as_str()
        .map(|u| u.replace("{formatter}", formatter).replace("{ext}", "webp"))
        .context("no cover")
}

// ---- What is installed: gogdl does not keep a list, so Arkor does (in Heroic's format) ----

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    installed: Vec<Entry>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    app_name: String,
    #[serde(rename = "install_path")]
    install_path: PathBuf,
    platform: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    build_id: String,
    #[serde(default, rename = "is_dlc")]
    is_dlc: bool,
}

fn file() -> anyhow::Result<PathBuf> {
    Ok(arkor_core::paths::stores_dir()?.join("gog/installed.json"))
}

fn read() -> anyhow::Result<File> {
    let path = file()?;
    if path.is_file() {
        super::read_json(&path)
    } else {
        Ok(File::default())
    }
}

pub(super) fn installed() -> anyhow::Result<Vec<Installed>> {
    Ok(read()?
        .installed
        .into_iter()
        .map(|e| Installed {
            shop: Shop::Gog,
            linux: e.platform == "linux",
            // Builds are told apart by their id; Linux installers only have a version.
            version: if e.build_id.is_empty() {
                e.version
            } else {
                e.build_id
            },
            title: e.title,
            id: e.app_name,
            path: e.install_path,
            size: None,
        })
        .collect())
}

fn platform(linux: bool) -> &'static str {
    if linux { "linux" } else { "windows" }
}

pub(super) fn info(id: &str, linux: bool) -> anyhow::Result<Info> {
    let mut cmd = helper::command(Shop::Gog)?;
    cmd.args(["info", id, "--os", platform(linux)]);
    let v: Value = helper::output(cmd)?.json()?;
    Ok(parse_info(&v))
}

pub(super) fn parse_info(v: &Value) -> Info {
    let languages: Vec<String> = v["languages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l.as_str().map(str::to_string))
        .filter(|l| l != "*")
        .collect();
    // Windows builds: sizes per language, "*" for what every language needs. The largest
    // language stands for the choice, which is made later.
    let size = |field: &str| -> Option<u64> {
        if let Some(n) = v[field].as_u64() {
            return Some(n);
        }
        let sizes = v["size"].as_object()?;
        let of = |s: &Value| s[field].as_u64().unwrap_or(0);
        let common = sizes.get("*").map_or(0, of);
        let language = sizes
            .iter()
            .filter(|(k, _)| *k != "*")
            .map(|(_, s)| of(s))
            .max()
            .unwrap_or(0);
        Some(common + language)
    };
    Info {
        download: size("download_size"),
        disk: size("disk_size"),
        folder: v["folder_name"]
            .as_str()
            .map(|f| f.trim_matches('/').to_string())
            // One plain folder: it is joined to the games folder.
            .filter(|f| !f.is_empty() && !f.contains(['/', '\\']) && f != ".." && f != "."),
        dlcs: v["dlcs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| {
                let id = d["id"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| d["id"].as_u64().map(|n| n.to_string()))?;
                // Sizes per language, as for the game.
                let size = d["size"]
                    .as_object()
                    .and_then(|s| s.values().filter_map(|l| l["download_size"].as_u64()).max());
                Some(Dlc {
                    id,
                    title: d["title"].as_str()?.trim().to_string(),
                    size,
                })
            })
            .collect(),
        version: v["versionName"].as_str().unwrap_or_default().to_string(),
        build: v["buildId"]
            .as_str()
            .map(str::to_string)
            .or_else(|| v["buildId"].as_u64().map(|n| n.to_string()))
            .unwrap_or_default(),
        languages,
    }
}

fn download_args(
    command: &str,
    id: &str,
    path: &Path,
    linux: bool,
    order: &Order,
) -> anyhow::Result<std::process::Command> {
    let mut cmd = helper::command(Shop::Gog)?;
    cmd.args([command, id, "--platform", platform(linux), "--path"])
        .arg(path)
        .arg("--support")
        .arg(shop_dir(Shop::Gog)?.join("support").join(id))
        .args(if order.dlcs.is_empty() {
            vec!["--skip-dlcs".to_string()]
        } else {
            vec!["--with-dlcs".into(), "--dlcs".into(), order.dlcs.join(",")]
        })
        .args([
            "--lang",
            if order.language.is_empty() {
                "en-US"
            } else {
                &order.language
            },
        ]);
    Ok(cmd)
}

pub(super) fn install(id: &str, order: &Order) -> anyhow::Result<()> {
    // gogdl waits without end when the login ran out.
    credentials()?;
    let info = info(id, order.linux)?;
    let base = order.folder.parent().context("no games folder")?;
    let cmd = download_args("download", id, base, order.linux, order)?;
    check(&helper::run(cmd, &order.title, order.size)?)?;
    let folder = info
        .folder
        .as_ref()
        .map_or(order.folder.clone(), |f| base.join(f));
    let present = if order.linux {
        folder.join("start.sh").is_file()
    } else {
        folder.join(format!("goggame-{id}.info")).is_file()
    };
    anyhow::ensure!(present, "the download ended without the game's files");
    record(id, &folder, order.linux, &order.title, &info)?;
    post_install(&folder, order.linux);
    Ok(())
}

/// GOG's Linux games bring a script that finishes the installation.
fn post_install(folder: &Path, linux: bool) {
    let script = folder.join("support/postinst.sh");
    if linux && script.is_file() {
        let _ = std::process::Command::new("sh")
            .arg(&script)
            .current_dir(folder)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn record(id: &str, folder: &Path, linux: bool, title: &str, info: &Info) -> anyhow::Result<()> {
    let mut f = read()?;
    f.installed.retain(|e| e.app_name != id);
    f.installed.push(Entry {
        app_name: id.to_string(),
        install_path: folder.to_path_buf(),
        platform: platform(linux).to_string(),
        title: title.to_string(),
        version: info.version.clone(),
        build_id: info.build.clone(),
        is_dlc: false,
    });
    super::write_json(&file()?, &f)
}

pub(super) fn update(game: &Installed, order: &Order, repair: bool) -> anyhow::Result<()> {
    credentials()?;
    let info = (!repair).then(|| info(&game.id, game.linux)).transpose()?;
    let command = if repair { "repair" } else { "update" };
    let cmd = download_args(command, &game.id, &game.path, game.linux, order)?;
    check(&helper::run(cmd, &game.title, None)?)?;
    if let Some(info) = info {
        record(&game.id, &game.path, game.linux, &game.title, &info)?;
        post_install(&game.path, game.linux);
    }
    Ok(())
}

pub(super) fn uninstall(game: &Installed) -> anyhow::Result<()> {
    if game.path.exists() {
        let ours = game
            .path
            .join(format!("goggame-{}.info", game.id))
            .is_file()
            || game.path.join("start.sh").is_file()
            || game.path.join("gameinfo").is_file();
        anyhow::ensure!(
            ours && super::deletable(&game.path),
            "{} does not look like this game's folder; delete it by hand",
            game.path.display()
        );
        arkor_core::fs_guard::remove_dir(&game.path)?;
    }
    let mut f = read()?;
    f.installed.retain(|e| e.app_name != game.id);
    super::write_json(&file()?, &f)?;
    forget(&shop_dir(Shop::Gog)?, &game.id);
    Ok(())
}

/// Removes gogdl's notes on a game that is gone, so a new install starts clean.
pub(super) fn forget(dir: &Path, id: &str) {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return;
    }
    for d in ["heroic_gogdl/manifests", "manifests"] {
        let _ = std::fs::remove_file(dir.join(d).join(id));
    }
    let _ = std::fs::remove_dir_all(dir.join("support").join(id));
}

pub(super) fn updates(games: &[Installed]) -> anyhow::Result<Vec<(Installed, String)>> {
    Ok(games
        .iter()
        .filter_map(|g| {
            let newest = if g.linux {
                info(&g.id, true).ok()?.version
            } else {
                newest_build(&g.id).ok()?
            };
            (!newest.is_empty() && newest != g.version).then(|| (g.clone(), newest))
        })
        .collect())
}

/// The newest public build of a Windows game.
fn newest_build(id: &str) -> anyhow::Result<String> {
    let text = tools::http_get_quick(&format!(
        "https://content-system.gog.com/products/{id}/os/windows/builds?generation=2"
    ))?;
    let v: Value = serde_json::from_str(&text)?;
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|b| b["branch"].is_null())
        .and_then(|b| b["build_id"].as_str())
        .map(str::to_string)
        .context("no build")
}

#[cfg(test)]
pub(super) mod network {
    /// Network: GOG's public endpoints.
    #[test]
    #[ignore]
    fn public_endpoints() {
        println!("{}", super::cover_url("1207658924").unwrap());
        println!("{}", super::newest_build("1207658924").unwrap());
    }
}
