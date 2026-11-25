//! Window tests on gpui's headless test platform: no real window, no real key presses.
//! Key strokes go through the same key bindings and actions as in the app.

use super::*;

/// A scratch config with one Windows game, so the game page has something to show.
fn scratch_home() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for (k, d) in [
        ("XDG_CONFIG_HOME", "cfg"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        std::fs::create_dir_all(tmp.path().join(d)).unwrap();
        // SAFETY: single-threaded test setup before the app starts.
        unsafe { std::env::set_var(k, tmp.path().join(d)) };
    }
    // Made-up logins are not worth sealing.
    // SAFETY: as above.
    unsafe { std::env::set_var("ARKOR_CREDS", "none") };
    let exe = tmp.path().join("Game.exe");
    std::fs::write(&exe, "").unwrap();
    let mut lib = GameLibrary::load().unwrap();
    lib.add(arkor_core::library::NewGame::windows("Test Game", exe));
    lib.save().unwrap();
    tmp
}

#[test]
fn keyboard_shortcuts_and_console() {
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        cx.bind_keys([gpui::KeyBinding::new("ctrl-h", ToggleHidden, None)]);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("local:test-game"));
        lib
    });
    cx.run_until_parked();
    let state = |cx: &mut gpui_test::VisualTestContext| {
        view.read_with(cx, |l, _| {
            (l.selection, l.game_tab, l.console.on, l.global_hud_tab)
        })
    };

    assert!(
        matches!(state(cx).0, Some(Selection::Game(_))),
        "opened on the game page"
    );
    assert_eq!(state(cx).1, GameTab::Overview);

    cx.simulate_keystrokes("ctrl-tab");
    assert_eq!(state(cx).1, GameTab::Game, "Ctrl+Tab: next tab");
    cx.simulate_keystrokes("ctrl-tab");
    assert_eq!(state(cx).1, GameTab::Runtime);
    cx.simulate_keystrokes("ctrl-shift-tab ctrl-shift-tab ctrl-shift-tab");
    assert_eq!(
        state(cx).1,
        GameTab::Logs,
        "Ctrl+Shift+Tab wraps around to the last tab"
    );

    cx.simulate_keystrokes("escape");
    assert!(
        matches!(state(cx).0, Some(Selection::Grid)),
        "Esc: back to the library"
    );

    cx.simulate_keystrokes("ctrl-,");
    assert!(
        matches!(state(cx).0, Some(Selection::Global)) && !state(cx).3,
        "Ctrl+,: Settings"
    );

    cx.simulate_keystrokes("ctrl-l");
    assert!(
        matches!(state(cx).0, Some(Selection::Grid)),
        "Ctrl+L: library"
    );

    // Ctrl+F focuses the library search; typing filters the library.
    cx.simulate_keystrokes("ctrl-f");
    cx.simulate_input("zzz");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |l, _| (l.search_text.clone(), l.visible.len())),
        ("zzz".to_string(), 0),
        "search typed into and applied"
    );
    cx.simulate_keystrokes("backspace backspace backspace");
    assert_eq!(view.read_with(cx, |l, _| l.visible.len()), 1);

    // F11 enters console mode and leaves it.
    cx.simulate_keystrokes("f11");
    assert!(state(cx).2, "F11: console mode");
    cx.simulate_keystrokes("f11");
    assert!(!state(cx).2, "F11 again: the desktop");
}

#[test]
fn console_mode_screens() {
    use super::console::{Act, Opt, Overlay, Screen};
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console"));
        lib
    });
    cx.run_until_parked();
    let screen =
        |cx: &mut gpui_test::VisualTestContext| view.read_with(cx, |l, _| l.console.screen());
    let overlay = |cx: &mut gpui_test::VisualTestContext| {
        view.read_with(cx, |l, _| l.console.overlay.clone())
    };
    assert_eq!(screen(cx), Screen::Home);

    // Enter opens the focused game; Esc goes back.
    cx.simulate_keystrokes("right left enter");
    assert_eq!(screen(cx), Screen::Game("test-game".into()));
    assert!(matches!(
        view.read_with(cx, |l, _| l.selection),
        Some(Selection::Game(_))
    ));
    // Down to a switch; Enter flips it and it is saved.
    let target = view.read_with(cx, |l, _| {
        l.console_rows()
            .iter()
            .position(|r| r.items[0].act == Act::Opt(Opt::Quick(4)))
            .unwrap()
    });
    for _ in 0..target {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    assert!(view.read_with(cx, |l, _| l.profile.no_sleep), "keep awake");
    let id: GameId = "local:test-game".parse().unwrap();
    assert!(Profile::load(&id).unwrap().no_sleep);
    // The Proton choice opens as a list and closes with Esc.
    cx.simulate_keystrokes("up up up up up up up up down enter");
    assert!(matches!(overlay(cx), Some(Overlay::Pick(..))));
    cx.simulate_keystrokes("escape");
    assert_eq!(overlay(cx), None);
    cx.simulate_keystrokes("escape");
    assert_eq!(screen(cx), Screen::Home);

    // Tab goes through the sections; all of them draw.
    for s in Screen::TABS.into_iter().skip(1).chain([Screen::Home]) {
        cx.simulate_keystrokes("tab");
        cx.run_until_parked();
        assert_eq!(screen(cx), s);
    }
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(screen(cx), Screen::Settings);

    // Search: with the on-screen keys and with real ones.
    cx.simulate_keystrokes("ctrl-f");
    assert_eq!(
        (screen(cx), overlay(cx)),
        (Screen::Library, Some(Overlay::Keyboard))
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        view.read_with(cx, |l, _| l.console.query.clone()),
        "t",
        "the key in the middle"
    );
    cx.simulate_input("est");
    cx.simulate_keystrokes("enter");
    assert_eq!(overlay(cx), None, "Enter after typing ends the search");
    let games = |cx: &mut gpui_test::VisualTestContext| {
        view.read_with(cx, |l, _| {
            l.console_rows()
                .iter()
                .filter(|r| r.kind == super::console::Kind::Grid)
                .flat_map(|r| r.items.clone())
                .count()
        })
    };
    assert_eq!(games(cx), 1);
    // The focus went to the result; searching for something else leaves nothing.
    cx.simulate_keystrokes("ctrl-f");
    cx.simulate_input("zzz");
    assert_eq!(games(cx), 0);
    cx.simulate_keystrokes("escape");

    // The menu; shutting down asks first and Esc answers no.
    cx.simulate_keystrokes("f10");
    assert_eq!(overlay(cx), Some(Overlay::Menu));
    cx.simulate_keystrokes("down down down down enter");
    assert!(matches!(overlay(cx), Some(Overlay::Ask(..))));
    cx.simulate_keystrokes("enter");
    assert_eq!(overlay(cx), None, "the first answer is Cancel");
    // F10, "Leave console mode".
    cx.simulate_keystrokes("f10 down enter");
    assert!(!view.read_with(cx, |l, _| l.console.on));
}

#[test]
fn store_games_install_from_the_desktop_and_the_console() {
    use arkor_launch::downloads;
    use arkor_launch::shops::{self, Owned, Shop};
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = scratch_home();
    // Logged in to GOG, with a helper that answers.
    let dir = shops::shop_dir(Shop::Gog).unwrap();
    std::fs::write(dir.join("auth.json"), r#"{"a":1}"#).unwrap();
    let game = |id: &str, title: &str| Owned {
        id: id.into(),
        title: title.into(),
        cover: None,
        wide: None,
        linux: true,
        windows: true,
        namespace: None,
    };
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("stores"));
        lib.global.games_dir = Some(home.path().join("games"));
        lib.shops.set_owned(
            Shop::Gog,
            vec![game("1", "Fox Tale"), game("2", "Hen House")],
        );
        lib
    });
    cx.run_until_parked();
    view.update(cx, |l, cx| {
        assert_eq!(l.shops.shown(None).len(), 2);
        l.shops.query = "fox".into();
        assert_eq!(l.shops.shown(None).len(), 1);
        l.shops.query.clear();
        l.shop_login(Shop::Epic, cx);
    });
    cx.run_until_parked();
    // Esc closes the login window, then a game's window.
    cx.simulate_keystrokes("escape");
    assert!(view.read_with(cx, |l, _| l.shops.login.is_none()));
    view.update(cx, |l, cx| l.shop_open_game(Shop::Gog, "1", cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |l, _| l.shops.game.as_ref().unwrap().linux));
    cx.simulate_keystrokes("escape");
    assert!(view.read_with(cx, |l, _| l.shops.game.is_none()));

    // In the console: Stores, the first game, and Install.
    cx.simulate_keystrokes("f11 tab tab enter");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |l, _| l.console.screen()),
        super::console::Screen::ShopGame(Shop::Gog, "1".into())
    );
    // No need to wait for the store's answer: Install queues it at once.
    cx.simulate_keystrokes("enter");
    assert!(downloads::pending("game:gog:1"), "in the queue");
    assert_eq!(downloads::label("game:gog:1"), "Fox Tale");
    cx.run_until_parked();
    // The downloads screen shows it; X (Delete) cancels it.
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    cx.simulate_keystrokes("delete");
    assert!(!downloads::pending("game:gog:1"));

    // An installed game joins the library with the store as its origin.
    let folder = home.path().join("games/Hen House");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("start.sh"), "#!/bin/sh\n").unwrap();
    std::fs::write(
        dir.join("installed.json"),
        format!(
            r#"{{"installed":[{{"appName":"2","install_path":"{}","platform":"linux","title":"Hen House"}}]}}"#,
            folder.display()
        ),
    )
    .unwrap();
    view.update(cx, |l, cx| {
        l.shops_after_download(cx);
        l.shop_show_in_library(Shop::Gog, "2", cx);
        let entry = l.shop_entry(Shop::Gog, "2").expect("in the library");
        let entry = l.library.get(&entry).unwrap().clone();
        assert_eq!(entry.kind, arkor_core::library::GameKind::Linux);
        assert_eq!(entry.exe, folder.join("start.sh"));
        assert_eq!(l.entry_shop(&entry), Some((Shop::Gog, "2".into())));
    });
    cx.run_until_parked();
}

#[test]
fn game_page_search_and_tabs_render() {
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("local:test-game#search=fps"));
        lib
    });
    cx.run_until_parked();
    // Every tab of a Windows game renders without panicking, closed panels first, then
    // with every panel open (so each feature's card is drawn).
    for open in [false, true] {
        for tab in GameTab::ALL {
            view.update(cx, |l, cx| {
                // What clicking a tab does.
                l.feature_query.clear();
                l.fields.inputs.remove("feature-search");
                l.game_tab = tab;
                if open {
                    for f in super::features::FEATURES {
                        l.game_ui.expanded.insert(f.id, true);
                    }
                }
                cx.notify();
            });
            cx.run_until_parked();
        }
    }
    // Ctrl+F on a game page goes to its settings search.
    cx.simulate_keystrokes("ctrl-f");
    cx.simulate_input("dlss");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |l, _| l.feature_query.clone()), "dlss");
    // Settings → Game defaults renders too.
    view.update(cx, |l, cx| {
        l.open(Some("settings#defaults"));
        cx.notify();
    });
    cx.run_until_parked();
}

#[test]
fn settings_overlay_and_hud_tabs_render() {
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("settings#shortcuts"));
        lib
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |l, _| l.settings_tab),
        global_page::SettingsTab::Shortcuts
    );
    // Every Settings section renders.
    for tab in global_page::SettingsTab::ALL {
        view.update(cx, |l, cx| {
            l.settings_tab = tab;
            cx.notify();
        });
        cx.run_until_parked();
    }
    // The overlay page opens on the tab named in the link, and every tab renders.
    view.update(cx, |l, _| l.open(Some("overlay#metrics")));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |l, _| (l.selection, l.hud_tab)),
        (Some(Selection::Global), hud_editor::HudTab::Metrics)
    );
    for tab in hud_editor::HudTab::ALL {
        view.update(cx, |l, cx| {
            l.hud_tab = tab;
            cx.notify();
        });
        cx.run_until_parked();
    }
    // A game's own MangoHud layout uses the same editor.
    view.update(cx, |l, cx| {
        l.open(Some("local:test-game#visuals"));
        l.set_hud_mode(HudMode::Custom, cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |l, _| l.profile.mangohud.settings.is_some()));
    // Esc from the game page goes back to the library.
    cx.simulate_keystrokes("escape");
    assert!(matches!(
        view.read_with(cx, |l, _| l.selection),
        Some(Selection::Grid)
    ));
}

#[test]
fn upscaling_subtabs_and_downloads_page_render() {
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("local:test-game#upscaling"));
        lib
    });
    cx.run_until_parked();
    for group in 0..super::features::UPSCALE_GROUPS.len() {
        view.update(cx, |l, cx| {
            l.game_ui.upscale_group = group;
            cx.notify();
        });
        cx.run_until_parked();
    }
    view.update(cx, |l, cx| l.go(super::shell::Nav::Downloads, cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |l, _| matches!(l.selection, Some(Selection::Downloads))));
}

#[test]
fn download_progress_shows_in_sidebar_and_downloads_page() {
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("downloads"));
        lib
    });
    // A job in progress: the page and the sidebar card draw it; it ends with the guard.
    let job = arkor_launch::progress::job("optiscaler", "OptiScaler");
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(arkor_launch::progress::running("optiscaler"));
    // Its own Download button is busy, others are not.
    assert!(!arkor_launch::progress::running("streamline"));
    drop(job);
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(!arkor_launch::progress::running("optiscaler"));
}

#[test]
fn download_queue_and_updates_render() {
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("downloads"));
        lib
    });
    use arkor_launch::downloads;
    // No worker runs in tests: queued items stay waiting.
    downloads::enqueue("test-queued");
    downloads::enqueue("test-paused");
    downloads::pause("test-paused");
    view.update(cx, |lib, cx| {
        lib.downloads_set_updates(vec![downloads::Update {
            key: "reshade".into(),
            label: "ReShade".into(),
            installed: "6.3.0".into(),
            latest: "6.8.0".into(),
        }]);
        cx.notify();
    });
    cx.run_until_parked();
    // "Update all" puts every update in the queue.
    view.update(cx, |lib, cx| lib.downloads_update_all(cx));
    assert!(downloads::pending("reshade"));
    // The Settings page shows the update interval.
    view.update(cx, |lib, cx| {
        lib.open(Some("settings"));
        cx.notify();
    });
    cx.run_until_parked();
    for key in ["test-queued", "test-paused", "reshade"] {
        downloads::cancel(key);
    }
    assert!(downloads::items().is_empty());
}

#[test]
fn console_game_settings_change_the_profile() {
    use super::console::{Act, Opt, Overlay, Screen};
    use super::console_settings::{GameCat, GameOpt};
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console#settings/test-game"));
        lib
    });
    cx.run_until_parked();
    view.update(cx, |l, cx| l.console_first(cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |l, _| l.console.screen()),
        Screen::GameSettings("test-game".into())
    );
    // The categories first; Performance holds Gamescope.
    let cat = |cx: &mut gpui_test::VisualTestContext, c: GameCat| {
        view.read_with(cx, |l, _| {
            l.console_rows()
                .iter()
                .position(|r| r.items[0].act == Act::Screen(Screen::GameCat("test-game".into(), c)))
                .expect("category is there")
        })
    };
    let open_cat = |cx: &mut gpui_test::VisualTestContext, c: GameCat| {
        cx.simulate_keystrokes("pageup pageup pageup");
        for _ in 0..cat(cx, c) {
            cx.simulate_keystrokes("down");
        }
        cx.simulate_keystrokes("enter");
    };
    let row_of = |cx: &mut gpui_test::VisualTestContext, o: GameOpt| {
        view.read_with(cx, |l, _| {
            l.console_rows()
                .iter()
                .position(|r| r.items[0].act == Act::Opt(Opt::Game(o)))
                .expect("row is there")
        })
    };
    let go_to = |cx: &mut gpui_test::VisualTestContext, row: usize| {
        cx.simulate_keystrokes("pageup pageup pageup pageup pageup pageup pageup pageup");
        for _ in 0..row {
            cx.simulate_keystrokes("down");
        }
    };
    open_cat(cx, GameCat::Overlay);
    // MangoHud first: right turns it from the shared layout to the game's own.
    cx.simulate_keystrokes("right");
    assert_eq!(
        view.read_with(cx, |l, _| l.profile.mangohud.mode),
        HudMode::Custom
    );
    cx.simulate_keystrokes("escape");
    open_cat(cx, GameCat::Performance);
    // Gamescope's own rows come with it.
    let gs = row_of(cx, GameOpt::Gamescope);
    go_to(cx, gs);
    cx.simulate_keystrokes("enter");
    assert!(view.read_with(cx, |l, _| l.profile.gamescope.enabled));
    let refresh = row_of(cx, GameOpt::GsRefresh);
    go_to(cx, refresh);
    cx.simulate_keystrokes("right right");
    assert_eq!(
        view.read_with(cx, |l, _| l.profile.gamescope.refresh),
        Some(75)
    );
    // A choice also opens as a list.
    cx.simulate_keystrokes("enter");
    assert!(matches!(
        view.read_with(cx, |l, _| l.console.overlay.clone()),
        Some(Overlay::Pick(..))
    ));
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("escape");
    open_cat(cx, GameCat::Starting);
    // Launch options are typed on the keyboard.
    let launch = row_of(cx, GameOpt::LaunchOptions);
    go_to(cx, launch);
    cx.simulate_keystrokes("enter");
    cx.simulate_input("-dx11");
    cx.simulate_keystrokes("enter");
    let id: GameId = "local:test-game".parse().unwrap();
    let saved = Profile::load(&id).unwrap();
    assert_eq!(saved.launch_options, "-dx11");
    assert_eq!(saved.gamescope.refresh, Some(75));
    // Back to the categories, then the game's screen.
    cx.simulate_keystrokes("escape escape");
    assert_eq!(
        view.read_with(cx, |l, _| l.console.screen()),
        Screen::Game("test-game".into())
    );
}

#[test]
fn console_folder_choice_password_and_tools() {
    use super::console::{FolderFor, Overlay, Screen};
    use super::console_settings::AppCat;
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console#settings"));
        lib
    });
    cx.run_until_parked();
    // Every settings category draws, and each is reachable.
    for c in AppCat::ALL {
        view.update(cx, |l, cx| l.console_open(Screen::SettingsCat(c), cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
    }
    std::fs::create_dir_all(home.path().join("games/inner")).unwrap();
    let start = home.path().join("games");
    let other = home.path().join("other-disk");
    std::fs::create_dir_all(&other).unwrap();
    let drive = |n: u32, mount: Option<String>| arkor_launch::admin::Drive {
        disk: arkor_launch::admin::Disk {
            path: format!("/dev/sdx{n}"),
            label: String::new(),
            uuid: format!("uuid-{n}"),
            fstype: "ext4".into(),
            size: "465,8G".into(),
        },
        mount,
        space: None,
    };
    view.update(cx, |l, cx| {
        l.console.drives = vec![
            drive(1, None),
            drive(2, None),
            drive(3, Some(other.display().to_string())),
            drive(4, None),
        ];
        l.console_overlay(Overlay::Folder(FolderFor::Games, start.clone()));
        cx.notify();
    });
    cx.run_until_parked();
    // Home and two disks in one line, the other two in the next.
    cx.simulate_keystrokes("down right right");
    assert_eq!(
        view.read_with(cx, |l, _| (
            l.console.over_focus.row,
            l.console.over_focus.col()
        )),
        (1, 2)
    );
    cx.simulate_keystrokes("down enter");
    assert_eq!(
        view.read_with(cx, |l, _| match &l.console.overlay {
            Some(Overlay::Folder(_, d)) => Some(d.clone()),
            _ => None,
        }),
        Some(other),
        "the other disk is open"
    );
    view.update(cx, |l, cx| {
        l.console_overlay(Overlay::Folder(FolderFor::Games, start.clone()));
        cx.notify();
    });
    // It can be left from anywhere: with its own button, and with Start.
    cx.simulate_keystrokes("right right enter");
    let open = |cx: &mut gpui_test::VisualTestContext| {
        view.read_with(cx, |l, _| l.console.overlay.is_some())
    };
    assert!(!open(cx), "cancelled");
    let reopen = |cx: &mut gpui_test::VisualTestContext| {
        view.update(cx, |l, cx| {
            l.console_overlay(Overlay::Folder(FolderFor::Games, start.clone()));
            cx.notify();
        });
    };
    reopen(cx);
    cx.simulate_keystrokes("f10");
    assert!(!open(cx), "Start leaves it");
    reopen(cx);
    // Down past the places to the first folder and into it; back leaves it.
    cx.simulate_keystrokes("down down down enter");
    let dir = |cx: &mut gpui_test::VisualTestContext| {
        view.read_with(cx, |l, _| match &l.console.overlay {
            Some(Overlay::Folder(_, d)) => Some(d.clone()),
            _ => None,
        })
    };
    assert_eq!(dir(cx), Some(start.join("inner")));
    cx.simulate_keystrokes("escape");
    assert_eq!(dir(cx), Some(start.clone()));
    cx.simulate_keystrokes("delete");
    assert_eq!(dir(cx), None, "chosen");
    assert_eq!(
        view.read_with(cx, |l, _| l.global.games_dir.clone()),
        Some(start)
    );
    // A password: typed, shown as dots only, gone when the keyboard closes.
    view.update(cx, |l, cx| {
        l.console_install_tools(vec!["test".into()]);
        cx.notify();
    });
    cx.simulate_input("aB1!");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |l, _| l.console.secret.as_ref().map(|s| s.1.clone())),
        Some("aB1!".into())
    );
    cx.simulate_keystrokes("escape");
    assert!(view.read_with(cx, |l, _| l.console.secret.is_none()
        && l.console.overlay.is_none()
        && l.console.query.is_empty()));
}

#[test]
fn console_login_message_answers_the_gamepad() {
    use super::console::Screen;
    use super::gamepad::Nav;
    use super::shops::LoginMode;
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console#stores"));
        lib
    });
    cx.run_until_parked();
    let closed = |cx: &mut gpui_test::VisualTestContext| {
        view.update(cx, |l, cx| {
            l.shop_login(arkor_launch::shops::Shop::Gog, cx);
            let ui = l.shops.login.as_mut().unwrap();
            ui.busy = false;
            ui.mode = LoginMode::Closed;
            cx.notify();
        });
        cx.run_until_parked();
    };
    let press = |nav: Nav, cx: &mut gpui_test::VisualTestContext| {
        view.update_in(cx, |l, w, cx| l.pad(nav, w, cx));
        cx.run_until_parked();
    };
    closed(cx);
    // The first button opens it again; in a test nothing is spawned, it waits.
    press(Nav::Accept, cx);
    assert!(view.read_with(cx, |l, _| {
        l.shops
            .login
            .as_ref()
            .is_some_and(|u| u.busy && u.mode == LoginMode::Starting)
    }));
    closed(cx);
    // To the second one: closed, and the stores are under it.
    press(Nav::Right, cx);
    press(Nav::Accept, cx);
    assert!(view.read_with(cx, |l, _| {
        l.shops.login.is_none() && l.console.screen() == Screen::Stores
    }));
    // B closes it too; without WebKitGTK the first button asks for the password.
    closed(cx);
    press(Nav::Back, cx);
    assert!(view.read_with(cx, |l, _| l.shops.login.is_none()));
    closed(cx);
    view.update(cx, |l, cx| {
        l.shops.login.as_mut().unwrap().no_webkit = true;
        l.console.over_focus = Default::default();
        cx.notify();
    });
    cx.run_until_parked();
    press(Nav::Accept, cx);
    assert!(view.read_with(cx, |l, _| {
        l.shops.login.is_none() && l.console.secret.is_some()
    }));
}

#[test]
fn console_intro_is_skipped_by_the_first_button_only() {
    use super::gamepad::Nav;
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console#settings"));
        lib.play_console_intro(false);
        lib
    });
    cx.run_until_parked();
    let state = |cx: &mut gpui_test::VisualTestContext| {
        view.read_with(cx, |l, _| (l.console_intro_opens(), l.console.focus().row))
    };
    let press = |nav: Nav, cx: &mut gpui_test::VisualTestContext| {
        view.update_in(cx, |l, w, cx| l.nav(nav, w, cx));
        cx.run_until_parked();
    };
    assert_eq!(state(cx), (Some(false), 0));
    press(Nav::Down, cx);
    assert_eq!(state(cx).1, 0, "this press only skipped the animation");
    assert_ne!(state(cx).0, Some(false));
    press(Nav::Down, cx);
    assert_eq!(state(cx).1, 1, "the next one is the console's");
    // Switched off, or with less motion, there is none.
    view.update(cx, |l, _| {
        l.console.intro = None;
        l.global.console_no_intro = true;
        l.play_console_intro(false);
        assert!(l.console.intro.is_none());
        l.global.console_no_intro = false;
        l.global.appearance.reduce_motion = true;
        l.play_console_intro(false);
        assert!(l.console.intro.is_none());
        // The volume is a choice like any other.
        l.opt_turn(&super::console::Opt::ConsoleVolume, 1);
        assert_eq!(l.global.console_volume, Some(85));
        l.global.appearance.sounds = false;
        assert_eq!(l.console_volume(), 0.);
    });
}

#[test]
fn console_steps_aside_while_steam_updates_a_game() {
    use super::console::{Act, Launch, Overlay, Screen};
    use super::gamepad::Nav;
    use arkor_launch::steam_downloads::Item;
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console#home"));
        lib
    });
    cx.run_until_parked();
    let item = Item {
        appid: 730,
        name: "Shooter".into(),
        done: 200,
        total: 300,
        speed: 0.,
        running: false,
        update: true,
        step: arkor_launch::steam_downloads::Step::Waiting,
        held: false,
    };
    let launch = || Launch {
        id: "shooter".into(),
        at: std::time::Instant::now(),
        playing: false,
        dismissed: false,
        steam: Some("730".into()),
        came: false,
        updating: false,
    };
    view.update(cx, |l, cx| {
        l.console.launching = Some(launch());
        l.steam_launch_step(false, None, cx);
        let s = l.console.launching.as_ref().unwrap();
        assert!(s.waits_for_steam() && !s.dismissed, "still starting");
        l.steam_launch_step(false, Some(item.clone()), cx);
        let s = l.console.launching.as_ref().unwrap();
        assert!(s.updating && s.dismissed, "the start screen is left");
        assert!(
            matches!(l.console.overlay, Some(Overlay::Pick(..))),
            "and says why"
        );
        // Said once, not with every look.
        l.console.overlay = None;
        l.steam_launch_step(false, Some(item.clone()), cx);
        assert!(l.console.overlay.is_none());
        // The update is done and the game there: its screen is back.
        l.steam_launch_step(true, None, cx);
        let s = l.console.launching.as_ref().unwrap();
        assert!(s.came && !s.dismissed && !s.updating && !s.waits_for_steam());
        // Known beforehand: no start screen at all, and it can be called off.
        l.console.launching = None;
        l.console_update_first("local:shooter".into(), item.clone(), cx);
        assert!(l.console.launching.is_none() && l.console.after_update.is_some());
    });
    cx.run_until_parked();
    let press = |nav: Nav, cx: &mut gpui_test::VisualTestContext| {
        view.update_in(cx, |l, w, cx| l.nav(nav, w, cx));
        cx.run_until_parked();
    };
    let options = view.read_with(cx, |l, _| match &l.console.overlay {
        Some(Overlay::Pick(_, o)) => o.iter().map(|(_, a, _)| a.clone()).collect::<Vec<_>>(),
        _ => Vec::new(),
    });
    assert_eq!(options, [Act::Close, Act::SeeDownloads, Act::DoNotStart]);
    press(Nav::Down, cx);
    press(Nav::Accept, cx);
    assert_eq!(
        view.read_with(cx, |l, _| l.console.screen()),
        Screen::Downloads
    );
    assert!(
        view.read_with(cx, |l, _| l.console.after_update.is_some()),
        "still wanted"
    );
    view.update_in(cx, |l, w, cx| l.console_do(Act::DoNotStart, w, cx));
    assert!(view.read_with(cx, |l, _| l.console.after_update.is_none()));
}

#[test]
fn console_shows_removal_and_arkors_update() {
    use super::console::{Act, Overlay, Screen};
    use arkor_launch::shops::Shop;
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console#gog/42"));
        lib
    });
    cx.run_until_parked();
    let pills = |l: &Library| {
        l.console_rows()
            .first()
            .map(|r| r.items.iter().map(|i| i.act.clone()).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    // While it is removed its screen stays, with a button that only turns.
    view.update(cx, |l, cx| {
        l.shop_uninstall(Shop::Gog, "42", cx);
        assert_eq!(pills(l), [Act::Nothing]);
        assert_eq!(l.console.screen(), Screen::ShopGame(Shop::Gog, "42".into()));
        // Gone: the screen goes with it.
        l.console_left_by(&(Shop::Gog, "42".into()));
        assert_eq!(l.console.screen(), Screen::Stores);
    });
    // A newer Arkor: in the settings and among the downloads, asked before it is installed.
    view.update_in(cx, |l, w, cx| {
        let acts = |l: &Library| {
            l.console_rows()
                .iter()
                .flat_map(|r| r.items.iter().map(|i| i.act.clone()))
                .collect::<Vec<_>>()
        };
        l.console_open(
            Screen::SettingsCat(super::console_settings::AppCat::Arkor),
            cx,
        );
        assert!(acts(l).contains(&Act::CheckArkor));
        l.app_update.available = Some(arkor_launch::self_update::Release {
            version: "9.9.9".into(),
            url: String::new(),
            size: 1000,
            notes: "## Install or update\n\nrun this\n```bash\ncurl x\n```\n## New\n\n> [!NOTE]\n- **Saves** from [GOG](https://x.y) in `cloud`\n".into(),
        });
        assert!(acts(l).contains(&Act::AskArkor));
        l.console_do(Act::ArkorNotes, w, cx);
        assert!(matches!(
            &l.console.overlay,
            Some(Overlay::Read(_, text, _)) if text == "NEW\n\n•  Saves from GOG in cloud"
        ));
        l.console.overlay = None;
        l.console_open(Screen::Downloads, cx);
        assert!(acts(l).contains(&Act::AskArkor));
        l.console_do(Act::AskArkor, w, cx);
        assert!(matches!(
            &l.console.overlay,
            Some(Overlay::Ask(t, _, Act::UpdateArkor)) if t.contains("9.9.9")
        ));
    });
}

#[test]
fn console_shows_what_steam_has_and_asks() {
    use super::console::{Act, Launch, Overlay, Screen, Value};
    use super::console_steam::{SteamData, SteamPage};
    use super::gamepad::Nav;
    use arkor_launch::steam_api::{Achievement, Action, Dlc, Eula, News, Screenshot, WorkshopItem};
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console#library"));
        lib
    });
    cx.run_until_parked();
    let achievement = |name: &str, unlocked, hidden| Achievement {
        name: name.into(),
        description: "Do it".into(),
        unlocked,
        at: 1,
        image: String::new(),
        hidden,
        progress: 3.,
        most: 10.,
        share: 12.,
    };
    let pages = [
        (
            SteamPage::Achievements,
            SteamData::Achievements(vec![
                achievement("First", true, false),
                achievement("Secret", false, true),
            ]),
        ),
        (
            SteamPage::Screenshots,
            SteamData::Screenshots(vec![Screenshot {
                game: "730".into(),
                handle: 1,
                path: "/nowhere/a.jpg".into(),
                at: 1,
            }]),
        ),
        (
            SteamPage::Dlc,
            SteamData::Dlc(vec![Dlc {
                id: 5,
                name: "More".into(),
                state: "installed".into(),
                on: true,
                size: 10.,
            }]),
        ),
        (
            SteamPage::Workshop,
            SteamData::Workshop(vec![WorkshopItem {
                id: "9".into(),
                title: "Map".into(),
                size: 10.,
                off: false,
                image: String::new(),
            }]),
        ),
    ];
    for (page, data) in pages {
        view.update(cx, |l, cx| {
            l.console_open(Screen::Steam(730, page), cx);
            let rows = l.console_rows();
            assert_eq!(rows[0].items[0].act, Act::Nothing, "loading");
            l.console.steam_page = Some((730, page, Some(Ok(data))));
            cx.notify();
        });
        cx.run_until_parked();
    }
    view.update(cx, |l, cx| {
        // A hidden one tells nothing; what is unlocked counts.
        let l2 = SteamData::Achievements(vec![
            achievement("First", true, false),
            achievement("Secret", false, true),
        ]);
        l.console.steam_page = Some((730, SteamPage::Achievements, Some(Ok(l2))));
        let rows = l.steam_page_rows(730, SteamPage::Achievements);
        assert!(
            rows[0]
                .title
                .as_ref()
                .is_some_and(|t| t.starts_with("1 / 2"))
        );
        let secret = &rows[1].items[0];
        assert!(!secret.label.contains("Secret") && !secret.sub.contains("Do it"));
        // A switch is shown at once.
        let dlc = SteamData::Dlc(vec![Dlc {
            id: 5,
            name: "More".into(),
            state: String::new(),
            on: true,
            size: 0.,
        }]);
        l.console.steam_page = Some((730, SteamPage::Dlc, Some(Ok(dlc))));
        l.steam_set_dlc(730, 5, false, cx);
        let rows = l.steam_page_rows(730, SteamPage::Dlc);
        assert!(matches!(
            &rows[0].items[0].look,
            super::console::Look::Line {
                value: Value::Switch(false),
                ..
            }
        ));
    });
    let launch = || Launch {
        id: "shooter".into(),
        at: std::time::Instant::now(),
        playing: false,
        dismissed: false,
        steam: Some("730".into()),
        came: false,
        updating: false,
    };
    let ask = |request: &str, details: &str| Action {
        id: 4,
        game: "730".into(),
        kind: "LaunchApp".into(),
        request: request.into(),
        details: details.into(),
        ..Action::default()
    };
    let press = |nav: Nav, cx: &mut gpui_test::VisualTestContext| {
        view.update_in(cx, |l, w, cx| l.nav(nav, w, cx));
        cx.run_until_parked();
    };
    // A cloud conflict: the start screen steps aside, is back after the answer.
    view.update(cx, |l, cx| {
        l.console.launching = Some(launch());
        l.steam_ask(ask("SynchronizingCloud", "cloudconflict"), cx);
        assert!(l.console.launching.as_ref().unwrap().dismissed);
        let acts: Vec<Act> = match &l.console.overlay {
            Some(Overlay::Pick(_, o)) => o.iter().map(|o| o.1.clone()).collect(),
            _ => Vec::new(),
        };
        assert_eq!(
            acts,
            [
                Act::SteamAnswer(4, "KeepLocal".into()),
                Act::SteamAnswer(4, "KeepRemote".into()),
                Act::SteamCancelAction(4)
            ]
        );
    });
    // It begins at what changes nothing.
    assert_eq!(view.read_with(cx, |l, _| l.console.over_focus.row), 2);
    press(Nav::Up, cx);
    press(Nav::Accept, cx);
    view.read_with(cx, |l, _| {
        assert!(l.console.overlay.is_none());
        assert!(!l.console.launching.as_ref().unwrap().dismissed);
    });
    // Back is no answer: the launch is called off.
    view.update(cx, |l, cx| {
        l.steam_ask(ask("SynchronizingCloud", "syncfailed"), cx)
    });
    press(Nav::Back, cx);
    assert!(view.read_with(cx, |l, _| l.console.launching.is_none()
        && l.console.overlay.is_none()));
    // A licence: only one that could be read here can be accepted here.
    let eula = |text: &str| Eula {
        id: "1".into(),
        version: 1,
        text: text.into(),
    };
    view.update(cx, |l, cx| {
        let first = |l: &Library| match &l.console.overlay {
            Some(Overlay::Read(_, _, o)) => o[0].1.clone(),
            _ => Act::Nothing,
        };
        let (yes, no) = (Act::SteamLicence(4), Act::SteamCancelAction(4));
        l.steam_show_licence(730, &[eula("")], yes.clone(), no.clone());
        assert_eq!(first(l), Act::ShowSteamWindow);
        l.steam_show_licence(730, &[eula("All of it"), eula("")], yes.clone(), no.clone());
        assert_eq!(first(l), Act::ShowSteamWindow);
        l.steam_show_licence(730, &[eula(&"Long text. ".repeat(4000))], yes, no);
        assert_eq!(first(l), Act::SteamLicence(4));
        assert_eq!(l.console.over_focus.col(), 1, "not on accepting");
        cx.notify();
    });
    cx.run_until_parked();
    press(Nav::Down, cx);
    assert!(
        view.read_with(cx, |l, _| l.console.read_at > 0.),
        "it moves"
    );
    press(Nav::Back, cx);
    assert!(view.read_with(cx, |l, _| l.console.overlay.is_none()));
    // Steam says no: the start screen ends.
    view.update(cx, |l, cx| {
        l.console.launching = Some(launch());
        l.steam_news(News::NotStarted("730".into(), "AppError_8".into()), cx);
        assert!(l.console.launching.is_none());
        assert!(matches!(l.console.overlay, Some(Overlay::Failed(..))));
    });
}

#[test]
fn saves_are_asked_about_and_put_back() {
    use super::console::{Act, Overlay, Screen, Value};
    use arkor_core::library::{GameKind, NewGame};
    use arkor_launch::saves::{self, Conflict, Report};
    use arkor_launch::shops::{self, Shop};
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let home = scratch_home();
    // A game from Epic with its saves in the cloud.
    let dir = shops::shop_dir(Shop::Epic).unwrap();
    std::fs::create_dir_all(dir.join("metadata")).unwrap();
    std::fs::create_dir_all(home.path().join("Fox")).unwrap();
    let write = |name: &str, text: String| std::fs::write(dir.join(name), text).unwrap();
    write("user.json", r#"{"account_id": "abc"}"#.into());
    write(
        "installed.json",
        format!(
            r#"{{"FoxApp": {{"title": "Fox", "install_path": "{}"}}}}"#,
            home.path().join("Fox").display()
        ),
    );
    write(
        "metadata/FoxApp.json",
        r#"{"metadata": {"customAttributes":
            {"CloudSaveFolder": {"value": "{AppData}/Fox/Saved"}}}}"#
            .into(),
    );
    let mut lib = GameLibrary::load().unwrap();
    let mut game = NewGame::windows("Fox", home.path().join("Fox"));
    game.kind = GameKind::Epic {
        app_name: "FoxApp".into(),
    };
    game.origin = Some("epic:FoxApp".into());
    game.prefix = Some(home.path().join("pfx"));
    let entry = lib.add(game);
    lib.save().unwrap();
    let file = home
        .path()
        .join("pfx/drive_c/users/steamuser/AppData/Local/Fox/Saved/one.sav");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "first").unwrap();

    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console"));
        lib
    });
    cx.run_until_parked();
    view.update(cx, |l, cx| {
        l.console_open(Screen::Game(entry.clone()), cx);
        l.console_open(Screen::Saves(entry.clone()), cx);
    });
    cx.run_until_parked();
    let labels = |l: &Library| -> Vec<String> {
        l.console_rows()
            .iter()
            .flat_map(|r| r.items.iter().map(|i| i.label.clone()))
            .collect()
    };
    view.update(cx, |l, cx| {
        assert_eq!(l.console.screen(), Screen::Saves(entry.clone()));
        assert_eq!(labels(l).len(), 3, "{:?}", labels(l));
        // Follows the cloud unless told otherwise, in Settings or here.
        let on = |l: &Library| match &l.console_rows()[0].items[0].look {
            super::console::Look::Line { value, .. } => value.clone(),
            _ => unreachable!(),
        };
        assert_eq!(on(l), Value::Switch(true));
        l.saves_do(Act::SavesToggle, cx);
        assert_eq!(on(l), Value::Switch(false));
        assert_eq!(l.profile.cloud_saves, Some(false));
        l.saves_do(Act::SavesToggle, cx);
        assert_eq!(l.profile.cloud_saves, None);
        // The game's screen leads here.
        let rows = l.game_rows(&entry);
        assert!(
            rows.iter()
                .flat_map(|r| &r.items)
                .any(|i| { i.act == Act::Screen(Screen::Saves(entry.clone())) })
        );
        l.saves_do(Act::SavesCopy, cx);
    });
    cx.run_until_parked();
    let copy = view.update(cx, |l, cx| {
        assert_eq!(labels(l).len(), 4);
        let Act::SavesAskRestore(copy) = l.console_rows()[3].items[0].act.clone() else {
            panic!("no copy")
        };
        std::fs::write(&file, "second").unwrap();
        l.saves_do(Act::SavesAskRestore(copy.clone()), cx);
        assert!(matches!(
            &l.console.overlay,
            Some(Overlay::Ask(_, _, Act::SavesRestore(c))) if *c == copy
        ));
        copy
    });
    view.update(cx, |l, cx| l.saves_do(Act::SavesRestore(copy), cx));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "first");
    view.update(cx, |l, cx| {
        assert_eq!(labels(l).len(), 5, "what was there is kept, too");
        // Both sides changed: asked, with the answer that changes nothing in focus.
        let game = saves::find(&l.games[0].id)
            .or_else(|_| saves::find(&l.games[1].id))
            .unwrap();
        let report = Report {
            conflict: Some(Conflict {
                here: Some(1),
                cloud: Some(2),
                here_bytes: 10,
                cloud_bytes: 20,
            }),
            ..Default::default()
        };
        l.saves_synced(&game, Ok(report), cx);
        let Some(Overlay::Pick(_, options)) = &l.console.overlay else {
            panic!("not asked")
        };
        let acts: Vec<Act> = options.iter().map(|o| o.1.clone()).collect();
        assert_eq!(
            acts,
            [
                Act::SavesKeep(entry.clone(), true),
                Act::SavesKeep(entry.clone(), false),
                Act::Close
            ]
        );
        assert_eq!(l.console.over_focus.row, 2);
    });
    cx.run_until_parked();
    // The desktop's card.
    view.update(cx, |l, cx| {
        l.console.overlay = None;
        l.console.on = false;
        assert!(l.render_saves(cx).is_some());
        // A failed sync starts nothing by itself and is said.
        let game = saves::find(&l.games[0].id)
            .or_else(|_| saves::find(&l.games[1].id))
            .unwrap();
        l.saves_synced(&game, Err(anyhow::anyhow!("offline")), cx);
        assert!(l.status.as_ref().unwrap().contains("offline"));
        assert!(l.running.is_empty());
    });
}

#[test]
fn trouble_is_found_fixed_and_undone() {
    use super::console::{Act, Overlay, Screen};
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        console::bind_keys(cx);
        shell::bind_keys(cx);
    });
    let (view, cx) = app.add_window_view(|_, _| {
        let mut lib = Library::new();
        lib.open(Some("console"));
        lib
    });
    cx.run_until_parked();
    let (id, entry) = view.update(cx, |l, _| (l.games[0].id.clone(), l.games[0].id.id.clone()));
    let log = arkor_core::paths::launch_log(&id).unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(
        &log,
        "0024:err:module:import_dll Library MSVCP140.dll (which is needed by L\"x\") not found\n",
    )
    .unwrap();
    // The game did not start: the console asks why.
    view.update(cx, |l, cx| {
        l.console_failed(&id, "Test Game", "exit status 1", &log);
        let rows = l.overlay_rows(l.console.overlay.as_ref().unwrap());
        assert_eq!(
            rows[0].items[0].act,
            Act::Screen(Screen::Trouble(entry.clone()))
        );
        l.console.overlay = None;
        l.console_open(Screen::Game(entry.clone()), cx);
        l.console_open(Screen::Trouble(entry.clone()), cx);
    });
    cx.run_until_parked();
    let acts = view.update(cx, |l, _| {
        l.console_rows()
            .iter()
            .flat_map(|r| r.items.iter().map(|i| (i.act.clone(), i.label.clone())))
            .collect::<Vec<_>>()
    });
    assert_eq!(acts[0].0, Act::Play(id.to_string()));
    assert!(
        acts.iter().any(|(_, l)| l.contains("vcrun2022")),
        "{acts:?}"
    );
    let wined3d = acts
        .iter()
        .find(|(_, l)| l.contains("WineD3D"))
        .map(|(a, _)| a.clone())
        .expect("a general fix");
    view.update(cx, |l, cx| {
        let Act::Fix(p, n) = wined3d else {
            unreachable!()
        };
        l.trouble_choose(p, n, cx);
        assert!(l.profile.tweaks.contains("wined3d"));
        let saved = arkor_core::Profile::load(&id).unwrap();
        assert!(saved.tweaks.contains("wined3d"));
        assert!(
            l.console_rows()[0]
                .items
                .iter()
                .any(|i| i.act == Act::TroubleUndo)
        );
        l.trouble_undo(cx);
        assert!(!l.profile.tweaks.contains("wined3d"));
        // A plain start is for the next start of this game only.
        l.trouble.trial = Some((
            id.clone(),
            arkor_launch::trouble::Trial {
                plain: true,
                detail: false,
            },
        ));
        assert!(l.trouble_trial(&id).plain);
        assert!(!l.trouble_trial(&id).plain);
        l.trouble_log();
        assert!(
            matches!(&l.console.overlay, Some(Overlay::Read(_, t, _)) if t.contains("MSVCP140"))
        );
    });
}
