//! A game's mods, on its page: through Amethyst Mod Manager (see `arkor_launch::mods`).
//! The list is read when the panel is first drawn for a game and after every change; each
//! change is placed in the game folder at once. An installer's questions (a FOMOD) are
//! asked here; everything else Amethyst does opens in its own window.

use std::path::PathBuf;

use arkor_core::GameId;
use arkor_launch::mods::{self, Answer, Choices, Installed, Link, Mod, amethyst, link};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, PathPromptOptions, div, prelude::*, px};
use serde_json::{Value, json};

use super::style::{self, pal};
use super::{Library, widgets};

/// What is known of the selected game's mods.
#[derive(Debug, Clone, Default)]
pub(super) enum Known {
    #[default]
    Nothing,
    Reading,
    /// Not linked yet: Amethyst's game it would be (if one fits), and where the game is.
    Unlinked {
        fits: Option<link::Candidate>,
        place: Option<link::Place>,
    },
    Linked {
        link: Link,
        mods: Vec<Mod>,
    },
    Failed(String),
}

/// An installer's questions while they are answered.
#[derive(Debug, Clone)]
pub(super) struct Asking {
    archive: PathBuf,
    choices: Choices,
    /// FOMOD: `{step: {group: [options]}}`, starting from the author's recommendation.
    picked: Value,
}

#[derive(Debug, Default)]
pub(super) struct ModsUi {
    /// The game this is about (its id as a string).
    game: Option<String>,
    pub(super) known: Known,
    /// What is being done now (shown instead of the buttons).
    busy: Option<&'static str>,
    asking: Option<Asking>,
    /// The mod asked about before it is removed.
    removing: Option<String>,
}

impl Library {
    /// Whether Arkor has Amethyst.
    pub(super) fn mods_ready(&self) -> bool {
        amethyst::installed_version().is_some()
    }

    fn mods_running(&self, id: &GameId) -> bool {
        self.running.contains_key(&id.to_string())
    }

    /// Reads the selected game's mods (again), off the UI thread.
    fn mods_load(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(game)) = (self.selected_id(), self.entry().cloned()) else {
            return;
        };
        self.mods.game = Some(id.to_string());
        self.mods.known = Known::Reading;
        let prefixes = self.global.prefixes_dir();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let key = id.to_string();
            let known = bg
                .spawn(async move {
                    amethyst::with(|c| {
                        if let Some(link) = link::load(&id) {
                            return Ok(Known::Linked {
                                mods: mods::list(c, &link)?,
                                link,
                            });
                        }
                        let place = link::place(&game, &prefixes);
                        let games = link::games(c)?;
                        let fits = link::best_match(
                            &games,
                            &game.name,
                            place.as_ref().and_then(|p| p.steam_appid.as_deref()),
                        )
                        .cloned();
                        Ok(Known::Unlinked { fits, place })
                    })
                    .unwrap_or_else(|e| Known::Failed(format!("{e:#}")))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.mods.game.as_deref() == Some(key.as_str()) {
                    this.mods.known = known;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Does `work` with Amethyst off the UI thread, then reads the list again.
    fn mods_do(
        &mut self,
        doing: &'static str,
        cx: &mut Context<Self>,
        work: impl FnOnce(&mut mods::rpc::Client) -> anyhow::Result<()> + Send + 'static,
    ) {
        if self.mods.busy.is_some() {
            return;
        }
        self.mods.busy = Some(doing);
        self.mods.removing = None;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { amethyst::with(work) }).await;
            let _ = this.update(cx, |this, cx| {
                this.mods.busy = None;
                if let Err(e) = result {
                    this.status = Some(format!("{e:#}").into());
                }
                this.mods_load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn mods_link(
        &mut self,
        candidate: link::Candidate,
        place: link::Place,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected_id() else { return };
        self.mods_do(
            self.tr("Setting up…", "Hazırlanıyor…"),
            cx,
            move |c| link::connect(c, &id, &candidate, &place).map(|_| ()),
        );
    }

    /// Picks an archive, then installs it.
    fn mods_pick_archive(&mut self, link: Link, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(self.tr("Install a mod", "Mod kur").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.mods_install(link, path, None, cx));
        })
        .detach();
    }

    fn mods_install(
        &mut self,
        link: Link,
        archive: PathBuf,
        answer: Option<Answer>,
        cx: &mut Context<Self>,
    ) {
        if self.mods.busy.is_some() {
            return;
        }
        self.mods.busy = Some(self.tr("Installing…", "Kuruluyor…"));
        self.mods.asking = None;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let path = archive.clone();
            let result = bg
                .spawn(async move {
                    amethyst::with(|c| mods::install(c, &link, &path, answer.as_ref()))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.mods.busy = None;
                match result {
                    Ok(Installed::Done(name)) => {
                        this.status =
                            Some(format!("{} {name}", this.tr("Installed:", "Kuruldu:")).into());
                    }
                    Ok(Installed::Asks(choices)) => {
                        this.mods.asking = Some(Asking {
                            archive,
                            picked: choices.defaults(),
                            choices,
                        });
                    }
                    Err(e) => this.status = Some(format!("{e:#}").into()),
                }
                this.mods_load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_mods(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let id = self.selected_id()?;
        if self.mods.game.as_deref() != Some(id.to_string().as_str()) {
            self.mods.asking = None;
            self.mods.removing = None;
            self.mods_load(cx);
        }
        let mut card = widgets::section(self.tr("Mods", "Modlar"));
        if let Some(asking) = self.mods.asking.clone() {
            return Some(card.child(self.render_mods_question(asking, cx)));
        }
        let running = self.mods_running(&id);
        match self.mods.known.clone() {
            Known::Nothing | Known::Reading => {
                card = card.child(widgets::label(self.tr(
                    "Asking Amethyst Mod Manager…",
                    "Amethyst Mod Manager'a soruluyor…",
                )));
            }
            Known::Failed(e) => {
                card = card.child(widgets::label(e)).child(
                    div().flex().gap_2().child(
                        style::button("mods-retry", self.tr("Try again", "Tekrar dene"), false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mods_load(cx);
                                cx.notify();
                            })),
                    ),
                );
            }
            Known::Unlinked { fits, place } => {
                card = card.child(self.render_mods_unlinked(fits, place, cx));
            }
            Known::Linked { link, mods } => {
                card = card.child(self.render_mods_list(link, mods, running, cx));
            }
        }
        Some(card)
    }

    fn render_mods_unlinked(
        &self,
        fits: Option<link::Candidate>,
        place: Option<link::Place>,
        cx: &mut Context<Self>,
    ) -> Div {
        let (Some(fits), Some(place)) = (fits, place.clone()) else {
            let text = if place.is_none() {
                self.tr(
                    "The game's folder could not be found, so its mods cannot be set up here.",
                    "Oyunun klasörü bulunamadı, bu yüzden modları burada ayarlanamıyor.",
                )
            } else {
                self.tr(
                    "Amethyst Mod Manager does not know this game yet. It can still be set up as a custom game in Amethyst's own window.",
                    "Amethyst Mod Manager bu oyunu henüz tanımıyor. Amethyst'in kendi penceresinde özel oyun olarak eklenebilir.",
                )
            };
            return div()
                .flex()
                .flex_col()
                .gap_3()
                .child(widgets::label(text))
                .child(div().flex().child(self.mods_window_button(cx)));
        };
        let text = format!(
            "{} {}. {}",
            self.tr("Amethyst knows this game as", "Amethyst bu oyunu şu adla tanıyor:"),
            fits.name,
            self.tr(
                "Mods go into the game's folder as links; taking them out leaves it as it was.",
                "Modlar oyunun klasörüne bağlantı olarak konur; çıkarılınca klasör eski hâline döner.",
            ),
        );
        let busy = self.mods.busy;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(widgets::label(text))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(match busy {
                        Some(doing) => style::button("mods-link", doing, true),
                        None => style::button(
                            "mods-link",
                            self.tr("Use mods for this game", "Bu oyunda mod kullan"),
                            true,
                        )
                        .child(style::icon("puzzle", 13., pal::on_accent()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.mods_link(fits.clone(), place.clone(), cx);
                            cx.notify();
                        })),
                    })
                    .child(self.mods_window_button(cx)),
            )
    }

    fn mods_window_button(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        style::button(
            "mods-window",
            self.tr("Open Amethyst", "Amethyst'i aç"),
            false,
        )
        .child(style::icon("external-link", 13., pal::muted()))
        .on_click(cx.listener(|this, _, _, cx| {
            if let Err(e) = amethyst::open_window() {
                this.status = Some(format!("{e:#}").into());
            }
            cx.notify();
        }))
    }

    fn render_mods_list(
        &self,
        link: Link,
        list: Vec<Mod>,
        running: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let busy = self.mods.busy;
        let locked = running || busy.is_some();
        let mut out = div().flex().flex_col().gap_3();
        let add_link = link.clone();
        out = out.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(match busy {
                    Some(doing) => style::button("mods-add", doing, true),
                    None => style::button("mods-add", self.tr("Install a mod…", "Mod kur…"), true)
                        .child(style::icon("plus", 13., pal::on_accent()))
                        .when(!running, |b| {
                            b.on_click(cx.listener(move |this, _, _, cx| {
                                this.mods_pick_archive(add_link.clone(), cx);
                            }))
                        }),
                })
                .child(self.mods_window_button(cx)),
        );
        if running {
            out = out.child(widgets::label(self.tr(
                "The game is running: its mods can be changed once it ends.",
                "Oyun çalışıyor: modları oyun kapanınca değiştirilebilir.",
            )));
        }
        if list.is_empty() {
            return out.child(widgets::label(self.tr(
                "No mods yet. Install one from a downloaded archive (zip, 7z, rar), or with Amethyst's window.",
                "Henüz mod yok. İndirilmiş bir arşivden (zip, 7z, rar) ya da Amethyst'in penceresinden kurabilirsin.",
            )));
        }
        out = out.child(widgets::subheading(format!(
            "{} · {}",
            self.tr("Highest priority first", "En yüksek öncelik üstte"),
            list.len()
        )));
        let last = list.len() - 1;
        for (n, m) in list.into_iter().enumerate() {
            out = out.child(self.render_mod_row(n, last, m, &link, locked, cx));
        }
        out
    }

    fn render_mod_row(
        &self,
        n: usize,
        last: usize,
        m: Mod,
        link: &Link,
        locked: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let sure = self.mods.removing.as_deref() == Some(m.name.as_str());
        let mut meta = Vec::new();
        if let Some(v) = &m.version {
            meta.push(v.clone());
        }
        let fixed = locked || m.locked;
        let toggle = {
            let (link, name, on) = (link.clone(), m.name.clone(), m.enabled);
            widgets::switch(m.enabled)
                .id(("mod-on", n))
                .when(!fixed, |s| {
                    s.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let (link, name) = (link.clone(), name.clone());
                            this.mods_do(this.tr("Saving…", "Kaydediliyor…"), cx, move |c| {
                                mods::set_enabled(c, &link, &name, !on)
                            });
                            cx.notify();
                        }))
                })
        };
        let shift = |up: bool, cx: &mut Context<Self>| {
            let (link, name) = (link.clone(), m.name.clone());
            let edge = if up { n == 0 } else { n == last };
            style::icon_button(
                (if up { "mod-up" } else { "mod-down" }, n),
                if up { "chevron-up" } else { "chevron-down" },
                false,
            )
            .when(fixed || edge, |b| b.opacity(0.35))
            .when(!fixed && !edge, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    let (link, name) = (link.clone(), name.clone());
                    this.mods_do(this.tr("Moving…", "Taşınıyor…"), cx, move |c| {
                        mods::shift(c, &link, &name, up)
                    });
                    cx.notify();
                }))
            })
        };
        let remove = {
            let (link, name) = (link.clone(), m.name.clone());
            style::button(
                ("mod-remove", n),
                if sure {
                    self.tr("Really remove", "Gerçekten kaldır")
                } else {
                    ""
                },
                sure,
            )
            .when(!sure, |b| {
                b.size(px(32.))
                    .p_0()
                    .child(style::icon("trash", 14., pal::muted()))
            })
            .when(fixed, |b| b.opacity(0.35))
            .when(!fixed, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    if this.mods.removing.as_deref() == Some(name.as_str()) {
                        let (link, name) = (link.clone(), name.clone());
                        this.mods_do(this.tr("Removing…", "Kaldırılıyor…"), cx, move |c| {
                            mods::remove(c, &link, &name)
                        });
                    } else {
                        this.mods.removing = Some(name.clone());
                    }
                    cx.notify();
                }))
            })
        };
        let page = m.url.clone().map(|url| {
            style::icon_button(("mod-page", n), "external-link", false).on_click(move |_, _, _| {
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            })
        });
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(toggle)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(if m.enabled { pal::text() } else { pal::muted() })
                            .child(m.title.clone()),
                    )
                    .when(!meta.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(pal::muted())
                                .child(meta.join(" · ")),
                        )
                    })
                    .when(m.update, |d| {
                        d.child(style::tag(self.tr("Update", "Güncelleme"), pal::accent()))
                    }),
            )
            .children(page)
            .child(shift(true, cx))
            .child(shift(false, cx))
            .child(remove)
    }

    /// An installer's questions: FOMOD steps with their groups and options; for other
    /// installers only the recommended choice.
    fn render_mods_question(&self, asking: Asking, cx: &mut Context<Self>) -> Div {
        let fomod = asking.choices.installer() == "fomod";
        let name = asking.choices.0["mod"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let mut out = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(widgets::label(format!(
                "{} {name}",
                self.tr(
                    "This mod asks what to install:",
                    "Bu mod neyin kurulacağını soruyor:"
                )
            )));
        if fomod {
            let steps = asking.choices.0["steps"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for step in steps.iter().filter(|s| s["visible_by_default"] != false) {
                let step_ix = step["index"].as_str().unwrap_or("0").to_string();
                out = out.child(widgets::subheading(
                    step["name"].as_str().unwrap_or_default().to_string(),
                ));
                for group in step["groups"].as_array().cloned().unwrap_or_default() {
                    out = out.child(self.render_fomod_group(&asking, &step_ix, &group, cx));
                }
            }
        } else {
            out = out.child(widgets::label(self.tr(
                "Its installer is set up the way its author recommends; Amethyst's window can choose otherwise.",
                "Kurulum, yazarının önerdiği biçimde yapılır; başka seçimler Amethyst'in penceresinden yapılabilir.",
            )));
        }
        let link = match &self.mods.known {
            Known::Linked { link, .. } => Some(link.clone()),
            _ => None,
        };
        let install = {
            let (link, archive, picked) =
                (link.clone(), asking.archive.clone(), asking.picked.clone());
            style::button("fomod-install", self.tr("Install", "Kur"), true).on_click(cx.listener(
                move |this, _, _, cx| {
                    let Some(link) = link.clone() else { return };
                    let answer = match fomod {
                        true => Answer::Fomod(picked.clone()),
                        false => Answer::Defaults,
                    };
                    this.mods_install(link, archive.clone(), Some(answer), cx);
                    cx.notify();
                },
            ))
        };
        out.child(div().flex().gap_2().child(install).child(
            style::button("fomod-cancel", self.tr("Cancel", "Vazgeç"), false).on_click(
                cx.listener(|this, _, _, cx| {
                    this.mods.asking = None;
                    cx.notify();
                }),
            ),
        ))
    }

    fn render_fomod_group(
        &self,
        asking: &Asking,
        step: &str,
        group: &Value,
        cx: &mut Context<Self>,
    ) -> Div {
        let gname = group["name"].as_str().unwrap_or_default().to_string();
        let kind = group["type"].as_str().unwrap_or("SelectAny").to_string();
        let picked: Vec<String> = asking.picked[step][&gname]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let mut opts = div().flex().flex_wrap().gap_2();
        for (n, opt) in group["options"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let oname = opt["name"].as_str().unwrap_or_default().to_string();
            let on = picked.contains(&oname);
            let hint = opt["description"].as_str().unwrap_or_default().to_string();
            let (step, gname, kind) = (step.to_string(), gname.clone(), kind.clone());
            let key = format!("{step}/{gname}/{n}");
            opts = opts.child(
                widgets::choice_card(
                    gpui::ElementId::Name(key.into()),
                    if on { "circle-check" } else { "circle" },
                    oname.clone(),
                    hint,
                    on,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(a) = this.mods.asking.as_mut() {
                        pick(&mut a.picked, &step, &gname, &kind, &oname);
                    }
                    cx.notify();
                })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .text_sm()
                    .text_color(pal::text_soft())
                    .child(group["label"].as_str().unwrap_or(&gname).to_string()),
            )
            .child(opts)
    }
}

/// Picks (or unpicks) an option of a FOMOD group, by the group's rule.
fn pick(picked: &mut Value, step: &str, group: &str, kind: &str, option: &str) {
    if !picked.is_object() {
        *picked = json!({});
    }
    if !picked[step].is_object() {
        picked[step] = json!({});
    }
    let now: Vec<String> = picked[step][group]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let has = now.iter().any(|o| o == option);
    let next: Vec<String> = match kind {
        "SelectExactlyOne" => vec![option.to_string()],
        "SelectAtMostOne" if has => Vec::new(),
        "SelectAtMostOne" => vec![option.to_string()],
        // Every option is installed; nothing to change.
        "SelectAll" => now,
        // At least one stays picked.
        "SelectAtLeastOne" if has && now.len() == 1 => now,
        _ if has => now.into_iter().filter(|o| o != option).collect(),
        _ => now.into_iter().chain([option.to_string()]).collect(),
    };
    picked[step][group] = json!(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_by_the_groups_rule() {
        let mut p = json!({"0": {"Quality": ["High"]}});
        pick(&mut p, "0", "Quality", "SelectExactlyOne", "Low");
        assert_eq!(p["0"]["Quality"], json!(["Low"]));
        pick(&mut p, "0", "Quality", "SelectExactlyOne", "Low");
        assert_eq!(p["0"]["Quality"], json!(["Low"]), "one always stays");

        pick(&mut p, "1", "Extras", "SelectAny", "A");
        pick(&mut p, "1", "Extras", "SelectAny", "B");
        assert_eq!(p["1"]["Extras"], json!(["A", "B"]));
        pick(&mut p, "1", "Extras", "SelectAny", "A");
        assert_eq!(p["1"]["Extras"], json!(["B"]));

        pick(&mut p, "1", "Patch", "SelectAtMostOne", "X");
        pick(&mut p, "1", "Patch", "SelectAtMostOne", "X");
        assert_eq!(p["1"]["Patch"], json!([]), "at most one: may be none");

        pick(&mut p, "1", "Core", "SelectAtLeastOne", "C");
        pick(&mut p, "1", "Core", "SelectAtLeastOne", "C");
        assert_eq!(p["1"]["Core"], json!(["C"]), "at least one stays");
    }
}
