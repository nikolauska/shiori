//! The one accent Shiori adds to gpui-component's neutral theme: the app
//! icon's cyan → blue → violet gradient (decision I22). Everything else uses
//! the theme's own colors so light and dark both stay readable.

use std::sync::Arc;

use gpui::{App, Background, Hsla, Image, ImageFormat, linear_color_stop, linear_gradient, rgb};
use gpui_component::ActiveTheme as _;

const CYAN: u32 = 0x4fd8f5;
const VIOLET: u32 = 0xa78bfa;

/// The gradient, left to right: progress bars and badges.
pub fn gradient() -> Background {
    linear_gradient(
        90.,
        linear_color_stop(rgb(CYAN), 0.),
        linear_color_stop(rgb(VIOLET), 1.),
    )
}

/// A single color from the middle of the gradient, for edges, tabs and text,
/// where a gradient cannot be drawn. Lighter on dark backgrounds.
pub fn accent(cx: &App) -> Hsla {
    if cx.theme().mode.is_dark() {
        rgb(0xa5b4fc).into()
    } else {
        rgb(0x6d5dfc).into()
    }
}

/// The accent behind badges, faint enough for text on it to stay readable.
pub fn accent_soft(cx: &App) -> Hsla {
    accent(cx).opacity(if cx.theme().mode.is_dark() {
        0.22
    } else {
        0.12
    })
}

/// The app icon at 64 px, for the sidebar logo.
pub fn logo() -> Arc<Image> {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../../assets/icon-64.png").to_vec(),
    ))
}

/// The app icon for the window, decoded once at start.
pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
    decode(include_bytes!("../../../../assets/icon-64.png")).map(Arc::new)
}

/// The app icon for the tray: RGBA pixels and the side length.
pub fn tray_icon() -> Option<(Vec<u8>, u32)> {
    decode(include_bytes!("../../../../assets/icon-32.png")).map(|image| {
        let side = image.width();
        (image.into_raw(), side)
    })
}

fn decode(png: &[u8]) -> Option<image::RgbaImage> {
    match image::load_from_memory_with_format(png, image::ImageFormat::Png) {
        Ok(image) => Some(image.into_rgba8()),
        Err(error) => {
            tracing::error!("Could not decode the app icon: {error}");
            None
        }
    }
}
