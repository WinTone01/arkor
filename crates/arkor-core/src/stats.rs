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

    /// Play time another launcher counted, for a game Arkor has none for yet.
    pub fn seed(id: &GameId, playtime_secs: u64, last_played: Option<u64>) -> anyhow::Result<()> {
        let mut stats = Self::load();
        let entry = stats.games.entry(id.to_string()).or_default();
        if entry.playtime_secs > 0 || playtime_secs == 0 {
            return Ok(());
        }
        entry.playtime_secs = playtime_secs;
        entry.last_played = entry.last_played.or(last_played);
        crate::profile::write_toml(&Self::path()?, &stats)
    }

    /// Adds one session (re-reading the file first, so parallel games don't lose time).
    pub fn record(id: &GameId, started: u64, ended: u64) -> anyhow::Result<()> {
        // Every session also goes to the history (for statistics and the play journal).
        if let Err(e) = append_session(&Session {
            game: id.to_string(),
            start: started,
            end: ended,
        }) {
            eprintln!("arkor: the session was not added to the history: {e:#}");
        }
        let mut stats = Self::load();
        let entry = stats.games.entry(id.to_string()).or_default();
        entry.playtime_secs += ended.saturating_sub(started);
        entry.last_played = Some(started);
        entry.launches += 1;
        crate::profile::write_toml(&Self::path()?, &stats)
    }
}

/// One time a game was played.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub game: String,
    pub start: u64,
    pub end: u64,
}

impl Session {
    pub fn secs(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }
}

/// The history of sessions: `~/.local/state/arkor/sessions.jsonl`, one per line.
pub fn sessions_path() -> anyhow::Result<PathBuf> {
    Ok(dirs::state_dir()
        .context("no XDG state dir")?
        .join("arkor/sessions.jsonl"))
}

fn append_session(s: &Session) -> anyhow::Result<()> {
    use std::io::Write;
    let path = sessions_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    // A short line in one write: parallel games do not mix their lines.
    let line = format!("{}\n", serde_json::to_string(s)?);
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// Every session played, oldest first.
pub fn sessions() -> Vec<Session> {
    let Ok(text) = sessions_path().and_then(|p| Ok(std::fs::read_to_string(p)?)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// A day of the calendar, in this computer's time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    pub day: u32,
    /// 0 = Monday … 6 = Sunday.
    pub weekday: u32,
    /// 0 to 23.
    pub hour: u32,
}

/// The local day (and hour) of a Unix time.
pub fn local_day(t: u64) -> Day {
    // SAFETY: localtime_r writes into the struct given; the time is a plain integer.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let time = t as libc::time_t;
    unsafe { libc::localtime_r(&time, &mut tm) };
    Day {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        weekday: ((tm.tm_wday + 6) % 7) as u32,
        hour: tm.tm_hour as u32,
    }
}

impl Day {
    /// Days since 1970-01-01 (for counting days in a row).
    pub fn ordinal(&self) -> i64 {
        let (y, m, d) = (self.year as i64, self.month as i64, self.day as i64);
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }
}

/// What a year (or all time) of play adds up to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub total: u64,
    pub sessions: usize,
    /// Play time per game, most first.
    pub per_game: Vec<(String, u64)>,
    pub per_month: [u64; 12],
    /// Monday first.
    pub per_weekday: [u64; 7],
    pub per_hour: [u64; 24],
    pub longest: Option<Session>,
    pub days_played: usize,
    /// The most days in a row with play.
    pub best_streak: usize,
    /// Games first played in the period.
    pub new_games: Vec<String>,
}

/// The years that have sessions, newest first.
pub fn years(sessions: &[Session]) -> Vec<i32> {
    let mut y: Vec<i32> = sessions.iter().map(|s| local_day(s.start).year).collect();
    y.sort_unstable_by(|a, b| b.cmp(a));
    y.dedup();
    y
}

/// Adds up `sessions` of `year` (all of them for `None`).
pub fn summarize(sessions: &[Session], year: Option<i32>) -> Summary {
    let mut out = Summary::default();
    let mut games: BTreeMap<String, u64> = BTreeMap::new();
    let mut days: Vec<i64> = Vec::new();
    let mut first: BTreeMap<&str, i32> = BTreeMap::new();
    for s in sessions {
        first
            .entry(s.game.as_str())
            .or_insert_with(|| local_day(s.start).year);
    }
    for s in sessions {
        let d = local_day(s.start);
        if year.is_some_and(|y| y != d.year) {
            continue;
        }
        let secs = s.secs();
        out.total += secs;
        out.sessions += 1;
        *games.entry(s.game.clone()).or_default() += secs;
        out.per_month[(d.month - 1) as usize] += secs;
        out.per_weekday[d.weekday as usize] += secs;
        out.per_hour[d.hour as usize] += secs;
        if out.longest.as_ref().is_none_or(|l| l.secs() < secs) {
            out.longest = Some(s.clone());
        }
        days.push(d.ordinal());
    }
    days.sort_unstable();
    days.dedup();
    out.days_played = days.len();
    let mut run = 0;
    for (i, d) in days.iter().enumerate() {
        run = if i > 0 && days[i - 1] + 1 == *d {
            run + 1
        } else {
            1
        };
        out.best_streak = out.best_streak.max(run);
    }
    out.per_game = games.into_iter().collect();
    out.per_game
        .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.new_games = out
        .per_game
        .iter()
        .filter(|(g, _)| year.is_none_or(|y| first.get(g.as_str()) == Some(&y)))
        .map(|(g, _)| g.clone())
        .collect();
    out
}

/// The play journal: the days with play, newest first (at most `days`), each with its
/// games and their time.
pub fn journal(sessions: &[Session], days: usize) -> Vec<(Day, Vec<(String, u64)>)> {
    let mut by_day: BTreeMap<(i32, u32, u32), (Day, BTreeMap<String, u64>)> = BTreeMap::new();
    for s in sessions {
        let d = local_day(s.start);
        let e = by_day
            .entry((d.year, d.month, d.day))
            .or_insert_with(|| (d, BTreeMap::new()));
        *e.1.entry(s.game.clone()).or_default() += s.secs();
    }
    by_day
        .into_values()
        .rev()
        .take(days)
        .map(|(d, games)| {
            let mut g: Vec<(String, u64)> = games.into_iter().collect();
            g.sort_by(|a, b| b.1.cmp(&a.1));
            (d, g)
        })
        .collect()
}

/// "2 h 15 min", "5 min", "<1 min".
pub fn format_duration(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    match (h, m) {
        (0, 0) => "<1 min".into(),
        (0, m) => format!("{m} min"),
        // From ten hours on the minutes add nothing (and don't fit under a cover).
        (h, _) if h >= 10 => format!("{h} h"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_years_streaks_and_journal() {
        // Noon UTC, so the local day is the same in every time zone people live in.
        let day = |y: i64, m: i64, d: i64| -> u64 {
            let t = Day {
                year: y as i32,
                month: m as u32,
                day: d as u32,
                weekday: 0,
                hour: 0,
            }
            .ordinal();
            (t * 86_400 + 12 * 3600) as u64
        };
        let s = |g: &str, t: u64, mins: u64| Session {
            game: g.into(),
            start: t,
            end: t + mins * 60,
        };
        let all = vec![
            s("local:a", day(2025, 12, 30), 60),
            s("local:a", day(2026, 1, 1), 30),
            s("local:b", day(2026, 1, 2), 120),
            s("local:a", day(2026, 1, 3), 10),
            s("local:b", day(2026, 3, 1), 5),
        ];
        assert_eq!(years(&all), [2026, 2025]);
        let y = summarize(&all, Some(2026));
        assert_eq!(y.sessions, 4);
        assert_eq!(y.total, (30 + 120 + 10 + 5) * 60);
        assert_eq!(y.per_game[0].0, "local:b");
        assert_eq!(y.best_streak, 3);
        assert_eq!(y.days_played, 4);
        assert_eq!(y.new_games, ["local:b"], "a was first played in 2025");
        assert_eq!(y.per_month[0], 160 * 60);
        assert_eq!(y.longest.unwrap().game, "local:b");
        let all_time = summarize(&all, None);
        assert_eq!(
            all_time.best_streak, 3,
            "1 to 3 January (31 December has no play)"
        );
        let j = journal(&all, 2);
        assert_eq!(j.len(), 2);
        assert_eq!((j[0].0.month, j[0].0.day), (3, 1));
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(30), "<1 min");
        assert_eq!(format_duration(5 * 60), "5 min");
        assert_eq!(format_duration(2 * 3600 + 15 * 60), "2 h 15 min");
        assert_eq!(format_duration(3600), "1 h");
        assert_eq!(format_duration(96 * 3600 + 20 * 60), "96 h");
    }
}
