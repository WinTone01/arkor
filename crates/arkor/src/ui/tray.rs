//! System tray icon (StatusNotifierItem over D-Bus, as KDE/GNOME-with-extension show it):
//! open Arkor, start a library game directly, or quit. Runs on its own thread and talks to
//! the GPUI side through a channel.

use std::sync::mpsc::{Receiver, Sender, channel};

use ksni::blocking::TrayMethods;

pub enum TrayCmd {
    Show,
    Quit,
}

struct ArkorTray {
    tx: Sender<TrayCmd>,
    games: Vec<(String, String)>,
    open_label: String,
    play_label: String,
    quit_label: String,
}

impl ksni::Tray for ArkorTray {
    fn id(&self) -> String {
        "arkor".into()
    }

    fn title(&self) -> String {
        "Arkor".into()
    }

    fn icon_name(&self) -> String {
        super::app_icon::NAME.into()
    }

    /// The logo as pixels too, for trays that do not look the name up in the icon theme.
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [
            &include_bytes!("../../assets/logo/arkor-32.png")[..],
            &include_bytes!("../../assets/logo/arkor-64.png")[..],
        ]
        .into_iter()
        .filter_map(|png| {
            let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
                .ok()?
                .to_rgba8();
            let (width, height) = img.dimensions();
            // StatusNotifierItem wants ARGB32 in network byte order.
            let data = img
                .pixels()
                .flat_map(|p| [p[3], p[0], p[1], p[2]])
                .collect();
            Some(ksni::Icon {
                width: width as i32,
                height: height as i32,
                data,
            })
        })
        .collect()
    }

    fn icon_theme_path(&self) -> String {
        super::app_icon::theme_root()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayCmd::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let games: Vec<MenuItem<Self>> = self
            .games
            .iter()
            .map(|(id, name)| {
                let id = id.clone();
                StandardItem {
                    label: name.clone(),
                    activate: Box::new(move |_: &mut Self| super::game_page::launch_detached(&id)),
                    ..Default::default()
                }
                .into()
            })
            .collect();
        vec![
            StandardItem {
                label: self.open_label.clone(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(TrayCmd::Show);
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: self.play_label.clone(),
                enabled: !games.is_empty(),
                submenu: games,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: self.quit_label.clone(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(TrayCmd::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Starts the tray icon; `None` if no tray host is available (or D-Bus fails).
pub(super) fn start(games: Vec<(String, String)>, labels: [&str; 3]) -> Option<Receiver<TrayCmd>> {
    let (tx, rx) = channel();
    let tray = ArkorTray {
        tx,
        games,
        open_label: labels[0].into(),
        play_label: labels[1].into(),
        quit_label: labels[2].into(),
    };
    match tray.spawn() {
        Ok(handle) => {
            // Keep the service alive for the whole process.
            std::mem::forget(handle);
            Some(rx)
        }
        Err(e) => {
            eprintln!("arkor: no system tray: {e}");
            None
        }
    }
}

/// Writes or removes `~/.config/autostart/arkor.desktop` (starts Arkor in the tray).
pub fn set_autostart(enabled: bool) -> anyhow::Result<()> {
    use anyhow::Context;
    let path = dirs::config_dir()
        .context("no XDG config dir")?
        .join("autostart/arkor.desktop");
    if !enabled {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    std::fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nName=Arkor\nComment=Game launcher\nExec=\"{}\" --tray\nIcon=arkor\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        ),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn autostart_file_written_and_removed() {
        let _lock = crate::ui::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests that change XDG variables hold TEST_ENV_LOCK.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmp.path()) };
        super::set_autostart(true).unwrap();
        let file = tmp.path().join("autostart/arkor.desktop");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("--tray") && text.starts_with("[Desktop Entry]"));
        super::set_autostart(false).unwrap();
        assert!(!file.exists());
        super::set_autostart(false).unwrap();
    }
}
