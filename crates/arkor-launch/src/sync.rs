//! Syncing with the user's other computers (`arkor_core::sync`) through the shared folder or
//! an rclone remote of the settings.

use anyhow::Context;
use arkor_core::Global;
use arkor_core::sync::{Report, machine_name, sync_folder};

/// Syncs now, if a place is set (`Ok(None)` when not). Blocking.
pub fn run() -> anyhow::Result<Option<Report>> {
    let global = Global::load_or_import()?;
    if let Some(folder) = &global.sync.folder {
        return sync_folder(folder).map(Some);
    }
    let Some(remote) = global
        .sync
        .rclone
        .as_deref()
        .filter(|r| !r.trim().is_empty())
    else {
        return Ok(None);
    };
    let rclone = crate::start::find_in_path("rclone")
        .context("rclone is not installed; install it, or sync through a shared folder")?;
    let cache = dirs::cache_dir()
        .context("no cache folder")?
        .join("arkor/sync-remote");
    std::fs::create_dir_all(&cache)?;
    let ok = |c: &mut std::process::Command| -> anyhow::Result<()> {
        let out = c.output().context("running rclone")?;
        anyhow::ensure!(
            out.status.success(),
            "rclone: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(())
    };
    let remote = remote.trim_end_matches('/');
    // The others' files down, this computer's up.
    ok(std::process::Command::new(&rclone)
        .args(["copy", remote])
        .arg(&cache)
        .args(["--include", "*.json"]))?;
    let report = sync_folder(&cache)?;
    let me = format!("{}.json", machine_name());
    ok(std::process::Command::new(&rclone)
        .arg("copyto")
        .arg(cache.join(&me))
        .arg(format!("{remote}/{me}")))?;
    Ok(Some(report))
}
