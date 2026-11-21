//! Arkor core: game model, per-game profiles, global settings and on-disk paths.
//! Contains no UI or store-specific code.

pub mod game;
pub mod mangohud;
pub mod paths;
pub mod profile;

pub use game::{Game, GameId, Store};
pub use profile::{Global, HudMode, Language, Profile};
