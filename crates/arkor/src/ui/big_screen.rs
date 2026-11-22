//! Big-screen mode: a fullscreen cover grid driven by a gamepad (or the keyboard), for the
//! couch. F11 / Start toggles it; D-pad or left stick moves, A/Enter plays, Y opens the
//! game's page, B/Escape leaves.

use std::sync::mpsc::{Receiver, channel};

use gpui::{
    Context, Div, KeyBinding, ObjectFit, StyledImage, Window, actions, div, img, prelude::*, px,
};

use super::widgets::{self, theme::*};
use super::{Library, Selection};

actions!(
    big_screen,
    [
        ToggleBigScreen,
        NavUp,
        NavDown,
        NavLeft,
        NavRight,
        NavAccept,
        NavBack,
        NavDetails
    ]
);

const CONTEXT: &str = "BigScreen";
const TILE_W: f32 = 220.;
const TILE_H: f32 = 330.;
const GAP: f32 = 24.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    Details,
    Toggle,
}

pub fn bind_keys(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("f11", ToggleBigScreen, None),
        KeyBinding::new("up", NavUp, Some(CONTEXT)),
        KeyBinding::new("down", NavDown, Some(CONTEXT)),
        KeyBinding::new("left", NavLeft, Some(CONTEXT)),
        KeyBinding::new("right", NavRight, Some(CONTEXT)),
        KeyBinding::new("enter", NavAccept, Some(CONTEXT)),
        KeyBinding::new("space", NavAccept, Some(CONTEXT)),
        KeyBinding::new("escape", NavBack, Some(CONTEXT)),
        KeyBinding::new("tab", NavDetails, Some(CONTEXT)),
    ]);
}

/// Reads gamepads on a thread and turns their input into navigation; `None` without gilrs.
pub(super) fn start_gamepad() -> Option<Receiver<Nav>> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("arkor-gamepad".into())
        .spawn(move || {
            let Ok(mut gilrs) = gilrs::Gilrs::new() else {
                return;
            };
            // Stick direction last reported per axis, so holding the stick moves once.
            let (mut x_state, mut y_state) = (0i8, 0i8);
            loop {
                while let Some(gilrs::Event { event, .. }) =
                    gilrs.next_event_blocking(Some(std::time::Duration::from_millis(500)))
                {
                    use gilrs::{Axis, Button, EventType};
                    let nav = match event {
                        EventType::ButtonPressed(b, _) => match b {
                            Button::DPadUp => Some(Nav::Up),
                            Button::DPadDown => Some(Nav::Down),
                            Button::DPadLeft => Some(Nav::Left),
                            Button::DPadRight => Some(Nav::Right),
                            Button::South => Some(Nav::Accept),
                            Button::East => Some(Nav::Back),
                            Button::North => Some(Nav::Details),
                            Button::Start | Button::Mode => Some(Nav::Toggle),
                            _ => None,
                        },
                        EventType::AxisChanged(
                            axis @ (Axis::LeftStickX | Axis::LeftStickY),
                            v,
                            _,
                        ) => {
                            let dir = if v > 0.6 {
                                1
                            } else if v < -0.6 {
                                -1
                            } else {
                                0
                            };
                            let state = if axis == Axis::LeftStickX {
                                &mut x_state
                            } else {
                                &mut y_state
                            };
                            if dir == *state {
                                None
                            } else {
                                *state = dir;
                                match (axis, dir) {
                                    (Axis::LeftStickX, 1) => Some(Nav::Right),
                                    (Axis::LeftStickX, -1) => Some(Nav::Left),
                                    (Axis::LeftStickY, 1) => Some(Nav::Up),
                                    (Axis::LeftStickY, -1) => Some(Nav::Down),
                                    _ => None,
                                }
                            }
                        }
                        _ => None,
                    };
                    if let Some(nav) = nav
                        && tx.send(nav).is_err()
                    {
                        return;
                    }
                }
            }
        })
        .ok()?;
    Some(rx)
}

impl Library {
    pub(super) fn toggle_big_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.big_screen = !self.big_screen;
        // Leave any focused text field so the arrow keys and Esc reach big-screen mode.
        self.refocus = true;
        if self.big_screen {
            self.big_focus = match self.selection {
                Some(Selection::Game(ix)) => {
                    self.visible.iter().position(|&v| v == ix).unwrap_or(0)
                }
                _ => 0,
            };
        }
        // Fullscreen follows the mode.
        if window.is_fullscreen() != self.big_screen {
            window.toggle_fullscreen();
        }
        cx.notify();
    }

    pub(super) fn nav(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        if nav == Nav::Toggle {
            self.toggle_big_screen(window, cx);
            return;
        }
        if !self.big_screen {
            return;
        }
        let n = self.visible.len();
        if n == 0 {
            if nav == Nav::Back {
                self.toggle_big_screen(window, cx);
            }
            return;
        }
        let cols = self.big_cols.max(1);
        let f = self.big_focus.min(n - 1);
        self.big_focus = match nav {
            Nav::Left => f.saturating_sub(1),
            Nav::Right => (f + 1).min(n - 1),
            Nav::Up => f.saturating_sub(cols),
            Nav::Down => (f + cols).min(n - 1),
            _ => f,
        };
        match nav {
            Nav::Accept => {
                let ix = self.visible[self.big_focus];
                self.select(Selection::Game(ix), cx);
                self.play(cx);
            }
            Nav::Details => {
                let ix = self.visible[self.big_focus];
                self.toggle_big_screen(window, cx);
                self.select(Selection::Game(ix), cx);
            }
            Nav::Back => self.toggle_big_screen(window, cx),
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn render_big(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let width = f32::from(window.viewport_size().width) - 2. * 48.;
        self.big_cols = ((width + GAP) / (TILE_W + GAP)).floor().max(1.) as usize;
        let mut grid = div().flex().flex_wrap().gap(px(GAP));
        for (row, &ix) in self.visible.iter().enumerate() {
            let game = &self.games[ix];
            let focused = row == self.big_focus;
            let cover: gpui::AnyElement = match self.covers.get(&game.id.id).cloned().flatten() {
                Some(path) => img(path)
                    .w(px(TILE_W - 12.))
                    .h(px(TILE_H))
                    .rounded_lg()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                None => self.cover_tile(&game.id.id, &game.name, TILE_W - 12., TILE_H),
            };
            let played = self.playtime(&game.id).0;
            grid = grid.child(
                div()
                    .id(("big", ix))
                    .w(px(TILE_W))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_1()
                    .rounded_lg()
                    .border_2()
                    .border_color(if focused {
                        super::style::pal::accent()
                    } else {
                        gpui::transparent_black()
                    })
                    .when(focused, |d| d.bg(super::style::pal::accent().opacity(0.12)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(row) = this.visible.iter().position(|&v| v == ix) {
                            this.big_focus = row;
                        }
                        this.select(Selection::Game(ix), cx);
                        this.play(cx);
                    }))
                    .child(cover)
                    .child(
                        div()
                            .truncate()
                            .text_lg()
                            .text_color(if focused { ACCENT() } else { TEXT() })
                            .child(game.name.clone()),
                    )
                    .child(widgets::label(if played > 0 {
                        arkor_core::stats::format_duration(played)
                    } else {
                        String::new()
                    })),
            );
        }
        let hint = self.tr(
            "D-pad / arrows: move  ·  A / Enter: play  ·  Y / Tab: game page  ·  B / Esc: leave  ·  Start / F11: toggle",
            "D-pad / oklar: gezin  ·  A / Enter: oyna  ·  Y / Tab: oyun sayfası  ·  B / Esc: çık  ·  Start / F11: aç/kapat",
        );
        div()
            .id("big-screen")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .p(px(48.))
            .gap_6()
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(super::style::logo(40.))
                            .child(
                                div()
                                    .text_3xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Arkor"),
                            ),
                    )
                    .child(widgets::label(hint)),
            )
            .children(self.status.clone().map(widgets::label))
            .child(grid)
    }
}
