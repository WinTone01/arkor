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
            fstype: "ext4".into(),
            size: "465,8G".into(),
        },
        mount,
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
