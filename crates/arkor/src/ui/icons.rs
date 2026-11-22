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
        if let Some(name) = path.strip_prefix("arkor/") {
            return gpui::assets::AllAssets.load(&format!("icons/{name}"));
        }
        gpui::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui::assets::Assets.list(path)
    }
}
