//! Short UI sounds from the freedesktop sound theme (when enabled in the settings).

#[derive(Debug, Clone, Copy)]
pub enum Sound {
    GameStart,
    GameExit,
    Done,
    Error,
}

impl Sound {
    fn theme_id(self) -> &'static str {
        match self {
            Sound::GameStart => "service-login",
            Sound::GameExit => "service-logout",
            Sound::Done => "complete",
            Sound::Error => "dialog-error",
        }
    }
}

/// Plays `sound` without waiting: libcanberra picks it from the desktop's sound theme; the
/// freedesktop file through PipeWire / PulseAudio is the fallback.
pub fn play(sound: Sound) {
    let id = sound.theme_id();
    std::thread::spawn(move || {
        let ok = std::process::Command::new("canberra-gtk-play")
            .args(["-i", id])
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            let file = format!("/usr/share/sounds/freedesktop/stereo/{id}.oga");
            for player in ["pw-play", "paplay"] {
                if std::process::Command::new(player)
                    .arg(&file)
                    .output()
                    .is_ok_and(|o| o.status.success())
                {
                    break;
                }
            }
        }
    });
}
