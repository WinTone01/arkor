//! "Why did it not start?": what `arkor_launch::trouble` finds about a game, in words, and
//! its fixes on buttons. A screen in console mode, a card on the desktop's game page.

use std::collections::HashMap;

use arkor_core::{GameId, Profile};
use arkor_launch::trouble::{self, Cause, Ended, Facts, Fix, Problem, Trial};
use arkor_launch::{downloads, tools};
use gpui::prelude::FluentBuilder;
use gpui::{Context, Div, FontWeight, div, prelude::*, px};

use super::console::{Act, Item, Kind, Overlay, Row, Screen, Value};
use super::style::{self, pal};
use super::{Library, widgets};

#[derive(Default)]
pub(super) struct TroubleUi {
    /// The game that was looked at, and what was found.
    found: Option<(GameId, Vec<Problem>)>,
    looking: bool,
    /// The card looked at this game by itself already.
    tried: Option<GameId>,
    /// What is being done.
    busy: Option<Fix>,
    /// What was done, and the game's settings before the first change.
    done: Vec<Fix>,
    before: Option<(GameId, Profile)>,
    /// How each game ended last.
    ended: HashMap<String, Ended>,
    /// The next start of this game is one of these.
    pub(super) trial: Option<(GameId, Trial)>,
    /// This game is running as a trial.
    trying: Option<(GameId, Trial)>,
}

impl Library {
    /// The way the next start of `id` differs from its settings.
    pub(super) fn trouble_trial(&mut self, id: &GameId) -> Trial {
        match self.trouble.trial.take() {
            Some((of, trial)) if &of == id => {
                self.trouble.trying = Some((of, trial));
                trial
            }
            _ => {
                self.trouble.trying = None;
                Trial::default()
            }
        }
    }

    /// A game ended: kept for looking at it.
    pub(super) fn trouble_ended(&mut self, id: &GameId, ended: Ended, cx: &mut Context<Self>) {
        self.trouble.ended.insert(id.to_string(), ended);
        let trial = self.trouble.trying.take().filter(|(of, _)| of == id);
        let Some((_, trial)) = trial else {
            return;
        };
        if trial.plain && ended.secs > 60 {
            self.status = Some(
                self.tr(
                    "It ran plainly: one of the things Arkor puts around it is the reason. Switch them off one by one.",
                    "Sade haliyle çalıştı: sebep Arkor'un eklediklerinden biri. Tek tek kapatarak bulabilirsin.",
                )
                .into(),
            );
        }
        if trial.detail {
            self.trouble_load(id, cx);
            if self.console.on && self.console.screen() != Screen::Trouble(id.id.clone()) {
                self.console_open(Screen::Trouble(id.id.clone()), cx);
            }
        }
    }

    /// Looks at the game.
    pub(super) fn trouble_load(&mut self, id: &GameId, cx: &mut Context<Self>) {
        if self.trouble.looking {
            return;
        }
        if self.trouble.found.as_ref().is_some_and(|(of, _)| of != id) {
            self.trouble.found = None;
            self.trouble.done.clear();
        }
        self.trouble.looking = true;
        cx.notify();
        let ended = self.trouble.ended.get(&id.to_string()).copied();
        let bg = cx.background_executor().clone();
        let of = id.clone();
        cx.spawn(async move |this, cx| {
            let id = of.clone();
            let found = bg
                .spawn(async move { Facts::gather(&id).map(|f| trouble::problems(&f, ended)) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.trouble.looking = false;
                match found {
                    Ok(list) => this.trouble.found = Some((of, list)),
                    Err(e) => this.status = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn trouble_of(&self, id: &GameId) -> &[Problem] {
        match &self.trouble.found {
            Some((of, list)) if of == id => list,
            _ => &[],
        }
    }

    /// Does what a fix says.
    pub(super) fn trouble_fix(&mut self, fix: Fix, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        if self.trouble.busy.is_some() {
            return;
        }
        if self.running.contains_key(&id.to_string()) {
            self.status = Some(self.tr("Close the game first.", "Önce oyunu kapat.").into());
            return cx.notify();
        }
        let before = self.profile.clone();
        if trouble::change(&mut self.profile, &fix) {
            if self.trouble.before.as_ref().is_none_or(|(of, _)| of != &id) {
                self.trouble.before = Some((id.clone(), before));
            }
            self.save();
            self.trouble.done.push(fix);
            self.status = Some(
                self.tr("Done. Try it again.", "Yapıldı. Yeniden dene.")
                    .into(),
            );
            return cx.notify();
        }
        match fix {
            Fix::Detail | Fix::Plain => {
                let trial = Trial {
                    plain: fix == Fix::Plain,
                    detail: fix == Fix::Detail,
                };
                self.trouble.trial = Some((id, trial));
                self.trouble_again(cx);
            }
            Fix::Mount => self.trouble_again(cx),
            Fix::Repair(shop, app) => {
                self.shop_repair(shop, &app, cx);
                self.trouble.done.push(Fix::Repair(shop, app));
                self.watch_progress(cx);
                self.status = Some(
                    self.tr(
                        "Its files are being checked: see Downloads.",
                        "Dosyaları denetleniyor: İndirilenler'den izleyebilirsin.",
                    )
                    .into(),
                );
            }
            Fix::Update(shop, app) => {
                downloads::enqueue(&arkor_launch::shops::key(shop, &app));
                self.trouble.done.push(Fix::Update(shop, app));
                self.watch_progress(cx);
            }
            Fix::Login(shop) => self.shop_login(shop, cx),
            Fix::Packages(list) if self.console.on => {
                self.console_install_tools(list.iter().map(|p| p.to_string()).collect())
            }
            fix => self.trouble_do(id, fix, cx),
        }
        cx.notify();
    }

    /// A fix that takes a while.
    fn trouble_do(&mut self, id: GameId, fix: Fix, cx: &mut Context<Self>) {
        self.trouble.busy = Some(fix.clone());
        self.status = Some(format!("{}…", self.fix_text(&fix).0).into());
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let (of, what) = (id.clone(), fix.clone());
            let result = bg
                .spawn(async move {
                    match &what {
                        Fix::Packages(list) => tools::install_packages(list),
                        what => trouble::apply(&of, what),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.trouble.busy = None;
                this.status = Some(match result {
                    Ok(()) => {
                        this.trouble.done.push(fix);
                        this.tr("Done. Try it again.", "Yapıldı. Yeniden dene.")
                            .into()
                    }
                    Err(e) => format!("{e:#}").into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Starts the game (again).
    fn trouble_again(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        if self.console.on {
            self.console_play(&id.to_string(), cx);
        } else {
            self.play(cx);
        }
    }

    /// The settings as they were before the fixes.
    pub(super) fn trouble_undo(&mut self, cx: &mut Context<Self>) {
        let Some((id, before)) = self.trouble.before.take() else {
            return;
        };
        if self.selected_id().as_ref() != Some(&id) {
            return;
        }
        self.profile = before;
        self.save();
        self.trouble.done.clear();
        self.status = Some(
            self.tr(
                "The game's settings are as they were.",
                "Oyunun ayarları eski haline döndü.",
            )
            .into(),
        );
        cx.notify();
    }

    fn trouble_can_undo(&self) -> bool {
        self.trouble
            .before
            .as_ref()
            .is_some_and(|(id, _)| self.selected_id().as_ref() == Some(id))
    }

    // ---- In words ----

    /// A cause: what it is, and what that means.
    fn cause_text(&self, c: &Cause) -> (String, String) {
        let t = |en: &str, tr: &str| self.tr_owned(en, tr);
        match c {
            Cause::DriveGone(d) => (
                format!("{}: {d}", t("Its disk is not mounted", "Diski bağlı değil")),
                t(
                    "The game or its prefix is on it.",
                    "Oyun ya da prefix'i onun üzerinde.",
                ),
            ),
            Cause::FilesGone => (
                t("The game's files are missing", "Oyunun dosyaları eksik"),
                t("They were moved or deleted.", "Taşınmış ya da silinmiş."),
            ),
            Cause::NoSpace => (
                t("The disk is full", "Disk dolu"),
                t(
                    "Make room on the disk of the game and its prefix.",
                    "Oyunun ve prefix'inin diskinde yer aç.",
                ),
            ),
            Cause::PrefixDisk(k) => (
                format!(
                    "{} {k}",
                    t("The prefix is on", "Prefix şu dosya sisteminde:")
                ),
                t(
                    "Wine prefixes need a Linux file system (ext4, btrfs). Choose another prefix folder in the game's settings.",
                    "Wine prefix'leri Linux dosya sistemi ister (ext4, btrfs). Oyunun ayarlarından başka bir prefix klasörü seç.",
                ),
            ),
            Cause::ProtonGone(p) => (
                format!(
                    "{}: {p}",
                    t("This Proton is not installed", "Bu Proton kurulu değil")
                ),
                t(
                    "The game is set to use it.",
                    "Oyun bunu kullanacak şekilde ayarlı.",
                ),
            ),
            Cause::NoProton => (
                t("There is no Proton", "Hiç Proton yok"),
                t("Windows games need it.", "Windows oyunları için gerekli."),
            ),
            Cause::Login(s) => (
                format!(
                    "{} {}",
                    s.name(),
                    t("wants a new login", "yeniden giriş istiyor")
                ),
                t("The login ended.", "Oturum sona ermiş."),
            ),
            Cause::Outdated => (
                t(
                    "The game has to be updated first",
                    "Önce oyunun güncellenmesi gerekiyor",
                ),
                t(
                    "The store does not start an older version.",
                    "Mağaza eski sürümü başlatmıyor.",
                ),
            ),
            Cause::PreLaunch => (
                t(
                    "The command before the game failed",
                    "Oyundan önceki komut başarısız oldu",
                ),
                t(
                    "It is in the game's Launch settings.",
                    "Oyunun Başlatma ayarlarında.",
                ),
            ),
            Cause::Wrapper(w) => (
                format!(
                    "{}: {w}",
                    t(
                        "This program could not be started",
                        "Bu program başlatılamadı"
                    )
                ),
                t(
                    "The settings put it in front of the game.",
                    "Ayarlar onu oyunun önüne koyuyor.",
                ),
            ),
            Cause::NotExecutable => (
                t("The file may not be run", "Dosyanın çalıştırma izni yok"),
                t(
                    "It is not marked as a program.",
                    "Program olarak işaretlenmemiş.",
                ),
            ),
            Cause::WineVersion => (
                t(
                    "Something of another Wine still runs in the prefix",
                    "Prefix'te başka bir Wine'dan kalan süreç var",
                ),
                t(
                    "Two versions do not work together.",
                    "İki sürüm birlikte çalışmaz.",
                ),
            ),
            Cause::Gamescope => (
                t("Gamescope could not start", "Gamescope başlatılamadı"),
                t(
                    "The game itself was not reached.",
                    "Oyunun kendisine sıra gelmedi.",
                ),
            ),
            Cause::NoVulkan(true) => (
                t(
                    "There is no 32-bit Vulkan driver",
                    "32 bit Vulkan sürücüsü yok",
                ),
                t("This is a 32-bit game.", "Bu 32 bit bir oyun."),
            ),
            Cause::NoVulkan(false) => (
                t("There is no Vulkan driver", "Vulkan sürücüsü yok"),
                t(
                    "Windows games are drawn with it.",
                    "Windows oyunları onunla çizilir.",
                ),
            ),
            Cause::Vulkan => (
                t("Direct3D could not be set up", "Direct3D kurulamadı"),
                t(
                    "The graphics driver does not offer what this Proton wants.",
                    "Ekran kartı sürücüsü bu Proton'un istediğini sunmuyor.",
                ),
            ),
            Cause::DeviceLost => (
                t(
                    "The graphics card gave up",
                    "Ekran kartı yanıt vermeyi bıraktı",
                ),
                t(
                    "Overclocking, too little video memory, or a layer over the game.",
                    "Hız aşırtma, yetersiz ekran belleği ya da oyunun üzerindeki bir katman.",
                ),
            ),
            Cause::Dll(d) => (
                format!(
                    "{}: {d}",
                    t(
                        "A Windows component is missing",
                        "Bir Windows bileşeni eksik"
                    )
                ),
                t(
                    "It is installed into the game's prefix.",
                    "Oyunun prefix'ine kurulur.",
                ),
            ),
            Cause::DotNet => (
                t("The game needs .NET", "Oyun .NET istiyor"),
                t(
                    "It is installed into the game's prefix; this takes a few minutes.",
                    "Oyunun prefix'ine kurulur; birkaç dakika sürer.",
                ),
            ),
            Cause::Media => (
                t("A video could not be played", "Bir video oynatılamadı"),
                t(
                    "Proton-GE brings more video formats.",
                    "Proton-GE daha çok video biçimi getirir.",
                ),
            ),
            Cause::SharedLibrary(l) => (
                format!("{}: {l}", t("A library is missing", "Bir kütüphane eksik")),
                t(
                    "Steam's Linux runtimes bring the usual ones.",
                    "Steam'in Linux çalışma ortamları yaygın olanları getirir.",
                ),
            ),
            Cause::AntiCheat => (
                t("The anti-cheat did not start", "Hile koruması başlamadı"),
                t(
                    "Its Linux part or its files may be damaged.",
                    "Linux parçası ya da dosyaları bozuk olabilir.",
                ),
            ),
            Cause::AntiCheatBlocked => (
                t(
                    "Its anti-cheat keeps Linux out",
                    "Hile koruması Linux'a izin vermiyor",
                ),
                t(
                    "That is the makers' choice or not solved yet; nothing here changes it.",
                    "Bu yapımcının tercihi ya da henüz çözülmemiş; buradan değiştirilemez.",
                ),
            ),
            Cause::SteamNeeded => (
                t(
                    "The game wants Steam to run",
                    "Oyun Steam'in açık olmasını istiyor",
                ),
                t(
                    "Start Steam, log in, then try again.",
                    "Steam'i aç, giriş yap, sonra yeniden dene.",
                ),
            ),
            Cause::Runtime => (
                t(
                    "The runtime is missing or damaged",
                    "Çalışma ortamı eksik ya da bozuk",
                ),
                t(
                    "umu and the Steam Linux Runtime are fetched again.",
                    "umu ve Steam Linux Runtime yeniden indirilir.",
                ),
            ),
            Cause::Namespaces => (
                t(
                    "The system does not allow containers",
                    "Sistem konteynerlere izin vermiyor",
                ),
                t(
                    "Unprivileged user namespaces have to be allowed (kernel.unprivileged_userns_clone=1).",
                    "Ayrıcalıksız kullanıcı ad alanlarına izin verilmeli (kernel.unprivileged_userns_clone=1).",
                ),
            ),
            Cause::OpenFiles => (
                t("Too many open files", "Çok fazla açık dosya"),
                t(
                    "The system's limit is too low for this game.",
                    "Sistemin sınırı bu oyun için çok düşük.",
                ),
            ),
            Cause::NoNetwork => (
                t("No connection to the internet", "İnternet bağlantısı yok"),
                t(
                    "Something had to be fetched before the start.",
                    "Başlamadan önce bir şeyin indirilmesi gerekiyordu.",
                ),
            ),
            Cause::Borked => (
                t(
                    "Others report that it does not run",
                    "Başkaları çalışmadığını bildiriyor",
                ),
                t(
                    "ProtonDB lists it as broken.",
                    "ProtonDB'de bozuk olarak listeleniyor.",
                ),
            ),
            Cause::Mods(list) => (
                format!(
                    "{}: {}",
                    t("Mods in its folder", "Klasöründeki modlar"),
                    list.join(", ")
                ),
                t(
                    "They change how the game draws; it may not like them.",
                    "Oyunun çizimini değiştirirler; oyun onlarla anlaşamıyor olabilir.",
                ),
            ),
            Cause::Crash => (
                t("The game crashed", "Oyun çöktü"),
                t(
                    "It ended by itself. These often help, from the top:",
                    "Kendiliğinden kapandı. Yukarıdan başlayarak sık işe yarayanlar:",
                ),
            ),
            Cause::Unknown => (
                t("What often helps", "Sık işe yarayanlar"),
                t(
                    "The log does not name a reason. From the top:",
                    "Günlük bir sebep söylemiyor. Yukarıdan başla:",
                ),
            ),
        }
    }

    /// A fix: its button, a note, its icon.
    fn fix_text(&self, f: &Fix) -> (String, String, &'static str) {
        let t = |en: &str, tr: &str| self.tr_owned(en, tr);
        let none = String::new();
        match f {
            Fix::Detail => (
                t(
                    "Start once with a detailed log",
                    "Ayrıntılı günlükle bir kez başlat",
                ),
                t(
                    "Then it is looked at again, with more to go by.",
                    "Sonra daha çok bilgiyle yeniden bakılır.",
                ),
                "search",
            ),
            Fix::Plain => (
                t("Start once plainly", "Bir kez sade başlat"),
                t(
                    "Without overlays, layers, options and gamescope; nothing is changed.",
                    "Katman, gösterge, seçenek ve gamescope olmadan; ayarlar değişmez.",
                ),
                "feather",
            ),
            Fix::Proton(p) => (
                format!("{}: {p}", t("Use another Proton", "Başka Proton kullan")),
                none,
                "wine",
            ),
            Fix::ProtonAuto => (
                t("Let Arkor choose the Proton", "Proton'u Arkor seçsin"),
                none,
                "wine",
            ),
            Fix::WineD3d => (
                t("Draw with WineD3D", "WineD3D ile çiz"),
                t(
                    "Slower; for old games and cards.",
                    "Daha yavaş; eski oyun ve kartlar için.",
                ),
                "palette",
            ),
            Fix::GamescopeOff => (
                t("Switch gamescope off", "Gamescope'u kapat"),
                none,
                "monitor-off",
            ),
            Fix::ClearPreLaunch => (t("Remove the command", "Komutu kaldır"), none, "x"),
            Fix::ClearLaunchOptions => (
                t("Clear the launch options", "Başlatma seçeneklerini temizle"),
                none,
                "x",
            ),
            Fix::Verbs(v) => (
                format!("{}: {}", t("Install", "Kur"), v.join(", ")),
                none,
                "package-plus",
            ),
            Fix::AntiCheat => (
                t(
                    "Fetch the anti-cheat runtimes again",
                    "Hile koruması parçalarını yeniden indir",
                ),
                none,
                "shield-check",
            ),
            Fix::Runtime => (
                t("Fetch the runtime again", "Çalışma ortamını yeniden indir"),
                none,
                "download",
            ),
            Fix::EndPrefix => (
                t("End what runs in the prefix", "Prefix'te kalanları kapat"),
                none,
                "circle-stop",
            ),
            Fix::UpdatePrefix => (
                t("Set the prefix up again", "Prefix'i yeniden hazırla"),
                t(
                    "What is in it, saves too, stays.",
                    "İçindekiler, kayıtlar dahil, kalır.",
                ),
                "refresh-cw",
            ),
            Fix::MakeExecutable(_) => (t("Allow running it", "Çalıştırma izni ver"), none, "check"),
            Fix::Repair(..) => (
                t("Check the game's files", "Oyun dosyalarını denetle"),
                none,
                "shield-check",
            ),
            Fix::Update(..) => (t("Update the game", "Oyunu güncelle"), none, "download"),
            Fix::Login(s) => (
                format!("{} {}", t("Log in to", "Giriş yap:"), s.name()),
                none,
                "log-in",
            ),
            Fix::Packages(l) => (
                format!("{}: {}", t("Install", "Kur"), l.join(", ")),
                t(
                    "Needs the administrator password.",
                    "Yönetici parolası ister.",
                ),
                "package-plus",
            ),
            Fix::Mount => (
                t("Mount it and start", "Bağla ve başlat"),
                none,
                "hard-drive",
            ),
            Fix::RemoveMod(m) => (
                format!("{} {}", t("Remove", "Kaldır:"), m.name()),
                t(
                    "The game's own files are put back.",
                    "Oyunun kendi dosyaları geri konur.",
                ),
                "trash",
            ),
            Fix::LinuxRuntime(r) => (
                format!(
                    "{} {}",
                    t("Run in", "Şununla çalıştır:"),
                    r.trim_start_matches("umu-")
                ),
                none,
                "box",
            ),
        }
    }

    fn tr_owned(&self, en: &str, tr: &str) -> String {
        match self.global.language {
            arkor_core::Language::Tr => tr.to_string(),
            arkor_core::Language::En => en.to_string(),
        }
    }

    // ---- Console mode ----

    /// The screen: the problems with their fixes, and trying again.
    pub(super) fn trouble_rows(&self, id: &str) -> Vec<Row> {
        let Some(gid) = self.selected_id().filter(|g| g.id == id) else {
            return Vec::new();
        };
        let mut pills = vec![Item::main(
            Act::Play(gid.to_string()),
            "play",
            self.tr("Try again", "Yeniden dene"),
        )];
        pills.push(Item::pill(
            Act::TroubleLog,
            "scroll-text",
            self.tr("The log", "Günlük"),
            false,
        ));
        if self.trouble_can_undo() {
            pills.push(Item::pill(
                Act::TroubleUndo,
                "undo-2",
                self.tr("Undo the changes", "Değişiklikleri geri al"),
                false,
            ));
        }
        let mut rows = vec![Row::new(Kind::Pills, pills)];
        let found = self.trouble_of(&gid);
        if found.is_empty() {
            let mut row = Row::new(
                Kind::Line,
                vec![Item::line(
                    Act::Nothing,
                    "loader-circle",
                    self.tr("Looking…", "Bakılıyor…"),
                    Value::None,
                )],
            );
            row.title = None;
            rows.push(row);
        }
        for (p, problem) in found.iter().enumerate() {
            let (title, why) = self.cause_text(&problem.cause);
            let mut first = true;
            let mut push = |item: Item| {
                let mut row = Row::new(Kind::Line, vec![item]);
                row.title = std::mem::take(&mut first).then(|| title.clone());
                rows.push(row);
            };
            if problem.fixes.is_empty() {
                let mut item = Item::line(Act::Nothing, "info", why.clone(), Value::None);
                item.sub = problem.line.clone().unwrap_or_default();
                push(item);
            }
            for (n, fix) in problem.fixes.iter().enumerate() {
                let (label, note, icon) = self.fix_text(fix);
                let (icon, value) = self.fix_state(fix, icon);
                let mut item = Item::line(Act::Fix(p, n), icon, label, value);
                item.sub = match (n, note.is_empty()) {
                    (0, true) => why.clone(),
                    (0, false) if problem.cause != Cause::Unknown => format!("{why} {note}"),
                    _ => note,
                };
                push(item);
            }
        }
        rows
    }

    /// Whether a fix is done or being done.
    fn fix_state(&self, fix: &Fix, icon: &'static str) -> (&'static str, Value) {
        if self.trouble.busy.as_ref() == Some(fix) {
            return (
                "loader-circle",
                Value::Text(self.tr("Working…", "Yapılıyor…").into()),
            );
        }
        if self.trouble.done.contains(fix) {
            return (
                "circle-check",
                Value::Text(self.tr("Done", "Yapıldı").into()),
            );
        }
        (icon, Value::None)
    }

    /// One of the fixes on the screen was chosen.
    pub(super) fn trouble_choose(&mut self, p: usize, n: usize, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else { return };
        let fix = self
            .trouble_of(&id)
            .get(p)
            .and_then(|p| p.fixes.get(n))
            .cloned();
        if let Some(fix) = fix {
            self.trouble_fix(fix, cx);
        }
    }

    /// The end of the log, to read.
    pub(super) fn trouble_log(&mut self) {
        let Some(id) = self.selected_id() else { return };
        let Ok(path) = arkor_core::paths::launch_log(&id) else {
            return;
        };
        let lines = super::game_page::log_tail(&path, 120);
        let text = match lines.is_empty() {
            true => self
                .tr("There is no log yet.", "Henüz günlük yok.")
                .to_string(),
            false => lines.join("\n"),
        };
        let title = self.tr("The log", "Günlük").to_string();
        let options = vec![(self.tr("Close", "Kapat").to_string(), Act::Close)];
        self.console.read_at = 0.;
        self.console_overlay(Overlay::Read(title, text, options));
    }

    // ---- The desktop's card ----

    pub(super) fn render_trouble(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let id = self.selected_id()?;
        let found = self.trouble_of(&id).to_vec();
        // Looked at once by itself.
        if found.is_empty() && !self.trouble.looking && self.trouble.tried.as_ref() != Some(&id) {
            self.trouble.tried = Some(id.clone());
            self.trouble_load(&id, cx);
        }
        let mut head = div().flex().flex_wrap().items_center().gap_2().child(
            style::button(
                "trouble-look",
                if found.is_empty() {
                    self.tr("Look for the reason", "Sebebini bul")
                } else {
                    self.tr("Look again", "Yeniden bak")
                },
                found.is_empty(),
            )
            .child(style::icon(
                "search",
                13.,
                if found.is_empty() {
                    pal::on_accent()
                } else {
                    pal::muted()
                },
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(id) = this.selected_id() {
                    this.trouble_load(&id, cx);
                }
            })),
        );
        if self.trouble.looking {
            head = head.child(self.loading_inline(self.tr("Looking…", "Bakılıyor…")));
        }
        if self.trouble_can_undo() {
            head = head.child(
                style::button(
                    "trouble-undo",
                    self.tr("Undo the changes", "Değişiklikleri geri al"),
                    false,
                )
                .child(style::icon("undo-2", 13., pal::muted()))
                .on_click(cx.listener(|this, _, _, cx| this.trouble_undo(cx))),
            );
        }
        let mut card = widgets::section(self.tr("Why did it not start?", "Neden açılmadı?"))
            .child(widgets::label(self.tr(
                "Looks at the last launch's log, the game's files, its prefix and settings, and this computer.",
                "Son başlatmanın günlüğüne, oyunun dosyalarına, prefix'ine, ayarlarına ve bu bilgisayara bakar.",
            )))
            .child(head);
        for (p, problem) in found.iter().enumerate() {
            let (title, why) = self.cause_text(&problem.cause);
            let mut body = div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                .child(widgets::label(why))
                .when_some(problem.line.clone(), |d, line| {
                    d.child(
                        div()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(pal::muted())
                            .child(line),
                    )
                });
            let mut row = div().flex().flex_wrap().gap_2();
            for (n, fix) in problem.fixes.iter().enumerate() {
                let (label, note, icon) = self.fix_text(fix);
                let (icon, _) = self.fix_state(fix, icon);
                let key = p * 100 + n;
                row = row.child(
                    style::button(("trouble-fix", key), label, p == 0 && n == 0)
                        .child(style::icon(
                            icon,
                            13.,
                            if p == 0 && n == 0 {
                                pal::on_accent()
                            } else {
                                pal::muted()
                            },
                        ))
                        .when(!note.is_empty(), |d| {
                            d.tooltip(move |_, cx| widgets::tip(note.clone().into(), cx))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.trouble_choose(p, n, cx))),
                );
            }
            body = body.child(row.max_w(px(900.)));
            card = card.child(widgets::divider()).child(body);
        }
        Some(card)
    }
}
