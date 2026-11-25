//! Keeps the stores' logins encrypted on disk.
//!
//! The helpers keep a login as a plain JSON file. Where systemd can encrypt for a user
//! (`systemd-creds --user`, which asks for nothing and needs no keyring), that file becomes
//!
//! - `<file>.sealed` beside it: the login, encrypted with a key of this computer, and
//! - a link to a copy in the user's runtime folder, which lives in memory only and goes away
//!   at log-out.
//!
//! So what lies on the disk, in backups and in copies of the home folder is of no use
//! elsewhere. Programs the user runs could still ask systemd to decrypt it. Without systemd's
//! help the file stays as it was, readable by the user only.

use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;

use super::Shop;

/// Encrypts and decrypts. The name is part of what is sealed.
trait Sealer {
    fn seal(&self, name: &str, plain: &[u8]) -> anyhow::Result<Vec<u8>>;
    fn unseal(&self, name: &str, sealed: &[u8]) -> anyhow::Result<Vec<u8>>;
}

/// systemd's own, through `program`.
struct Systemd {
    program: String,
}

impl Systemd {
    fn pipe(&self, name: &str, action: &str, input: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut child = Command::new(&self.program)
            .args(["--user", "--no-ask-password"])
            .arg(format!("--name={name}"))
            .args([action, "-", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("running systemd-creds")?;
        // Logins are small: they fit a pipe, so writing first cannot block.
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input)?;
        }
        let out = child.wait_with_output()?;
        anyhow::ensure!(
            out.status.success(),
            "systemd-creds: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(out.stdout)
    }
}

impl Sealer for Systemd {
    fn seal(&self, name: &str, plain: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.pipe(name, "encrypt", plain)
    }

    fn unseal(&self, name: &str, sealed: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.pipe(name, "decrypt", sealed)
    }
}

/// How a login is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    /// There is no login.
    Nothing,
    /// Encrypted.
    Sealed,
    /// As a plain file only the user can read.
    Plain,
}

/// One login file.
struct Login {
    name: String,
    /// Where the helper looks for it.
    file: PathBuf,
    /// The runtime folder it is opened in, if there is one.
    runtime: Option<PathBuf>,
}

impl Login {
    fn of(shop: Shop) -> Option<Login> {
        let dir = arkor_core::paths::stores_dir().ok()?;
        let file = dir.join(shop.key()).join(match shop {
            Shop::Epic => "user.json",
            Shop::Gog => "auth.json",
            Shop::Amazon => "nile/current_user.json",
            Shop::Steam => return None,
        });
        // Another data folder has other logins.
        use std::hash::{Hash, Hasher};
        let mut hash = std::hash::DefaultHasher::new();
        dir.hash(&mut hash);
        Some(Login {
            name: format!("arkor-{}", shop.key()),
            file,
            runtime: dirs::runtime_dir().map(|r| {
                r.join("arkor")
                    .join(format!("logins-{:016x}", hash.finish()))
            }),
        })
    }

    fn sealed(&self) -> PathBuf {
        suffixed(&self.file, ".sealed")
    }

    /// The opened copy and the note on what of it is sealed already.
    fn open(&self) -> Option<(PathBuf, PathBuf)> {
        let run = self.runtime.as_ref()?.join(format!("{}.json", self.name));
        let note = suffixed(&run, ".note");
        Some((run, note))
    }

    fn kept(&self) -> Kept {
        match std::fs::symlink_metadata(&self.file) {
            Ok(m) if m.is_file() && is_login(&read(&self.file)) => Kept::Plain,
            Ok(m) if m.is_symlink() && is_login(&read(&self.file)) => Kept::Sealed,
            // Removed by its helper, which is how those log out.
            Err(_) if self.open().is_some_and(|(run, _)| run.exists()) => Kept::Nothing,
            _ if self.sealed().is_file() => Kept::Sealed,
            _ => Kept::Nothing,
        }
    }

    /// Makes the login ready for its helper, and seals what the helper changed.
    fn settle(&self, sealer: &dyn Sealer) -> anyhow::Result<()> {
        let Some((run, note)) = self.open() else {
            return self.without_runtime(sealer);
        };
        let meta = std::fs::symlink_metadata(&self.file).ok();
        let sealed = self.sealed();
        if meta.is_none() && !sealed.exists() {
            return Ok(());
        }
        let _lock = self.lock()?;
        match meta {
            // A plain login: an older Arkor's, or a helper that replaced the link.
            Some(m) if m.is_file() => {
                let plain = read(&self.file);
                if !is_login(&plain) {
                    return Ok(());
                }
                let result = self.seal(sealer, &plain).and_then(|()| {
                    write_private(&run, &plain)?;
                    note_sealed(&run, &note)?;
                    self.link(&run)
                });
                if result.is_err() {
                    let _ = std::fs::set_permissions(
                        &self.file,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
                result
            }
            // Removed while it was open: that is how the helpers log out.
            None if run.exists() => {
                self.wipe();
                Ok(())
            }
            _ if !run.exists() => {
                if sealed.exists() {
                    let plain = sealer.unseal(&self.name, &std::fs::read(&sealed)?)?;
                    anyhow::ensure!(is_login(&plain), "the saved login is damaged");
                    write_private(&run, &plain)?;
                    note_sealed(&run, &note)?;
                }
                self.link(&run)
            }
            _ => {
                self.link(&run)?;
                if std::fs::read_to_string(&note).is_ok_and(|n| n == stamp(&run)) {
                    return Ok(());
                }
                let plain = read(&run);
                // Being written right now: the next call finds it whole.
                if !is_login(&plain) {
                    return Ok(());
                }
                // Noted as it was before reading: a change meanwhile is sealed next time.
                self.seal(sealer, &plain)?;
                if read(&run) == plain {
                    note_sealed(&run, &note)?;
                }
                Ok(())
            }
        }
    }

    /// No runtime folder (no session): the helper gets a plain file.
    fn without_runtime(&self, sealer: &dyn Sealer) -> anyhow::Result<()> {
        let usable = std::fs::metadata(&self.file).is_ok_and(|m| m.is_file());
        if usable || !self.sealed().exists() {
            return Ok(());
        }
        let plain = sealer.unseal(&self.name, &std::fs::read(self.sealed())?)?;
        write_private(&self.file, &plain)
    }

    /// Writes the sealed file, and makes sure it can be opened again.
    fn seal(&self, sealer: &dyn Sealer, plain: &[u8]) -> anyhow::Result<()> {
        let sealed = sealer.seal(&self.name, plain)?;
        anyhow::ensure!(
            sealer.unseal(&self.name, &sealed)? == plain,
            "the sealed login could not be opened again"
        );
        anyhow::ensure!(
            !contains(&sealed, plain),
            "the login was not encrypted at all"
        );
        write_private(&self.sealed(), &sealed)
    }

    /// Makes the helper's file a link to the opened copy.
    fn link(&self, run: &Path) -> anyhow::Result<()> {
        if std::fs::read_link(&self.file).is_ok_and(|to| to == run) {
            return Ok(());
        }
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let fresh = suffixed(&self.file, ".link");
        let _ = std::fs::remove_file(&fresh);
        std::os::unix::fs::symlink(run, &fresh)?;
        std::fs::rename(&fresh, &self.file)
            .with_context(|| format!("linking {}", self.file.display()))
    }

    /// Before logging in: the new login is written to memory, never to the disk.
    fn prepare(&self, sealer: &dyn Sealer) -> anyhow::Result<()> {
        let Some((run, _)) = self.open() else {
            return Ok(());
        };
        if self.kept() != Kept::Nothing {
            return Ok(());
        }
        let probe = b"probe".as_slice();
        let sealed = sealer.seal(&self.name, probe)?;
        anyhow::ensure!(sealer.unseal(&self.name, &sealed)? == probe);
        let _lock = self.lock()?;
        self.wipe();
        self.link(&run)
    }

    /// Removes every trace of the login.
    fn wipe(&self) {
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_file(self.sealed());
        if let Some((run, note)) = self.open() {
            let _ = std::fs::remove_file(run);
            let _ = std::fs::remove_file(note);
        }
    }

    /// Only one of Arkor's processes at a time.
    fn lock(&self) -> anyhow::Result<std::fs::File> {
        let dir = self.runtime.as_ref().context("no runtime folder")?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        // SAFETY: no arguments, cannot fail.
        let uid = unsafe { libc::getuid() };
        anyhow::ensure!(
            std::fs::symlink_metadata(dir)
                .is_ok_and(|m| m.is_dir() && m.uid() == uid && m.mode() & 0o077 == 0),
            "{} is not private",
            dir.display()
        );
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(dir.join(format!("{}.lock", self.name)))?;
        // SAFETY: a valid descriptor; released when the file is closed.
        let ok = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) };
        anyhow::ensure!(ok == 0, "locking the login");
        Ok(f)
    }
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    s.into()
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

/// Whether a file's text is a login: a JSON document with something in it.
fn is_login(text: &[u8]) -> bool {
    match serde_json::from_slice::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(o)) => !o.is_empty(),
        _ => false,
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Which state of the opened copy something is about.
fn stamp(run: &Path) -> String {
    std::fs::metadata(run)
        .map(|m| format!("{} {} {}", m.mtime(), m.mtime_nsec(), m.size()))
        .unwrap_or_default()
}

fn note_sealed(run: &Path, note: &Path) -> anyhow::Result<()> {
    write_private(note, stamp(run).as_bytes())
}

/// Writes a file only the user can read, whole or not at all.
fn write_private(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let fresh = suffixed(path, ".new");
    let _ = std::fs::remove_file(&fresh);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&fresh)?;
    f.write_all(data)?;
    f.sync_all()?;
    std::fs::rename(&fresh, path).with_context(|| format!("writing {}", path.display()))
}

/// systemd's sealer, unless logins are kept plain here. Tests with stand-in helpers have no
/// logins worth sealing (they name their own program when they want it).
fn sealer() -> Option<Systemd> {
    match std::env::var("ARKOR_CREDS") {
        Ok(p) if p.is_empty() || p == "none" => None,
        Ok(program) => Some(Systemd { program }),
        Err(_) if cfg!(test) || std::env::var_os("ARKOR_STORE_HELPERS").is_some() => None,
        Err(_) => Some(Systemd {
            program: "systemd-creds".into(),
        }),
    }
}

/// Makes a store's login ready for its helper, and seals what changed. Cheap when nothing
/// did. Blocking.
pub fn settle(shop: Shop) {
    let (Some(login), Some(sealer)) = (Login::of(shop), sealer()) else {
        return;
    };
    let result = login.settle(&sealer);
    let mut locked = LOCKED.lock().unwrap_or_else(|e| e.into_inner());
    locked.retain(|s| *s != shop);
    if let Err(e) = result {
        // Sealed on another computer, or by a system that was installed anew.
        if login.sealed().exists() && !login.file.exists() {
            locked.push(shop);
            eprintln!("arkor: log in to {} again: {e:#}", shop.name());
        } else if !TOLD.swap(true, Ordering::Relaxed) {
            eprintln!("arkor: the {} login is not sealed: {e:#}", shop.name());
        }
    }
}

/// Said once: it is tried again with every use of a helper.
static TOLD: AtomicBool = AtomicBool::new(false);

/// Stores whose sealed login could not be opened here.
static LOCKED: Mutex<Vec<Shop>> = Mutex::new(Vec::new());

/// All of them.
pub fn settle_all() {
    for shop in Shop::ALL {
        settle(shop);
    }
}

/// Before a login: keeps the new one off the disk.
pub(super) fn prepare(shop: Shop) {
    if let (Some(login), Some(sealer)) = (Login::of(shop), sealer())
        && let Err(e) = login.prepare(&sealer)
    {
        eprintln!("arkor: logins stay plain files: {e:#}");
    }
}

/// After logging out.
pub(super) fn forget(shop: Shop) {
    if let Some(login) = Login::of(shop) {
        login.wipe();
    }
}

/// How a store's login is kept. Reads files only.
pub fn kept(shop: Shop) -> Kept {
    let locked = LOCKED.lock().unwrap_or_else(|e| e.into_inner());
    match Login::of(shop) {
        Some(l) if !(locked.contains(&shop) && !l.file.exists()) => l.kept(),
        _ => Kept::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns every byte around; "unsealing" needs the same name.
    struct Fake {
        works: bool,
    }

    impl Sealer for Fake {
        fn seal(&self, name: &str, plain: &[u8]) -> anyhow::Result<Vec<u8>> {
            anyhow::ensure!(self.works, "no systemd");
            let mut out = format!("{name}:").into_bytes();
            out.extend(plain.iter().map(|b| b ^ 0x5a));
            Ok(out)
        }

        fn unseal(&self, name: &str, sealed: &[u8]) -> anyhow::Result<Vec<u8>> {
            let rest = sealed
                .strip_prefix(format!("{name}:").as_bytes())
                .context("another login")?;
            Ok(rest.iter().map(|b| b ^ 0x5a).collect())
        }
    }

    const ON: Fake = Fake { works: true };
    const OLD: &str = r#"{"token":"old"}"#;
    const NEW: &str = r#"{"token":"new"}"#;

    fn login(tmp: &Path) -> Login {
        std::fs::create_dir_all(tmp.join("stores/epic")).unwrap();
        Login {
            name: "arkor-epic".into(),
            file: tmp.join("stores/epic/user.json"),
            runtime: Some(tmp.join("run/logins")),
        }
    }

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().mode() & 0o777
    }

    #[test]
    fn plain_login_is_sealed_and_opened_again() {
        let tmp = tempfile::tempdir().unwrap();
        let l = login(tmp.path());
        l.settle(&ON).unwrap();
        assert_eq!(l.kept(), Kept::Nothing);
        std::fs::write(&l.file, OLD).unwrap();
        assert_eq!(l.kept(), Kept::Plain);
        l.settle(&ON).unwrap();
        assert_eq!(l.kept(), Kept::Sealed);
        // The helper reads the same; on the disk it is sealed.
        assert_eq!(std::fs::read_to_string(&l.file).unwrap(), OLD);
        assert!(l.file.is_symlink());
        assert!(!contains(&read(&l.sealed()), b"token"));
        let (run, _) = l.open().unwrap();
        assert_eq!((mode(&run), mode(&l.sealed())), (0o600, 0o600));
        // After a restart memory is empty.
        std::fs::remove_dir_all(tmp.path().join("run")).unwrap();
        assert_eq!(l.kept(), Kept::Sealed);
        l.settle(&ON).unwrap();
        assert_eq!(std::fs::read_to_string(&l.file).unwrap(), OLD);
    }

    #[test]
    fn changes_of_the_helper_are_sealed() {
        let tmp = tempfile::tempdir().unwrap();
        let l = login(tmp.path());
        std::fs::write(&l.file, OLD).unwrap();
        l.settle(&ON).unwrap();
        // Through the link, as the helpers write.
        std::fs::write(&l.file, NEW).unwrap();
        l.settle(&ON).unwrap();
        assert_eq!(
            ON.unseal(&l.name, &read(&l.sealed())).unwrap(),
            NEW.as_bytes()
        );
        // Half written: kept for the next time.
        std::fs::write(&l.file, "{\"tok").unwrap();
        l.settle(&ON).unwrap();
        assert_eq!(
            ON.unseal(&l.name, &read(&l.sealed())).unwrap(),
            NEW.as_bytes()
        );
        // A helper that replaces the link by a file.
        std::fs::remove_file(&l.file).unwrap();
        std::fs::write(&l.file, OLD).unwrap();
        l.settle(&ON).unwrap();
        assert!(l.file.is_symlink());
        assert_eq!(
            ON.unseal(&l.name, &read(&l.sealed())).unwrap(),
            OLD.as_bytes()
        );
    }

    #[test]
    fn logging_out_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let l = login(tmp.path());
        std::fs::write(&l.file, OLD).unwrap();
        l.settle(&ON).unwrap();
        // The helper removes its file.
        std::fs::remove_file(&l.file).unwrap();
        assert_eq!(l.kept(), Kept::Nothing);
        l.settle(&ON).unwrap();
        let (run, _) = l.open().unwrap();
        assert!(!run.exists() && !l.sealed().exists() && !l.file.is_symlink());
        // Not mistaken for a login after a restart.
        std::fs::remove_dir_all(tmp.path().join("run")).unwrap();
        l.settle(&ON).unwrap();
        assert_eq!(l.kept(), Kept::Nothing);
    }

    #[test]
    fn new_login_never_touches_the_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let l = login(tmp.path());
        l.prepare(&ON).unwrap();
        assert_eq!(l.kept(), Kept::Nothing);
        assert!(!l.file.exists());
        std::fs::write(&l.file, NEW).unwrap();
        assert!(l.file.is_symlink());
        l.settle(&ON).unwrap();
        assert_eq!(l.kept(), Kept::Sealed);
        assert!(l.sealed().is_file());
    }

    #[test]
    fn without_systemd_the_login_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let l = login(tmp.path());
        let off = Fake { works: false };
        std::fs::write(&l.file, OLD).unwrap();
        assert!(l.settle(&off).is_err());
        assert_eq!(l.kept(), Kept::Plain);
        assert_eq!(std::fs::read_to_string(&l.file).unwrap(), OLD);
        assert_eq!(mode(&l.file), 0o600);
        assert!(l.prepare(&off).is_ok(), "nothing to prepare over a login");
        // No session: the helper gets a plain file back.
        l.settle(&ON).unwrap();
        let bare = Login {
            runtime: None,
            ..login(tmp.path())
        };
        std::fs::remove_dir_all(tmp.path().join("run")).unwrap();
        bare.settle(&ON).unwrap();
        assert_eq!(bare.kept(), Kept::Plain);
        assert_eq!(std::fs::read_to_string(&bare.file).unwrap(), OLD);
    }

    #[test]
    fn wrong_computer_keeps_the_sealed_file() {
        let tmp = tempfile::tempdir().unwrap();
        let l = login(tmp.path());
        write_private(&l.sealed(), b"someone else's").unwrap();
        assert!(l.settle(&ON).is_err());
        assert!(l.sealed().is_file());
    }

    /// The real one, with a made-up login.
    #[test]
    #[ignore = "needs systemd 256 or newer"]
    fn real_systemd() {
        let tmp = tempfile::tempdir().unwrap();
        let l = login(tmp.path());
        let s = Systemd {
            program: "systemd-creds".into(),
        };
        std::fs::write(&l.file, OLD).unwrap();
        l.settle(&s).unwrap();
        std::fs::remove_dir_all(tmp.path().join("run")).unwrap();
        l.settle(&s).unwrap();
        assert_eq!(std::fs::read_to_string(&l.file).unwrap(), OLD);
        assert!(!contains(&read(&l.sealed()), b"token"));
    }
}
