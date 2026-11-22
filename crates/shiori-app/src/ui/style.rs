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

/// The badge tint made opaque, for badges laid over a poster. The plain
/// tint lets the artwork show through and the text gets lost in it.
pub fn badge_on_image(cx: &App) -> Hsla {
    cx.theme().background.blend(accent_soft(cx))
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

/// The app icon for the tray: RGBA pixels and the side length. `badge` is
/// the color of a dot in the bottom right corner, if any.
pub fn tray_icon(badge: Option<u32>) -> Option<(Vec<u8>, u32)> {
    decode(include_bytes!("../../../../assets/icon-32.png")).map(|mut image| {
        let side = image.width();
        if let Some(color) = badge {
            draw_badge(&mut image, color);
        }
        (image.into_raw(), side)
    })
}

/// A filled dot over a quarter of the icon, with a dark ring so that it
/// stands out on both light and dark panels.
fn draw_badge(image: &mut image::RgbaImage, color: u32) {
    let side = image.width() as f32;
    let radius = side * 0.24;
    let ring = radius + side * 0.06;
    let (cx, cy) = (side - ring, side - ring);
    let [_, r, g, b] = color.to_be_bytes();
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
        if distance <= radius {
            *pixel = image::Rgba([r, g, b, 0xff]);
        } else if distance <= ring {
            *pixel = image::Rgba([0x1f, 0x1f, 0x23, 0xff]);
        }
    }
}

/// Badge colors: green while the playing episode is recognized, amber
/// while something plays that Shiori could not recognize.
pub const BADGE_IDENTIFIED: u32 = 0x22c55e;
pub const BADGE_UNIDENTIFIED: u32 = 0xf59e0b;

fn decode(png: &[u8]) -> Option<image::RgbaImage> {
    match image::load_from_memory_with_format(png, image::ImageFormat::Png) {
        Ok(image) => Some(image.into_rgba8()),
        Err(error) => {
            tracing::error!("Could not decode the app icon: {error}");
            None
        }
    }
}
