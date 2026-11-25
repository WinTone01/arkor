use std::os::unix::fs::PermissionsExt;

use super::*;

/// A stand-in helper: a shell script in `dir`.
fn fake(dir: &Path, name: &str, script: &str) {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A home of its own for a test, with the lock that keeps tests apart.
struct Home {
    tmp: tempfile::TempDir,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Home {
    fn new() -> Self {
        let lock = crate::CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("bin")).unwrap();
        // SAFETY: tests that change the environment hold CONFIG_ENV_LOCK.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().join("data"));
            std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("config"));
            std::env::set_var("XDG_CACHE_HOME", tmp.path().join("cache"));
            std::env::set_var("ARKOR_STORE_HELPERS", tmp.path().join("bin"));
        }
        Home { tmp, _lock: lock }
    }

    fn helper(&self, name: &str, script: &str) {
        fake(&self.tmp.path().join("bin"), name, script);
    }

    fn games(&self) -> PathBuf {
        self.tmp.path().join("games")
    }

    fn store(&self, shop: Shop) -> PathBuf {
        shop_dir(shop).unwrap()
    }
}

fn game(id: &str, title: &str) -> Owned {
    Owned {
        id: id.into(),
        title: title.into(),
        cover: None,
        wide: None,
        linux: false,
        windows: true,
        namespace: None,
    }
}

#[test]
fn login_codes_are_found_in_what_is_pasted() {
    let epic = "0123456789abcdef0123456789ABCDEF";
    let page = format!(r#"{{"warning":"x","authorizationCode":"{epic}","sid":null}}"#);
    assert_eq!(find_code(Shop::Epic, &page).as_deref(), Some(epic));
    assert_eq!(
        find_code(Shop::Epic, &format!(" {epic}\n")).as_deref(),
        Some(epic)
    );
    assert_eq!(find_code(Shop::Epic, "hello"), None);
    assert_eq!(
        find_code(Shop::Epic, r#"{"authorizationCode":null}"#),
        None,
        "not logged in on Epic's page"
    );

    let code = "a-B_c".repeat(12);
    let url = format!("https://embed.gog.com/on_login_success?origin=client&code={code}&x=1");
    assert_eq!(find_code(Shop::Gog, &url), Some(code.clone()));
    assert_eq!(find_code(Shop::Gog, &code), Some(code));
    assert_eq!(find_code(Shop::Gog, "https://www.gog.com/"), None);
    assert_eq!(
        find_code(Shop::Gog, &"--delete".repeat(10)),
        None,
        "never an option"
    );

    let url = "https://www.amazon.com/?openid.assoc_handle=amzn&openid.oa2.authorization_code=ANabcDEFghiJKLmno&openid.x=1";
    assert_eq!(
        find_code(Shop::Amazon, url).as_deref(),
        Some("ANabcDEFghiJKLmno")
    );
    assert_eq!(find_code(Shop::Amazon, "https://www.amazon.com/"), None);
}

#[test]
fn dlc_with_files_are_offered() {
    let v = serde_json::json!({"game": {"owned_dlc": [
        {"app_name": "d1", "title": "Season Pass ", "installable": true},
        {"app_name": null, "title": "Skin", "installable": null},
        {"app_name": "d3", "title": "Soundtrack", "installable": false}]}});
    let dlcs = epic::parse_dlcs(&v);
    assert_eq!(dlcs.len(), 1);
    assert_eq!(
        (dlcs[0].id.as_str(), dlcs[0].title.as_str()),
        ("d1", "Season Pass")
    );
    let info = gog::parse_info(&serde_json::json!({"dlcs": [
        {"id": 7, "title": "Hearts", "size": {"en-US": {"download_size": 5}}}]}));
    assert_eq!(info.dlcs[0].id, "7");
    assert_eq!(info.dlcs[0].size, Some(5));
    // Only what the store offers is ordered.
    let o = order(
        &Owned {
            id: "1".into(),
            title: "T".into(),
            cover: None,
            wide: None,
            linux: false,
            windows: true,
            namespace: None,
        },
        &info,
        Path::new("/tmp/games"),
        false,
        "",
        vec!["7".into(), "99".into()],
    );
    assert_eq!(o.dlcs, ["7"]);
}

#[test]
fn query_values_are_decoded() {
    assert_eq!(
        query_value("https://a/?x=1&code=a%2Db#f", "code").unwrap(),
        "a-b"
    );
    assert_eq!(query_value("https://a/?barcode=1", "code"), None);
    assert_eq!(query_value("code=%zz", "code").unwrap(), "%zz");
}

#[test]
fn folder_names_are_safe() {
    assert_eq!(folder_name("Tomb: Raider™ / 2", "1"), "Tomb Raider 2");
    assert_eq!(folder_name("..", "abc-1"), "abc1");
    assert_eq!(folder_name("../../etc", "1"), "etc");
}

#[test]
fn keys_name_their_game() {
    assert_eq!(key(Shop::Gog, "12"), "game:gog:12");
    assert_eq!(
        parse_key("game:amazon:amzn1.adg.product.x"),
        Some((Shop::Amazon, "amzn1.adg.product.x", false))
    );
    assert_eq!(
        parse_key("repair:epic:Owl"),
        Some((Shop::Epic, "Owl", true))
    );
    assert_eq!(parse_key("mako"), None);
    assert_eq!(
        parse_key("game:steam:620"),
        Some((Shop::Steam, "620", false))
    );
    assert_eq!(parse_key("game:origin:1"), None);
}

#[test]
fn reads_what_the_stores_list() {
    let epic: serde_json::Value = serde_json::from_str(
        r#"{"app_name":"Owl","app_title":"Owl Game ","metadata":{"keyImages":[
            {"type":"DieselGameBox","url":"https://cdn/w ide.jpg"},
            {"type":"DieselGameBoxTall","url":"https://cdn/tall.jpg"}],
            "customAttributes":{},"categories":[{"path":"games"}]}}"#,
    )
    .unwrap();
    let g = epic::parse_owned(&epic).unwrap();
    assert_eq!((g.id.as_str(), g.title.as_str()), ("Owl", "Owl Game"));
    assert_eq!(g.cover.as_deref(), Some("https://cdn/tall.jpg"));
    assert_eq!(g.wide.as_deref(), Some("https://cdn/w%20ide.jpg"));
    let mut dlc = epic.clone();
    dlc["metadata"]["mainGameItem"] = serde_json::json!({"id": "x"});
    assert!(epic::parse_owned(&dlc).is_none());
    let mut ea = epic.clone();
    ea["metadata"]["customAttributes"] =
        serde_json::json!({"ThirdPartyManagedApp": {"value": "The EA App"}});
    assert!(
        epic::parse_owned(&ea).is_none(),
        "the EA app would install it"
    );

    let page: serde_json::Value = serde_json::from_str(
        r#"{"totalPages":1,"products":[
            {"id":1,"title":"Both","isGame":true,"image":"//img/a","worksOn":{"Windows":true,"Mac":true,"Linux":true}},
            {"id":2,"title":"Mac","isGame":true,"worksOn":{"Windows":false,"Mac":true,"Linux":false}}]}"#,
    )
    .unwrap();
    let list = gog::parse_page(&page);
    assert_eq!(list.len(), 1, "nothing to run the Mac game with");
    assert!(list[0].linux && list[0].windows);
    assert_eq!(list[0].wide.as_deref(), Some("https://img/a.jpg"));

    let info = gog::parse_info(
        &serde_json::from_str(
            r#"{"size":{"*":{"download_size":10,"disk_size":20},
                        "en-US":{"download_size":1,"disk_size":2},
                        "tr-TR":{"download_size":3,"disk_size":4}},
                "languages":["en-US","tr-TR"],"folder_name":"Both Game","buildId":"77",
                "versionName":"1.1","dlcs":[]}"#,
        )
        .unwrap(),
    );
    assert_eq!((info.download, info.disk), (Some(13), Some(24)));
    assert_eq!(info.folder.as_deref(), Some("Both Game"));
    let bad = gog::parse_info(&serde_json::json!({"folder_name": "../x"}));
    assert_eq!(bad.folder, None);

    let amazon = serde_json::json!({"id":"e","product":{"id":"amzn1.a","title":"Fox",
        "productDetail":{"iconUrl":"https://m/i.png","details":{"backgroundUrl2":"https://m/b.png"}}}});
    let g = amazon::parse_owned(&amazon).unwrap();
    assert_eq!(g.cover.as_deref(), Some("https://m/i.png"));
    assert_eq!(g.wide.as_deref(), Some("https://m/b.png"));
}

#[test]
fn epic_login_install_and_uninstall() {
    let home = Home::new();
    home.helper(
        "legendary",
        r#"cd "$LEGENDARY_CONFIG_PATH" || exit 9
echo "$@" >> calls
case "$*" in
  "auth --code 0123456789abcdef0123456789abcdef") echo '{"displayName":"Tester"}' > user.json;;
  "auth --code"*) echo '[cli] ERROR: Login attempt failed, please see log for details.' >&2;;
  "auth --delete") rm -f user.json;;
  "list --json"*) echo '[cli] INFO: Logging in...' >&2
    echo '[{"app_name":"Owl","app_title":"Owl Game","asset_infos":{"Windows":{"build_version":"2.0"}},"metadata":{"keyImages":[]}}]';;
  "info Owl"*) echo '{"game":{"owned_dlc":[]},"manifest":{"download_size":1000,"disk_size":2000}}';;
  "-y install Owl"*) mkdir -p "$7/$9"; echo x > "$7/$9/Owl.exe"
    echo '[DLManager] INFO: = Progress: 100.00% (1/1), Running for 00:00:01, ETA: 00:00:00' >&2
    echo "{\"Owl\":{\"title\":\"Owl Game\",\"install_path\":\"$7/$9\",\"version\":\"1.0\",\"install_size\":2000}}" > installed.json;;
  "-y uninstall Owl"*) echo '{}' > installed.json;;
  *) echo "[cli] ERROR: unexpected $*" >&2; exit 1;;
esac"#,
    );
    assert!(!logged_in(Shop::Epic));
    let login = begin_login(Shop::Epic).unwrap();
    let err = finish_login(&login, &"f".repeat(32)).unwrap_err();
    assert!(err.to_string().contains("Login attempt failed"), "{err}");
    finish_login(&login, "0123456789abcdef0123456789abcdef").unwrap();
    assert_eq!(account(Shop::Epic).as_deref(), Some("Tester"));

    let list = refresh(Shop::Epic).unwrap();
    assert_eq!(owned(Shop::Epic), list);
    let info = info(Shop::Epic, "Owl", false).unwrap();
    assert_eq!(info.download, Some(1000));
    let order = order(&list[0], &info, &home.games(), false, "", Vec::new());
    assert_eq!(order.folder, home.games().join("Owl Game"));
    with_orders(|o| o.insert("epic:Owl".into(), order)).unwrap();
    assert_eq!(crate::downloads::label("game:epic:Owl"), "Owl Game");

    assert_eq!(run("game:epic:Owl").unwrap(), "Owl Game");
    assert_eq!(take_fresh(), [(Shop::Epic, "Owl".to_string())]);
    let got = installed(Shop::Epic);
    assert_eq!(got[0].path, home.games().join("Owl Game"));
    let entry = library_entry(Shop::Epic, "Owl").unwrap();
    assert_eq!(entry.origin.as_deref(), Some("epic:Owl"));
    // The game is found by the launcher in Arkor's own folder.
    let leg = arkor_stores::epic::Legendary::locate_for("Owl").map(|l| l.config_dir);
    if let Some(dir) = leg {
        assert_eq!(dir, home.store(Shop::Epic));
    }
    let up = updates();
    assert_eq!((up.len(), up[0].1.as_str()), (1, "2.0"));

    uninstall(Shop::Epic, "Owl").unwrap();
    assert!(installed(Shop::Epic).is_empty());
    logout(Shop::Epic).unwrap();
    assert!(!logged_in(Shop::Epic) && owned(Shop::Epic).is_empty());
    let mode = std::fs::metadata(home.store(Shop::Epic))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700, "logins are the user's only");
}

#[test]
fn gog_installs_records_and_deletes_its_folder_only() {
    let home = Home::new();
    home.helper(
        "gogdl",
        r#"shift 2
echo "$@" >> "$GOGDL_CONFIG_PATH/calls"
case "$1" in
  auth) echo '{"access_token":"t","user_id":"1"}';;
  info) echo '{"size":{"*":{"download_size":10,"disk_size":20}},"languages":["en-US"],"folder_name":"Fox Tale","buildId":"7","versionName":"1.0","dlcs":[]}';;
  download) mkdir -p "$6/Fox Tale"; echo '{"name":"Fox Tale","playTasks":[{"isPrimary":true,"type":"FileTask","category":"game","path":"fox.exe"}]}' > "$6/Fox Tale/goggame-42.info";;
esac"#,
    );
    std::fs::write(home.store(Shop::Gog).join("auth.json"), "{\"a\":1}").unwrap();
    let info = info(Shop::Gog, "42", false).unwrap();
    let o = order(
        &game("42", "Fox Tale"),
        &info,
        &home.games(),
        false,
        "en-US",
        Vec::new(),
    );
    assert_eq!(o.folder, home.games().join("Fox Tale"));
    with_orders(|all| all.insert("gog:42".into(), o)).unwrap();
    run("game:gog:42").unwrap();
    let got = installed(Shop::Gog);
    assert_eq!(
        (got[0].version.as_str(), got[0].title.as_str()),
        ("7", "Fox Tale")
    );
    let entry = library_entry(Shop::Gog, "42").unwrap();
    assert_eq!(entry.exe, home.games().join("Fox Tale/fox.exe"));

    // A folder that is not this game's is never deleted.
    let other = home.games().join("Other");
    std::fs::create_dir_all(&other).unwrap();
    let mut moved = got[0].clone();
    moved.path = other.clone();
    assert!(gog::uninstall(&moved).is_err() && other.exists());

    uninstall(Shop::Gog, "42").unwrap();
    assert!(!home.games().join("Fox Tale").exists());
    assert!(other.exists() && installed(Shop::Gog).is_empty());
}

#[test]
fn cancelling_removes_a_new_partial_download_only() {
    let home = Home::new();
    let fresh = order(
        &game("1", "New"),
        &Info::default(),
        &home.games(),
        false,
        "",
        Vec::new(),
    );
    std::fs::create_dir_all(&fresh.folder).unwrap();
    std::fs::write(fresh.folder.join("part"), "x").unwrap();
    with_orders(|o| o.insert("amazon:1".into(), fresh.clone())).unwrap();
    cancelled("game:amazon:1");
    assert!(!fresh.folder.exists() && load_order(Shop::Amazon, "1").is_none());

    // Something was there before: another name is used for stores that allow it…
    let taken = home.games().join("Taken");
    std::fs::create_dir_all(&taken).unwrap();
    std::fs::write(taken.join("save"), "x").unwrap();
    let o = order(
        &game("2", "Taken"),
        &Info::default(),
        &home.games(),
        false,
        "",
        Vec::new(),
    );
    assert_eq!(o.folder, home.games().join("Taken (2)"));
    // …and a fixed folder (GOG) is left alone.
    let info = Info {
        folder: Some("Taken".into()),
        ..Info::default()
    };
    let o = order(
        &game("3", "Taken"),
        &info,
        &home.games(),
        false,
        "",
        Vec::new(),
    );
    assert!(!o.fresh);
    with_orders(|all| all.insert("gog:3".into(), o)).unwrap();
    cancelled("game:gog:3");
    assert!(taken.join("save").exists());
}

#[test]
fn whole_places_are_never_deletable() {
    assert!(!deletable(Path::new("/")));
    assert!(!deletable(Path::new("/usr")));
    assert!(!deletable(&dirs::home_dir().unwrap_or("/home".into())));
    assert!(!deletable(Path::new("/nonexistent/a/b/c")));
}

#[test]
fn amazon_says_why_it_did_not_install() {
    let home = Home::new();
    home.helper(
        "nile",
        r#"case "$*" in
  install*) echo 'ERROR [DOWNLOAD]:	 Not enough space available' >&2;;
esac"#,
    );
    std::fs::create_dir_all(home.store(Shop::Amazon).join("nile")).unwrap();
    std::fs::write(
        home.store(Shop::Amazon).join("nile/current_user.json"),
        "{\"name\":\"T\"}",
    )
    .unwrap();
    let o = order(
        &game("a1", "Hen"),
        &Info::default(),
        &home.games(),
        false,
        "",
        Vec::new(),
    );
    with_orders(|all| all.insert("amazon:a1".into(), o)).unwrap();
    let err = run("game:amazon:a1").unwrap_err();
    assert_eq!(err.to_string(), "Not enough space available");
}

/// Network: the pinned helpers download, pass their checksum and run.
#[test]
#[ignore]
fn helpers_download_and_run() {
    let _home = Home::new();
    // SAFETY: the lock is held (see Home).
    unsafe { std::env::remove_var("ARKOR_STORE_HELPERS") };
    for shop in Shop::ALL {
        let bin = helper::ensure(shop).unwrap();
        let out = helper::output({
            let mut c = helper::command(shop).unwrap();
            c.arg("--help");
            c
        })
        .unwrap();
        println!("{} ok={}", bin.display(), out.ok);
        assert!(out.ok);
    }
    let login = begin_login(Shop::Amazon).unwrap();
    println!(
        "amazon login page: {}",
        login.url.split('?').next().unwrap()
    );
    println!("{}", info(Shop::Epic, "Owl", false).unwrap_err());
    println!("{}", refresh(Shop::Gog).unwrap_err());
    println!("{}", refresh(Shop::Amazon).unwrap_err());
}

#[test]
fn free_space_of_a_folder_that_does_not_exist_yet() {
    assert!(free_space(Path::new("/tmp/does/not/exist")).unwrap() > 0);
}
