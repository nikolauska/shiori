//! How values are written in the UI. Every user-visible string the views
//! build goes through here or `text`, so translations can be added later.

use shiori_core::date::Season;
use shiori_core::{Anime, AnimeType, FuzzyDate, ListEntry, ListStatus, SeasonName};
use shiori_sync::anilist::score::{ScoreFormat, format_rating};

use shiori_app::list_view::ListColumn;
use shiori_app::state::AppState;

pub fn status_label(status: ListStatus) -> &'static str {
    match status {
        ListStatus::Watching => "Watching",
        ListStatus::Completed => "Completed",
        ListStatus::OnHold => "On hold",
        ListStatus::Dropped => "Dropped",
        ListStatus::PlanToWatch => "Plan to watch",
        ListStatus::NotInList => "Not in list",
    }
}

pub fn type_label(kind: AnimeType) -> &'static str {
    match kind {
        AnimeType::Unknown => "",
        AnimeType::Tv => "TV",
        AnimeType::Ova => "OVA",
        AnimeType::Movie => "Movie",
        AnimeType::Special => "Special",
        AnimeType::Ona => "ONA",
        AnimeType::Music => "Music",
    }
}

pub fn season_label(date: FuzzyDate) -> String {
    let season = Season::from_date(date);
    if !season.is_valid() {
        return String::new();
    }
    let name = match season.name {
        SeasonName::Winter => "Winter",
        SeasonName::Spring => "Spring",
        SeasonName::Summer => "Summer",
        SeasonName::Fall => "Fall",
        SeasonName::Unknown => return String::new(),
    };
    format!("{name} {}", season.year)
}

pub fn date_label(date: FuzzyDate) -> String {
    if date.is_empty() {
        String::new()
    } else {
        date.to_string()
    }
}

pub fn score_format(state: &AppState) -> ScoreFormat {
    ScoreFormat::from_name(&state.accounts.anilist.rating_system)
}

pub fn score_label(state: &AppState, score: u8) -> String {
    if score == 0 {
        String::new()
    } else {
        format_rating(score_format(state), score)
    }
}

pub fn progress_label(entry: &ListEntry, anime: Option<&Anime>) -> String {
    let total = anime
        .and_then(|a| a.episode_count)
        .map_or("?".to_owned(), |c| c.to_string());
    format!("{}/{total}", entry.watched_episodes)
}

/// The text of one cell in the anime list.
pub fn cell(
    state: &AppState,
    column: ListColumn,
    anime: &Anime,
    entry: Option<&ListEntry>,
) -> String {
    match column {
        ListColumn::Title => state.title(anime.id),
        ListColumn::Progress => entry
            .map(|e| progress_label(e, Some(anime)))
            .unwrap_or_default(),
        ListColumn::Duration => anime
            .episode_length
            .map(|m| format!("{m} min"))
            .unwrap_or_default(),
        ListColumn::Rewatches => entry
            .filter(|e| e.rewatched_times > 0)
            .map(|e| e.rewatched_times.to_string())
            .unwrap_or_default(),
        ListColumn::Score => entry
            .map(|e| score_label(state, e.score))
            .unwrap_or_default(),
        ListColumn::Average => {
            if anime.score > 0.0 {
                format!("{:.2}", anime.score)
            } else {
                String::new()
            }
        }
        ListColumn::Type => type_label(anime.kind).to_owned(),
        ListColumn::Season => season_label(anime.date_started),
        ListColumn::Started => entry
            .map(|e| date_label(e.date_started))
            .unwrap_or_default(),
        ListColumn::Completed => entry
            .map(|e| date_label(e.date_completed))
            .unwrap_or_default(),
        ListColumn::LastUpdated => entry
            .filter(|e| e.last_updated.as_second() > 0)
            .map(|e| e.last_updated.strftime("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default(),
        ListColumn::Notes => entry.map(|e| e.notes.clone()).unwrap_or_default(),
    }
}
