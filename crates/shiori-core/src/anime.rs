//! Anime as known from the list service, and the rules derived from it.

use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::date::FuzzyDate;
use crate::entry::AnimeSettings;

/// Id of an anime on the active service. Only meaningful together with that
/// service, which is why each service has its own database.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct AnimeId(pub u32);

impl std::fmt::Display for AnimeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The highest episode number considered possible.
pub const MAX_EPISODE_COUNT: u32 = 1900;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgeRating {
    #[default]
    Unknown,
    G,
    Pg,
    Pg13,
    R17,
    R18,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AiringStatus {
    #[default]
    Unknown,
    FinishedAiring,
    Airing,
    NotYetAired,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnimeType {
    #[default]
    Unknown,
    Tv,
    Ova,
    Movie,
    Special,
    Ona,
    Music,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TitleLanguage {
    #[default]
    Romaji,
    English,
    Native,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Titles {
    pub romaji: String,
    pub english: String,
    pub japanese: String,
    pub synonyms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Anime {
    pub id: AnimeId,
    /// The same anime's MyAnimeList id, when the service knows it.
    pub mal_id: Option<u32>,
    /// When Shiori last received this anime from the service.
    pub last_modified: Option<Timestamp>,
    pub episode_count: Option<u32>,
    /// Minutes.
    pub episode_length: Option<u32>,
    pub age_rating: AgeRating,
    pub status: AiringStatus,
    pub kind: AnimeType,
    pub date_started: FuzzyDate,
    pub date_finished: FuzzyDate,
    /// Community average on a 0 to 10 scale; 0.0 is unknown.
    pub score: f32,
    pub popularity_rank: u32,
    pub image_url: String,
    /// Kitsu only.
    pub slug: String,
    pub synopsis: String,
    /// YouTube video id.
    pub trailer_id: String,
    pub titles: Titles,
    pub genres: Vec<String>,
    pub producers: Vec<String>,
    pub studios: Vec<String>,
    pub tags: Vec<String>,
    /// 0 when unknown.
    pub last_aired_episode: u32,
    pub next_episode_time: Option<Timestamp>,
}

impl Default for Anime {
    fn default() -> Self {
        Self::new(AnimeId(0))
    }
}

impl Anime {
    pub fn new(id: AnimeId) -> Self {
        Self {
            id,
            last_modified: None,
            episode_count: None,
            episode_length: None,
            age_rating: AgeRating::Unknown,
            status: AiringStatus::Unknown,
            kind: AnimeType::Unknown,
            date_started: FuzzyDate::default(),
            date_finished: FuzzyDate::default(),
            score: 0.0,
            popularity_rank: 0,
            image_url: String::new(),
            slug: String::new(),
            synopsis: String::new(),
            trailer_id: String::new(),
            titles: Titles::default(),
            genres: Vec::new(),
            producers: Vec::new(),
            studios: Vec::new(),
            tags: Vec::new(),
            last_aired_episode: 0,
            next_episode_time: None,
            mal_id: None,
        }
    }
}

/// Airing status worked out from the stored status and the dates.
pub fn airing_status(anime: &Anime, today_in_japan: Date) -> AiringStatus {
    // The service is trusted when it says something already happened, but not
    // the opposite, because stored data may be old. This also covers finished
    // anime with completely unknown dates.
    if anime.status == AiringStatus::FinishedAiring {
        return AiringStatus::FinishedAiring;
    }
    if anime.date_started.is_empty() && anime.date_finished.is_empty() {
        return AiringStatus::NotYetAired;
    }
    let today = FuzzyDate::from(today_in_japan);
    if !anime.date_finished.is_empty() && anime.date_finished < today {
        return AiringStatus::FinishedAiring;
    }
    if !anime.date_started.is_empty() && anime.date_started <= today {
        return AiringStatus::Airing;
    }
    AiringStatus::NotYetAired
}

pub fn is_aired_yet(anime: &Anime, today_in_japan: Date) -> bool {
    let aired = |s| matches!(s, AiringStatus::FinishedAiring | AiringStatus::Airing);
    aired(anime.status) || aired(airing_status(anime, today_in_japan))
}

/// Whether the stored data is old or incomplete enough to fetch again.
pub fn is_stale(anime: &Anime, now: Timestamp, today_in_japan: Date) -> bool {
    let Some(last_modified) = anime.last_modified else {
        return true;
    };
    if anime.synopsis.is_empty() || anime.genres.is_empty() {
        return true;
    }
    if anime.score == 0.0 && is_aired_yet(anime, today_in_japan) {
        return true;
    }
    let age = now.duration_since(last_modified);
    if airing_status(anime, today_in_japan) == AiringStatus::FinishedAiring {
        age >= SignedDuration::from_hours(24 * 7)
    } else {
        age >= SignedDuration::from_hours(1)
    }
}

/// Episode count, estimated from the highest episode seen when unknown.
pub fn estimate_episode_count(anime: &Anime, highest_seen: u32) -> Option<u32> {
    if let Some(count) = anime.episode_count {
        return Some(count);
    }
    // Most TV series run one or two cours (12-13 or 24-26 episodes), so round
    // up to the usual lengths. Past a year there is no sensible guess.
    match highest_seen {
        0..12 => Some(12),
        12..24 => Some(26),
        24..50 => Some(52),
        _ => None,
    }
}

/// Episode length in minutes, estimated from the type when unknown.
pub fn estimate_episode_length(anime: &Anime) -> Option<u32> {
    if anime.episode_length.is_some() {
        return anime.episode_length;
    }
    match anime.kind {
        AnimeType::Tv | AnimeType::Ova | AnimeType::Ona => Some(24),
        AnimeType::Movie => Some(90),
        AnimeType::Special => Some(12),
        AnimeType::Music => Some(5),
        AnimeType::Unknown => None,
    }
}

/// Last aired episode of a TV series, estimated from its weekly schedule.
/// 0 when it cannot be estimated.
pub fn estimate_last_aired_episode(anime: &Anime, today_in_japan: Date) -> u32 {
    if anime.kind != AnimeType::Tv {
        return 0;
    }
    let Some(start) = anime.date_started.to_date() else {
        return 0;
    };
    if today_in_japan < start {
        return 0;
    }
    // One day is taken off because the airing hour is unknown.
    let days = i64::from(start.until(today_in_japan).map_or(0, |s| s.get_days())) - 1;
    let weeks = u32::try_from(days / 7).unwrap_or(0);
    let count = anime.episode_count.unwrap_or(0);
    // Delays and breaks make the guess unreliable for longer series.
    if (count == 0 || weeks < count) && weeks < 13 {
        return weeks + 1;
    }
    count
}

/// Age rating R18, or an unknown rating with the Hentai genre.
pub fn is_nsfw(anime: &Anime) -> bool {
    match anime.age_rating {
        AgeRating::R18 => true,
        AgeRating::Unknown => anime.genres.iter().any(|g| g == "Hentai"),
        _ => false,
    }
}

pub fn preferred_title<'a>(
    anime: &'a Anime,
    settings: Option<&'a AnimeSettings>,
    language: TitleLanguage,
) -> &'a str {
    if let Some(settings) = settings
        && !settings.display_title.is_empty()
    {
        return &settings.display_title;
    }
    let titles = &anime.titles;
    let or_romaji = |title: &'a String| {
        if title.is_empty() {
            &titles.romaji
        } else {
            title
        }
    };
    match language {
        TitleLanguage::Romaji => &titles.romaji,
        TitleLanguage::English => or_romaji(&titles.english),
        TitleLanguage::Native => or_romaji(&titles.japanese),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn anime() -> Anime {
        Anime::new(AnimeId(1))
    }

    #[test]
    fn airing_status_rules() {
        let today = date(2026, 11, 11);
        let mut a = anime();
        assert_eq!(airing_status(&a, today), AiringStatus::NotYetAired);
        a.status = AiringStatus::FinishedAiring;
        assert_eq!(airing_status(&a, today), AiringStatus::FinishedAiring);

        let mut a = anime();
        a.status = AiringStatus::NotYetAired;
        a.date_started = FuzzyDate::new(2026, 10, 1);
        assert_eq!(airing_status(&a, today), AiringStatus::Airing);
        a.date_finished = FuzzyDate::new(2026, 11, 10);
        assert_eq!(airing_status(&a, today), AiringStatus::FinishedAiring);
        a.date_finished = FuzzyDate::new(2026, 11, 0);
        assert_eq!(airing_status(&a, today), AiringStatus::Airing);
        a.date_started = FuzzyDate::new(2026, 11, 12);
        a.date_finished = FuzzyDate::default();
        assert_eq!(airing_status(&a, today), AiringStatus::NotYetAired);
        assert!(!is_aired_yet(&a, today));
        a.status = AiringStatus::Airing;
        assert!(is_aired_yet(&a, today));
    }

    #[test]
    fn staleness() {
        let now: Timestamp = "2026-11-11T12:00:00Z".parse().unwrap();
        let today = date(2026, 11, 11);
        let mut a = anime();
        assert!(is_stale(&a, now, today));
        a.last_modified = Some(now);
        a.synopsis = "x".into();
        a.genres = vec!["Action".into()];
        a.score = 7.5;
        a.status = AiringStatus::FinishedAiring;
        assert!(!is_stale(&a, now, today));
        a.last_modified = Some(now - SignedDuration::from_hours(24 * 7 - 1));
        assert!(!is_stale(&a, now, today));
        a.last_modified = Some(now - SignedDuration::from_hours(24 * 7));
        assert!(is_stale(&a, now, today));

        a.status = AiringStatus::Airing;
        a.date_started = FuzzyDate::new(2026, 10, 1);
        a.last_modified = Some(now - SignedDuration::from_mins(59));
        assert!(!is_stale(&a, now, today));
        a.last_modified = Some(now - SignedDuration::from_mins(60));
        assert!(is_stale(&a, now, today));

        a.last_modified = Some(now);
        a.score = 0.0;
        assert!(is_stale(&a, now, today));
    }

    #[test]
    fn episode_estimates() {
        let mut a = anime();
        assert_eq!(estimate_episode_count(&a, 0), Some(12));
        assert_eq!(estimate_episode_count(&a, 11), Some(12));
        assert_eq!(estimate_episode_count(&a, 12), Some(26));
        assert_eq!(estimate_episode_count(&a, 24), Some(52));
        assert_eq!(estimate_episode_count(&a, 50), None);
        a.episode_count = Some(13);
        assert_eq!(estimate_episode_count(&a, 50), Some(13));

        assert_eq!(estimate_episode_length(&a), None);
        a.kind = AnimeType::Movie;
        assert_eq!(estimate_episode_length(&a), Some(90));
        a.episode_length = Some(100);
        assert_eq!(estimate_episode_length(&a), Some(100));
    }

    #[test]
    fn last_aired_estimate() {
        let mut a = anime();
        a.kind = AnimeType::Tv;
        a.date_started = FuzzyDate::new(2026, 10, 1);
        assert_eq!(estimate_last_aired_episode(&a, date(2026, 9, 30)), 0);
        assert_eq!(estimate_last_aired_episode(&a, date(2026, 10, 1)), 1);
        assert_eq!(estimate_last_aired_episode(&a, date(2026, 10, 8)), 1);
        assert_eq!(estimate_last_aired_episode(&a, date(2026, 10, 9)), 2);
        // 13 weeks in: no longer trusted, falls back to the count (unknown).
        assert_eq!(estimate_last_aired_episode(&a, date(2027, 1, 1)), 0);
        a.episode_count = Some(4);
        assert_eq!(estimate_last_aired_episode(&a, date(2026, 11, 11)), 4);
        a.kind = AnimeType::Movie;
        assert_eq!(estimate_last_aired_episode(&a, date(2026, 10, 9)), 0);
    }

    #[test]
    fn nsfw() {
        let mut a = anime();
        assert!(!is_nsfw(&a));
        a.genres = vec!["Hentai".into()];
        assert!(is_nsfw(&a));
        a.age_rating = AgeRating::R17;
        assert!(!is_nsfw(&a));
        a.age_rating = AgeRating::R18;
        assert!(is_nsfw(&a));
    }

    #[test]
    fn preferred_titles() {
        let mut a = anime();
        a.titles.romaji = "Shingeki no Kyojin".into();
        assert_eq!(
            preferred_title(&a, None, TitleLanguage::English),
            "Shingeki no Kyojin"
        );
        a.titles.english = "Attack on Titan".into();
        assert_eq!(
            preferred_title(&a, None, TitleLanguage::English),
            "Attack on Titan"
        );
        assert_eq!(
            preferred_title(&a, None, TitleLanguage::Native),
            "Shingeki no Kyojin"
        );
        let settings = AnimeSettings {
            display_title: "AoT".into(),
            ..AnimeSettings::new(a.id)
        };
        assert_eq!(
            preferred_title(&a, Some(&settings), TitleLanguage::Romaji),
            "AoT"
        );
    }
}
