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
use super::console::{Act, FolderFor, Item, Kind, Overlay, Row, Screen, Value};
use super::console_settings::AppCat;

/// What the administrator password is asked for.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum NeedsPassword {
    /// Packages from the repositories.
    Packages(Vec<String>),
    /// A disk to mount, and what comes then.
    Mount(Disk, AfterDisk),
    /// A disk to take out of use, by where it is mounted.
    Unmount(String),
    /// A wireless network's password (no administrator's).
    Wifi(String),
}

/// What a disk was mounted for.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum AfterDisk {
    /// The folder choice goes on there.
    Folder(FolderFor),
    /// A game on it starts.
    Play(String),
    Stay,
}

/// Release notes as plain text: without Markdown's signs and the part about installing.
pub(super) fn plain_notes(notes: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut skip = false;
    let mut code = false;
    for line in notes.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            code = !code;
            continue;
        }
        if let Some(title) = t.strip_prefix('#') {
            let title = title.trim_start_matches('#').trim();
            skip = title.to_lowercase().starts_with("install");
            if !skip {
                out.push(String::new());
                out.push(title.to_uppercase());
            }
            continue;
        }
        if skip || code || t.starts_with("> [!") {
            continue;
        }
        let t = t.strip_prefix("> ").unwrap_or(t);
        let t = match t.strip_prefix("- ") {
            Some(item) => format!("•  {item}"),
            None => t.to_string(),
        };
        out.push(links(&t).replace("**", "").replace('`', ""));
    }
    let text = out.join("\n");
    let mut text = text.trim().to_string();
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text
}

/// `[text](address)` as just the text.
fn links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some((before, after)) = rest.split_once('[') {
        let Some((text, tail)) = after.split_once("](") else {
            break;
        };
        let Some((_, tail)) = tail.split_once(')') else {
            break;
        };
        out.push_str(before);
        out.push_str(text);
        rest = tail;
    }
    out + rest
}

impl Library {
    fn line(&self, act: Act, icon: &'static str, label: &str, value: Value) -> Row {
        Row::new(Kind::Line, vec![Item::line(act, icon, label, value)])
    }

    /// How Arkor's own update stands, in a few words.
    pub(super) fn console_arkor_state(&self) -> String {
        let u = &self.app_update;
        if u.installing {
            return match self.arkor_progress() {
                Some(f) => self.percent(f),
                None => self.tr("Updating…", "Güncelleniyor…").into(),
            };
        }
        match &u.available {
            Some(r) => format!("{} {}", r.version, self.tr("is out", "çıktı")),
            None => arkor_launch::self_update::current_version().to_string(),
        }
    }

    /// How much of the update is downloaded.
    fn arkor_progress(&self) -> Option<f32> {
        arkor_launch::progress::list()
            .iter()
            .find(|e| e.key == "arkor-update")
            .and_then(|e| e.fraction())
    }

    /// Arkor itself: its version, a look for a newer one and its installation.
    pub(super) fn console_arkor_row(&self) -> Row {
        let u = &self.app_update;
        let (act, icon, value) = if u.installing {
            (Act::Nothing, "loader-circle", self.console_arkor_state())
        } else if u.checking {
            (
                Act::Nothing,
                "loader-circle",
                self.tr("Looking…", "Bakılıyor…").to_string(),
            )
        } else if let Some(r) = &u.available {
            (
                Act::AskArkor,
                "download",
                format!("{} {}", self.tr("Update to", "Güncelle:"), r.version),
            )
        } else {
            (
                Act::CheckArkor,
                "sparkles",
                self.tr("Look for an update", "Güncelleme denetle")
                    .to_string(),
            )
        };
        let version = arkor_launch::self_update::current_version();
        let mut row = self.line(act, icon, &format!("Arkor {version}"), Value::Text(value));
        row.items[0].progress = u.installing.then(|| self.arkor_progress()).flatten();
        row.items[0].sub = match (&u.message, &u.available) {
            (Some(m), _) => m.clone(),
            (None, Some(r)) => format!(
                "{} {} · {}",
                self.tr("New version:", "Yeni sürüm:"),
                r.version,
                arkor_launch::progress::human_bytes(r.size)
            ),
            (None, None) => String::new(),
        };
        row
    }

    /// The section of Arkor's updates.
    pub(super) fn console_arkor_rows(&self) -> Vec<Row> {
        let mut rows = vec![self.console_arkor_row()];
        let u = &self.app_update;
        if u.available.is_some() && !u.installing {
            rows.push(self.line(
                Act::ArkorNotes,
                "scroll-text",
                self.tr("What is new", "Yenilikler"),
                Value::None,
            ));
            rows.push(self.line(
                Act::CheckArkor,
                "refresh-cw",
                self.tr("Look again", "Yeniden denetle"),
                Value::None,
            ));
        }
        rows
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
        rows.push(self.console_arkor_row());
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

    /// The programs games use (Tools).
    pub(super) fn console_program_rows(&self) -> Vec<Row> {
        let Some((_, tools)) = &self.system.info else {
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
                    if arkor_launch::tools::has_aur() {
                        self.tr("AUR · on the desktop", "AUR · masaüstünden")
                    } else {
                        self.tr("not packaged here", "bu dağıtımda paketi yok")
                    }
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
        rows
    }

    /// The disks and this computer (System).
    pub(super) fn console_system_rows(&self) -> Vec<Row> {
        let mut rows = self.console_disk_rows();
        let Some((info, _)) = &self.system.info else {
            rows.push(self.line(
                Act::Nothing,
                "loader-circle",
                self.tr("Looking…", "Bakılıyor…"),
                Value::None,
            ));
            return rows;
        };
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

    /// How a disk is called.
    fn disk_name(&self, d: &Disk) -> String {
        if d.label.is_empty() {
            format!("Disk {}", d.size)
        } else {
            d.label.clone()
        }
    }

    /// "120 GB free of 465 GB".
    fn room(&self, (all, free): (u64, u64)) -> String {
        let (all, free) = (
            arkor_launch::progress::human_bytes(all),
            arkor_launch::progress::human_bytes(free),
        );
        match self.global.language {
            arkor_core::Language::Tr => format!("{free} boş / {all}"),
            arkor_core::Language::En => format!("{free} free of {all}"),
        }
    }

    fn console_disk_rows(&self) -> Vec<Row> {
        let share = |(all, free): (u64, u64)| {
            (all > 0).then(|| (all.saturating_sub(free)) as f32 / all as f32)
        };
        let mut items = Vec::new();
        if let Some(space) = self.console.system_space {
            let mut i = Item::line(
                Act::Nothing,
                "hard-drive",
                self.tr("System disk", "Sistem diski"),
                Value::Text(self.room(space)),
            );
            i.progress = share(space);
            i.sub = "/".into();
            items.push(i);
        }
        for d in &self.console.drives {
            let path = d.disk.path.clone();
            let (act, icon, value) = match (&d.mount, d.space) {
                (Some(_), space) => (
                    Act::DiskMenu(path),
                    "hard-drive",
                    space.map(|s| self.room(s)).unwrap_or_default(),
                ),
                (None, _) if self.console.mounting => (
                    Act::Nothing,
                    "loader-circle",
                    self.tr("Mounting…", "Bağlanıyor…").to_string(),
                ),
                (None, _) => (
                    Act::Mount(path),
                    "hard-drive-download",
                    self.tr("Not mounted · mount", "Bağlı değil · bağla")
                        .to_string(),
                ),
            };
            let mut i = Item::line(act, icon, self.disk_name(&d.disk), Value::Text(value));
            i.sub = match &d.mount {
                Some(m) => format!("{} · {m}", d.disk.fstype),
                None => format!("{} · {}", d.disk.fstype, d.disk.path),
            };
            i.progress = d.space.and_then(share);
            items.push(i);
        }
        let mut rows: Vec<Row> = items
            .into_iter()
            .map(|i| Row::new(Kind::Line, vec![i]))
            .collect();
        if let Some(first) = rows.first_mut() {
            first.title = Some(self.tr("Disks", "Diskler").into());
        }
        rows
    }

    /// What can be done with a mounted disk.
    pub(super) fn console_disk_menu(&mut self, path: &str) {
        let Some(d) = self.console.drives.iter().find(|d| d.disk.path == path) else {
            return;
        };
        let Some(mount) = d.mount.clone() else {
            return;
        };
        let title = self.disk_name(&d.disk);
        let options = vec![
            (
                self.tr("Choose a games folder on it", "Üzerinde oyun klasörü seç")
                    .to_string(),
                Act::FolderAt(PathBuf::from(&mount)),
                false,
            ),
            (
                self.tr("Unmount", "Bağlantısını kes").to_string(),
                Act::Unmount(path.to_string()),
                false,
            ),
            (self.tr("Close", "Kapat").to_string(), Act::Close, false),
        ];
        self.console_overlay(Overlay::Pick(title, options));
    }

    /// Takes a disk out of use: without a password when allowed, else asks.
    pub(super) fn console_unmount(&mut self, path: &str, cx: &mut Context<Self>) {
        self.console.overlay = None;
        let Some(d) = self.console.drives.iter().find(|d| d.disk.path == path) else {
            return;
        };
        let (disk, Some(point)) = (d.disk.clone(), d.mount.clone()) else {
            return;
        };
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = bg.spawn(async move { admin::unmount(&disk) }).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.console_unmounted(Ok(())),
                    Err(MountError::NeedsPassword) => {
                        this.console_password(NeedsPassword::Unmount(point))
                    }
                    Err(MountError::Failed(e)) => this.console_unmounted(Err(anyhow::anyhow!(e))),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn console_unmounted(&mut self, result: anyhow::Result<()>) {
        self.console.drives_at = None;
        self.status = Some(match result {
            Ok(()) => self.tr("Unmounted.", "Bağlantısı kesildi.").into(),
            Err(e) if e.to_string().contains("busy") => self
                .tr(
                    "Something on that disk is in use.",
                    "O diskteki bir şey kullanılıyor.",
                )
                .into(),
            Err(e) => self.password_error(&e),
        });
    }

    /// The disk a game is on, when it is not where the game expects it.
    pub(super) fn game_disk(&self, id: &arkor_core::GameId) -> Option<admin::Drive> {
        let g = self.library.get(&id.id)?;
        let paths = [
            g.exe.clone(),
            g.effective_prefix(&self.global.prefixes_dir()),
        ];
        // Looked at only when something is missing (it takes a moment).
        let missing = paths
            .iter()
            .any(|p| admin::disk_folder(p).is_some() && !p.exists());
        if cfg!(test) || !missing {
            return None;
        }
        admin::missing_disk(&paths, &admin::drives())
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
            NeedsPassword::Mount(disk, then) => {
                self.console.mounting = true;
                cx.spawn(async move |this, cx| {
                    let result = bg
                        .spawn(async move { admin::mount_with_password(&disk, &password) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.console.mounting = false;
                        this.console_mounted(result, then, cx);
                        cx.notify();
                    });
                })
                .detach();
            }
            NeedsPassword::Wifi(ssid) => self.wifi_with_password(ssid, password, cx),
            NeedsPassword::Unmount(point) => {
                cx.spawn(async move |this, cx| {
                    let result = bg
                        .spawn(async move { admin::unmount_with_password(&point, &password) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.console_unmounted(result);
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

    /// Mounts one of the listed disks.
    pub(super) fn console_mount(&mut self, path: &str, then: AfterDisk, cx: &mut Context<Self>) {
        let drive = self.console.drives.iter().find(|d| d.disk.path == path);
        if let Some(d) = drive.cloned() {
            self.console_mount_drive(d, then, cx);
        }
    }

    /// Mounts a disk: without a password when allowed, else asks for it.
    pub(super) fn console_mount_drive(
        &mut self,
        drive: admin::Drive,
        then: AfterDisk,
        cx: &mut Context<Self>,
    ) {
        // Mounted elsewhere already: it goes to its own place too.
        if drive.mount.is_some() {
            return self.console_password(NeedsPassword::Mount(drive.disk, then));
        }
        if std::mem::replace(&mut self.console.mounting, true) {
            return;
        }
        let disk = drive.disk;
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let d = disk.clone();
            let result = bg.spawn(async move { admin::mount(&d) }).await;
            let _ = this.update(cx, |this, cx| {
                this.console.mounting = false;
                match result {
                    Ok(point) => this.console_mounted(Ok(point), then, cx),
                    Err(MountError::NeedsPassword) => {
                        this.console_password(NeedsPassword::Mount(disk, then))
                    }
                    Err(MountError::Failed(e)) => {
                        this.console_mounted(Err(anyhow::anyhow!(e)), then, cx)
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn console_mounted(
        &mut self,
        result: anyhow::Result<String>,
        then: AfterDisk,
        cx: &mut Context<Self>,
    ) {
        self.console.drives_at = None;
        // The list of folders may have been looked at while the disk was away.
        *self.console.folders.borrow_mut() = None;
        match (result, then) {
            (Ok(point), AfterDisk::Folder(what)) if !point.is_empty() => {
                self.console_overlay(Overlay::Folder(what, PathBuf::from(point)));
            }
            (Ok(_), AfterDisk::Play(id)) => self.console_play(&id, cx),
            (Ok(point), _) => {
                self.status = Some(format!("{}: {point}", self.tr("Mounted", "Bağlandı")).into());
            }
            (Err(e), then) => {
                self.cue(super::console_sound::Cue::Error);
                self.status = Some(self.password_error(&e));
                self.console_disk_not_mounted(&then);
            }
        }
    }

    /// The disk stays away: back to where it was asked for.
    pub(super) fn console_disk_not_mounted(&mut self, then: &AfterDisk) {
        if let AfterDisk::Folder(what) = then {
            let home = dirs::home_dir().unwrap_or_default();
            self.console_overlay(Overlay::Folder(*what, home));
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
            let look = || (admin::drives(), admin::space(std::path::Path::new("/")));
            let (found, system) = bg.spawn(async move { look() }).await;
            let _ = this.update(cx, |this, cx| {
                if this.console.drives != found || this.console.system_space != system {
                    this.console.drives = found;
                    this.console.system_space = system;
                    cx.notify();
                }
            });
            // A disk plugged in while the choice is open shows up by itself.
            bg.timer(Duration::from_secs(5)).await;
            let _ = this.update(cx, |this, cx| {
                if this.console_shows_disks() {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn console_shows_disks(&self) -> bool {
        self.console.on
            && (matches!(self.console.overlay, Some(Overlay::Folder(..)))
                || self.console.screen() == Screen::SettingsCat(AppCat::System))
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
            let name = self.disk_name(&d.disk);
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
