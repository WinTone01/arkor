//! Play statistics: total play time, last played and launch count per game.
//! Stored as `~/.local/state/arkor/stats.toml`, written when a game exits.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::GameId;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameStats {
    pub playtime_secs: u64,
    /// Unix time of the last start.
    pub last_played: Option<u64>,
    pub launches: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub games: BTreeMap<String, GameStats>,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Stats {
    pub fn path() -> anyhow::Result<PathBuf> {
        Ok(dirs::state_dir()
            .context("no XDG state dir")?
            .join("arkor/stats.toml"))
    }

    pub fn load() -> Self {
        Self::path()
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn get(&self, id: &GameId) -> GameStats {
        self.games.get(&id.to_string()).copied().unwrap_or_default()
    }

    /// Adds one session (re-reading the file first, so parallel games don't lose time).
    pub fn record(id: &GameId, started: u64, ended: u64) -> anyhow::Result<()> {
        let mut stats = Self::load();
        let entry = stats.games.entry(id.to_string()).or_default();
        entry.playtime_secs += ended.saturating_sub(started);
        entry.last_played = Some(started);
        entry.launches += 1;
        crate::profile::write_toml(&Self::path()?, &stats)
    }
}

/// "2 h 15 min", "5 min", "<1 min".
pub fn format_duration(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    match (h, m) {
        (0, 0) => "<1 min".into(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(format_duration(30), "<1 min");
        assert_eq!(format_duration(5 * 60), "5 min");
        assert_eq!(format_duration(2 * 3600 + 15 * 60), "2 h 15 min");
        assert_eq!(format_duration(3600), "1 h");
    }
}
