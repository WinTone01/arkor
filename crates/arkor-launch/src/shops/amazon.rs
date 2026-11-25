//! Amazon Games through nile.

use anyhow::Context;
use arkor_stores::amazon::Nile;
use serde::Deserialize;
use serde_json::Value;

use super::{Info, Installed, Order, Owned, Shop, check, helper, plain_code, shop_dir};

/// A started login (nile makes the keys, the code belongs to them).
#[derive(Debug, Clone, Deserialize)]
pub(super) struct Registration {
    client_id: String,
    code_verifier: String,
    serial: String,
    pub(super) url: String,
}

pub(super) fn begin_login() -> anyhow::Result<Registration> {
    if super::logged_in(Shop::Amazon) {
        // nile refuses a new login over one that ran out.
        logout()?;
    }
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args(["auth", "--login", "--non-interactive"]);
    let r: Registration = helper::output(cmd)?.json()?;
    anyhow::ensure!(
        r.url.starts_with("https://amazon.com/") || r.url.starts_with("https://www.amazon."),
        "unexpected login page"
    );
    Ok(r)
}

pub(super) fn find_code(text: &str) -> Option<String> {
    let text = text.trim();
    let code = if text.contains("authorization_code") {
        super::query_value(text, "openid.oa2.authorization_code")?
    } else {
        text.to_string()
    };
    // Bare codes are short; never take a long text for one.
    (plain_code(&code, 16) && code.len() <= 128).then_some(code)
}

pub(super) fn finish_login(r: &Registration, code: &str) -> anyhow::Result<String> {
    anyhow::ensure!(plain_code(code, 16), "that is not an Amazon login code");
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args(["register", "--code", code])
        .args(["--code-verifier", &r.code_verifier])
        .args(["--serial", &r.serial])
        .args(["--client-id", &r.client_id]);
    let out = helper::output(cmd)?;
    anyhow::ensure!(
        super::logged_in(Shop::Amazon),
        "Amazon did not accept the code: {}",
        out.error()
    );
    Ok(account_name().unwrap_or_default())
}

fn account_name() -> anyhow::Result<String> {
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args(["auth", "--status"]);
    let v: Value = helper::output(cmd)?.json()?;
    Ok(v["Username"].as_str().unwrap_or_default().to_string())
}

pub(super) fn logout() -> anyhow::Result<()> {
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args(["auth", "--logout"]);
    let _ = helper::output(cmd);
    let _ = std::fs::remove_file(shop_dir(Shop::Amazon)?.join("nile/current_user.json"));
    Ok(())
}

fn library() -> anyhow::Result<Vec<Value>> {
    super::read_json(&shop_dir(Shop::Amazon)?.join("nile/library.json"))
}

pub(super) fn owned() -> anyhow::Result<Vec<Owned>> {
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args(["library", "sync"]);
    check(&helper::output(cmd)?)?;
    Ok(library()?.iter().filter_map(parse_owned).collect())
}

pub(super) fn parse_owned(v: &Value) -> Option<Owned> {
    let product = &v["product"];
    let detail = &product["productDetail"];
    let url = |u: &Value| {
        u.as_str()
            .filter(|u| u.starts_with("https://"))
            .map(str::to_string)
    };
    Some(Owned {
        id: product["id"].as_str()?.to_string(),
        title: product["title"].as_str()?.trim().to_string(),
        cover: url(&detail["iconUrl"]),
        wide: url(&detail["details"]["backgroundUrl1"])
            .or_else(|| url(&detail["details"]["backgroundUrl2"])),
        linux: false,
        windows: true,
        namespace: None,
    })
}

pub(super) fn installed() -> anyhow::Result<Vec<Installed>> {
    let dir = arkor_core::paths::stores_dir()?.join("amazon");
    let nile = Nile {
        bin: "nile".into(),
        config_dir: dir,
    };
    let titles = nile.games()?;
    Ok(nile
        .installed()?
        .into_iter()
        .map(|g| Installed {
            shop: Shop::Amazon,
            title: titles
                .iter()
                .find(|t| t.id.id == g.id)
                .map_or_else(|| g.id.clone(), |t| t.name.clone()),
            id: g.id,
            path: g.path,
            version: g.version,
            linux: false,
            size: g.size,
        })
        .collect())
}

pub(super) fn info(id: &str) -> anyhow::Result<Info> {
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args(["install", "--info", "--json", id]);
    let v: Value = helper::output(cmd)?.json()?;
    let size = v["download_size"].as_u64().context("no download size")?;
    Ok(Info {
        // Amazon's files come as they are installed.
        download: Some(size),
        disk: Some(size),
        ..Info::default()
    })
}

pub(super) fn install(id: &str, order: &Order) -> anyhow::Result<()> {
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args(["install", "--path"]).arg(&order.folder).arg(id);
    let out = helper::run(cmd, &order.title, order.size)?;
    check(&out)?;
    // nile ends with 0 even when it gave up (no space, no manifest).
    let there = installed()?.iter().any(|g| g.id == id);
    anyhow::ensure!(there, "{}", out.error());
    Ok(())
}

pub(super) fn update(game: &Installed, repair: bool) -> anyhow::Result<()> {
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args([if repair { "verify" } else { "update" }, &game.id]);
    check(&helper::run(cmd, &game.title, None)?)
}

pub(super) fn uninstall(game: &Installed) -> anyhow::Result<()> {
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args(["uninstall", &game.id]);
    let out = helper::output(cmd)?;
    let left = installed()?.iter().any(|g| g.id == game.id);
    anyhow::ensure!(!left, "{}", out.error());
    Ok(())
}

pub(super) fn updates(games: &[Installed]) -> anyhow::Result<Vec<(Installed, String)>> {
    let mut cmd = helper::command(Shop::Amazon)?;
    cmd.args(["list-updates", "--json"]);
    let ids: Vec<String> = helper::output(cmd)?.json()?;
    Ok(games
        .iter()
        .filter(|g| ids.contains(&g.id))
        .map(|g| (g.clone(), String::new()))
        .collect())
}
