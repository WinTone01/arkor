//! Installing third-party game launchers into their own prefix (like Faugus): download the
//! official installer, run it through umu, then add the launcher to the library.

use std::path::PathBuf;

use anyhow::Context;

pub struct ThirdParty {
    pub id: &'static str,
    pub name: &'static str,
    pub installer_url: &'static str,
    pub installer_file: &'static str,
    /// Launcher executable inside the prefix once installed.
    pub exe: &'static str,
}

pub const LAUNCHERS: &[ThirdParty] = &[
    ThirdParty {
        id: "battle",
        name: "Battle.net",
        installer_url: "https://downloader.battle.net/download/getInstaller?os=win&installer=Battle.net-Setup.exe",
        installer_file: "Battle.net-Setup.exe",
        exe: "drive_c/Program Files (x86)/Battle.net/Battle.net.exe",
    },
    ThirdParty {
        id: "ea",
        name: "EA app",
        installer_url: "https://origin-a.akamaihd.net/EA-Desktop-Client-Download/installer-releases/EAappInstaller.exe",
        installer_file: "EAappInstaller.exe",
        exe: crate::ea_fix::EA_LAUNCHER,
    },
    ThirdParty {
        id: "epic",
        name: "Epic Games Launcher",
        installer_url: "https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/installer/download/EpicGamesLauncherInstaller.msi",
        installer_file: "EpicGamesLauncherInstaller.msi",
        exe: "drive_c/Program Files/Epic Games/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe",
    },
    ThirdParty {
        id: "gog",
        name: "GOG Galaxy",
        installer_url: "https://webinstallers.gog-statics.com/download/GOG_Galaxy_2.0.exe",
        installer_file: "GOG_Galaxy_2.0.exe",
        exe: "drive_c/Program Files (x86)/GOG Galaxy/GalaxyClient.exe",
    },
    ThirdParty {
        id: "ubisoft",
        name: "Ubisoft Connect",
        installer_url: "https://static3.cdn.ubi.com/orbit/launcher_installer/UbisoftConnectInstaller.exe",
        installer_file: "UbisoftConnectInstaller.exe",
        exe: "drive_c/Program Files (x86)/Ubisoft/Ubisoft Game Launcher/UbisoftConnect.exe",
    },
    ThirdParty {
        id: "rockstar",
        name: "Rockstar Games Launcher",
        installer_url: "https://gamedownloads.rockstargames.com/public/installer/Rockstar-Games-Launcher.exe",
        installer_file: "Rockstar-Games-Launcher.exe",
        exe: "drive_c/Program Files/Rockstar Games/Launcher/Launcher.exe",
    },
    ThirdParty {
        id: "amazon",
        name: "Amazon Games",
        installer_url: "https://download.amazongames.com/AmazonGamesSetup.exe",
        installer_file: "AmazonGamesSetup.exe",
        exe: "drive_c/users/steamuser/AppData/Local/Amazon Games/App/Amazon Games.exe",
    },
    ThirdParty {
        id: "wargaming",
        name: "Wargaming Game Center",
        installer_url: "https://redirect.wargaming.net/WGC/Wargaming_Game_Center_Install_NA.exe",
        installer_file: "Wargaming_Game_Center_Install_NA.exe",
        exe: "drive_c/ProgramData/Wargaming.net/GameCenter/wgc.exe",
    },
];

pub fn find(id: &str) -> Option<&'static ThirdParty> {
    LAUNCHERS.iter().find(|l| l.id == id)
}

/// Downloads the installer (cached) and returns its path. Blocking.
pub fn fetch_installer(launcher: &ThirdParty) -> anyhow::Result<PathBuf> {
    let path = dirs::cache_dir()
        .context("no cache dir")?
        .join("arkor/installers")
        .join(launcher.installer_file);
    if !path.is_file() {
        crate::tools::download(launcher.installer_url, &path)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_unique_and_paths_inside_drive_c() {
        let mut ids: Vec<_> = LAUNCHERS.iter().map(|l| l.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), LAUNCHERS.len());
        assert!(
            LAUNCHERS
                .iter()
                .all(|l| l.exe.starts_with("drive_c/") && l.installer_url.starts_with("https://"))
        );
    }
}
