//! Arkor core: game model, per-game profiles, global settings and on-disk paths.
//! Contains no UI or store-specific code.

pub mod backup;
pub mod bench;
pub mod fs_guard;
pub mod game;
pub mod layers;
pub mod library;
pub mod mangohud;
pub mod paths;
pub mod profile;
pub mod stats;
pub mod tweaks;
pub mod vkbasalt;

pub use game::{Game, GameId, Store};
pub use profile::{Global, HudMode, Language, Lsfg, LsfgMode, Profile};
