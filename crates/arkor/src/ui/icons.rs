//! Serves the Lucide icon catalog bundled with gpui-kit under `arkor/<name>.svg`, and
//! gpui-kit's own assets for everything else.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct ArkorAssets;

impl AssetSource for ArkorAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = path.strip_prefix("arkor/") {
            return gpui::assets::AllAssets.load(&format!("icons/{name}"));
        }
        gpui::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui::assets::Assets.list(path)
    }
}
