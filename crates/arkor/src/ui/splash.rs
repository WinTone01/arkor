//! A small "Starting <game>…" window shown while a game starts (optional, Global settings).

use std::time::Duration;

use gpui::{
    App, Bounds, Context, SharedString, Window, WindowBounds, WindowDecorations, WindowKind,
    WindowOptions, div, prelude::*, px, size,
};

use super::widgets::theme::*;

const SHOWN_FOR: Duration = Duration::from_secs(5);

struct Splash {
    title: SharedString,
    subtitle: SharedString,
}

impl Render for Splash {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .bg(PANEL())
            .border_1()
            .border_color(ACCENT())
            .child(div().text_sm().text_color(ACCENT()).child("Arkor"))
            .child(div().text_xl().text_color(TEXT()).child(self.title.clone()))
            .child(
                div()
                    .text_sm()
                    .text_color(MUTED())
                    .child(self.subtitle.clone()),
            )
    }
}

/// Opens the splash and closes it after a few seconds; runs `then` once it is gone.
pub(super) fn show(
    title: String,
    subtitle: &'static str,
    cx: &mut App,
    then: impl FnOnce(&mut App) + 'static,
) {
    let bounds = Bounds::centered(None, size(px(460.), px(150.)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            window_decorations: Some(WindowDecorations::Client),
            app_id: Some("arkor-splash".into()),
            ..Default::default()
        },
        |_, cx| {
            cx.new(|_| Splash {
                title: title.into(),
                subtitle: subtitle.into(),
            })
        },
    );
    let timer = cx.background_executor().timer(SHOWN_FOR);
    cx.spawn(async move |cx| {
        timer.await;
        cx.update(|cx| {
            if let Ok(h) = handle {
                let _ = h.update(cx, |_, window, _| window.remove_window());
            }
            then(cx);
        });
    })
    .detach();
}
