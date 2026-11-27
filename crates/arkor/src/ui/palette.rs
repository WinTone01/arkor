//! The command palette (Ctrl+K): type a few letters, then Enter, to go to a page, open or
//! play a game, switch to console mode or look for updates. Up and Down pick another line;
//! Esc closes it.

use gpui::Div;
use gpui::prelude::FluentBuilder;
use gpui::{
    App, Context, Entity, Focusable, FontWeight, KeyBinding, Stateful, Subscription, Window,
    actions, div, prelude::*, px,
};

use super::shell::Nav;
use super::style::{self, pal};
use super::text_input::{InputEvent, TextInput};
use super::{Library, Selection};

actions!(palette, [OpenPalette, PaletteUp, PaletteDown]);

const CONTEXT: &str = "Palette";

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-k", OpenPalette, Some("Arkor")),
        KeyBinding::new("up", PaletteUp, Some(CONTEXT)),
        KeyBinding::new("down", PaletteDown, Some(CONTEXT)),
    ]);
}

/// What a line of the palette does.
#[derive(Clone, PartialEq)]
enum Do {
    Go(Nav),
    Open(usize),
    Play(usize),
    Console,
    Updates,
}

pub(super) struct Palette {
    input: Entity<TextInput>,
    _sub: Subscription,
    query: String,
    selected: usize,
}

fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'ı' => 'i',
            'ş' => 's',
            'ğ' => 'g',
            'ü' => 'u',
            'ö' => 'o',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

impl Library {
    /// Every line the palette offers, in order: (icon, text, what it does).
    fn palette_lines(&self) -> Vec<(&'static str, String, Do)> {
        let mut out: Vec<(&'static str, String, Do)> = [
            ("library-big", Nav::Library),
            ("plus", Nav::Add),
            ("store", Nav::Shops),
            ("wine", Nav::Protons),
            ("joystick", Nav::Emulators),
            ("chart-column", Nav::Stats),
            ("gauge", Nav::Overlay),
            ("chart-line", Nav::Bench),
            ("hard-drive-download", Nav::Downloads),
            ("cpu", Nav::System),
            ("settings", Nav::Settings),
        ]
        .into_iter()
        .map(|(icon, nav)| {
            (
                icon,
                format!("{} {}", self.tr("Go to", "Git:"), self.nav_title(nav)),
                Do::Go(nav),
            )
        })
        .collect();
        out.push((
            "tv",
            self.tr("Console mode", "Konsol modu").into(),
            Do::Console,
        ));
        out.push((
            "refresh-cw",
            self.tr("Look for updates", "Güncellemeleri denetle").into(),
            Do::Updates,
        ));
        for (ix, g) in self.games.iter().enumerate() {
            out.push((
                "play",
                format!("{} {}", self.tr("Play", "Oyna:"), g.name),
                Do::Play(ix),
            ));
            out.push(("gamepad-2", g.name.to_string(), Do::Open(ix)));
        }
        out
    }

    /// The lines matching what was typed (every word somewhere in the line), a dozen at most.
    fn palette_matches(&self) -> Vec<(&'static str, String, Do)> {
        let query = self
            .palette
            .as_ref()
            .map(|p| fold(&p.query))
            .unwrap_or_default();
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut lines = self.palette_lines();
        if words.is_empty() {
            // Nothing typed: the pages, then the games played last.
            lines.retain(|(_, _, d)| !matches!(d, Do::Play(_) | Do::Open(_)));
            let mut recent: Vec<usize> = self.recent_games();
            recent.truncate(4);
            for ix in recent {
                if let Some(g) = self.games.get(ix) {
                    lines.insert(
                        0,
                        (
                            "play",
                            format!("{} {}", self.tr("Play", "Oyna:"), g.name),
                            Do::Play(ix),
                        ),
                    );
                }
            }
        } else {
            // Every word somewhere in the line, or close to it (typos forgiven); best first.
            let mut scored: Vec<(usize, (&'static str, String, Do))> = lines
                .into_iter()
                .filter_map(|l| {
                    let t = fold(&l.1);
                    let exact = words.iter().all(|w| t.contains(w));
                    let s = if exact {
                        Some(0)
                    } else {
                        arkor_core::fuzzy::score(&query, &l.1).map(|s| s + 1)
                    };
                    s.map(|s| (s, l))
                })
                .collect();
            scored.sort_by_key(|(s, _)| *s);
            lines = scored.into_iter().map(|(_, l)| l).collect();
        }
        lines.truncate(12);
        lines
    }

    pub(super) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.console.on {
            return;
        }
        let placeholder = self.tr(
            "Type a page, a game, or \"play\"…",
            "Bir sayfa, bir oyun ya da \"oyna\" yaz…",
        );
        let input = cx.new(|cx| TextInput::field("", placeholder, cx));
        let sub = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::Changed(text) => {
                    if let Some(p) = this.palette.as_mut() {
                        p.query = text.clone();
                        p.selected = 0;
                    }
                }
                InputEvent::Submit(_) => this.palette_run(None, window, cx),
                InputEvent::Cancel => this.palette = None,
            }
            cx.notify();
        });
        let handle = input.read(cx).focus_handle(cx);
        self.palette = Some(Palette {
            input,
            _sub: sub,
            query: String::new(),
            selected: 0,
        });
        window.focus(&handle, cx);
        cx.notify();
    }

    fn palette_step(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = self.palette_matches().len();
        if let Some(p) = self.palette.as_mut()
            && n > 0
        {
            p.selected = (p.selected as i32 + delta).rem_euclid(n as i32) as usize;
            cx.notify();
        }
    }

    /// Does the chosen line (`at`, else the selected one) and closes the palette.
    fn palette_run(&mut self, at: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let lines = self.palette_matches();
        let ix = at.unwrap_or_else(|| self.palette.as_ref().map_or(0, |p| p.selected));
        self.palette = None;
        let Some((_, _, what)) = lines.get(ix).cloned() else {
            cx.notify();
            return;
        };
        match what {
            Do::Go(nav) => self.go(nav, cx),
            Do::Open(g) => self.select(Selection::Game(g), cx),
            Do::Play(g) => {
                self.select(Selection::Game(g), cx);
                self.play(cx);
            }
            Do::Console => self.toggle_console(window, cx),
            Do::Updates => {
                self.go(Nav::Downloads, cx);
                self.check_tool_updates(cx);
            }
        }
        cx.notify();
    }

    /// The palette over the window, when open.
    pub(super) fn render_palette(&mut self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let p = self.palette.as_ref()?;
        let input = p.input.clone();
        let selected = p.selected;
        let lines = self.palette_matches();
        let mut list = div().flex().flex_col().gap_0p5().p_1();
        for (ix, (icon, text, _)) in lines.into_iter().enumerate() {
            let on = ix == selected;
            list = list.child(
                div()
                    .id(("palette", ix))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(pal::accent().opacity(0.22)))
                    .when(!on, |d| d.hover(|h| h.bg(pal::text().opacity(0.05))))
                    .child(style::icon(
                        icon,
                        15.,
                        if on {
                            pal::accent_light()
                        } else {
                            pal::muted()
                        },
                    ))
                    .child(
                        div()
                            .truncate()
                            .when(on, |d| d.font_weight(FontWeight::MEDIUM))
                            .child(text),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.palette_run(Some(ix), window, cx)
                    })),
            );
        }
        Some(
            div()
                .id("palette-scrim")
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .pt(px(90.))
                .bg(gpui::black().opacity(0.45))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.palette = None;
                    cx.notify();
                }))
                .child(
                    style::card()
                        .id("palette")
                        .key_context(CONTEXT)
                        .on_action(
                            cx.listener(|this, _: &PaletteUp, _, cx| this.palette_step(-1, cx)),
                        )
                        .on_action(
                            cx.listener(|this, _: &PaletteDown, _, cx| this.palette_step(1, cx)),
                        )
                        // Clicks inside do not close it.
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .w(px(560.))
                        .max_h(px(520.))
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(div().p_3().child(input))
                        .child(list),
                ),
        )
    }
}
