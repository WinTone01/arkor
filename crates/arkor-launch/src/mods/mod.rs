//! Mods, through Amethyst Mod Manager running as a headless service: Arkor's own screens on
//! top, Amethyst's knowledge of each game (where mods go, its installers, its loaders' DLL
//! overrides) underneath. Mods are placed into the game folder as links (`deploy`) when
//! they change, so the game itself still starts the way Arkor starts it.

pub mod amethyst;
pub mod rpc;
