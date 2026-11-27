//! Mods, through Amethyst Mod Manager running as a headless service: Arkor's own screens on
//! top, Amethyst's knowledge of each game (where mods go, its installers, its loaders' DLL
//! overrides) underneath. Mods are placed into the game folder as links (`deploy`) just
//! before `arkor run` starts the game and taken out when it ends (`play`), or, when the game
//! does not start through Arkor or that is turned off, whenever they change.
//!
//! Everything here is blocking: call it off the UI thread.

pub mod amethyst;
pub mod link;
pub mod nexus;
pub mod play;
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
    /// What it needs that is not installed: mods Nexus names and Amethyst found missing,
    /// and tools outside Nexus (a loader like MelonLoader) that nobody can check.
    pub needs: Vec<String>,
}

/// A mod's `requirements.nexus`: the missing ones, then the outside tools (mod id 0).
fn needs_of(m: &Value) -> Vec<String> {
    let nexus = &m["requirements"]["nexus"];
    let names = |k: &str, outside: bool| -> Vec<String> {
        nexus[k]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|r| !outside || r["mod_id"].as_u64() == Some(0))
                    .filter_map(|r| r["name"].as_str())
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out = names("missing", false);
    for n in names("all", true) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&n)) {
            out.push(n);
        }
    }
    out
}

/// The mod loader a game's mods are placed for but which is not in its folder:
/// `BepInEx` when there are plugins and no BepInEx core.
pub fn missing_loader(game_dir: &Path) -> Option<&'static str> {
    let bepinex = game_dir.join("BepInEx");
    let plugins = std::fs::read_dir(bepinex.join("plugins")).is_ok_and(|mut d| {
        d.any(|e| e.is_ok_and(|e| !e.file_name().to_string_lossy().starts_with('.')))
    });
    (plugins && !bepinex.join("core").is_dir()).then_some("BepInEx")
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
                        needs: needs_of(m),
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

/// Switches a mod on or off, then settles the game folder ([`settle`]).
pub fn set_enabled(c: &mut Client, link: &Link, name: &str, on: bool) -> anyhow::Result<()> {
    c.job(
        "mods.set_enabled.start",
        params(link, json!({"changes": [{"name": name, "enabled": on}]})),
        |_| {},
    )?;
    settle(c, link)
}

/// Moves a mod one place up (higher priority) or down, then settles the game folder.
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
    settle(c, link)
}

/// Removes a mod for good (its files leave the game folder too).
pub fn remove(c: &mut Client, link: &Link, name: &str) -> anyhow::Result<()> {
    c.job(
        "mods.remove.start",
        params(link, json!({"mods": [name], "confirm": true})),
        |_| {},
    )?;
    settle(c, link)
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
            settle(c, link)?;
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

/// Installs a Thunderstore package with what it depends on (BepInEx…), the newest version
/// unless one is named, then places the mods; the package's name. No login is needed.
pub fn install_thunderstore(
    c: &mut Client,
    link: &Link,
    namespace: &str,
    name: &str,
    version: Option<&str>,
) -> anyhow::Result<String> {
    let version = match version {
        Some(v) => v.to_string(),
        None => {
            let v = c.job(
                "thunderstore.packages.versions.start",
                json!({"game": link.game, "namespace": namespace, "package": name}),
                |_| {},
            )?;
            newest_version(&v["version_catalog"])
                .ok_or_else(|| anyhow::anyhow!("{namespace}-{name} has no version to install"))?
        }
    };
    let plan = c.job(
        "thunderstore.install.plan.start",
        params(
            link,
            json!({"namespace": namespace, "package": name, "version": version}),
        ),
        |_| {},
    )?;
    let plan = &plan["plan"];
    c.job(
        "thunderstore.install.start",
        params(
            link,
            json!({
                "plan_id": plan["id"],
                "revision": plan["revision"],
                "confirm": true,
                "if_exists": "replace",
                "installer_policy": "defaults",
            }),
        ),
        |_| {},
    )?;
    settle(c, link)?;
    Ok(format!("{namespace}-{name}"))
}

/// The version a catalogue recommends, else its newest.
fn newest_version(catalog: &Value) -> Option<String> {
    catalog["recommended_version"]
        .as_str()
        .or_else(|| catalog["versions"][0]["version"].as_str())
        .map(str::to_string)
}

/// The game's BepInEx package on Thunderstore (`BepInEx/BepInExPack`, or one made for the
/// game such as `denikson/BepInExPack_Valheim`), when its community has one.
pub fn find_bepinex(c: &mut Client, link: &Link) -> anyhow::Result<Option<(String, String)>> {
    let v = c.job(
        "thunderstore.packages.search.start",
        json!({"game": link.game, "query": "BepInExPack", "sort": "most-downloaded"}),
        |_| {},
    )?;
    Ok(pick_bepinex(&v["packages"]))
}

fn pick_bepinex(packages: &Value) -> Option<(String, String)> {
    packages
        .as_array()?
        .iter()
        .filter_map(|p| Some((p["namespace"].as_str()?, p["name"].as_str()?)))
        .find(|(_, n)| n.starts_with("BepInExPack"))
        .map(|(ns, n)| (ns.to_string(), n.to_string()))
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

/// After the mods changed: placed in the game folder again, or, when they are there only
/// while the game runs, kept out of it.
pub fn settle(c: &mut Client, link: &Link) -> anyhow::Result<()> {
    if link.while_playing {
        restore(c, link)
    } else {
        deploy(c, link)
    }
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
        // BepInEx's loader is handed to Wine at launch, whatever the prefix says.
        assert_eq!(
            link::launch_overrides(&id),
            [
                ("version".into(), "n,b".into()),
                ("winhttp".into(), "n,b".into())
            ]
        );

        let got = install(&mut c, &link, Path::new("/home/user/ArkorTest.zip"), None).unwrap();
        assert_eq!(got, Installed::Done("ArkorTest".into()));
        // Mods only while playing (the default): installed, but not in the game folder until
        // a launch places them; the folder is seen from outside with `ARKOR_TEST_AMETHYST_HOME`.
        assert!(link.while_playing);
        if let Ok(home) = std::env::var("ARKOR_TEST_AMETHYST_HOME") {
            let plugins = Path::new(&home).join("Games/Valheim/BepInEx/plugins");
            let placed = || {
                std::fs::read_dir(&plugins)
                    .map(|d| {
                        d.flatten()
                            .any(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    })
                    .unwrap_or(false)
            };
            assert!(!placed(), "out of the folder after the install");
            deploy(&mut c, &link).unwrap();
            assert!(placed(), "placed for a launch");
            settle(&mut c, &link).unwrap();
            assert!(!placed(), "out again after it");
            let always = Link {
                while_playing: false,
                ..link.clone()
            };
            settle(&mut c, &always).unwrap();
            assert!(placed(), "always in the folder when turned off");
            settle(&mut c, &link).unwrap();
        }
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

    /// BepInEx from Thunderstore for R.E.P.O., over the network: `ARKOR_TEST_AMETHYST_NET`
    /// starts Amethyst in a sandbox with network whose home has `Games/REPO/REPO.exe` and
    /// a prefix at `Games/pfx`.
    #[test]
    #[ignore = "needs a sandboxed Amethyst with network"]
    fn bepinex_from_thunderstore() {
        let serve = std::env::var("ARKOR_TEST_AMETHYST_NET").expect("ARKOR_TEST_AMETHYST_NET");
        let mut c = Client::spawn(std::process::Command::new(serve)).unwrap();
        let games = link::games(&mut c).unwrap();
        let repo = link::best_match(&games, "R.E.P.O.", Some("3241660"))
            .unwrap()
            .clone();
        let place = link::Place {
            game_dir: "/home/user/Games/REPO".into(),
            prefix: Some("/home/user/Games/pfx".into()),
            steam_appid: None,
        };
        let id = arkor_core::GameId::new(arkor_core::Store::Local, "repo-test");
        let link = link::connect(&mut c, &id, &repo, &place).unwrap();
        let (ns, name) = find_bepinex(&mut c, &link)
            .unwrap()
            .expect("a BepInEx pack");
        eprintln!("pack: {ns}/{name}");
        install_thunderstore(&mut c, &link, &ns, &name, None).unwrap();
        let mods = list(&mut c, &link).unwrap();
        eprintln!(
            "mods: {:?}",
            mods.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
        assert!(!mods.is_empty());
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
        assert!(mods[0].needs.is_empty());
    }

    #[test]
    fn says_what_a_mod_still_needs() {
        let m: Value = serde_json::from_str(
            r#"{"requirements":{"nexus":{
              "all":[{"mod_id":0,"name":"Melonloader"},{"mod_id":7,"name":"Core Lib"}],
              "missing":[{"mod_id":7,"name":"Core Lib"}]}}}"#,
        )
        .unwrap();
        assert_eq!(needs_of(&m), ["Core Lib", "Melonloader"]);
        assert!(needs_of(&json!({})).is_empty());
    }

    #[test]
    fn picks_the_bepinex_pack_and_its_version() {
        let found = json!([
            {"namespace": "Someone", "name": "MoreSlots"},
            {"namespace": "denikson", "name": "BepInExPack_Valheim"},
            {"namespace": "BepInEx", "name": "BepInExPack"}
        ]);
        assert_eq!(
            pick_bepinex(&found),
            Some(("denikson".into(), "BepInExPack_Valheim".into())),
            "the most downloaded one that is a pack"
        );
        assert_eq!(pick_bepinex(&json!([])), None);
        assert_eq!(
            newest_version(
                &json!({"recommended_version": "5.4.2100", "versions": [{"version": "6.0"}]})
            ),
            Some("5.4.2100".into())
        );
        assert_eq!(
            newest_version(
                &json!({"recommended_version": null, "versions": [{"version": "1.2.0"}]})
            ),
            Some("1.2.0".into())
        );
    }

    #[test]
    fn notices_bepinex_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let g = tmp.path();
        assert_eq!(missing_loader(g), None, "no mods: nothing to say");
        std::fs::create_dir_all(g.join("BepInEx/plugins")).unwrap();
        std::fs::write(g.join("BepInEx/plugins/.mm_deployed"), "").unwrap();
        assert_eq!(missing_loader(g), None, "only Amethyst's marker");
        std::fs::write(g.join("BepInEx/plugins/Mod.dll"), "").unwrap();
        assert_eq!(missing_loader(g), Some("BepInEx"));
        std::fs::create_dir_all(g.join("BepInEx/core")).unwrap();
        assert_eq!(missing_loader(g), None);
    }

    #[test]
    fn adds_the_game_and_profile_to_every_request() {
        let link = Link {
            game: "Valheim".into(),
            profile: "default".into(),
            ..Link::default()
        };
        let p = params(&link, json!({"mods": ["A"]}));
        assert_eq!(p["game"], "Valheim");
        assert_eq!(p["profile"], "default");
        assert_eq!(p["mods"][0], "A");
    }
}
