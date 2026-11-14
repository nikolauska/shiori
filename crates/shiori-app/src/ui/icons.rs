//! Icons beyond gpui-component's default set, and the asset source that
//! serves them. The SVGs are Lucide icons copied into `assets/icons`
//! (ISC licence beside them), so the binary carries only the dozen it uses
//! instead of the whole catalog.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};
use gpui_component::IconNamed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppIcon {
    House,
    Clock,
    Bookmark,
    LayoutList,
    Table,
    CirclePlay,
    CircleCheck,
    CirclePause,
    CircleX,
    PanelRight,
    Laptop,
    Cloud,
    CloudUpload,
    Copy,
    FilePlus,
    LogIn,
    ArrowRight,
}

impl AppIcon {
    const FILES: [(&'static str, &'static [u8]); 17] = [
        (
            "app-icons/house.svg",
            include_bytes!("../../../../assets/icons/house.svg"),
        ),
        (
            "app-icons/clock.svg",
            include_bytes!("../../../../assets/icons/clock.svg"),
        ),
        (
            "app-icons/bookmark.svg",
            include_bytes!("../../../../assets/icons/bookmark.svg"),
        ),
        (
            "app-icons/layout-list.svg",
            include_bytes!("../../../../assets/icons/layout-list.svg"),
        ),
        (
            "app-icons/table.svg",
            include_bytes!("../../../../assets/icons/table.svg"),
        ),
        (
            "app-icons/circle-play.svg",
            include_bytes!("../../../../assets/icons/circle-play.svg"),
        ),
        (
            "app-icons/circle-check.svg",
            include_bytes!("../../../../assets/icons/circle-check.svg"),
        ),
        (
            "app-icons/circle-pause.svg",
            include_bytes!("../../../../assets/icons/circle-pause.svg"),
        ),
        (
            "app-icons/circle-x.svg",
            include_bytes!("../../../../assets/icons/circle-x.svg"),
        ),
        (
            "app-icons/panel-right.svg",
            include_bytes!("../../../../assets/icons/panel-right.svg"),
        ),
        (
            "app-icons/laptop.svg",
            include_bytes!("../../../../assets/icons/laptop.svg"),
        ),
        (
            "app-icons/cloud.svg",
            include_bytes!("../../../../assets/icons/cloud.svg"),
        ),
        (
            "app-icons/cloud-upload.svg",
            include_bytes!("../../../../assets/icons/cloud-upload.svg"),
        ),
        (
            "app-icons/copy.svg",
            include_bytes!("../../../../assets/icons/copy.svg"),
        ),
        (
            "app-icons/file-plus.svg",
            include_bytes!("../../../../assets/icons/file-plus.svg"),
        ),
        (
            "app-icons/log-in.svg",
            include_bytes!("../../../../assets/icons/log-in.svg"),
        ),
        (
            "app-icons/arrow-right.svg",
            include_bytes!("../../../../assets/icons/arrow-right.svg"),
        ),
    ];
}

impl IconNamed for AppIcon {
    fn path(self) -> SharedString {
        let index = self as usize;
        Self::FILES[index].0.into()
    }
}

/// gpui-kit's default icons plus Shiori's own.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = AppIcon::FILES.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut names = gpui_kit_assets::Assets.list(path)?;
        names.extend(
            AppIcon::FILES
                .iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
        Ok(names)
    }
}
