//! Steam friends in the sidebar: who is online, and what they play. Read from Steam's client
//! through its developer port (so only with Steam control on), every minute while Arkor's
//! window is open; nothing is kept on disk.

use std::time::Duration;

use arkor_launch::steam_control::{self, Friend};
use gpui::{Context, Div, div, prelude::*, px};

use super::Library;
use super::style::pal;

#[derive(Default)]
pub(super) struct FriendsUi {
    list: Vec<Friend>,
    open: bool,
}

impl Library {
    /// Reads the friends now and every minute after.
    pub(super) fn friends_watch(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                let wanted = this
                    .update(cx, |this, _| this.global.steam_control)
                    .unwrap_or(false);
                let list = if wanted {
                    bg.spawn(async { steam_control::friends().unwrap_or_default() })
                        .await
                } else {
                    Vec::new()
                };
                if this
                    .update(cx, |this, cx| {
                        if this.friends.list != list {
                            this.friends.list = list;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
                bg.timer(Duration::from_secs(60)).await;
            }
        })
        .detach();
    }

    /// The friends online, in the sidebar (closed to a line until opened).
    pub(super) fn render_friends(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let online: Vec<Friend> = self
            .friends
            .list
            .iter()
            .filter(|f| f.online())
            .cloned()
            .collect();
        if online.is_empty() {
            return None;
        }
        let playing = online.iter().filter(|f| f.app != 0).count();
        let open = self.friends.open;
        let mut out = div().flex().flex_col().gap_0p5().child(
            div()
                .id("friends-head")
                .flex()
                .items_center()
                .justify_between()
                .px_2()
                .pt_3()
                .pb_1()
                .cursor_pointer()
                .text_xs()
                .text_color(pal::faint())
                .child(format!(
                    "{} · {} {}",
                    self.tr("FRIENDS", "ARKADAŞLAR"),
                    online.len(),
                    self.tr("online", "çevrimiçi")
                ))
                .child(if open { "−" } else { "+" })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.friends.open = !this.friends.open;
                    cx.notify();
                })),
        );
        if !open {
            if playing > 0 {
                out = out.child(
                    div()
                        .px_2()
                        .text_xs()
                        .text_color(pal::muted())
                        .child(format!("{playing} {}", self.tr("playing", "oyunda"))),
                );
            }
            return Some(out);
        }
        for f in online.iter().take(12) {
            let dot = if f.app != 0 {
                pal::success()
            } else if f.state == 1 || f.state >= 5 {
                pal::accent_light()
            } else {
                pal::muted()
            };
            out = out.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_0p5()
                    .child(div().size(px(7.)).flex_none().rounded_full().bg(dot))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .text_color(pal::text_soft())
                                    .child(f.name.clone()),
                            )
                            .when(!f.game.is_empty(), |d| {
                                d.child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(pal::muted())
                                        .child(f.game.clone()),
                                )
                            }),
                    ),
            );
        }
        Some(out)
    }
}
