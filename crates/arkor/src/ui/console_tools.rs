//! Console mode's tools: what Arkor downloaded (with updates and removal), Proton builds,
//! the system's programs, and what needs the administrator password (asked on the
//! console's own keyboard, never in a desktop window).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use arkor_launch::admin::{self, Disk, MountError};
use arkor_launch::components::Latest;
use arkor_launch::downloads;
use arkor_launch::proton_dl::Variant;
use gpui::Context;

use super::Library;
use super::console::{Act, FolderFor, Item, Kind, Overlay, Row, Value};

/// What the administrator password is asked for.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum NeedsPassword {
    /// Packages from the repositories.
    Packages(Vec<String>),
    /// A disk to mount, and the folder choice to go back to.
    Mount(Disk, FolderFor),
}

impl Library {
    fn line(&self, act: Act, icon: &'static str, label: &str, value: Value) -> Row {
        Row::new(Kind::Line, vec![Item::line(act, icon, label, value)])
    }

    /// The Downloads screen's part below the queue: updates, then what is installed.
    pub(super) fn console_tool_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let checking = self.downloads_checking();
        let mut pills = vec![Item::pill(
            if checking {
                Act::Nothing
            } else {
                Act::CheckUpdates
            },
            "refresh-cw",
            if checking {
                self.tr("Looking for updates…", "Güncellemeler aranıyor…")
            } else {
                self.tr("Look for updates", "Güncellemeleri denetle")
            },
            false,
        )];
        let waiting: Vec<_> = self
            .downloads
            .updates
            .iter()
            .filter(|u| !downloads::pending(&u.key))
            .collect();
        if !waiting.is_empty() {
            pills.insert(
                0,
                Item::main(
                    Act::UpdateAll,
                    "download",
                    self.tr("Update all", "Tümünü güncelle"),
                ),
            );
        }
        let mut row = Row::new(Kind::Pills, pills);
        row.title = Some(self.tr("Updates", "Güncellemeler").into());
        rows.push(row);
        for u in waiting {
            let value = if u.latest.is_empty() {
                self.tr("Update", "Güncelle").to_string()
            } else {
                format!("{} → {}", u.installed, u.latest)
            };
            rows.push(self.line(
                Act::Queue(u.key.clone()),
                "circle-arrow-up",
                &u.label,
                Value::Text(value),
            ));
        }
        for (n, t) in self.installed_things().into_iter().enumerate() {
            let mut row = self.line(Act::ToolMenu(t.id.clone()), t.icon, &t.title, Value::None);
            row.items[0].sub = t.detail.clone();
            if n == 0 {
                row.title = Some(self.tr("Installed", "Kurulu").into());
            }
            rows.push(row);
        }
        rows
    }

    /// What can be done with an installed thing: a list of its actions.
    pub(super) fn console_tool_menu(&mut self, id: &str) {
        let Some(t) = self.installed_things().into_iter().find(|t| t.id == id) else {
            return;
        };
        let mut options = Vec::new();
        if let Some(key) = &t.update {
            options.push((
                self.tr("Look for an update", "Güncelleme denetle")
                    .to_string(),
                Act::Queue(key.clone()),
                false,
            ));
        }
        if t.removable {
            options.push((
                self.tr("Remove", "Kaldır").to_string(),
                Act::AskRemoveTool(t.id.clone()),
                false,
            ));
        }
        if options.is_empty() {
            self.status = Some(
                self.tr(
                    "Installed by the system: remove it with the package manager.",
                    "Sistem kurmuş: paket yöneticisiyle kaldırılır.",
                )
                .into(),
            );
            return;
        }
        self.console_overlay(Overlay::Pick(t.title, options));
    }

    pub(super) fn console_ask_remove_tool(&mut self, id: &str) {
        let Some(t) = self.installed_things().into_iter().find(|t| t.id == id) else {
            return;
        };
        let text = format!("{}: {}", t.title, self.tr("remove it?", "kaldırılsın mı?"));
        let yes = self.tr("Remove", "Kaldır").to_string();
        self.console_overlay(Overlay::Ask(text, yes, Act::RemoveTool(id.to_string())));
    }

    // ---- Proton ----

    /// The Proton screen: the build family, its "Latest" build, its releases, and what is
    /// installed.
    pub(super) fn console_proton_rows(&self) -> Vec<Row> {
        let variant = self.protons.variant;
        let pills = Variant::ALL
            .into_iter()
            .enumerate()
            .map(|(i, v)| Item::pill(Act::ProtonVariant(i), "wine", v.label(), v == variant))
            .collect();
        let mut rows = vec![Row::new(Kind::Pills, pills)];
        let installed = self.installed_things();
        let latest = Latest::ALL
            .into_iter()
            .find(|l| l.variant() == variant)
            .unwrap_or(Latest::Ge);
        let busy = |key: &str| downloads::pending(key);
        let key = format!("latest:{}", latest.dir_name());
        let (act, value) = match latest.installed_version() {
            Some(v) => (
                installed
                    .iter()
                    .find(|t| t.proton && t.title == latest.dir_name())
                    .map_or(Act::Nothing, |t| Act::ToolMenu(t.id.clone())),
                format!("{} {v}", self.tr("installed", "kurulu")),
            ),
            None if busy(&key) => (Act::Nothing, self.tr("In queue", "Sırada").to_string()),
            None => (Act::Queue(key), self.tr("Install", "Kur").to_string()),
        };
        let mut latest_row = self.line(act, "sparkles", latest.dir_name(), Value::Text(value));
        latest_row.items[0].sub = self
            .tr(
                "Always the newest release (recommended)",
                "Hep en yeni sürüm (önerilen)",
            )
            .into();
        rows.push(latest_row);
        let have: Vec<String> = installed
            .iter()
            .filter(|t| t.proton)
            .map(|t| t.title.to_lowercase())
            .collect();
        match self.protons.releases.get(variant.label()) {
            None => rows.push(self.line(
                Act::Nothing,
                "loader-circle",
                self.tr("Loading releases…", "Sürümler yükleniyor…"),
                Value::None,
            )),
            Some(Err(e)) => rows.push(self.line(Act::Nothing, "triangle-alert", e, Value::None)),
            Some(Ok(list)) => {
                for (n, r) in list.iter().take(25).enumerate() {
                    let tag = r.tag.to_lowercase();
                    let present = have.iter().any(|h| *h == tag || h.ends_with(&tag));
                    let value = if self.protons.installing.as_deref() == Some(r.tag.as_str()) {
                        self.tr("Installing…", "Kuruluyor…").to_string()
                    } else if present {
                        self.tr("Installed", "Kurulu").to_string()
                    } else {
                        arkor_stores::prefix::human_size(r.size)
                    };
                    let act = if present || self.protons.installing.is_some() {
                        Act::Nothing
                    } else {
                        Act::InstallRelease(r.tag.clone())
                    };
                    let mut row = self.line(act, "download", &r.tag, Value::Text(value));
                    if n == 0 {
                        row.title = Some(self.tr("A specific version", "Belirli bir sürüm").into());
                    }
                    rows.push(row);
                }
            }
        }
        for (n, t) in installed.into_iter().filter(|t| t.proton).enumerate() {
            let mut row = self.line(Act::ToolMenu(t.id.clone()), "wine", &t.title, Value::None);
            row.items[0].sub = t.detail;
            if n == 0 {
                row.title = Some(self.tr("Installed", "Kurulu").into());
            }
            rows.push(row);
        }
        rows
    }

    pub(super) fn console_install_release(&mut self, tag: &str, cx: &mut Context<Self>) {
        let release = self
            .protons
            .releases
            .get(self.protons.variant.label())
            .and_then(|r| r.as_ref().ok())
            .and_then(|l| l.iter().find(|r| r.tag == tag).cloned());
        if let Some(r) = release {
            self.install_proton(r, cx);
        }
    }

    // ---- The system's programs ----

    pub(super) fn console_system_rows(&self) -> Vec<Row> {
        let Some((info, tools)) = &self.system.info else {
            return vec![self.line(
                Act::Nothing,
                "loader-circle",
                self.tr("Looking…", "Bakılıyor…"),
                Value::None,
            )];
        };
        let mut rows = Vec::new();
        // AUR packages want their build script read first: that needs the desktop.
        let aur: &[&str] = self.system.split.as_ref().map_or(&[], |(_, other)| other);
        let missing: Vec<&str> = tools
            .iter()
            .filter(|t| t.found.is_none() && !aur.contains(&t.package))
            .map(|t| t.package)
            .collect();
        if self.system.installing {
            rows.push(self.line(
                Act::Nothing,
                "loader-circle",
                self.tr("Installing…", "Kuruluyor…"),
                Value::None,
            ));
        } else if !missing.is_empty() {
            rows.push(Row::new(
                Kind::Pills,
                vec![Item::main(
                    Act::InstallTools(missing.iter().map(|p| p.to_string()).collect()),
                    "download",
                    format!(
                        "{} ({})",
                        self.tr("Install what is missing", "Eksikleri kur"),
                        missing.len()
                    ),
                )],
            ));
        }
        for (n, t) in tools.iter().enumerate() {
            let (act, value) = match &t.found {
                Some(v) => (Act::Nothing, v.clone()),
                None if self.system.installing => (Act::Nothing, "—".into()),
                None if aur.contains(&t.package) => (
                    Act::Nothing,
                    self.tr("AUR · on the desktop", "AUR · masaüstünden")
                        .to_string(),
                ),
                None => (
                    Act::InstallTools(vec![t.package.to_string()]),
                    self.tr("Install", "Kur").to_string(),
                ),
            };
            let icon = if t.found.is_some() {
                "circle-check"
            } else {
                "circle-x"
            };
            let mut row = self.line(act, icon, t.name, Value::Text(value));
            if n == 0 {
                row.title = Some(self.tr("Programs", "Programlar").into());
            }
            rows.push(row);
        }
        for (n, (key, value)) in info.iter().enumerate() {
            let (icon, label) = super::system_page::info_look(key, self);
            let mut row = self.line(
                Act::Nothing,
                icon,
                label,
                Value::Text(super::system_page::tidy(key, value)),
            );
            if n == 0 {
                row.title = Some(self.tr("This computer", "Bu bilgisayar").into());
            }
            rows.push(row);
        }
        rows
    }

    // ---- The password, and what it is for ----

    /// Asks for the administrator password on the console keyboard.
    pub(super) fn console_password(&mut self, why: NeedsPassword) {
        self.console.secret = Some((why, String::new()));
        self.console_overlay(Overlay::Keyboard);
    }

    pub(super) fn console_install_tools(&mut self, packages: Vec<String>) {
        self.console_password(NeedsPassword::Packages(packages));
    }

    /// The password was typed: does what it was asked for.
    pub(super) fn console_with_password(
        &mut self,
        why: NeedsPassword,
        password: String,
        cx: &mut Context<Self>,
    ) {
        let bg = cx.background_executor().clone();
        match why {
            NeedsPassword::Packages(packages) => {
                self.system.installing = true;
                self.watch_progress(cx);
                cx.spawn(async move |this, cx| {
                    let label = packages.join(", ");
                    let result = bg
                        .spawn(async move {
                            let _job = arkor_launch::progress::job("system-tools", &label);
                            let refs: Vec<&str> = packages.iter().map(String::as_str).collect();
                            admin::install_packages(&refs, &password)
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.system.installing = false;
                        this.status = Some(match result {
                            Ok(()) => this.tr("Installed.", "Kuruldu.").into(),
                            Err(e) => this.password_error(&e),
                        });
                        this.scan_system(cx);
                        this.rescan_tools();
                    });
                })
                .detach();
            }
            NeedsPassword::Mount(disk, what) => {
                cx.spawn(async move |this, cx| {
                    let result = bg
                        .spawn(async move { admin::mount_with_password(&disk, &password) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.console_mounted(result, what);
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    fn password_error(&self, e: &anyhow::Error) -> gpui::SharedString {
        if e.to_string() == "wrong password" {
            self.tr("Wrong password.", "Şifre yanlış.").into()
        } else {
            format!("{e:#}").into()
        }
    }

    /// Mounts a disk from the folder choice: without a password when allowed, else asks.
    pub(super) fn console_mount(&mut self, path: &str, what: FolderFor, cx: &mut Context<Self>) {
        let Some(disk) = self
            .console
            .drives
            .iter()
            .find(|d| d.disk.path == path)
            .map(|d| d.disk.clone())
        else {
            return;
        };
        if std::mem::replace(&mut self.console.mounting, true) {
            return;
        }
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let d = disk.clone();
            let result = bg.spawn(async move { admin::mount(&d) }).await;
            let _ = this.update(cx, |this, cx| {
                this.console.mounting = false;
                match result {
                    Ok(point) => this.console_mounted(Ok(point), what),
                    Err(MountError::NeedsPassword) => {
                        this.console_password(NeedsPassword::Mount(disk, what))
                    }
                    Err(MountError::Failed(e)) => this.status = Some(e.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn console_mounted(&mut self, result: anyhow::Result<String>, what: FolderFor) {
        self.console.drives_at = None;
        match result {
            Ok(point) if !point.is_empty() => {
                self.console_overlay(Overlay::Folder(what, PathBuf::from(point)));
            }
            Ok(_) => {}
            Err(e) => self.status = Some(self.password_error(&e)),
        }
    }

    /// Looks at the disks again when the last look is a few seconds old.
    pub(super) fn console_look_drives(&mut self, cx: &mut Context<Self>) {
        let fresh = |t: Instant| t.elapsed() < Duration::from_secs(4);
        if cfg!(test) || self.console.drives_at.is_some_and(fresh) {
            return;
        }
        self.console.drives_at = Some(Instant::now());
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let found = bg.spawn(async { admin::drives() }).await;
            let _ = this.update(cx, |this, cx| {
                if this.console.drives != found {
                    this.console.drives = found;
                    cx.notify();
                }
            });
            // A disk plugged in while the choice is open shows up by itself.
            bg.timer(Duration::from_secs(5)).await;
            let _ = this.update(cx, |this, cx| {
                if matches!(this.console.overlay, Some(Overlay::Folder(..))) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The places to jump to: home, then every disk (one that is not mounted yet gets
    /// mounted). Three beside each other.
    pub(super) fn drive_rows(&self) -> Vec<Row> {
        let mut items = Vec::new();
        if let Some(h) = dirs::home_dir() {
            let name = self.tr("Home", "Ev");
            let mut i = Item::line(Act::FolderOpen(h.clone()), "house", name, Value::None);
            i.sub = h.display().to_string();
            items.push(i);
        }
        for d in &self.console.drives {
            let name = if d.disk.label.is_empty() {
                format!("{} {}", self.tr("Disk", "Disk"), d.disk.size)
            } else {
                d.disk.label.clone()
            };
            let (act, icon, sub) = match &d.mount {
                Some(m) => (
                    Act::FolderOpen(PathBuf::from(m)),
                    "hard-drive",
                    format!("{} · {}", d.disk.size, d.disk.fstype),
                ),
                None => (
                    Act::Mount(d.disk.path.clone()),
                    "hard-drive-download",
                    format!(
                        "{} · {}",
                        d.disk.fstype,
                        self.tr("not mounted", "bağlı değil")
                    ),
                ),
            };
            let mut i = Item::line(act, icon, name, Value::None);
            i.sub = sub;
            items.push(i);
        }
        items
            .chunks(3)
            .map(|c| Row::new(Kind::Pills, c.to_vec()))
            .collect()
    }
}
