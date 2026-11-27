//! Disk space on the System page: the disks, what each game takes (biggest first) and the
//! leftovers that can go, each deleted with a second press.

use std::path::PathBuf;

use arkor_launch::storage::{self, Disk, Leftover};
use gpui::{Context, Div, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, widgets};

#[derive(Default)]
pub(super) struct StorageUi {
    /// Disks, games by size, leftovers (read in the background).
    data: Option<(Vec<Disk>, Vec<(String, PathBuf, u64)>, Vec<Leftover>)>,
    loading: bool,
    /// The leftover asked about once (deleted on the second press).
    confirm: Option<PathBuf>,
}

impl Library {
    fn storage_scan(&mut self, cx: &mut Context<Self>) {
        if self.storage.loading || cfg!(test) {
            return;
        }
        self.storage.loading = true;
        let mut paths = vec![
            dirs::home_dir().unwrap_or_default(),
            self.global.games_dir(),
        ];
        paths.push(self.global.prefixes_dir());
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let data = bg
                .spawn(async move {
                    (
                        storage::disks(&paths),
                        storage::game_sizes(),
                        storage::leftovers(),
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.storage.data = Some(data);
                this.storage.loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_storage(&mut self, cx: &mut Context<Self>) -> Div {
        if self.storage.data.is_none() {
            self.storage_scan(cx);
        }
        let mut card = widgets::section(self.tr("Disk space", "Disk alanı")).child(widgets::label(self.tr(
            "How full the disks are, what each game takes, and what can be deleted safely.",
            "Disklerin ne kadar dolu olduğu, her oyunun ne kadar yer kapladığı ve neyin güvenle silinebileceği.",
        )));
        let Some((disks, games, leftovers)) = self.storage.data.clone() else {
            return card.child(widgets::label(self.tr("Measuring…", "Ölçülüyor…")));
        };
        for d in &disks {
            let used = d.size.saturating_sub(d.free);
            let frac = if d.size > 0 {
                used as f32 / d.size as f32
            } else {
                0.
            };
            card = card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_sm()
                            .child(d.mount.display().to_string())
                            .child(div().text_color(pal::muted()).child(format!(
                                "{} {} / {}",
                                storage::human(d.free),
                                self.tr("free of", "boş, toplam"),
                                storage::human(d.size)
                            ))),
                    )
                    .child(
                        div()
                            .h(px(10.))
                            .rounded_full()
                            .bg(pal::text().opacity(0.08))
                            .child(div().h_full().rounded_full().w(gpui::relative(frac)).bg(
                                if frac > 0.9 {
                                    pal::danger()
                                } else {
                                    pal::accent()
                                },
                            )),
                    ),
            );
        }
        if !games.is_empty() {
            let biggest = games[0].2.max(1);
            card = card.child(widgets::subheading(self.tr("Games", "Oyunlar")));
            for (name, dir, bytes) in games.iter().take(15) {
                let dir = dir.clone();
                card = card.child(
                    div()
                        .id(gpui::SharedString::from(format!("st-{name}")))
                        .flex()
                        .items_center()
                        .gap_3()
                        .cursor_pointer()
                        .child(div().w(px(220.)).truncate().text_sm().child(name.clone()))
                        .child(
                            div()
                                .flex_1()
                                .h(px(8.))
                                .rounded_full()
                                .bg(pal::text().opacity(0.06))
                                .child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .w(gpui::relative(*bytes as f32 / biggest as f32))
                                        .bg(pal::accent()),
                                ),
                        )
                        .child(
                            div()
                                .w(px(80.))
                                .text_sm()
                                .text_color(pal::muted())
                                .child(storage::human(*bytes)),
                        )
                        .on_click(move |_, _, _| {
                            let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
                        }),
                );
            }
        }
        card = card.child(widgets::subheading(
            self.tr("Can be deleted", "Silinebilir"),
        ));
        if leftovers.is_empty() {
            card = card.child(widgets::label(
                self.tr("Nothing left over.", "Artık bir şey yok."),
            ));
        }
        for (ix, l) in leftovers.into_iter().enumerate() {
            let what = match l.kind {
                "prefix" => self.tr(
                    "Wine prefix of a game no longer in the library",
                    "Artık kütüphanede olmayan bir oyunun Wine prefix'i",
                ),
                "installers" => self.tr(
                    "Launcher installers kept after they ran",
                    "Çalıştıktan sonra saklanan başlatıcı kurulum dosyaları",
                ),
                _ => self.tr(
                    "Sandbox folder of a removed game",
                    "Kaldırılmış bir oyunun yalıtım klasörü",
                ),
            };
            let asked = self.storage.confirm.as_ref() == Some(&l.path);
            let item = l.clone();
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(style::icon("trash-2", 14., pal::muted()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_sm()
                                    .child(format!("{what} · {}", storage::human(l.bytes))),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(pal::muted())
                                    .truncate()
                                    .child(l.path.display().to_string()),
                            ),
                    )
                    .child(
                        widgets::chip(
                            ("st-del", ix),
                            if asked {
                                self.tr("Sure? Press again", "Emin misin? Tekrar bas")
                            } else {
                                self.tr("Delete", "Sil")
                            },
                            asked,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.storage.confirm.as_ref() != Some(&item.path) {
                                this.storage.confirm = Some(item.path.clone());
                            } else {
                                this.storage.confirm = None;
                                match storage::remove(&item) {
                                    Ok(()) => this.storage.data = None,
                                    Err(e) => this.status = Some(format!("{e:#}").into()),
                                }
                            }
                            cx.notify();
                        })),
                    ),
            );
        }
        card.child(div().flex().child(
            widgets::chip("st-again", self.tr("Measure again", "Yeniden ölç"), false).on_click(
                cx.listener(|this, _, _, cx| {
                    this.storage.data = None;
                    cx.notify();
                }),
            ),
        ))
    }
}
