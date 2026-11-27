//! Epic Games Store through legendary.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;

use super::{Dlc, Info, Installed, Order, Owned, Shop, check, helper, plain_code, shop_dir};

/// Epic's login; it ends on a page with the code.
pub(super) const LOGIN_URL: &str = "https://legendary.gl/epiclogin";

pub(super) fn find_code(text: &str) -> Option<String> {
    let text = text.trim();
    // The page shows {"redirectUrl":...,"authorizationCode":"...",...}.
    let code = match text.split_once("authorizationCode") {
        Some((_, rest)) => {
            let start = rest.find(|c: char| c.is_ascii_hexdigit())?;
            let rest = &rest[start..];
            let end = rest
                .find(|c: char| !c.is_ascii_hexdigit())
                .unwrap_or(rest.len());
            &rest[..end]
        }
        None => text.trim_matches('"'),
    };
    (code.len() == 32 && code.chars().all(|c| c.is_ascii_hexdigit())).then(|| code.to_string())
}

pub(super) fn finish_login(code: &str) -> anyhow::Result<String> {
    anyhow::ensure!(plain_code(code, 32), "that is not an Epic login code");
    let mut cmd = helper::command(Shop::Epic)?;
    cmd.args(["auth", "--code", code]);
    let out = helper::output(cmd)?;
    // legendary ends with 0 either way; a login leaves user.json behind.
    anyhow::ensure!(super::logged_in(Shop::Epic), "{}", out.error());
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct User {
        #[serde(default)]
        display_name: String,
    }
    let user: User = super::read_json(&shop_dir(Shop::Epic)?.join("user.json"))?;
    Ok(user.display_name)
}

pub(super) fn logout() -> anyhow::Result<()> {
    let mut cmd = helper::command(Shop::Epic)?;
    cmd.args(["auth", "--delete"]);
    let _ = helper::output(cmd);
    // Whatever the helper said: the login must be gone from here.
    let _ = std::fs::remove_file(shop_dir(Shop::Epic)?.join("user.json"));
    Ok(())
}

fn list() -> anyhow::Result<Vec<Value>> {
    let mut cmd = helper::command(Shop::Epic)?;
    cmd.args(["list", "--json", "--platform", "Windows"]);
    helper::output(cmd)?.json()
}

pub(super) fn owned() -> anyhow::Result<Vec<Owned>> {
    Ok(list()?.iter().filter_map(parse_owned).collect())
}

/// A game legendary can install; not DLC, mods or games another launcher must install.
pub(super) fn parse_owned(v: &Value) -> Option<Owned> {
    let meta = &v["metadata"];
    let third_party = ["ThirdPartyManagedApp", "ThirdPartyManagedProvider"]
        .iter()
        .any(|k| !meta["customAttributes"][k]["value"].is_null());
    let mods = meta["categories"]
        .as_array()
        .is_some_and(|c| c.iter().any(|c| c["path"] == "mods"));
    if third_party || mods || !meta["mainGameItem"].is_null() {
        return None;
    }
    let image = |types: &[&str]| {
        let images = meta["keyImages"].as_array()?;
        types.iter().find_map(|t| {
            images
                .iter()
                .find(|i| i["type"] == *t)
                .and_then(|i| i["url"].as_str())
                .map(|u| u.replace(' ', "%20"))
        })
    };
    Some(Owned {
        id: v["app_name"].as_str()?.to_string(),
        title: v["app_title"].as_str()?.trim().to_string(),
        cover: image(&[
            "DieselGameBoxTall",
            "OfferImageTall",
            "DieselStoreFrontTall",
        ]),
        wide: image(&["DieselGameBox", "OfferImageWide", "DieselStoreFrontWide"]),
        linux: false,
        windows: true,
        namespace: meta["namespace"]
            .as_str()
            .or_else(|| v["namespace"].as_str())
            .map(str::to_string),
    })
}

#[derive(Deserialize)]
struct Entry {
    title: String,
    install_path: PathBuf,
    #[serde(default)]
    version: String,
    #[serde(default)]
    install_size: Option<u64>,
    #[serde(default)]
    is_dlc: bool,
}

pub(super) fn installed() -> anyhow::Result<Vec<Installed>> {
    let path = arkor_core::paths::stores_dir()?.join("epic/installed.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let games: BTreeMap<String, Entry> = super::read_json(&path)?;
    Ok(games
        .into_iter()
        .filter(|(_, g)| !g.is_dlc)
        .map(|(id, g)| Installed {
            shop: Shop::Epic,
            id,
            title: g.title,
            path: g.install_path,
            version: g.version,
            linux: false,
            size: g.install_size,
        })
        .collect())
}

pub(super) fn info(id: &str) -> anyhow::Result<Info> {
    let mut cmd = helper::command(Shop::Epic)?;
    cmd.args(["info", id, "--json", "--platform", "Windows"]);
    let v: Value = helper::output(cmd)?.json()?;
    let manifest = &v["manifest"];
    anyhow::ensure!(
        manifest["download_size"].is_number(),
        "Epic gave no download for this game"
    );
    Ok(Info {
        download: manifest["download_size"].as_u64(),
        disk: manifest["disk_size"].as_u64(),
        dlcs: parse_dlcs(&v),
        ..Info::default()
    })
}

/// DLC with files of their own (the others come with the game by themselves).
pub(super) fn parse_dlcs(v: &Value) -> Vec<Dlc> {
    v["game"]["owned_dlc"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| !d["installable"].is_null() && d["installable"] != false)
        .filter_map(|d| {
            Some(Dlc {
                id: d["app_name"].as_str()?.to_string(),
                title: d["title"].as_str()?.trim().to_string(),
                size: None,
            })
        })
        .collect()
}

/// Runs a legendary download; more shared memory when it asks for it.
fn download(args: &[String], order: &Order, total: Option<u64>) -> anyhow::Result<()> {
    let attempt = |extra: &[&str]| -> anyhow::Result<helper::Output> {
        let mut cmd = helper::command(Shop::Epic)?;
        cmd.arg("-y").args(args).args(extra);
        helper::run(cmd, &order.title, total)
    };
    let mut out = attempt(&[])?;
    if !out.ok && (out.stderr.contains("MemoryError") || out.stderr.contains("shared memory")) {
        out = attempt(&["--max-shared-memory", "5000"])?;
    }
    check(&out)
}

/// The chosen DLC, one after the other (legendary puts them into the game's folder).
fn dlcs(command: &str, order: &Order) -> anyhow::Result<()> {
    for dlc in &order.dlcs {
        let args = [
            command.to_string(),
            dlc.clone(),
            "--skip-sdl".into(),
            "--skip-dlcs".into(),
        ];
        download(&args, order, None)?;
    }
    Ok(())
}

pub(super) fn install(id: &str, order: &Order) -> anyhow::Result<()> {
    let base = order.folder.parent().context("no games folder")?;
    let name = order.folder.file_name().context("no folder name")?;
    let args = [
        "install".into(),
        id.to_string(),
        "--platform".into(),
        "Windows".into(),
        "--base-path".into(),
        base.display().to_string(),
        "--game-folder".into(),
        name.to_string_lossy().into_owned(),
        "--skip-sdl".into(),
        "--skip-dlcs".into(),
    ];
    download(&args, order, order.size)?;
    dlcs("install", order)
}

pub(super) fn update(game: &Installed, order: &Order, repair: bool) -> anyhow::Result<()> {
    let args = [
        if repair { "repair" } else { "update" }.into(),
        game.id.clone(),
        "--skip-sdl".into(),
        "--skip-dlcs".into(),
    ];
    download(&args, order, None)?;
    dlcs(if repair { "repair" } else { "update" }, order)
}

pub(super) fn uninstall(game: &Installed) -> anyhow::Result<()> {
    let mut cmd = helper::command(Shop::Epic)?;
    cmd.args(["-y", "uninstall", &game.id, "--skip-uninstaller"]);
    let out = helper::output(cmd)?;
    let left = installed()?.iter().any(|g| g.id == game.id);
    anyhow::ensure!(!left, "{}", out.error());
    Ok(())
}

pub(super) fn updates(games: &[Installed]) -> anyhow::Result<Vec<(Installed, String)>> {
    let list = list()?;
    Ok(games
        .iter()
        .filter_map(|g| {
            let entry = list.iter().find(|v| v["app_name"] == g.id.as_str())?;
            let newest = entry["asset_infos"]["Windows"]["build_version"].as_str()?;
            (newest != g.version).then(|| (g.clone(), newest.to_string()))
        })
        .collect())
}

/// Epic's overlay (EOS: friends, achievements and invites in games that use Epic Online
/// Services), as legendary installs it once for every game and turns on per prefix.
pub fn eos_overlay(prefix: &std::path::Path, on: bool) -> anyhow::Result<()> {
    if on {
        let mut install = helper::command(Shop::Epic)?;
        install.args(["-y", "eos-overlay", "install"]);
        let out = helper::output(install)?;
        anyhow::ensure!(
            out.ok,
            "the Epic overlay was not installed: {}",
            out.error()
        );
    }
    let mut cmd = helper::command(Shop::Epic)?;
    cmd.args([
        "-y",
        "eos-overlay",
        if on { "enable" } else { "disable" },
        "--prefix",
    ])
    .arg(prefix);
    let out = helper::output(cmd)?;
    anyhow::ensure!(out.ok, "{}", out.error());
    Ok(())
}

/// Whether Epic's overlay is on in `prefix` (its registry names the overlay's folder).
pub fn eos_overlay_on(prefix: &std::path::Path) -> bool {
    std::fs::read_to_string(prefix.join("user.reg"))
        .is_ok_and(|r| r.contains("EOSOverlayPath") || r.contains("EOS Overlay"))
}
