//! Unpacking zip files (RetroArch's cores, some emulators' downloads) without an outside
//! program, so it works whatever the distribution has installed. Stored and deflated
//! entries only, which is what these downloads use; names that would land outside the
//! folder are refused.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};

/// The end of the central directory: where it is and how many entries it has.
fn end_record(file: &mut File) -> anyhow::Result<(u64, u64)> {
    let len = file.metadata()?.len();
    let tail = len.min(66 * 1024);
    file.seek(SeekFrom::Start(len - tail))?;
    let mut buf = vec![0; tail as usize];
    file.read_exact(&mut buf)?;
    let at = (0..buf.len().saturating_sub(21))
        .rev()
        .find(|&i| buf[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .context("not a zip file")?;
    let r = &buf[at..];
    let entries = u16::from_le_bytes([r[10], r[11]]) as u64;
    let offset = u32::from_le_bytes([r[16], r[17], r[18], r[19]]) as u64;
    anyhow::ensure!(
        offset != 0xffff_ffff && entries != 0xffff,
        "zip64 files are not supported"
    );
    Ok((offset, entries))
}

/// A safe path for `name` inside `dest`.
fn inside(dest: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let rel = Path::new(name);
    if rel
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("the archive holds a file outside its folder: {name}");
    }
    Ok(dest.join(rel))
}

/// Unpacks the zip at `path` into `dest`. Blocking.
pub fn unpack(path: &Path, dest: &Path) -> anyhow::Result<()> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let (mut at, entries) = end_record(&mut file)?;
    std::fs::create_dir_all(dest)?;
    for _ in 0..entries {
        file.seek(SeekFrom::Start(at))?;
        let mut h = [0u8; 46];
        file.read_exact(&mut h)?;
        anyhow::ensure!(h[..4] == [0x50, 0x4b, 0x01, 0x02], "damaged zip file");
        let u16_at = |i: usize| u16::from_le_bytes([h[i], h[i + 1]]) as u64;
        let u32_at = |i: usize| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]) as u64;
        let method = u16_at(10);
        let packed = u32_at(20);
        let (name_len, extra_len, comment_len) = (u16_at(28), u16_at(30), u16_at(32));
        let unix_mode = (u32_at(38) >> 16) as u32;
        let local = u32_at(42);
        let mut name = vec![0; name_len as usize];
        file.read_exact(&mut name)?;
        let name = String::from_utf8_lossy(&name).into_owned();
        at += 46 + name_len + extra_len + comment_len;

        let target = inside(dest, &name)?;
        if name.ends_with('/') {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        file.seek(SeekFrom::Start(local))?;
        let mut lh = [0u8; 30];
        file.read_exact(&mut lh)?;
        anyhow::ensure!(lh[..4] == [0x50, 0x4b, 0x03, 0x04], "damaged zip file");
        let skip = u16::from_le_bytes([lh[26], lh[27]]) as u64
            + u16::from_le_bytes([lh[28], lh[29]]) as u64;
        file.seek(SeekFrom::Current(skip as i64))?;
        let data = (&mut file).take(packed);
        let mut out =
            File::create(&target).with_context(|| format!("writing {}", target.display()))?;
        match method {
            0 => {
                std::io::copy(&mut { data }, &mut out)?;
            }
            8 => {
                std::io::copy(&mut flate2::read::DeflateDecoder::new(data), &mut out)?;
            }
            m => bail!("{name}: compression {m} is not supported"),
        }
        let is_link = unix_mode & 0o170000 == 0o120000;
        if is_link {
            // A symbolic link's target is its content.
            let to = std::fs::read_to_string(&target)?;
            std::fs::remove_file(&target)?;
            // Only links that stay beside themselves (later entries could be written
            // through one that points elsewhere).
            if inside(dest, &to).is_ok() {
                std::os::unix::fs::symlink(to, &target)?;
            }
        } else if unix_mode & 0o111 != 0 {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_a_zip_made_by_python_or_refuses_it() {
        // A zip with one deflated file (`a/b.txt` = "hello hello hello"), built by hand.
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("t.zip");
        let made = std::process::Command::new("python3")
            .args([
                "-I",
                "-c",
                "import zipfile,sys\nz=zipfile.ZipFile(sys.argv[1],'w',zipfile.ZIP_DEFLATED)\nz.writestr('a/b.txt','hello hello hello')\nz.writestr('c.txt','x')\nz.close()",
            ])
            .arg(&zip)
            .status();
        if !made.is_ok_and(|s| s.success()) {
            return;
        }
        let out = dir.path().join("out");
        unpack(&zip, &out).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("a/b.txt")).unwrap(),
            "hello hello hello"
        );
        assert_eq!(std::fs::read_to_string(out.join("c.txt")).unwrap(), "x");
        assert!(inside(&out, "../evil").is_err());
        assert!(inside(&out, "/etc/passwd").is_err());
        std::fs::write(dir.path().join("bad.zip"), b"nope").unwrap();
        assert!(unpack(&dir.path().join("bad.zip"), &out).is_err());
    }
}
