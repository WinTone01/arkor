//! Emulators the user adds (ported from Irori's `custom`): a name, the program, the
//! argument line it takes and the systems it plays. Each is a manifest of its own in
//! `~/.config/arkor/emulators/<id>.json`, its program recorded in the emulators' state.
//! Choosing, launching and opening then treat it like every other emulator: the program
//! is started directly with the expanded arguments, never through a shell.

use std::path::PathBuf;

use anyhow::{Context, bail, ensure};

use super::manifest::{Arg, EngineKind, EngineManifest, LaunchTemplate};

/// Ids of the user's emulators start with this, so they never take one Arkor ships.
pub const ID_PREFIX: &str = "custom-";

/// Placeholders an argument line may use.
pub const PLACEHOLDERS: &[&str] = &[
    "rom",
    "rom_dir",
    "home",
    "engine_home",
    "engine_dir",
    "xdg_data",
];

const NAME_MAX: usize = 80;

/// What the user enters for an emulator of their own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomEngine {
    pub name: String,
    pub program: PathBuf,
    /// The arguments as typed. `{rom}` stands for the game; without it the game goes last.
    pub args: String,
    /// System ids.
    pub systems: Vec<String>,
}

/// Splits an argument line, refusing placeholders Arkor does not fill in.
pub fn parse_args(line: &str) -> anyhow::Result<Vec<String>> {
    let args = arkor_core::profile::shell_words(line);
    for arg in &args {
        let mut rest = arg.as_str();
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let close = after.find('}').with_context(|| {
                format!("\"{arg}\" opens a placeholder with {{ but never closes it")
            })?;
            let name = &after[..close];
            ensure!(
                PLACEHOLDERS.contains(&name),
                "{{{name}}} is not a placeholder Arkor fills in; use {}",
                PLACEHOLDERS
                    .iter()
                    .map(|p| format!("{{{p}}}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            rest = &after[close + 1..];
        }
    }
    Ok(args)
}

/// A new id for an emulator named `name`, one `taken` does not hold.
pub fn new_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
        if slug.len() >= 32 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    let base = format!(
        "{ID_PREFIX}{}",
        if slug.is_empty() { "emulator" } else { slug }
    );
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken(id))
        .expect("some suffix is free")
}

/// The manifest of the emulator `id` the user described.
pub fn manifest(id: &str, def: &CustomEngine) -> anyhow::Result<EngineManifest> {
    let name = def.name.trim();
    ensure!(!name.is_empty(), "Give the emulator a name");
    let name: String = name.chars().take(NAME_MAX).collect();
    let mut systems: Vec<String> = Vec::new();
    for s in &def.systems {
        ensure!(
            super::systems::get(s).is_some(),
            "Arkor does not know the system {s}"
        );
        if !systems.contains(s) {
            systems.push(s.clone());
        }
    }
    ensure!(!systems.is_empty(), "Choose at least one system");
    let args = parse_args(&def.args)?;
    let manifest = EngineManifest {
        id: id.to_string(),
        name,
        license: String::new(),
        homepage: String::new(),
        kind: EngineKind::Standalone,
        systems,
        sources: Vec::new(),
        binaries: Default::default(),
        launch: LaunchTemplate {
            args: args.into_iter().map(Arg::Plain).collect(),
            env: Default::default(),
        },
        options: Vec::new(),
        bios_dir: Default::default(),
        portable_marker: Default::default(),
        exit_hotkey: String::new(),
        experimental: false,
        cpu_features: Vec::new(),
        cores: Vec::new(),
        custom: true,
    };
    manifest.validate()?;
    Ok(manifest)
}

fn file(id: &str) -> PathBuf {
    super::dirs::custom().join(format!("{id}.json"))
}

/// The user's emulators, as saved.
pub fn load() -> Vec<EngineManifest> {
    let Ok(entries) = std::fs::read_dir(super::dirs::custom()) else {
        return Vec::new();
    };
    let mut out: Vec<EngineManifest> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let m = EngineManifest::parse(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            (m.custom && m.id.starts_with(ID_PREFIX)).then_some(m)
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Saves the emulator the user described: a new one when `id` is `None`, else the custom
/// emulator `id`; its id. Blocking.
pub fn save(id: Option<&str>, def: &CustomEngine) -> anyhow::Result<String> {
    ensure!(
        def.program.is_file(),
        "{} does not exist",
        def.program.display()
    );
    let id = match id {
        Some(id) => {
            ensure!(
                super::engine(id).is_some_and(|m| m.custom),
                "{id} is not an emulator you added"
            );
            id.to_string()
        }
        None => new_id(&def.name, |id| {
            super::engine(id).is_some() || file(id).exists()
        }),
    };
    let manifest = manifest(&id, def)?;
    super::write_json(&file(&id), &manifest)
        .with_context(|| format!("saving the emulator {}", manifest.name))?;
    let program = def.program.clone();
    super::discovery::update_state(|s| s.custom.insert(id.clone(), program))?;
    super::reload();
    Ok(id)
}

/// Forgets the user's emulator `id` (the program itself stays where it is).
pub fn remove(id: &str) -> anyhow::Result<()> {
    let Some(manifest) = super::engine(id) else {
        bail!("{id} is not an emulator Arkor knows");
    };
    ensure!(
        manifest.custom,
        "{} is not an emulator you added",
        manifest.name
    );
    manifest.validate()?;
    match std::fs::remove_file(file(id)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(e).with_context(|| format!("removing the emulator {}", manifest.name));
        }
        _ => {}
    }
    super::discovery::update_state(|s| s.custom.remove(id))?;
    super::reload();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_users_emulator_is_saved_chosen_and_launched() {
        let root = super::super::test_root("custom");
        let program = root.join("my-emu");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        let def = CustomEngine {
            name: "My Emu!".into(),
            program: program.clone(),
            args: "--full {rom}".into(),
            systems: vec!["snes".into()],
        };
        let id = save(None, &def).unwrap();
        assert_eq!(id, "custom-my-emu");
        assert!(
            super::super::choices_for("snes")
                .iter()
                .any(|c| c.engine == id)
        );
        let rom = root.join("Game.sfc");
        std::fs::write(&rom, b"x").unwrap();
        let mut settings = arkor_core::profile::Emulators::default();
        settings.defaults.insert("snes".into(), id.clone());
        let c = super::super::launch::command(&rom, "snes", None, &settings).unwrap();
        assert_eq!(
            c.argv,
            [
                program.display().to_string(),
                "--full".into(),
                rom.display().to_string()
            ]
        );
        remove(&id).unwrap();
        assert!(super::super::engine(&id).is_none());

        assert!(parse_args("{nope}").is_err());
        assert!(parse_args("-a {rom_dir}").is_ok());
        let e = manifest(
            "custom-x",
            &CustomEngine {
                name: "X".into(),
                ..def
            },
        )
        .map(|_| ());
        assert!(e.is_ok());
    }
}
