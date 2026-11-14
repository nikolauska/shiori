//! AniList's JSON answers and how they map onto Shiori's types.

use jiff::Timestamp;
use reqwest::StatusCode;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use shiori_core::{
    AiringStatus, Anime, AnimeId, AnimeType, Fields, FuzzyDate, ListEntry, ListEntryId, ListStatus,
    RelationKind, SearchParams, SearchSort, SeasonName,
};

use crate::ServiceError;
use crate::error::ddos_protection;

pub(crate) const EXPIRED_TOKEN: &str =
    "Access token has expired. Please re-authorize your account via Settings.";

#[derive(Deserialize)]
struct Envelope {
    data: Option<Value>,
    errors: Option<Vec<ErrorJson>>,
}

#[derive(Deserialize)]
struct ErrorJson {
    message: Option<String>,
    validation: Option<Map<String, Value>>,
}

/// Turns an HTTP answer into the `data` object, or the error to show.
/// `fallback` is used when something went wrong and AniList gave no message.
pub(crate) fn decode<T: DeserializeOwned>(
    status: StatusCode,
    server: Option<&str>,
    body: &[u8],
    fallback: &str,
) -> Result<T, ServiceError> {
    if let Some(error) = ddos_protection(status, server) {
        return Err(error);
    }
    if status == StatusCode::NOT_FOUND {
        return Err(ServiceError::NotFound);
    }
    let envelope: Option<Envelope> = serde_json::from_slice(body).ok();
    if let Some(error) = envelope
        .as_ref()
        .and_then(|e| e.errors.as_deref())
        .and_then(error_message)
    {
        return Err(error);
    }
    if !status.is_success() {
        return Err(ServiceError::Api(fallback.to_owned()));
    }
    envelope
        .and_then(|e| e.data)
        .and_then(|data| serde_json::from_value(data).ok())
        .ok_or_else(|| ServiceError::Parse(fallback.to_owned()))
}

fn error_message(errors: &[ErrorJson]) -> Option<ServiceError> {
    let error = errors
        .iter()
        .find(|e| e.message.as_deref().is_some_and(|m| !m.is_empty()))?;
    let message = error.message.as_deref().unwrap_or_default();
    // Validation errors carry a generic "validation" message; the useful
    // text is in the per-field list.
    if let Some(first) = error
        .validation
        .as_ref()
        .and_then(|fields| fields.values().next())
        .and_then(|messages| messages.get(0))
        .and_then(Value::as_str)
    {
        return Some(ServiceError::Api(format!("Validation error: {first}")));
    }
    if message == "Invalid token" {
        return Some(ServiceError::Unauthorized(EXPIRED_TOKEN.to_owned()));
    }
    Some(ServiceError::Api(message.to_owned()))
}

// ---- Viewer -------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct ViewerData {
    #[serde(rename = "Viewer")]
    pub viewer: Viewer,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Viewer {
    pub id: u64,
    pub name: String,
    pub media_list_options: Option<MediaListOptions>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaListOptions {
    pub score_format: Option<String>,
}

// ---- Media ----------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct RelationsData {
    #[serde(rename = "Media")]
    pub media: Option<RelationsMediaJson>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct RelationsMediaJson {
    relations: Option<RelationsJson>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RelationsJson {
    edges: Vec<RelationEdgeJson>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RelationEdgeJson {
    relation_type: Option<String>,
    node: Option<RelatedMediaJson>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RelatedMediaJson {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(flatten)]
    media: MediaJson,
}

/// The anime an anime is related to, with how. Manga and other media are
/// left out: only anime can be in an episode numbering.
pub(crate) fn to_relations(m: RelationsMediaJson, now: Timestamp) -> Vec<(RelationKind, Anime)> {
    let edges = m.relations.map(|r| r.edges).unwrap_or_default();
    edges
        .into_iter()
        .filter_map(|edge| {
            let node = edge.node?;
            if node.kind.as_deref() != Some("ANIME") {
                return None;
            }
            let kind = match edge.relation_type.as_deref()? {
                "PREQUEL" => RelationKind::Prequel,
                "SEQUEL" => RelationKind::Sequel,
                "SIDE_STORY" => RelationKind::SideStory,
                "ALTERNATIVE" => RelationKind::Alternative,
                "SPIN_OFF" => RelationKind::SpinOff,
                "SUMMARY" => RelationKind::Summary,
                "PARENT" => RelationKind::Parent,
                _ => RelationKind::Other,
            };
            Some((kind, to_anime(node.media, now)?))
        })
        .collect()
}

#[derive(Deserialize)]
pub(crate) struct MediaData {
    #[serde(rename = "Media")]
    pub media: Option<MediaJson>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct MediaJson {
    id: Option<u32>,
    id_mal: Option<u32>,
    title: Option<TitleJson>,
    format: Option<String>,
    status: Option<String>,
    description: Option<String>,
    start_date: Option<DateJson>,
    end_date: Option<DateJson>,
    episodes: Option<u32>,
    duration: Option<u32>,
    country_of_origin: Option<String>,
    trailer: Option<TrailerJson>,
    cover_image: Option<CoverJson>,
    genres: Option<Vec<Option<String>>>,
    synonyms: Option<Vec<Option<String>>>,
    average_score: Option<u32>,
    popularity: Option<u32>,
    tags: Option<Vec<TagJson>>,
    studios: Option<StudiosJson>,
    next_airing_episode: Option<NextEpisodeJson>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TitleJson {
    romaji: Option<String>,
    english: Option<String>,
    native: Option<String>,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(default)]
pub(crate) struct DateJson {
    year: Option<u16>,
    month: Option<u8>,
    day: Option<u8>,
}

impl From<DateJson> for FuzzyDate {
    fn from(d: DateJson) -> Self {
        FuzzyDate::new(
            d.year.unwrap_or(0),
            d.month.unwrap_or(0),
            d.day.unwrap_or(0),
        )
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TrailerJson {
    id: Option<String>,
    site: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CoverJson {
    extra_large: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TagJson {
    name: Option<String>,
    is_media_spoiler: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct StudiosJson {
    edges: Vec<StudioEdgeJson>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StudioEdgeJson {
    is_main: bool,
    node: Option<StudioJson>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct StudioJson {
    name: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct NextEpisodeJson {
    airing_at: Option<i64>,
    episode: Option<u32>,
}

fn strings(list: Option<Vec<Option<String>>>) -> Vec<String> {
    list.into_iter()
        .flatten()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect()
}

/// `None` when the object has no usable id.
pub(crate) fn to_anime(m: MediaJson, now: Timestamp) -> Option<Anime> {
    let id = m.id.filter(|&id| id != 0)?;
    let mut anime = Anime::new(AnimeId(id));
    anime.last_modified = Some(now);
    anime.mal_id = m.id_mal.filter(|&id| id != 0);
    anime.episode_count = m.episodes;
    anime.episode_length = m.duration;
    anime.status = match m.status.as_deref() {
        Some("FINISHED") => AiringStatus::FinishedAiring,
        Some("RELEASING") => AiringStatus::Airing,
        Some("NOT_YET_RELEASED" | "CANCELLED") => AiringStatus::NotYetAired,
        _ => AiringStatus::Unknown,
    };
    anime.kind = match m.format.as_deref() {
        Some("TV" | "TV_SHORT") => AnimeType::Tv,
        Some("MOVIE") => AnimeType::Movie,
        Some("SPECIAL") => AnimeType::Special,
        Some("OVA") => AnimeType::Ova,
        Some("ONA") => AnimeType::Ona,
        Some("MUSIC") => AnimeType::Music,
        _ => AnimeType::Unknown,
    };
    anime.date_started = m.start_date.unwrap_or_default().into();
    anime.date_finished = m.end_date.unwrap_or_default().into();
    anime.score = m.average_score.unwrap_or(0) as f32 / 10.0;
    anime.popularity_rank = m.popularity.unwrap_or(0);
    anime.image_url = m
        .cover_image
        .and_then(|c| c.extra_large)
        .unwrap_or_default();
    anime.synopsis = m.description.unwrap_or_default();
    anime.trailer_id = m
        .trailer
        .filter(|t| {
            t.site
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("youtube"))
        })
        .and_then(|t| t.id)
        .unwrap_or_default();

    let title = m.title.unwrap_or_default();
    anime.titles.romaji = title.romaji.unwrap_or_default();
    anime.titles.english = title.english.unwrap_or_default();
    anime.titles.synonyms = strings(m.synonyms);
    let native = title.native.unwrap_or_default();
    // Shiori's "Japanese title" slot only fits Japanese works. A Chinese or
    // Korean native title is still useful for recognition, so it becomes a
    // synonym instead of being dropped.
    if m.country_of_origin.as_deref() == Some("JP") {
        anime.titles.japanese = native;
    } else if !native.is_empty() {
        anime.titles.synonyms.push(native);
    }

    anime.genres = strings(m.genres);
    anime.tags = m
        .tags
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t.is_media_spoiler != Some(true))
        .filter_map(|t| t.name)
        .filter(|n| !n.is_empty())
        .collect();
    for edge in m.studios.unwrap_or_default().edges {
        let Some(name) = edge.node.and_then(|n| n.name).filter(|n| !n.is_empty()) else {
            continue;
        };
        if edge.is_main {
            anime.studios.push(name);
        } else {
            anime.producers.push(name);
        }
    }
    if let Some(next) = m.next_airing_episode {
        anime.next_episode_time = next.airing_at.and_then(|t| Timestamp::from_second(t).ok());
        anime.last_aired_episode = next.episode.unwrap_or(0).saturating_sub(1);
    }
    Some(anime)
}

// ---- List entries ---------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct MediaListJson {
    id: Option<i64>,
    status: Option<String>,
    score: Option<f64>,
    progress: Option<u32>,
    repeat: Option<u32>,
    private: Option<bool>,
    notes: Option<String>,
    started_at: Option<DateJson>,
    completed_at: Option<DateJson>,
    updated_at: Option<i64>,
    media: Option<MediaJson>,
}

/// The entry, plus its anime when it came along. `None` when the entry has no
/// usable anime id.
pub(crate) fn to_entry(e: MediaListJson, now: Timestamp) -> Option<(ListEntry, Option<Anime>)> {
    let media = e.media?;
    let anime_id = media.id.filter(|&id| id != 0)?;
    let mut entry = ListEntry::new(AnimeId(anime_id));
    entry.id = e.id.map(ListEntryId);
    entry.watched_episodes = e.progress.unwrap_or(0);
    entry.score = e.score.unwrap_or(0.0).round().clamp(0.0, 100.0) as u8;
    entry.status = match e.status.as_deref() {
        Some("CURRENT" | "REPEATING") => ListStatus::Watching,
        Some("PLANNING") => ListStatus::PlanToWatch,
        Some("COMPLETED") => ListStatus::Completed,
        Some("DROPPED") => ListStatus::Dropped,
        Some("PAUSED") => ListStatus::OnHold,
        _ => ListStatus::NotInList,
    };
    entry.rewatching = e.status.as_deref() == Some("REPEATING");
    entry.is_private = e.private.unwrap_or(false);
    entry.rewatched_times = e.repeat.unwrap_or(0);
    entry.date_started = e.started_at.unwrap_or_default().into();
    entry.date_completed = e.completed_at.unwrap_or_default().into();
    entry.last_updated = e
        .updated_at
        .and_then(|t| Timestamp::from_second(t).ok())
        .unwrap_or(Timestamp::UNIX_EPOCH);
    entry.notes = e.notes.unwrap_or_default();
    Some((entry, to_anime(media, now)))
}

#[derive(Deserialize)]
pub(crate) struct CollectionData {
    #[serde(rename = "MediaListCollection")]
    pub collection: Option<CollectionJson>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct CollectionJson {
    pub lists: Vec<ListJson>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct ListJson {
    pub entries: Vec<MediaListJson>,
}

#[derive(Deserialize)]
pub(crate) struct SaveData {
    #[serde(rename = "SaveMediaListEntry")]
    pub saved: Option<MediaListJson>,
}

#[derive(Deserialize)]
pub(crate) struct DeleteData {
    #[serde(rename = "DeleteMediaListEntry")]
    #[allow(dead_code)] // only checked for presence
    pub deleted: Option<Value>,
}

fn status_name(status: ListStatus) -> Option<&'static str> {
    match status {
        ListStatus::Watching => Some("CURRENT"),
        ListStatus::Completed => Some("COMPLETED"),
        ListStatus::OnHold => Some("PAUSED"),
        ListStatus::Dropped => Some("DROPPED"),
        ListStatus::PlanToWatch => Some("PLANNING"),
        ListStatus::NotInList => None,
    }
}

fn date_input(date: FuzzyDate) -> Value {
    json!({ "year": date.year, "month": date.month, "day": date.day })
}

/// Variables for `SaveMediaListEntry`. Only dirty fields are sent so that an
/// edit made on the website in the meantime is not overwritten.
pub(crate) fn save_variables(entry: &ListEntry, dirty: Fields) -> Value {
    let mut vars = Map::new();
    vars.insert("mediaId".into(), entry.anime_id.0.into());
    if let Some(id) = entry.id {
        vars.insert("id".into(), id.0.into());
    }
    if dirty.intersects(Fields::STATUS | Fields::REWATCHING) {
        // AniList has no separate rewatching flag; it is a status of its own.
        let status = if entry.rewatching {
            Some("REPEATING")
        } else {
            status_name(entry.status)
        };
        if let Some(status) = status {
            vars.insert("status".into(), status.into());
        }
    }
    if dirty.contains(Fields::SCORE) {
        vars.insert("scoreRaw".into(), entry.score.into());
    }
    if dirty.contains(Fields::EPISODE) {
        vars.insert("progress".into(), entry.watched_episodes.into());
    }
    if dirty.contains(Fields::REWATCHED_TIMES) {
        vars.insert("repeat".into(), entry.rewatched_times.into());
    }
    if dirty.contains(Fields::NOTES) {
        vars.insert("notes".into(), entry.notes.as_str().into());
    }
    if dirty.contains(Fields::DATE_STARTED) {
        vars.insert("startedAt".into(), date_input(entry.date_started));
    }
    if dirty.contains(Fields::DATE_COMPLETED) {
        vars.insert("completedAt".into(), date_input(entry.date_completed));
    }
    Value::Object(vars)
}

// ---- Search ---------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct PageData {
    #[serde(rename = "Page")]
    pub page: PageJson,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PageJson {
    pub media: Vec<MediaJson>,
    pub page_info: PageInfoJson,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PageInfoJson {
    pub has_next_page: bool,
}

pub(crate) fn search_variables(params: &SearchParams, page: u32) -> Value {
    let mut vars = Map::new();
    vars.insert("page".into(), page.into());
    let text = params.text.trim();
    if !text.is_empty() {
        vars.insert("query".into(), text.into());
    }
    let season = match params.season {
        Some(SeasonName::Winter) => Some("WINTER"),
        Some(SeasonName::Spring) => Some("SPRING"),
        Some(SeasonName::Summer) => Some("SUMMER"),
        Some(SeasonName::Fall) => Some("FALL"),
        Some(SeasonName::Unknown) | None => None,
    };
    if let Some(season) = season {
        vars.insert("season".into(), season.into());
    }
    if let Some(year) = params.year {
        vars.insert("seasonYear".into(), year.into());
    }
    let format = match params.kind {
        Some(AnimeType::Tv) => Some("TV"),
        Some(AnimeType::Ova) => Some("OVA"),
        Some(AnimeType::Movie) => Some("MOVIE"),
        Some(AnimeType::Special) => Some("SPECIAL"),
        Some(AnimeType::Ona) => Some("ONA"),
        Some(AnimeType::Music) => Some("MUSIC"),
        Some(AnimeType::Unknown) | None => None,
    };
    if let Some(format) = format {
        vars.insert("format".into(), format.into());
    }
    let status = match params.status {
        Some(AiringStatus::FinishedAiring) => Some("FINISHED"),
        Some(AiringStatus::Airing) => Some("RELEASING"),
        Some(AiringStatus::NotYetAired) => Some("NOT_YET_RELEASED"),
        Some(AiringStatus::Unknown) | None => None,
    };
    if let Some(status) = status {
        vars.insert("status".into(), status.into());
    }
    vars.insert("sort".into(), json!([search_sort(params)]));
    Value::Object(vars)
}

fn search_sort(params: &SearchParams) -> String {
    // With search text, relevance is the only order that makes sense; the
    // chosen column sort applies to browsing.
    if !params.text.trim().is_empty() {
        return "SEARCH_MATCH".into();
    }
    let Some(sort) = params.sort else {
        return "START_DATE".into();
    };
    let name = match sort {
        SearchSort::Title => "TITLE_ROMAJI",
        SearchSort::Duration => "DURATION",
        SearchSort::Score => "SCORE",
        SearchSort::Type => "FORMAT",
        SearchSort::StartDate => "START_DATE",
    };
    if params.descending {
        format!("{name}_DESC")
    } else {
        name.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEDIA: &str = include_str!("../../tests/data/anilist/media.json");
    const MEDIA_NOT_FOUND: &str = include_str!("../../tests/data/anilist/media-not-found.json");
    const INVALID_TOKEN: &str = include_str!("../../tests/data/anilist/invalid-token.json");
    const VALIDATION: &str = include_str!("../../tests/data/anilist/validation.json");
    const LIST: &str = include_str!("../../tests/data/anilist/list.json");
    const SEARCH: &str = include_str!("../../tests/data/anilist/search.json");
    const VIEWER: &str = include_str!("../../tests/data/anilist/viewer.json");

    fn now() -> Timestamp {
        "2026-11-11T12:00:00Z".parse().unwrap()
    }

    fn ok<T: DeserializeOwned>(body: &str) -> T {
        decode(StatusCode::OK, None, body.as_bytes(), "fallback").unwrap()
    }

    #[test]
    fn media_mapping() {
        let data: MediaData = ok(MEDIA);
        let anime = to_anime(data.media.unwrap(), now()).unwrap();
        assert_eq!(anime.id, AnimeId(4224));
        assert_eq!(anime.mal_id, Some(4224));
        assert_eq!(anime.last_modified, Some(now()));
        assert_eq!(anime.titles.romaji, "Toradora!");
        assert_eq!(anime.titles.english, "Toradora!");
        assert_eq!(anime.titles.japanese, "とらドラ！");
        assert!(anime.titles.synonyms.contains(&"Tiger X Dragon".to_owned()));
        assert_eq!(anime.kind, AnimeType::Tv);
        assert_eq!(anime.status, AiringStatus::FinishedAiring);
        assert_eq!(anime.episode_count, Some(25));
        assert_eq!(anime.episode_length, Some(24));
        assert_eq!(anime.date_started, FuzzyDate::new(2008, 10, 2));
        assert_eq!(anime.date_finished, FuzzyDate::new(2009, 3, 26));
        assert_eq!(anime.score, 7.7);
        assert_eq!(anime.popularity_rank, 551598);
        assert_eq!(anime.trailer_id, "ya570uUgQNc");
        assert!(anime.image_url.ends_with("bx4224-PXVMBLNwy2aF.jpg"));
        assert!(anime.synopsis.contains("<br>"));
        assert_eq!(
            anime.genres,
            ["Comedy", "Drama", "Romance", "Slice of Life"]
        );
        assert!(anime.tags.contains(&"Tsundere".to_owned()));
        // Spoiler tags are left out.
        assert!(!anime.tags.contains(&"Tragedy".to_owned()));
        assert_eq!(anime.studios, ["J.C.STAFF"]);
        assert!(anime.producers.contains(&"Genco".to_owned()));
        assert!(!anime.producers.contains(&"J.C.STAFF".to_owned()));
        assert_eq!(anime.next_episode_time, None);
        assert_eq!(anime.last_aired_episode, 0);
    }

    #[test]
    fn non_japanese_native_title_becomes_synonym() {
        let media = MediaJson {
            id: Some(1),
            country_of_origin: Some("CN".into()),
            title: Some(TitleJson {
                native: Some("天官赐福".into()),
                ..TitleJson::default()
            }),
            next_airing_episode: Some(NextEpisodeJson {
                airing_at: Some(1_800_000_000),
                episode: Some(5),
            }),
            ..MediaJson::default()
        };
        let anime = to_anime(media, now()).unwrap();
        assert_eq!(anime.titles.japanese, "");
        assert_eq!(anime.titles.synonyms, ["天官赐福"]);
        assert_eq!(anime.last_aired_episode, 4);
        assert_eq!(
            anime.next_episode_time,
            Some(Timestamp::from_second(1_800_000_000).unwrap())
        );
    }

    #[test]
    fn media_without_id_is_skipped() {
        assert!(to_anime(MediaJson::default(), now()).is_none());
        let zero = MediaJson {
            id: Some(0),
            ..MediaJson::default()
        };
        assert!(to_anime(zero, now()).is_none());
    }

    #[test]
    fn list_mapping() {
        let data: CollectionData = ok(LIST);
        let entries: Vec<_> = data
            .collection
            .unwrap()
            .lists
            .into_iter()
            .flat_map(|l| l.entries)
            .filter_map(|e| to_entry(e, now()))
            .collect();
        // The entry without media is skipped; the one in two lists appears
        // twice, which storing by anime id makes harmless.
        assert_eq!(entries.len(), 3);
        let (watching, anime) = &entries[0];
        assert_eq!(watching.id, Some(ListEntryId(111)));
        assert_eq!(watching.anime_id, AnimeId(4224));
        assert_eq!(watching.status, ListStatus::Watching);
        assert!(!watching.rewatching);
        assert_eq!(watching.watched_episodes, 7);
        assert_eq!(watching.score, 85);
        assert_eq!(watching.rewatched_times, 1);
        assert!(watching.is_private);
        assert_eq!(watching.notes, "good");
        assert_eq!(watching.date_started, FuzzyDate::new(2026, 10, 0));
        assert_eq!(watching.date_completed, FuzzyDate::default());
        assert_eq!(
            watching.last_updated,
            Timestamp::from_second(1_790_000_000).unwrap()
        );
        assert_eq!(anime.as_ref().unwrap().titles.romaji, "Toradora!");

        let (repeating, _) = &entries[1];
        assert_eq!(repeating.status, ListStatus::Watching);
        assert!(repeating.rewatching);
        assert_eq!(repeating.notes, "");
        assert_eq!(entries[2].0.anime_id, AnimeId(4224));
    }

    #[test]
    fn viewer() {
        let data: ViewerData = ok(VIEWER);
        assert_eq!(data.viewer.id, 123);
        assert_eq!(data.viewer.name, "someone");
        assert_eq!(
            data.viewer
                .media_list_options
                .unwrap()
                .score_format
                .as_deref(),
            Some("POINT_10_DECIMAL")
        );
    }

    #[test]
    fn search_page() {
        let data: PageData = ok(SEARCH);
        assert!(!data.page.page_info.has_next_page);
        let ids: Vec<_> = data
            .page
            .media
            .into_iter()
            .filter_map(|m| to_anime(m, now()))
            .map(|a| a.id.0)
            .collect();
        assert_eq!(ids[0], 4224);
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn errors() {
        let decode_err = |status: u16, body: &str| {
            decode::<Value>(
                StatusCode::from_u16(status).unwrap(),
                Some("nginx"),
                body.as_bytes(),
                "Could not parse anime list.",
            )
            .unwrap_err()
        };
        assert_eq!(decode_err(404, MEDIA_NOT_FOUND), ServiceError::NotFound);
        assert_eq!(
            decode_err(400, INVALID_TOKEN),
            ServiceError::Unauthorized(EXPIRED_TOKEN.into())
        );
        assert_eq!(
            decode_err(400, VALIDATION).to_string(),
            "Validation error: The score raw may not be greater than 100."
        );
        assert_eq!(
            decode_err(500, r#"{"errors":[{"message":"Internal Server Error"}]}"#),
            ServiceError::Api("Internal Server Error".into())
        );
        // No message from AniList: the fixed text for the operation.
        assert_eq!(
            decode_err(502, "<html>Bad gateway</html>"),
            ServiceError::Api("Could not parse anime list.".into())
        );
        assert_eq!(
            decode_err(200, r#"{"data":{"Viewer":"#),
            ServiceError::Parse("Could not parse anime list.".into())
        );
        let ddos = decode::<Value>(
            StatusCode::SERVICE_UNAVAILABLE,
            Some("cloudflare"),
            INVALID_TOKEN.as_bytes(),
            "x",
        )
        .unwrap_err();
        assert!(matches!(ddos, ServiceError::DdosProtection { .. }));
    }

    #[test]
    fn save_sends_only_dirty_fields() {
        let mut entry = ListEntry::new(AnimeId(4224));
        entry.watched_episodes = 3;
        entry.score = 70;
        entry.status = ListStatus::Watching;
        entry.notes = "n".into();
        entry.date_started = FuzzyDate::new(2026, 11, 0);
        let vars = save_variables(&entry, Fields::EPISODE);
        assert_eq!(vars, json!({ "mediaId": 4224, "progress": 3 }));

        entry.id = Some(ListEntryId(9));
        let vars = save_variables(
            &entry,
            Fields::STATUS | Fields::SCORE | Fields::NOTES | Fields::DATE_STARTED,
        );
        assert_eq!(
            vars,
            json!({
                "mediaId": 4224, "id": 9, "status": "CURRENT", "scoreRaw": 70, "notes": "n",
                "startedAt": { "year": 2026, "month": 11, "day": 0 },
            })
        );

        entry.rewatching = true;
        entry.rewatched_times = 2;
        let vars = save_variables(&entry, Fields::REWATCHING | Fields::REWATCHED_TIMES);
        assert_eq!(
            vars,
            json!({ "mediaId": 4224, "id": 9, "status": "REPEATING", "repeat": 2 })
        );
    }

    #[test]
    fn search_variables_by_case() {
        let params = SearchParams {
            text: "toradora".into(),
            sort: Some(SearchSort::Score),
            ..SearchParams::default()
        };
        assert_eq!(
            search_variables(&params, 1),
            json!({ "page": 1, "query": "toradora", "sort": ["SEARCH_MATCH"] })
        );

        let params = SearchParams {
            year: Some(2026),
            season: Some(SeasonName::Fall),
            kind: Some(AnimeType::Tv),
            status: Some(AiringStatus::Airing),
            sort: Some(SearchSort::Score),
            descending: true,
            ..SearchParams::default()
        };
        assert_eq!(
            search_variables(&params, 2),
            json!({
                "page": 2, "season": "FALL", "seasonYear": 2026, "format": "TV",
                "status": "RELEASING", "sort": ["SCORE_DESC"],
            })
        );

        let params = SearchParams::default();
        assert_eq!(
            search_variables(&params, 1),
            json!({ "page": 1, "sort": ["START_DATE"] })
        );
    }
}
