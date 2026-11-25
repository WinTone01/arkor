//! Serves the Lucide icon catalog bundled with gpui-kit under `arkor/<name>.svg`, and
//! gpui-kit's own assets for everything else.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct ArkorAssets;

/// Arkor's mark (the arch and its core), one colour so it can be tinted.
pub const MARK: &str = "arkor-logo/mark.svg";

impl AssetSource for ArkorAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == MARK {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../assets/logo/arkor-mark.svg"
            ))));
        }
        // The stores' marks (Simple Icons, CC0), to tell the stores apart.
        let store: Option<&'static [u8]> = match path {
            "arkor/store-epic.svg" => Some(include_bytes!("../../assets/stores/epicgames.svg")),
            "arkor/store-gog.svg" => Some(include_bytes!("../../assets/stores/gogdotcom.svg")),
            "arkor/store-amazon.svg" => Some(include_bytes!("../../assets/stores/amazongames.svg")),
            "arkor/store-steam.svg" => Some(include_bytes!("../../assets/stores/steam.svg")),
            _ => None,
        };
        if let Some(bytes) = store {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(name) = path.strip_prefix("arkor/") {
            return gpui::assets::AllAssets.load(&format!("icons/{name}"));
        }
        gpui::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui::assets::Assets.list(path)
    }
}
