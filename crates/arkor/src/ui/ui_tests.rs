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
fn keyboard_shortcuts_and_big_screen() {
    let _lock = super::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = scratch_home();
    let mut app = gpui_test::TestAppContext::single();
    app.update(|cx| {
        gpui::init(cx);
        text_input::bind_keys(cx);
        cx.bind_keys([gpui::KeyBinding::new("ctrl-h", ToggleHidden, None)]);
        big_screen::bind_keys(cx);
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
            (l.selection, l.game_tab, l.big_screen, l.global_hud_tab)
        })
    };

    assert!(
        matches!(state(cx).0, Some(Selection::Game(_))),
        "opened on the game page"
    );
    assert_eq!(state(cx).1, GameTab::Overview);

    cx.simulate_keystrokes("ctrl-tab");
    assert_eq!(state(cx).1, GameTab::Launch, "Ctrl+Tab: next tab");
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

    // F11 enters big-screen mode; arrows move, Esc leaves.
    cx.simulate_keystrokes("f11");
    assert!(state(cx).2, "F11: big-screen mode");
    cx.simulate_keystrokes("right left");
    assert_eq!(view.read_with(cx, |l, _| l.big_focus), 0);
    cx.simulate_keystrokes("tab");
    assert!(
        !state(cx).2 && matches!(state(cx).0, Some(Selection::Game(_))),
        "Tab: the focused game's page"
    );
    cx.simulate_keystrokes("f11 escape");
    assert!(!state(cx).2, "Esc leaves big-screen mode");
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
    // Every tab of a Windows game renders without panicking.
    for tab in GameTab::ALL {
        view.update(cx, |l, cx| {
            // What clicking a tab does.
            l.feature_query.clear();
            l.fields.inputs.remove("feature-search");
            l.game_tab = tab;
            cx.notify();
        });
        cx.run_until_parked();
    }
    // Ctrl+F on a game page goes to its settings search.
    cx.simulate_keystrokes("ctrl-f");
    cx.simulate_input("dlss");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |l, _| l.feature_query.clone()), "dlss");
}
