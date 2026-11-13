use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use shiori_core::recognition::{ParseOptions, Relations, TitleIndex, identify, parse, parse_path};
use shiori_core::{Anime, AnimeId, Episode};

/// A copy of what recognition needs, taken from the app state when a
/// search or scan starts. Being owned, it can move to a background thread;
/// changes made to the list meanwhile are seen by the next search.
#[derive(Debug, Clone, Default)]
pub struct Recognizer {
    pub index: TitleIndex,
    pub anime: BTreeMap<AnimeId, Anime>,
    pub relations: Relations,
    pub library_folders: Vec<PathBuf>,
}

impl Recognizer {
    /// Parses and identifies a file. `None` when it is not a video file,
    /// so that subtitles next to an episode are never taken for it.
    pub fn file(&self, path: &Path) -> Option<Episode> {
        // Parsing is the slow part; most of a library is subtitles, fonts
        // and images, which the extension rules out at once.
        if !crate::scan::is_video_name(path) {
            return None;
        }
        let mut episode = parse_path(path, &self.library_folders, ParseOptions::default());
        if !episode.is_video_file() {
            return None;
        }
        identify(&mut episode, &self.index, &self.anime, &self.relations);
        Some(episode)
    }

    /// Identifies a folder name as an anime. Episode numbers and the like
    /// are not parsed, as for anime folders in `parse_path`: a folder named
    /// `Show 2` is the anime "Show 2", not episode 2 of "Show".
    pub fn folder(&self, path: &Path) -> Option<AnimeId> {
        let name = path.file_name()?.to_string_lossy();
        let options = ParseOptions {
            parse_episode: false,
            parse_episode_title: false,
            parse_file_checksum: false,
            parse_file_extension: false,
            ..ParseOptions::default()
        };
        let mut episode = parse(&name, options);
        identify(&mut episode, &self.index, &self.anime, &self.relations)
    }
}
