//! "Import from prefix": add a game no launcher knows about by pointing at its Wine prefix.
//! Arkor only records the prefix and executable; it never copies or modifies anything.

use std::path::PathBuf;

use arkor_core::imported::Imported;
use arkor_core::{GameId, Store};
use arkor_stores::prefix::{self, Candidate};
use gpui::{Context, Div, PathPromptOptions, div, prelude::*, px, rgb};

use super::Library;
use super::widgets::{self, theme::*};

#[derive(Default)]
pub(super) struct ImportState {
    prefix: Option<PathBuf>,
    candidates: Vec<Candidate>,
    scanning: bool,
    error: Option<String>,
}

impl Library {
    fn choose_prefix(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(self.t().choose_prefix.into()),
        });
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            // Cancelled dialog or portal error: nothing chosen, nothing to do.
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(chosen) = paths.into_iter().next() else {
                return;
            };
            let Some(root) = prefix::resolve(&chosen) else {
                let _ = this.update(cx, |this, cx| {
                    this.import = ImportState {
                        error: Some(format!("{}: {}", this.t().not_a_prefix, chosen.display())),
                        ..Default::default()
                    };
                    cx.notify();
                });
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.import = ImportState {
                    prefix: Some(root.clone()),
                    scanning: true,
                    ..Default::default()
                };
                cx.notify();
            });
            // Scanning a big prefix touches many files: keep it off the UI thread.
            let scan_root = root.clone();
            let candidates = bg
                .spawn(async move { prefix::find_executables(&scan_root) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.import.prefix.as_ref() == Some(&root) {
                    this.import.candidates = candidates;
                    this.import.scanning = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(self.t().choose_file.into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(exe) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.add_imported(exe, cx));
        })
        .detach();
    }

    fn add_imported(&mut self, exe: PathBuf, cx: &mut Context<Self>) {
        let Some(prefix) = self.import.prefix.clone() else {
            return;
        };
        let name = prefix::suggested_name(&exe);
        let result = Imported::load().and_then(|mut imported| {
            let id = imported.add(name, prefix, exe);
            imported.save().map(|()| id)
        });
        match result {
            Ok(id) => {
                self.import = ImportState::default();
                self.reload_games();
                self.select_game(&GameId::new(Store::Local, id), cx);
            }
            Err(e) => {
                self.import.error = Some(format!("{}: {e:#}", self.t().save_failed));
                cx.notify();
            }
        }
    }

    pub(super) fn render_import(&self, cx: &mut Context<Self>) -> Div {
        let t = self.t();
        let state = &self.import;
        let mut page = div()
            .child(Self::header(t.import_prefix, t.import_intro))
            .child(
                div().flex().gap_2().child(
                    widgets::chip("choose-prefix", t.choose_prefix, false)
                        .on_click(cx.listener(|this, _, _, cx| this.choose_prefix(cx))),
                ),
            );
        if let Some(error) = &state.error {
            page = page.child(div().text_sm().text_color(rgb(ACCENT)).child(error.clone()));
        }
        let Some(prefix_root) = &state.prefix else {
            return page;
        };

        let mut card = widgets::section(t.pick_exe).child(widgets::label(format!(
            "{}: {}",
            t.prefix_label,
            prefix_root.display()
        )));
        if state.scanning {
            card = card.child(widgets::label(t.scanning));
        } else if state.candidates.is_empty() {
            card = card.child(widgets::label(t.no_exes_found));
        }
        for (ix, candidate) in state.candidates.iter().enumerate() {
            let rel = candidate
                .path
                .strip_prefix(prefix_root.join("drive_c"))
                .unwrap_or(&candidate.path)
                .display()
                .to_string();
            let size = if candidate.size > 0 {
                prefix::human_size(candidate.size)
            } else {
                String::new()
            };
            let exe = candidate.path.clone();
            card = card.child(
                div()
                    .id(("candidate", ix))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(BG))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(ROW_HOVER)))
                    .on_click(cx.listener(move |this, _, _, cx| this.add_imported(exe.clone(), cx)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .child(prefix::suggested_name(&candidate.path)),
                            )
                            .child(div().truncate().child(widgets::label(rel))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(80.))
                            .flex()
                            .justify_end()
                            .child(widgets::label(size)),
                    ),
            );
        }
        card = card.child(
            div().pt_2().child(
                widgets::chip("choose-file", t.choose_file, false)
                    .on_click(cx.listener(|this, _, _, cx| this.choose_file(cx))),
            ),
        );
        page.child(card)
    }
}
