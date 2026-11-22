//! What to execute for games Arkor starts directly (everything except Steam and Epic, whose
//! clients start them): an executable, optionally inside a Wine prefix.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct ExeTarget {
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    /// Linux build: run directly instead of through umu/Proton.
    pub native: bool,
    /// Wine prefix the source launcher uses for this game, if it has one.
    pub prefix: Option<PathBuf>,
    /// Proton build the source launcher uses for this game, if known.
    pub proton: Option<PathBuf>,
}

impl ExeTarget {
    /// A Windows executable that runs from its own folder, no launcher-specific settings.
    pub fn windows(exe: PathBuf) -> Self {
        let working_dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        Self {
            exe,
            args: Vec::new(),
            working_dir,
            native: false,
            prefix: None,
            proton: None,
        }
    }

    /// Windows shortcuts and scripts that must be opened through Wine's `start`
    /// (e.g. Ubisoft's `.url` files pointing at `uplay://launch/...`).
    pub fn needs_start(&self) -> bool {
        self.exe
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| {
                ["lnk", "url", "bat", "cmd"].contains(&e.to_ascii_lowercase().as_str())
            })
    }
}

/// Directories where Proton builds are installed (system packages first).
pub fn compat_tool_roots() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![
        PathBuf::from("/usr/share/steam/compatibilitytools.d"),
        home.join(".local/share/Steam/compatibilitytools.d"),
        home.join(".steam/root/compatibilitytools.d"),
        home.join(".config/heroic/tools/proton"),
    ]
}

/// A Proton build directory by its folder name (e.g. `GE-Proton10-25`), if installed.
pub fn proton_by_name(name: &str) -> Option<PathBuf> {
    compat_tool_roots()
        .into_iter()
        .map(|root| root.join(name))
        .find(|dir| dir.join("proton").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_is_needed_for_shortcuts() {
        assert!(ExeTarget::windows("/p/Far Cry 5.URL".into()).needs_start());
        assert!(ExeTarget::windows("/p/Ubisoft Connect.lnk".into()).needs_start());
        assert!(!ExeTarget::windows("/p/game.exe".into()).needs_start());
        assert_eq!(
            ExeTarget::windows("/p/bin/game.exe".into()).working_dir,
            PathBuf::from("/p/bin")
        );
    }
}
