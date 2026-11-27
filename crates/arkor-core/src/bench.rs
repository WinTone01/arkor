//! MangoHud benchmark logs (CSV written when logging is toggled in game): parsing and the
//! numbers people compare runs by (average, 1% / 0.1% lows, frame-time spread).

use std::path::{Path, PathBuf};

use anyhow::Context;

/// Default folder MangoHud writes logs to when the HUD has no `output_folder`.
pub fn default_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("arkor/benchmarks"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub path: PathBuf,
    /// `os`, `cpu`, `gpu`, … from the log's system header.
    pub specs: Vec<(String, String)>,
    /// Frame times in milliseconds, in order.
    pub frametimes: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub avg_fps: f64,
    pub low_1: f64,
    pub low_01: f64,
    pub min_fps: f64,
    pub max_fps: f64,
    pub avg_frametime: f64,
    /// Standard deviation of frame times (ms): how uneven the pacing was.
    pub frametime_sd: f64,
    pub seconds: f64,
    pub frames: usize,
}

/// Parses a MangoHud log. Columns are found by name, so extra version headers or a
/// different column order don't matter.
pub fn parse(path: &Path) -> anyhow::Result<Run> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_text(path, &text)
}

fn parse_text(path: &Path, text: &str) -> anyhow::Result<Run> {
    let lines: Vec<&str> = text.lines().collect();
    let mut specs = Vec::new();
    if let Some(i) = lines.iter().position(|l| l.starts_with("os,"))
        && let Some(values) = lines.get(i + 1)
    {
        specs = lines[i]
            .split(',')
            .zip(values.split(','))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
    }
    let header = lines
        .iter()
        .position(|l| l.split(',').any(|c| c.trim() == "frametime"))
        .context("not a MangoHud log (no frametime column)")?;
    let columns: Vec<&str> = lines[header].split(',').map(str::trim).collect();
    let ft = columns
        .iter()
        .position(|c| *c == "frametime")
        .expect("found above");
    let frametimes: Vec<f64> = lines[header + 1..]
        .iter()
        .filter_map(|l| l.split(',').nth(ft)?.trim().parse::<f64>().ok())
        .filter(|v| *v > 0.0 && v.is_finite())
        .collect();
    anyhow::ensure!(!frametimes.is_empty(), "the log has no frames");
    Ok(Run {
        path: path.to_path_buf(),
        specs,
        frametimes,
    })
}

impl Run {
    pub fn name(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn summary(&self) -> Summary {
        let ft = &self.frametimes;
        let n = ft.len();
        let total_ms: f64 = ft.iter().sum();
        let avg_ft = total_ms / n as f64;
        let mut sorted = ft.clone();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)); // slowest first
        // "1% low": the average FPS of the slowest 1% of frames (at least one frame).
        let low = |fraction: f64| {
            let k = ((n as f64 * fraction).ceil() as usize).clamp(1, n);
            let worst: f64 = sorted[..k].iter().sum::<f64>() / k as f64;
            1000.0 / worst
        };
        let var = ft.iter().map(|v| (v - avg_ft).powi(2)).sum::<f64>() / n as f64;
        Summary {
            avg_fps: n as f64 / (total_ms / 1000.0),
            low_1: low(0.01),
            low_01: low(0.001),
            min_fps: 1000.0 / sorted[0],
            max_fps: 1000.0 / sorted[n - 1],
            avg_frametime: avg_ft,
            frametime_sd: var.sqrt(),
            seconds: total_ms / 1000.0,
            frames: n,
        }
    }

    /// Frame times reduced to `buckets` points (each the worst frame in its slice), for charts.
    pub fn downsample(&self, buckets: usize) -> Vec<f64> {
        let n = self.frametimes.len();
        if n <= buckets {
            return self.frametimes.clone();
        }
        (0..buckets)
            .map(|b| {
                let (start, end) = (
                    b * n / buckets,
                    ((b + 1) * n / buckets).max(b * n / buckets + 1),
                );
                self.frametimes[start..end]
                    .iter()
                    .cloned()
                    .fold(0.0, f64::max)
            })
            .collect()
    }
}

/// Log files in a folder, newest first (summary files skipped).
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "csv")
                && !p.to_string_lossy().ends_with("_summary.csv")
        })
        .map(|p| {
            (
                std::fs::metadata(&p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH),
                p,
            )
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().map(|(_, p)| p).collect()
}

/// Gives a log another name (its `_summary.csv` too); the new path. Blocking.
pub fn rename(path: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let name = name.trim().trim_end_matches(".csv").trim();
    anyhow::ensure!(
        !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.'),
        "`{name}` cannot be a file name"
    );
    let dir = path.parent().context("the log has no folder")?;
    let to = dir.join(format!("{name}.csv"));
    if to == path {
        return Ok(to);
    }
    anyhow::ensure!(!to.exists(), "a log named `{name}` already exists");
    std::fs::rename(path, &to).with_context(|| format!("renaming {}", path.display()))?;
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        let summary = dir.join(format!("{stem}_summary.csv"));
        if summary.is_file() {
            let _ = std::fs::rename(summary, dir.join(format!("{name}_summary.csv")));
        }
    }
    Ok(to)
}

/// Deletes a log (and its `_summary.csv`). Blocking.
pub fn remove(path: &Path) -> anyhow::Result<()> {
    std::fs::remove_file(path).with_context(|| format!("deleting {}", path.display()))?;
    if let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) {
        let _ = std::fs::remove_file(dir.join(format!("{stem}_summary.csv")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(frametimes: &[f64]) -> String {
        let mut s = String::from(
            "v1\nmangohud 0.8.4\nos,cpu,gpu,ram,kernel,driver,cpuscheduler\nCachyOS,i5-12400F,RTX 3060,16GB,7.3,620.73,\n",
        );
        s.push_str("fps,frametime,cpu_load,gpu_load,elapsed\n");
        for (i, f) in frametimes.iter().enumerate() {
            s.push_str(&format!("{:.1},{f},10,90,{i}\n", 1000.0 / f));
        }
        s
    }

    #[test]
    fn lows_and_average() {
        // 99 smooth frames at 10 ms and one 50 ms hitch.
        let mut ft = vec![10.0; 99];
        ft.push(50.0);
        let run = parse_text(Path::new("/b/game_2026.csv"), &log(&ft)).unwrap();
        assert_eq!(run.specs[1], ("cpu".into(), "i5-12400F".into()));
        let s = run.summary();
        assert_eq!(s.frames, 100);
        assert!((s.avg_fps - 100.0 / 1.04).abs() < 1e-9);
        assert!(
            (s.low_1 - 20.0).abs() < 1e-9,
            "the single 50 ms frame is the worst 1%"
        );
        assert!((s.min_fps - 20.0).abs() < 1e-9 && (s.max_fps - 100.0).abs() < 1e-9);
        assert!(s.frametime_sd > 3.9 && s.frametime_sd < 4.0);
        assert_eq!(run.name(), "game_2026");
        assert_eq!(run.downsample(10).len(), 10);
        assert_eq!(run.downsample(10)[9], 50.0, "downsampling keeps spikes");
    }

    #[test]
    fn renames_and_removes_with_the_summary() {
        let dir = std::env::temp_dir().join(format!("arkor-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("Game_2026-11-27_20-00-00.csv");
        std::fs::write(&log, "x").unwrap();
        std::fs::write(dir.join("Game_2026-11-27_20-00-00_summary.csv"), "y").unwrap();
        std::fs::write(dir.join("taken.csv"), "z").unwrap();
        assert!(rename(&log, "a/b").is_err());
        assert!(rename(&log, "taken").is_err());
        let to = rename(&log, "With OptiScaler.csv").unwrap();
        assert_eq!(to, dir.join("With OptiScaler.csv"));
        assert!(dir.join("With OptiScaler_summary.csv").is_file());
        assert_eq!(list(&dir).len(), 2);
        remove(&to).unwrap();
        assert!(!dir.join("With OptiScaler_summary.csv").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_other_csv() {
        assert!(parse_text(Path::new("/x.csv"), "a,b\n1,2\n").is_err());
        assert!(parse_text(Path::new("/x.csv"), "fps,frametime\n").is_err());
    }
}
