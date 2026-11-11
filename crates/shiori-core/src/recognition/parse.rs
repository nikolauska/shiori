//! Splitting a file name or title into elements.

use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

use crate::episode::{Element, ElementKind, Episode};

/// Which elements the parser looks for. All on by default.
///
/// Mirrors the parser library's options so that the library can be swapped
/// without touching callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    pub parse_episode: bool,
    pub parse_episode_title: bool,
    pub parse_file_checksum: bool,
    pub parse_file_extension: bool,
    pub parse_part: bool,
    pub parse_release_group: bool,
    pub parse_season: bool,
    pub parse_title: bool,
    pub parse_video_resolution: bool,
    pub parse_year: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            parse_episode: true,
            parse_episode_title: true,
            parse_file_checksum: true,
            parse_file_extension: true,
            parse_part: true,
            parse_release_group: true,
            parse_season: true,
            parse_title: true,
            parse_video_resolution: true,
            parse_year: true,
        }
    }
}

impl From<ParseOptions> for anitomy_ng::Options {
    fn from(o: ParseOptions) -> Self {
        anitomy_ng::Options {
            parse_episode: o.parse_episode,
            parse_episode_title: o.parse_episode_title,
            parse_file_checksum: o.parse_file_checksum,
            parse_file_extension: o.parse_file_extension,
            parse_part: o.parse_part,
            parse_release_group: o.parse_release_group,
            parse_season: o.parse_season,
            parse_title: o.parse_title,
            parse_video_resolution: o.parse_video_resolution,
            parse_year: o.parse_year,
        }
    }
}

fn kind(kind: anitomy_ng::ElementKind) -> ElementKind {
    use anitomy_ng::ElementKind as A;
    match kind {
        A::AudioTerm => ElementKind::AudioTerm,
        A::Device => ElementKind::Device,
        A::Episode => ElementKind::Episode,
        A::EpisodeTitle => ElementKind::EpisodeTitle,
        A::FileChecksum => ElementKind::FileChecksum,
        A::FileExtension => ElementKind::FileExtension,
        A::Language => ElementKind::Language,
        A::Other => ElementKind::Other,
        A::Part => ElementKind::Part,
        A::ReleaseGroup => ElementKind::ReleaseGroup,
        A::ReleaseInformation => ElementKind::ReleaseInformation,
        A::ReleaseVersion => ElementKind::ReleaseVersion,
        A::Season => ElementKind::Season,
        A::Source => ElementKind::Source,
        A::Subtitles => ElementKind::Subtitles,
        A::Title => ElementKind::Title,
        A::Type => ElementKind::Type,
        A::VideoResolution => ElementKind::VideoResolution,
        A::VideoTerm => ElementKind::VideoTerm,
        A::Volume => ElementKind::Volume,
        A::Year => ElementKind::Year,
    }
}

/// Parses a file name or a window or page title.
pub fn parse(input: &str, options: ParseOptions) -> Episode {
    // The parser expects composed text; file names from some systems
    // (macOS in particular) arrive decomposed.
    let input: String = input.nfc().collect();
    let elements = anitomy_ng::parse(&input, options.into())
        .into_iter()
        .map(|e| Element::new(kind(e.kind), e.value))
        .collect();
    Episode::new(elements)
}

/// Parses the file name of a path. When that gives no title, the parent
/// folders are tried, because many libraries keep a plain episode name such
/// as `01.mkv` inside a folder named after the anime.
pub fn parse_path(path: &Path, library_folders: &[PathBuf], options: ParseOptions) -> Episode {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let mut episode = parse(&file_name, options);
    if episode.has(ElementKind::Title) {
        return episode;
    }
    if let Some((title, season)) = parse_parent_folders(path, library_folders) {
        episode
            .elements
            .push(Element::new(ElementKind::Title, title));
        if let Some(season) = season
            && !episode.has(ElementKind::Season)
        {
            episode
                .elements
                .push(Element::new(ElementKind::Season, season));
        }
    }
    episode
}

const MAX_FOLDER_DEPTH: usize = 2;

/// Returns the anime title from the nearest usable folder name, and the season
/// number from a skipped folder such as `Season 2`.
fn parse_parent_folders(
    path: &Path,
    library_folders: &[PathBuf],
) -> Option<(String, Option<String>)> {
    let mut season: Option<String> = None;
    let mut folder = path.parent()?;
    for _ in 0..MAX_FOLDER_DEPTH {
        // A filesystem root has no name and cannot be an anime folder.
        let name = folder.file_name()?.to_string_lossy();
        // A library folder, a drive root such as "C:", or anything above them
        // cannot be an anime folder.
        if is_library_folder(folder, library_folders) || name.contains(':') {
            return None;
        }
        if !is_unusable_folder_name(&name) {
            return Some((extract_title(&name), season));
        }
        if season.is_none() {
            season = parse(&name, ParseOptions::default())
                .get(ElementKind::Season)
                .map(str::to_owned);
        }
        folder = folder.parent()?;
    }
    None
}

fn is_library_folder(folder: &Path, library_folders: &[PathBuf]) -> bool {
    let key = |p: &Path| {
        let text = p.to_string_lossy().to_lowercase();
        text.trim_end_matches(['/', '\\']).to_owned()
    };
    let folder = key(folder);
    library_folders.iter().any(|f| key(f) == folder)
}

fn is_unusable_folder_name(name: &str) -> bool {
    const GENERIC: [&str; 5] = ["anime", "download", "downloads", "extra", "extras"];
    name.is_empty()
        || GENERIC.iter().any(|g| g.eq_ignore_ascii_case(name))
        // Names made only of keywords, such as "Season 2" or "1080p".
        || !parse(name, ParseOptions::default()).has(ElementKind::Title)
}

/// The title from a folder name, which may look like `[Group] Title [1080p]`.
fn extract_title(name: &str) -> String {
    let options = ParseOptions {
        parse_episode: false,
        parse_episode_title: false,
        parse_file_checksum: false,
        parse_file_extension: false,
        ..ParseOptions::default()
    };
    parse(name, options)
        .title()
        .map_or_else(|| name.to_owned(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_example() {
        let e = parse(
            "[TaigaSubs]_Toradora!_(2008)_-_01v2_-_Tiger_and_Dragon_[1280x720_H.264_FLAC][1234ABCD].mkv",
            ParseOptions::default(),
        );
        let get = |k| e.get(k).unwrap();
        assert_eq!(get(ElementKind::ReleaseGroup), "TaigaSubs");
        assert_eq!(get(ElementKind::Title), "Toradora!");
        assert_eq!(get(ElementKind::Year), "2008");
        assert_eq!(get(ElementKind::Episode), "01");
        assert_eq!(get(ElementKind::ReleaseVersion), "2");
        assert_eq!(get(ElementKind::EpisodeTitle), "Tiger and Dragon");
        assert_eq!(get(ElementKind::FileExtension), "mkv");
        assert!(e.is_video_file());
    }

    #[test]
    fn non_video_extension_is_not_a_file_extension() {
        let e = parse("[Group] Title - 01.txt", ParseOptions::default());
        assert!(!e.is_video_file());
    }

    #[test]
    fn folder_fallback() {
        let e = parse_path(
            Path::new("/media/anime/Toradora!/01.mkv"),
            &[],
            ParseOptions::default(),
        );
        assert_eq!(e.title(), Some("Toradora!"));
        assert_eq!(e.get(ElementKind::Episode), Some("01"));
    }

    #[test]
    fn folder_fallback_skips_season_folder() {
        let e = parse_path(
            Path::new("/media/Shingeki no Kyojin/Season 2/05.mkv"),
            &[],
            ParseOptions::default(),
        );
        assert_eq!(e.title(), Some("Shingeki no Kyojin"));
        assert_eq!(e.get(ElementKind::Season), Some("2"));
    }

    #[test]
    fn folder_fallback_strips_group_and_tags() {
        let e = parse_path(
            Path::new("/media/[Group] Toradora! [1080p]/01.mkv"),
            &[],
            ParseOptions::default(),
        );
        assert_eq!(e.title(), Some("Toradora!"));
    }

    #[test]
    fn folder_fallback_stops_at_library_and_generic_names() {
        let lib = [PathBuf::from("/media/Library")];
        let e = parse_path(
            Path::new("/media/library/01.mkv"),
            &lib,
            ParseOptions::default(),
        );
        assert_eq!(e.title(), None);
        let e = parse_path(
            Path::new("/x/Downloads/01.mkv"),
            &[],
            ParseOptions::default(),
        );
        assert_eq!(e.title(), Some("x"));
        let e = parse_path(
            Path::new("/a/b/Extras/Downloads/01.mkv"),
            &[],
            ParseOptions::default(),
        );
        assert_eq!(e.title(), None, "only two levels up are looked at");
    }
}
