//! Mods, through Amethyst Mod Manager running as a headless service: Arkor's own screens on
//! top, Amethyst's knowledge of each game (where mods go, its installers, its loaders' DLL
//! overrides) underneath. Mods are placed into the game folder as links (`deploy`) whenever
//! they change, so the game itself still starts the way Arkor starts it.
//!
//! Everything here is blocking: call it off the UI thread.

pub mod amethyst;
pub mod link;
pub mod nexus;
pub mod rpc;

use std::path::Path;

use serde_json::{Value, json};

pub use link::Link;
use rpc::{Client, RpcError};

/// One installed mod, highest priority first in a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mod {
    /// Amethyst's name for it (what every change names).
    pub name: String,
    /// What to show.
    pub title: String,
    pub enabled: bool,
    /// Amethyst keeps it where it is (a collection's, a framework's).
    pub locked: bool,
    pub version: Option<String>,
    pub update: bool,
    /// From Nexus Mods (its page), or Thunderstore.
    pub url: Option<String>,
}

/// The mods of a linked game, highest priority first (separators left out).
pub fn list(c: &mut Client, link: &Link) -> anyhow::Result<Vec<Mod>> {
    let v = c.call(
        "mods.list",
        json!({"game": link.game, "profile": link.profile}),
    )?;
    Ok(parse_mods(&v))
}

fn parse_mods(v: &Value) -> Vec<Mod> {
    v["mods"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|m| m["kind"] == "mod")
                .filter_map(|m| {
                    let name = m["name"].as_str()?.to_string();
                    let sources = &m["sources"];
                    let url = sources["nexus"]["url"]
                        .as_str()
                        .or_else(|| sources["thunderstore"]["website_url"].as_str())
                        .map(str::to_string);
                    Some(Mod {
                        title: m["display_name"]
                            .as_str()
                            .filter(|t| !t.is_empty())
                            .unwrap_or(&name)
                            .to_string(),
                        name,
                        enabled: m["enabled"].as_bool().unwrap_or(false),
                        locked: m["locked"].as_bool().unwrap_or(false),
                        version: m["version"].as_str().map(str::to_string),
                        update: m["update_available"].as_bool().unwrap_or(false),
                        url,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn params(link: &Link, extra: Value) -> Value {
    let mut p = json!({"game": link.game, "profile": link.profile});
    if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    p
}

/// Switches a mod on or off, then places the mods in the game folder again.
pub fn set_enabled(c: &mut Client, link: &Link, name: &str, on: bool) -> anyhow::Result<()> {
    c.job(
        "mods.set_enabled.start",
        params(link, json!({"changes": [{"name": name, "enabled": on}]})),
        |_| {},
    )?;
    deploy(c, link)
}

/// Moves a mod one place up (higher priority) or down, then places the mods again.
pub fn shift(c: &mut Client, link: &Link, name: &str, up: bool) -> anyhow::Result<()> {
    let mods = list(c, link)?;
    let Some(at) = mods.iter().position(|m| m.name == name) else {
        anyhow::bail!("{name} is not installed");
    };
    let anchor = if up {
        at.checked_sub(1).map(|i| json!({"before": mods[i].name}))
    } else {
        mods.get(at + 1).map(|m| json!({"after": m.name}))
    };
    let Some(position) = anchor else {
        return Ok(());
    };
    c.job(
        "mods.move.start",
        params(link, json!({"mods": [name], "position": position})),
        |_| {},
    )?;
    deploy(c, link)
}

/// Removes a mod for good (its files leave the game folder too).
pub fn remove(c: &mut Client, link: &Link, name: &str) -> anyhow::Result<()> {
    c.job(
        "mods.remove.start",
        params(link, json!({"mods": [name], "confirm": true})),
        |_| {},
    )?;
    deploy(c, link)
}

/// What an archive's installer asks before it installs (a FOMOD's steps, BAIN packages,
/// an RE Engine bundle's options), as Amethyst describes them.
#[derive(Debug, Clone, PartialEq)]
pub struct Choices(pub Value);

impl Choices {
    /// `fomod`, `bain` or `re_engine_bundle`.
    pub fn installer(&self) -> &str {
        self.0["installer"].as_str().unwrap_or_default()
    }

    /// The FOMOD selections its author recommends (`{step: {group: [options]}}`).
    pub fn defaults(&self) -> Value {
        self.0["defaults"].clone()
    }
}

/// How an archive went in.
#[derive(Debug, Clone, PartialEq)]
pub enum Installed {
    /// It is installed (and placed in the game folder) under this name.
    Done(String),
    /// Its installer has questions; answer them with [`Answer`] and install again.
    Asks(Choices),
}

/// Answers to an installer's questions.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// The installer's own recommendation.
    Defaults,
    /// FOMOD: `{step index: {group name: [option names]}}`.
    Fomod(Value),
}

/// Installs an archive (zip, 7z, rar…) as a mod, replacing one of the same name, then
/// places the mods in the game folder.
pub fn install(
    c: &mut Client,
    link: &Link,
    archive: &Path,
    answer: Option<&Answer>,
) -> anyhow::Result<Installed> {
    let mut p = params(
        link,
        json!({"archive_path": archive.to_string_lossy(), "if_exists": "replace"}),
    );
    match answer {
        Some(Answer::Defaults) => p["installer_policy"] = json!("defaults"),
        Some(Answer::Fomod(sel)) => p["fomod_selections"] = sel.clone(),
        None => {}
    }
    match c.job("downloads.install.start", p, |_| {}) {
        Ok(result) => {
            deploy(c, link)?;
            let name = result["mod"]["name"]
                .as_str()
                .or_else(|| result["mods"][0]["name"].as_str())
                .unwrap_or_default()
                .to_string();
            Ok(Installed::Done(name))
        }
        Err(e) => match e.downcast_ref::<RpcError>() {
            Some(r) if r.name == "installer_choices_required" => {
                Ok(Installed::Asks(Choices(r.details.clone())))
            }
            _ => Err(e),
        },
    }
}

/// Places the profile's mods in the game folder (as links). Blocking.
pub fn deploy(c: &mut Client, link: &Link) -> anyhow::Result<()> {
    c.job(
        "deploy.start",
        params(link, json!({"options": {"backup": false}})),
        |_| {},
    )?;
    Ok(())
}

/// Takes every mod out of the game folder again (the mods stay installed). Blocking.
pub fn restore(c: &mut Client, link: &Link) -> anyhow::Result<()> {
    c.job("restore.start", params(link, json!({})), |_| {})?;
    Ok(())
}

/// The DLL overrides Amethyst set in the game's prefix for its mod loaders
/// (`winhttp` → `native,builtin` for BepInEx…).
pub fn dll_overrides(c: &mut Client, link: &Link) -> anyhow::Result<Vec<(String, String)>> {
    let v = c.call("wine.dll_overrides.get", params(link, json!({})))?;
    let map = &v["wine_overrides"]["configured_overrides"];
    Ok(map
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against a real Amethyst: `ARKOR_TEST_AMETHYST` names a program that starts its headless
    /// service in a sandbox whose home has `Games/Valheim/valheim.exe`, an Arkor-style
    /// prefix at `Games/Arkor/Prefixes/valheim/pfx`, `ArkorTest.zip` (a BepInEx plugin) and
    /// `FomodTest.zip` (a FOMOD asking for "Texture Quality"); run with a scratch
    /// `XDG_CONFIG_HOME`, since the link is saved.
    #[test]
    #[ignore = "needs a sandboxed Amethyst"]
    fn with_a_real_amethyst() {
        let serve = std::env::var("ARKOR_TEST_AMETHYST").expect("ARKOR_TEST_AMETHYST");
        let mut c = Client::spawn(std::process::Command::new(serve)).unwrap();
        let games = link::games(&mut c).unwrap();
        let valheim = link::best_match(&games, "Valheim", Some("892970"))
            .unwrap()
            .clone();
        let place = link::Place {
            game_dir: "/home/user/Games/Valheim".into(),
            prefix: Some("/home/user/Games/Arkor/Prefixes/valheim/pfx".into()),
            steam_appid: Some("892970".into()),
        };
        // Its paths are inside the sandbox, so the link is made without the folder check.
        let id = arkor_core::GameId::new(arkor_core::Store::Local, "valheim-test");
        let link = link::connect(&mut c, &id, &valheim, &place).unwrap();
        assert_eq!(link::load(&id), Some(link.clone()));

        let got = install(&mut c, &link, Path::new("/home/user/ArkorTest.zip"), None).unwrap();
        assert_eq!(got, Installed::Done("ArkorTest".into()));
        let Installed::Asks(choices) =
            install(&mut c, &link, Path::new("/home/user/FomodTest.zip"), None).unwrap()
        else {
            panic!("a FOMOD asks first");
        };
        assert_eq!(choices.installer(), "fomod");
        assert_eq!(choices.defaults()["0"]["Texture Quality"][0], "High");
        let low = Answer::Fomod(json!({"0": {"Texture Quality": ["Low"]}}));
        let path = Path::new("/home/user/FomodTest.zip");
        assert!(matches!(
            install(&mut c, &link, path, Some(&low)).unwrap(),
            Installed::Done(_)
        ));

        let mods = list(&mut c, &link).unwrap();
        let names: Vec<&str> = mods.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        let first = names[0].to_string();
        shift(&mut c, &link, &first, false).unwrap();
        assert_eq!(list(&mut c, &link).unwrap()[1].name, first, "moved down");
        set_enabled(&mut c, &link, "ArkorTest", false).unwrap();
        assert!(
            !list(&mut c, &link)
                .unwrap()
                .iter()
                .any(|m| m.name == "ArkorTest" && m.enabled)
        );
        let dlls = dll_overrides(&mut c, &link).unwrap();
        assert!(
            dlls.iter()
                .any(|(k, v)| k == "winhttp" && v.starts_with("native")),
            "{dlls:?}"
        );
        remove(&mut c, &link, "ArkorTest").unwrap();
        assert!(!nexus::signed_in(&mut c).unwrap());
        assert_eq!(nexus::account(&mut c).unwrap(), None);
        let page = nexus::login_start(&mut c).unwrap();
        assert!(page.starts_with("https://"), "{page}");
        nexus::login_cancel(&mut c).unwrap();
        assert_eq!(list(&mut c, &link).unwrap().len(), 1);
        restore(&mut c, &link).unwrap();
    }

    #[test]
    fn reads_a_mod_list() {
        let v: Value = serde_json::from_str(
            r#"{"order":"highest_priority_first","mods":[
              {"kind":"separator","name":"Gameplay_separator","display_name":"Gameplay","enabled":true},
              {"kind":"mod","name":"ArkorTest","display_name":"ArkorTest","enabled":true,"locked":false,
               "version":"1.2","update_available":true,
               "sources":{"nexus":{"url":"https://www.nexusmods.com/valheim/mods/1"},"thunderstore":null}},
              {"kind":"mod","name":"Tex","display_name":"","enabled":false,"locked":true,"version":null,
               "update_available":false,"sources":{"nexus":null,"thunderstore":{"website_url":"https://thunderstore.io/x"}}}
            ]}"#,
        )
        .unwrap();
        let mods = parse_mods(&v);
        assert_eq!(mods.len(), 2, "the separator is left out");
        assert_eq!(mods[0].version.as_deref(), Some("1.2"));
        assert!(mods[0].update && mods[0].enabled);
        assert_eq!(
            mods[0].url.as_deref(),
            Some("https://www.nexusmods.com/valheim/mods/1")
        );
        assert_eq!(
            mods[1].title, "Tex",
            "an empty display name falls back to the name"
        );
        assert!(mods[1].locked && !mods[1].enabled);
        assert_eq!(mods[1].url.as_deref(), Some("https://thunderstore.io/x"));
    }

    #[test]
    fn adds_the_game_and_profile_to_every_request() {
        let link = Link {
            game: "Valheim".into(),
            profile: "default".into(),
        };
        let p = params(&link, json!({"mods": ["A"]}));
        assert_eq!(p["game"], "Valheim");
        assert_eq!(p["profile"], "default");
        assert_eq!(p["mods"][0], "A");
    }
}
