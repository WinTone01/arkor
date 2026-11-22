//! Discord "Playing <game>" activity through Discord's local IPC socket
//! (`$XDG_RUNTIME_DIR/discord-ipc-N`, also inside Flatpak/Snap Discord's runtime dirs).
//!
//! Frames are `u32 opcode (LE) + u32 length (LE) + JSON`. Opcode 0 is the handshake,
//! 1 a command. Any failure (Discord not running, wrong id) is silent: presence is a nicety.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub struct Presence {
    stream: Option<UnixStream>,
}

impl Presence {
    /// Connects and sets the activity; the activity clears when this is dropped.
    pub fn start(app_id: &str, game: &str, pid: u32) -> Self {
        let stream = connect(app_id).and_then(|mut s| {
            let start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            let payload = activity_payload(game, pid, start);
            write_frame(&mut s, 1, &payload).ok()?;
            read_frame(&mut s).ok()?;
            Some(s)
        });
        Self { stream }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        if let Some(s) = &mut self.stream {
            let clear = format!(
                r#"{{"cmd":"SET_ACTIVITY","args":{{"pid":{}}},"nonce":"arkor-clear"}}"#,
                std::process::id()
            );
            let _ = write_frame(s, 1, &clear);
        }
    }
}

fn sockets() -> Vec<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/tmp".into());
    let dirs = [
        runtime.clone(),
        runtime.join("app/com.discordapp.Discord"),
        runtime.join("app/dev.vencord.Vesktop"),
        runtime.join("snap.discord"),
    ];
    dirs.iter()
        .flat_map(|d| (0..10).map(move |i| d.join(format!("discord-ipc-{i}"))))
        .collect()
}

fn connect(app_id: &str) -> Option<UnixStream> {
    for path in sockets() {
        let Ok(mut s) = UnixStream::connect(&path) else {
            continue;
        };
        let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(3)));
        let hello = format!(r#"{{"v":1,"client_id":"{}"}}"#, json_escape(app_id));
        if write_frame(&mut s, 0, &hello).is_ok() && read_frame(&mut s).is_ok() {
            return Some(s);
        }
    }
    None
}

fn activity_payload(game: &str, pid: u32, start: u64) -> String {
    format!(
        r#"{{"cmd":"SET_ACTIVITY","args":{{"pid":{pid},"activity":{{"details":"{}","timestamps":{{"start":{start}}},"assets":{{}}}}}},"nonce":"arkor-{start}"}}"#,
        json_escape(game)
    )
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn write_frame(s: &mut UnixStream, op: u32, json: &str) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(8 + json.len());
    buf.extend_from_slice(&op.to_le_bytes());
    buf.extend_from_slice(&(json.len() as u32).to_le_bytes());
    buf.extend_from_slice(json.as_bytes());
    s.write_all(&buf)
}

fn read_frame(s: &mut UnixStream) -> std::io::Result<(u32, String)> {
    let mut head = [0u8; 8];
    s.read_exact(&mut head)?;
    let op = u32::from_le_bytes(head[..4].try_into().expect("4 bytes"));
    let len = u32::from_le_bytes(head[4..].try_into().expect("4 bytes")) as usize;
    let mut body = vec![0u8; len.min(1 << 20)];
    s.read_exact(&mut body)?;
    Ok((op, String::from_utf8_lossy(&body).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn handshake_and_activity_frames() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("discord-ipc-0");
        let listener = UnixListener::bind(&sock).unwrap();
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let (op, hello) = read_frame(&mut c).unwrap();
            write_frame(&mut c, 1, r#"{"evt":"READY"}"#).unwrap();
            let (op2, activity) = read_frame(&mut c).unwrap();
            write_frame(&mut c, 1, "{}").unwrap();
            let (_, clear) = read_frame(&mut c).unwrap();
            (op, hello, op2, activity, clear)
        });
        // SAFETY: only this test reads XDG_RUNTIME_DIR in this crate's test process.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
        drop(Presence::start("123", "Far \"Cry\" 6", 42));
        let (op, hello, op2, activity, clear) = server.join().unwrap();
        assert_eq!((op, op2), (0, 1));
        assert_eq!(hello, r#"{"v":1,"client_id":"123"}"#);
        assert!(
            activity.contains(r#""details":"Far \"Cry\" 6""#) && activity.contains(r#""pid":42"#),
            "{activity}"
        );
        assert!(clear.contains("SET_ACTIVITY") && !clear.contains("activity\":{"));
    }
}
