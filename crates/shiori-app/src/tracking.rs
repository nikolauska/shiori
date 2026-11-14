//! From detection results to an identified episode, and the glue between
//! `AppState` and the update session in `shiori-core`. See
//! docs/detection.md ("How the app uses the result") and docs/tracking.md.

use std::path::{Path, PathBuf};

use shiori_core::entry::is_in_list;
use shiori_core::recognition::{
    MatchReason, ParseOptions, Relations, close_matches, identify_with_reason, parse, parse_path,
};
use shiori_core::{
    Anime, AnimeId, ElementKind, Episode, ListEntry, ListStatus, Lookup, UpdateSettings,
};
use shiori_detect::{Detected, MediaInfoKind, Player, PlayerType, WindowId};
use shiori_store::{Folders, Settings};

use crate::state::AppState;

impl Lookup for AppState {
    fn anime(&self, id: AnimeId) -> Option<&Anime> {
        self.anime.get(&id)
    }

    fn entry(&self, id: AnimeId) -> Option<&ListEntry> {
        self.entries.get(&id)
    }
}

/// Where the media came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    /// A window or page title.
    Title(String),
}

/// The process and window media was found in, to keep following the same
/// one on the next poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerKey {
    pub process_id: u32,
    pub window: Option<WindowId>,
}

impl PlayerKey {
    pub fn of(detected: &Detected) -> Self {
        Self {
            process_id: detected.process_id,
            window: detected.window,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub player: String,
    pub key: PlayerKey,
    pub source: Source,
    pub url: Option<String>,
    /// Parsed; `anime_id` is set once identified.
    pub episode: Episode,
    /// As parsed, before identification, which can rewrite the episode
    /// numbers; identifying again starts from this.
    pub parsed: Episode,
    /// Why `episode.anime_id` is what it is; `None` while not identified.
    pub matched: Option<MatchReason>,
}

/// The media to follow among `results`, parsed but not identified.
///
/// Players turned off in the settings are skipped, and browsers when
/// streaming detection is off. The player followed last time is kept while
/// it is still there, so that a second player does not take over.
pub fn choose_media(
    results: &[Detected],
    settings: &Settings,
    last: Option<PlayerKey>,
) -> Option<Media> {
    let wanted = |d: &&Detected| {
        !settings.disabled_players.contains(&d.player.name)
            && (settings.streaming_enabled || d.player.kind != PlayerType::WebBrowser)
    };
    let candidates: Vec<&Detected> = results.iter().filter(wanted).collect();
    let detected = last
        .and_then(|key| candidates.iter().find(|d| PlayerKey::of(d) == key))
        .or_else(|| candidates.first())?;

    let files: Vec<&str> = detected
        .media
        .iter()
        .filter(|m| m.kind == MediaInfoKind::File)
        .map(|m| m.value.as_str())
        .collect();
    let url = detected
        .media
        .iter()
        .rev()
        .find(|m| m.kind == MediaInfoKind::Url)
        .map(|m| m.value.clone());
    // The window title beats the page title: a player's own title is what
    // the user sees, while a page title may be about another tab.
    let title = [MediaInfoKind::Unknown, MediaInfoKind::Title]
        .iter()
        .find_map(|&kind| detected.media.iter().find(|m| m.kind == kind))
        .map(|m| m.value.clone());

    let (source, episode) = if !files.is_empty() {
        // A player also has subtitles and fonts open. When none of the
        // files is a video there is no media: the title would describe the
        // same non-video file.
        files.iter().find_map(|file| {
            let path = Path::new(file);
            let episode = parse_path(path, &settings.library_folders, ParseOptions::default());
            episode
                .is_video_file()
                .then(|| (Source::File(path.to_owned()), episode))
        })?
    } else {
        let title = title?;
        let episode = parse(&title, ParseOptions::default());
        (Source::Title(title), episode)
    };
    Some(Media {
        player: detected.player.name.clone(),
        key: PlayerKey::of(detected),
        source,
        url,
        parsed: episode.clone(),
        episode,
        matched: None,
    })
}

/// Identifies the episode in place against the list and the database.
pub fn identify_media(media: &mut Media, state: &AppState, relations: &Relations) {
    media.episode = media.parsed.clone();
    media.matched = identify_with_reason(
        &mut media.episode,
        &state.title_index,
        &state.anime,
        relations,
        &state.numbering,
    )
    .map(|(_, reason)| reason);
}

/// Anime whose titles are close to the episode's title, best first: the
/// likely answers when it matched nothing. Watching entries go first among
/// equals, since that is what the user is most likely playing.
pub fn suggestions(state: &AppState, episode: &Episode, limit: usize) -> Vec<AnimeId> {
    let Some(title) = episode.title() else {
        return Vec::new();
    };
    let season = episode
        .get(ElementKind::Season)
        .and_then(|s| s.trim().parse().ok());
    close_anime(state, title, season, limit)
}

/// Anime with titles close to `title`, best first; see `suggestions`.
pub fn close_anime(
    state: &AppState,
    title: &str,
    season: Option<u32>,
    limit: usize,
) -> Vec<AnimeId> {
    let titles = state.anime.values().flat_map(|anime| {
        let t = &anime.titles;
        let own = state
            .anime_settings
            .get(&anime.id)
            .into_iter()
            .flat_map(|s| s.synonyms.iter());
        [&t.romaji, &t.english, &t.japanese]
            .into_iter()
            .chain(t.synonyms.iter())
            .chain(own)
            .map(move |title| (anime.id, title.as_str()))
    });
    let mut found = close_matches(title, season, titles, limit * 4);
    let watching = |id: &AnimeId| {
        state
            .entries
            .get(id)
            .is_some_and(|e| is_in_list(Some(e)) && e.status == ListStatus::Watching)
    };
    for (id, score) in &mut found {
        if watching(id) {
            *score += 0.05;
        }
    }
    found.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    found.into_iter().take(limit).map(|(id, _)| id).collect()
}

/// Whether the file is outside every library folder while the settings
/// say to ignore such files. Titles and an empty library never are.
pub fn is_outside_library(settings: &Settings, source: &Source) -> bool {
    let Source::File(path) = source else {
        return false;
    };
    if !settings.library_only || settings.library_folders.is_empty() {
        return false;
    }
    let file = clean(path);
    !settings.library_folders.iter().any(|folder| {
        let folder = clean(folder);
        file.len() > folder.len() + 1
            && file.starts_with(&folder)
            && file[folder.len()..].starts_with('/')
    })
}

/// Lower case with forward slashes and no trailing slash, so that
/// `D:\Anime\` and `d:/anime` compare equal. Case is ignored on every
/// system, as in Taiga.
fn clean(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/").to_lowercase();
    text.trim_end_matches('/').to_owned()
}

pub fn update_settings(settings: &Settings) -> UpdateSettings {
    UpdateSettings {
        delay: settings.update_delay_s,
        trigger: settings.update_trigger,
        pause_when_unfocused: settings.pause_when_unfocused,
    }
}

/// The players file in the data folder, or the built-in one when there is
/// none. A broken user file is reported and the built-in one used, so that
/// detection keeps working.
pub fn load_players(folders: &Folders) -> (Vec<Player>, Option<String>) {
    let path = folders.players_override();
    match std::fs::read_to_string(&path) {
        Ok(text) => match shiori_detect::parse_players(&text) {
            Ok(players) => (players, None),
            Err(error) => (
                shiori_detect::builtin_players(),
                Some(format!("{}: {error}", path.display())),
            ),
        },
        Err(_) => (shiori_detect::builtin_players(), None),
    }
}

const RELATIONS: &str = include_str!("../../../data/anime-relations.txt");

/// The anime relations file in the data folder (S14), or the built-in one.
pub fn load_relations(folders: &Folders, settings: &Settings) -> Relations {
    let text = std::fs::read_to_string(folders.relations_override())
        .unwrap_or_else(|_| RELATIONS.to_owned());
    Relations::parse(&text, settings.service)
}

#[cfg(test)]
mod tests {
    use shiori_detect::MediaInfo;

    use super::*;

    fn player(name: &str, kind: PlayerType) -> Player {
        let mut players = shiori_detect::parse_players(&format!(
            "{name}\n\texecutables:\n\t\tx\n\tstrategies:\n\t\topen_files\n"
        ))
        .unwrap();
        let mut player = players.remove(0);
        player.kind = kind;
        player
    }

    fn detected(name: &str, pid: u32, media: &[(MediaInfoKind, &str)]) -> Detected {
        Detected {
            player: player(name, PlayerType::Default),
            process_id: pid,
            window: None,
            window_title: String::new(),
            media: media
                .iter()
                .map(|&(kind, value)| MediaInfo {
                    kind,
                    value: value.into(),
                })
                .collect(),
            playing: None,
        }
    }

    use MediaInfoKind::*;

    #[test]
    fn the_first_video_file_wins() {
        let results = [detected(
            "mpv",
            1,
            &[
                (File, "/a/[Group] Show - 03.ass"),
                (File, "/a/[Group] Show - 03.mkv"),
                (Unknown, "Other - 09.mkv"),
            ],
        )];
        let media = choose_media(&results, &Settings::default(), None).unwrap();
        assert_eq!(
            media.source,
            Source::File("/a/[Group] Show - 03.mkv".into())
        );
        assert_eq!(media.episode.title(), Some("Show"));
        assert_eq!(media.episode.episode_number_range(), Some((3, 3)));
    }

    #[test]
    fn files_that_are_not_video_mean_no_media() {
        let results = [detected(
            "mpv",
            1,
            &[(File, "/music/song.flac"), (Unknown, "Show - 01.mkv")],
        )];
        assert_eq!(choose_media(&results, &Settings::default(), None), None);
    }

    #[test]
    fn without_files_the_window_title_beats_the_page_title() {
        let results = [detected(
            "Browser",
            1,
            &[
                (Title, "Page - 02"),
                (Url, "https://a"),
                (Unknown, "Show - 05"),
                (Url, "https://b"),
            ],
        )];
        let media = choose_media(&results, &Settings::default(), None).unwrap();
        assert_eq!(media.source, Source::Title("Show - 05".into()));
        assert_eq!(media.url.as_deref(), Some("https://b"));
    }

    #[test]
    fn keeps_following_the_same_player() {
        let results = [
            detected("mpv", 1, &[(Unknown, "A - 01")]),
            detected("vlc", 2, &[(Unknown, "B - 01")]),
        ];
        let settings = Settings::default();
        assert_eq!(
            choose_media(&results, &settings, None).unwrap().player,
            "mpv"
        );
        let last = PlayerKey {
            process_id: 2,
            window: None,
        };
        assert_eq!(
            choose_media(&results, &settings, Some(last))
                .unwrap()
                .player,
            "vlc"
        );
    }

    #[test]
    fn disabled_players_and_browsers_are_skipped() {
        let mut browser = detected("Chrome", 2, &[(Title, "B - 01")]);
        browser.player.kind = PlayerType::WebBrowser;
        let results = [detected("mpv", 1, &[(Unknown, "A - 01")]), browser];
        let mut settings = Settings {
            disabled_players: vec!["mpv".into()],
            streaming_enabled: false,
            ..Settings::default()
        };
        assert_eq!(choose_media(&results, &settings, None), None);
        settings.streaming_enabled = true;
        assert_eq!(
            choose_media(&results, &settings, None).unwrap().player,
            "Chrome"
        );
    }

    #[test]
    fn library_folder_check() {
        let mut settings = Settings {
            library_only: true,
            library_folders: vec![PathBuf::from(r"D:\Anime\")],
            ..Settings::default()
        };
        let file = |p: &str| Source::File(PathBuf::from(p));
        assert!(!is_outside_library(
            &settings,
            &file(r"d:\anime\Show\01.mkv")
        ));
        assert!(!is_outside_library(&settings, &file("D:/Anime/01.mkv")));
        assert!(is_outside_library(&settings, &file(r"D:\Anime2\01.mkv")));
        assert!(is_outside_library(&settings, &file(r"C:\Downloads\01.mkv")));
        assert!(!is_outside_library(
            &settings,
            &Source::Title("A - 01".into())
        ));
        settings.library_only = false;
        assert!(!is_outside_library(
            &settings,
            &file(r"C:\Downloads\01.mkv")
        ));
    }
}
