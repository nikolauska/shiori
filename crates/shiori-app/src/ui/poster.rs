//! Posters in memory, and the element that shows one.
//!
//! Files on disk follow the rules in `shiori_app::posters`. Read images are
//! kept here, the most recently used first, bounded by their total size so
//! that scrolling through a large list does not grow memory without end.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use gpui::{
    AnyElement, App, Entity, Image, ImageFormat, IntoElement, ObjectFit, Styled, div, img,
    prelude::*,
};
use gpui_component::ActiveTheme as _;
use shiori_core::AnimeId;

use super::model::Model;

/// About 400 posters of a typical size.
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub struct Posters {
    images: HashMap<AnimeId, Arc<Image>>,
    /// Least recently added first.
    order: VecDeque<AnimeId>,
    bytes: usize,
    /// Being read or downloaded.
    pub pending: HashSet<AnimeId>,
    /// Could not be read or downloaded in this run; not tried again until
    /// a refresh, so a missing poster does not cause a request every frame.
    pub failed: HashSet<AnimeId>,
}

impl Posters {
    pub fn get(&self, id: AnimeId) -> Option<Arc<Image>> {
        self.images.get(&id).cloned()
    }

    pub fn insert(&mut self, id: AnimeId, bytes: Vec<u8>) {
        let Some(format) = format(&bytes) else {
            self.failed.insert(id);
            return;
        };
        self.remove(id);
        self.bytes += bytes.len();
        self.images
            .insert(id, Arc::new(Image::from_bytes(format, bytes)));
        self.order.push_back(id);
        while self.bytes > MEMORY_LIMIT {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(image) = self.images.remove(&oldest) {
                self.bytes -= image.bytes().len();
            }
        }
    }

    pub fn remove(&mut self, id: AnimeId) {
        if let Some(image) = self.images.remove(&id) {
            self.bytes -= image.bytes().len();
            self.order.retain(|i| *i != id);
        }
        self.failed.remove(&id);
    }
}

fn format(bytes: &[u8]) -> Option<ImageFormat> {
    Some(match shiori_app::posters::extension(bytes)? {
        "jpg" => ImageFormat::Jpeg,
        "png" => ImageFormat::Png,
        "webp" => ImageFormat::Webp,
        _ => ImageFormat::Gif,
    })
}

/// The poster of an anime, filling its container. Starts loading it when it
/// is not in memory yet.
pub fn view(model: &Entity<Model>, id: AnimeId, cx: &mut App) -> AnyElement {
    let posters = &model.read(cx).posters;
    if let Some(image) = posters.get(id) {
        return img(image)
            .size_full()
            .object_fit(ObjectFit::Contain)
            .into_any_element();
    }
    if !posters.pending.contains(&id) && !posters.failed.contains(&id) {
        let model = model.clone();
        // Views call this while rendering, when the model cannot be changed.
        cx.defer(move |cx| model.update(cx, |model, cx| model.load_poster(id, false, cx)));
    }
    div()
        .size_full()
        .rounded_md()
        .bg(cx.theme().muted)
        .into_any_element()
}
