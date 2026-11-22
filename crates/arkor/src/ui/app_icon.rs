//! Arkor's logo as a desktop icon: installed into the user's icon theme
//! (`~/.local/share/icons/hicolor/<size>/apps/arkor.png`) so the tray, the autostart entry
//! and shortcuts can name it `arkor`. Written only when missing or changed.

use std::path::PathBuf;

/// Icon name in the hicolor theme.
pub(super) const NAME: &str = "arkor";

const PNGS: [(u32, &[u8]); 8] = [
    (16, include_bytes!("../../assets/logo/arkor-16.png")),
    (24, include_bytes!("../../assets/logo/arkor-24.png")),
    (32, include_bytes!("../../assets/logo/arkor-32.png")),
    (48, include_bytes!("../../assets/logo/arkor-48.png")),
    (64, include_bytes!("../../assets/logo/arkor-64.png")),
    (128, include_bytes!("../../assets/logo/arkor-128.png")),
    (256, include_bytes!("../../assets/logo/arkor-256.png")),
    (512, include_bytes!("../../assets/logo/arkor-512.png")),
];
const SVG: &[u8] = include_bytes!("../../assets/logo/arkor.svg");

/// The icon theme root Arkor installs into (also given to the tray as its theme path).
pub(super) fn theme_root() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("icons"))
}

fn write_if_changed(path: PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    if std::fs::read(&path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)
}

/// Installs the logo into the user's hicolor theme. Errors are ignored: a missing icon
/// only means desktops show a generic one.
pub(super) fn install() {
    let Some(root) = theme_root() else {
        return;
    };
    let hicolor = root.join("hicolor");
    for (size, bytes) in PNGS {
        let _ = write_if_changed(
            hicolor.join(format!("{size}x{size}/apps/{NAME}.png")),
            bytes,
        );
    }
    let _ = write_if_changed(hicolor.join(format!("scalable/apps/{NAME}.svg")), SVG);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_every_size_once() {
        let _lock = super::super::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests touching XDG variables hold TEST_ENV_LOCK.
        unsafe { std::env::set_var("XDG_DATA_HOME", tmp.path()) };
        install();
        let icon = tmp.path().join("icons/hicolor/48x48/apps/arkor.png");
        assert!(icon.is_file());
        assert!(
            tmp.path()
                .join("icons/hicolor/scalable/apps/arkor.svg")
                .is_file()
        );
        let before = std::fs::metadata(&icon).unwrap().modified().unwrap();
        install();
        assert_eq!(
            std::fs::metadata(&icon).unwrap().modified().unwrap(),
            before
        );
    }
}
