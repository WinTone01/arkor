//! Grouping the library (by status, store, genre, series, year, Steam tag or category),
//! showing only one group, and saved views: a name for the filters, grouping and order in
//! use, which stay current by themselves (a game that starts to match shows up).

use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, div, prelude::*, px};

use super::style::{self, pal};
use super::{Library, Sort, library_views::Smart, widgets};

/// What the library can be grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Facet {
    Status,
    Store,
    Genre,
    Series,
    Year,
    Tag,
    Category,
}

impl Facet {
    pub(super) const ALL: [Facet; 7] = [
        Facet::Status,
        Facet::Store,
        Facet::Genre,
        Facet::Series,
        Facet::Year,
        Facet::Tag,
        Facet::Category,
    ];

    pub(super) fn key(self) -> &'static str {
        match self {
            Facet::Status => "status",
            Facet::Store => "store",
            Facet::Genre => "genre",
            Facet::Series => "series",
            Facet::Year => "year",
            Facet::Tag => "tag",
            Facet::Category => "category",
        }
    }

    pub(super) fn from_key(k: &str) -> Option<Facet> {
        Facet::ALL.into_iter().find(|f| f.key() == k)
    }
}

impl Sort {
    fn key(self) -> &'static str {
        match self {
            Sort::Name => "name",
            Sort::Recent => "recent",
            Sort::Playtime => "playtime",
        }
    }

    fn from_key(k: &str) -> Sort {
        match k {
            "recent" => Sort::Recent,
            "playtime" => Sort::Playtime,
            _ => Sort::Name,
        }
    }
}

impl Library {
    fn facet_name(&self, f: Facet) -> &'static str {
        match f {
            Facet::Status => self.tr("Status", "Durum"),
            Facet::Store => self.tr("Store", "Mağaza"),
            Facet::Genre => self.tr("Genre", "Tür"),
            Facet::Series => self.tr("Series", "Seri"),
            Facet::Year => self.tr("Year", "Yıl"),
            Facet::Tag => self.tr("Steam tag", "Steam etiketi"),
            Facet::Category => self.tr("Category", "Kategori"),
        }
    }

    pub(super) fn library_group(&self) -> Option<Facet> {
        self.global
            .library_group
            .as_deref()
            .and_then(Facet::from_key)
    }

    /// The values of `f` a game has (a game can have several genres or tags).
    pub(super) fn facet_values(&self, f: Facet, id: &str) -> Vec<String> {
        let Some(entry) = self.library.get(id) else {
            return Vec::new();
        };
        let info = match self.info.known.get(id) {
            Some(Ok(Some(i))) => Some(i),
            _ => None,
        };
        match f {
            Facet::Status => entry
                .status
                .map(|s| vec![self.status_name(s).to_string()])
                .unwrap_or_default(),
            Facet::Store => {
                let s = self.source_label(id);
                if s.is_empty() { Vec::new() } else { vec![s] }
            }
            Facet::Genre => info.map(|i| i.genres.clone()).unwrap_or_default(),
            Facet::Series => info.map(|i| i.series.clone()).unwrap_or_default(),
            Facet::Year => info
                .and_then(|i| i.year())
                .map(|y| vec![y.to_string()])
                .unwrap_or_default(),
            Facet::Tag => info.map(|i| i.tags.clone()).unwrap_or_default(),
            Facet::Category => entry.category.clone().into_iter().collect(),
        }
    }

    /// Whether a game passes the "only this group" filter.
    pub(super) fn only_matches(&self, id: &str) -> bool {
        match &self.filter.only {
            Some((f, v)) => self.facet_values(*f, id).contains(v),
            None => true,
        }
    }

    /// The shown games in groups (title, rows); one untitled group when not grouped. A game
    /// is under its first value; games with none come last.
    pub(super) fn library_groups(&self) -> Vec<(Option<String>, Vec<usize>)> {
        let Some(f) = self.library_group() else {
            return vec![(None, self.visible.clone())];
        };
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        let mut none = Vec::new();
        for &ix in &self.visible {
            let Some(game) = self.games.get(ix) else {
                continue;
            };
            match self.facet_values(f, &game.id.id).into_iter().next() {
                Some(v) => match groups.iter_mut().find(|(t, _)| *t == v) {
                    Some((_, rows)) => rows.push(ix),
                    None => groups.push((v, vec![ix])),
                },
                None => none.push(ix),
            }
        }
        match f {
            Facet::Year => groups.sort_by(|a, b| b.0.cmp(&a.0)),
            _ => groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0))),
        }
        let mut out: Vec<(Option<String>, Vec<usize>)> =
            groups.into_iter().map(|(t, r)| (Some(t), r)).collect();
        if !none.is_empty() {
            let other = match f {
                Facet::Status => self.tr("No status", "Durumu yok"),
                Facet::Category => self.tr("No category", "Kategorisi yok"),
                _ => self.tr("Unknown", "Bilinmiyor"),
            };
            out.push((Some(other.to_string()), none));
        }
        out
    }

    /// A group's title over its games: the name, how many, and "only these".
    pub(super) fn group_header(
        &self,
        ix: usize,
        title: &str,
        n: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let f = self.library_group();
        let value = title.to_string();
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(title.to_string()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(pal::faint())
                    .child(n.to_string()),
            )
            .when_some(f.filter(|_| self.filter.only.is_none()), |d, f| {
                d.child(
                    div()
                        .id(("group-only", ix))
                        .text_xs()
                        .text_color(pal::muted())
                        .cursor_pointer()
                        .hover(|s| s.text_color(pal::accent_light()))
                        .child(self.tr("Only these ›", "Yalnızca bunlar ›"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.filter.only = Some((f, value.clone()));
                            this.apply_filter();
                            cx.notify();
                        })),
                )
            })
    }

    fn save_global(&mut self) {
        let r = self.global.save();
        self.report(r);
    }

    /// The view in use, as a saved view named `name`.
    fn current_view(&self, name: &str) -> arkor_core::profile::SavedView {
        arkor_core::profile::SavedView {
            name: name.to_string(),
            shelf: self.filter.smart.map(|s| s.key().to_string()),
            genre: self.filter.genre.clone(),
            status: self.filter.status,
            category: self.filter.category.clone(),
            only: self
                .filter
                .only
                .clone()
                .map(|(f, v)| (f.key().to_string(), v)),
            group: self.global.library_group.clone(),
            sort: (self.filter.sort != Sort::Name).then(|| self.filter.sort.key().to_string()),
            search: self.search_text.clone(),
        }
    }

    fn open_view(&mut self, v: &arkor_core::profile::SavedView, cx: &mut Context<Self>) {
        self.filter.smart = v.shelf.as_deref().and_then(Smart::from_key);
        self.filter.genre = v.genre.clone();
        self.filter.status = v.status;
        self.filter.category = v.category.clone();
        self.filter.only = v
            .only
            .as_ref()
            .and_then(|(f, val)| Facet::from_key(f).map(|f| (f, val.clone())));
        self.filter.sort = Sort::from_key(v.sort.as_deref().unwrap_or("name"));
        self.global.library_group = v.group.clone();
        self.save_global();
        self.set_search(v.search.clone(), cx);
        self.apply_filter();
        cx.notify();
    }

    fn any_filter(&self) -> bool {
        let f = &self.filter;
        f.smart.is_some()
            || f.genre.is_some()
            || f.status.is_some()
            || f.category.is_some()
            || f.only.is_some()
            || self.global.library_group.is_some()
            || !self.search_text.trim().is_empty()
    }

    /// Saved views, "group by", "only these" and "save this view".
    pub(super) fn render_views_bar(&mut self, cx: &mut Context<Self>) -> Div {
        let mut row = div().flex().flex_wrap().items_center().gap_1p5();
        // Saved views first.
        let views = self.global.views.clone();
        if !views.is_empty() {
            row = row.child(style::icon("bookmark", 14., pal::faint()));
        }
        for (ix, v) in views.into_iter().enumerate() {
            let on = self.current_view(&v.name) == v;
            let open = v.clone();
            row = row.child(
                widgets::chip(("view", ix), v.name.clone(), on)
                    .on_click(cx.listener(move |this, _, _, cx| this.open_view(&open, cx))),
            );
            if on {
                let name = v.name.clone();
                row = row.child(
                    widgets::chip(("view-del", ix), "✕", false).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.global.views.retain(|x| x.name != name);
                            this.save_global();
                            cx.notify();
                        },
                    )),
                );
            }
        }
        // Group by.
        row = row.child(div().w(px(8.))).child(
            div()
                .text_xs()
                .text_color(pal::faint())
                .child(self.tr("Group by", "Grupla:")),
        );
        let current = self.library_group();
        for (ix, f) in std::iter::once(None)
            .chain(Facet::ALL.into_iter().map(Some))
            .enumerate()
        {
            let label = match f {
                None => self.tr("None", "Yok"),
                Some(f) => self.facet_name(f),
            };
            row = row.child(
                widgets::chip(("group-by", ix), label, current == f).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.global.library_group = f.map(|f| f.key().to_string());
                        this.save_global();
                        cx.notify();
                    },
                )),
            );
        }
        // "Only these".
        if let Some((f, v)) = self.filter.only.clone() {
            row = row.child(
                widgets::chip(
                    "only-clear",
                    format!("{}: {v}  ✕", self.facet_name(f)),
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.filter.only = None;
                    this.apply_filter();
                    cx.notify();
                })),
            );
        }
        // Save this view.
        if self.any_filter() {
            if self.views_naming {
                let input = self.submit_field(
                    "view-name",
                    self.tr("Name of the view, then Enter", "Görünümün adı, sonra Enter"),
                    cx,
                    |this, text, cx| {
                        let name = text.trim().to_string();
                        if !name.is_empty() {
                            let v = this.current_view(&name);
                            this.global.views.retain(|x| x.name != name);
                            this.global.views.push(v);
                            this.save_global();
                        }
                        this.views_naming = false;
                        this.fields.remove("view-name");
                        cx.notify();
                    },
                );
                row = row.child(div().w(px(240.)).child(input));
            } else {
                row = row.child(
                    widgets::chip(
                        "view-save",
                        self.tr("Save this view", "Bu görünümü kaydet"),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.views_naming = true;
                        cx.notify();
                    })),
                );
            }
        }
        row
    }
}
