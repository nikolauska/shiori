//! MyAnimeList's JSON answers, its error rules, and how its fields map onto
//! Shiori's types. See docs/sync-myanimelist.md.

use jiff::Timestamp;
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use shiori_core::{
    AgeRating, AiringStatus, Anime, AnimeId, AnimeType, Fields, FuzzyDate, ListEntry, ListStatus,
};

use super::score::{parse_list_score, to_list_score};
use crate::ServiceError;
use crate::error::ddos_protection;

pub(crate) const ANIME_FIELDS: &str = "alternative_titles,average_episode_duration,end_date,\
    genres,id,main_picture,mean,media_type,num_episodes,popularity,rating,start_date,status,\
    studios,synopsis,title";

pub(crate) const LIST_STATUS_FIELDS: &str = "comments,finish_date,is_rewatching,\
    num_times_rewatched,num_watched_episodes,score,start_date,status,updated_at";

/// What came back, kept until we know whether it is an error.
pub(crate) struct Reply {
    pub status: StatusCode,
    pub server: Option<String>,
    pub www_authenticate: Option<String>,
    pub body: Vec<u8>,
}

impl Reply {
    /// MyAnimeList's sign that the access token expired and a refresh may
    /// help, as opposed to a token that was never valid for this request.
    pub fn token_expired(&self) -> bool {
        self.status == StatusCode::UNAUTHORIZED
            && self
                .www_authenticate
                .as_deref()
                .and_then(|h| header_value(h, "error"))
                .as_deref()
                == Some("invalid_token")
    }
}

/// `key="value"` from a `WWW-Authenticate` header.
fn header_value(header: &str, key: &str) -> Option<String> {
    let start = header.find(&format!("{key}=\""))? + key.len() + 2;
    let rest = &header[start..];
    Some(rest[..rest.find('"')?].to_owned())
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ErrorJson {
    error: Option<String>,
    message: Option<String>,
    hint: Option<String>,
}

/// The message to show for a failed request, tried in the order the spec
/// gives: the header's description, then the body's message and hint.
fn error_message(reply: &Reply) -> Option<String> {
    if let Some(description) = reply
        .www_authenticate
        .as_deref()
        .and_then(|h| header_value(h, "error_description"))
        .filter(|d| !d.is_empty())
    {
        return Some(description);
    }
    let json: ErrorJson = serde_json::from_slice(&reply.body).ok()?;
    let message = json.message.filter(|m| !m.is_empty());
    let hint = json.hint.filter(|h| !h.is_empty());
    match (message, hint) {
        (Some(m), Some(h)) => Some(format!("{m} ({h})")),
        (Some(m), None) => Some(m),
        (None, Some(h)) => Some(h),
        (None, None) if json.error.as_deref() == Some("forbidden") => {
            Some("Please set up your account via Settings.".to_owned())
        }
        (None, None) => None,
    }
}

/// Turns an answer into `T`, or the error to show. `fallback` is used when
/// something went wrong and MyAnimeList gave no message.
pub(crate) fn decode<T: DeserializeOwned>(
    reply: &Reply,
    fallback: &str,
) -> Result<T, ServiceError> {
    check(reply, fallback)?;
    serde_json::from_slice(&reply.body).map_err(|_| ServiceError::Parse(fallback.to_owned()))
}

/// The error for a failed answer; `Ok` for a successful one.
pub(crate) fn check(reply: &Reply, fallback: &str) -> Result<(), ServiceError> {
    if let Some(error) = ddos_protection(reply.status, reply.server.as_deref()) {
        return Err(error);
    }
    if reply.status == StatusCode::NOT_FOUND {
        return Err(ServiceError::NotFound);
    }
    if reply.status.is_success() {
        return Ok(());
    }
    let message = error_message(reply).unwrap_or_else(|| fallback.to_owned());
    if reply.status == StatusCode::UNAUTHORIZED {
        return Err(ServiceError::Unauthorized(message));
    }
    Err(ServiceError::Api(message))
}

/// The `offset` of the URL in `paging.next`, if there is a next page.
pub(crate) fn next_offset(paging: &PagingJson) -> Option<u32> {
    let url = Url::parse(paging.next.as_deref()?).ok()?;
    url.query_pairs()
        .find(|(k, _)| k == "offset")
        .and_then(|(_, v)| v.parse().ok())
}

// ---- Tokens and users -------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct TokenJson {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub(crate) struct UserJson {
    pub id: u64,
    pub name: String,
}

// ---- Anime ------------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct AnimeJson {
    id: Option<u32>,
    title: Option<String>,
    alternative_titles: Option<AlternativeTitlesJson>,
    average_episode_duration: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
    genres: Option<Vec<NamedJson>>,
    studios: Option<Vec<NamedJson>>,
    main_picture: Option<PictureJson>,
    mean: Option<f32>,
    media_type: Option<String>,
    num_episodes: Option<u32>,
    popularity: Option<u32>,
    rating: Option<String>,
    status: Option<String>,
    synopsis: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AlternativeTitlesJson {
    en: Option<String>,
    ja: Option<String>,
    synonyms: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct NamedJson {
    name: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct PictureJson {
    large: Option<String>,
    medium: Option<String>,
}

/// `YYYY-MM-DD`, `YYYY-MM` or `YYYY`; missing parts are unknown.
fn parse_date(text: Option<&str>) -> FuzzyDate {
    let mut parts = text.unwrap_or_default().split('-').map(|p| p.parse().ok());
    let year: Option<u16> = parts.next().flatten();
    let month: Option<u16> = parts.next().flatten();
    let day: Option<u16> = parts.next().flatten();
    FuzzyDate::new(
        year.unwrap_or(0),
        month.and_then(|m| u8::try_from(m).ok()).unwrap_or(0),
        day.and_then(|d| u8::try_from(d).ok()).unwrap_or(0),
    )
}

fn names(list: Option<Vec<NamedJson>>) -> Vec<String> {
    list.into_iter()
        .flatten()
        .filter_map(|n| n.name)
        .filter(|n| !n.is_empty())
        .collect()
}

/// `None` when the object has no usable id.
pub(crate) fn to_anime(m: AnimeJson, now: Timestamp) -> Option<Anime> {
    let id = m.id.filter(|&id| id != 0)?;
    let mut anime = Anime::new(AnimeId(id));
    anime.last_modified = Some(now);
    // The id is the MyAnimeList id here. Setting it as well lets the code
    // that looks anime up on AniList by MyAnimeList id treat both services
    // the same.
    anime.mal_id = Some(id);
    anime.episode_count = m.num_episodes.filter(|&n| n != 0);
    anime.episode_length = m
        .average_episode_duration
        .map(|seconds| seconds / 60)
        .filter(|&minutes| minutes != 0);
    anime.age_rating = match m.rating.as_deref() {
        Some("g") => AgeRating::G,
        Some("pg") => AgeRating::Pg,
        Some("pg_13") => AgeRating::Pg13,
        Some("r" | "r+") => AgeRating::R17,
        Some("rx") => AgeRating::R18,
        _ => AgeRating::Unknown,
    };
    anime.status = match m.status.as_deref() {
        Some("currently_airing") => AiringStatus::Airing,
        Some("finished_airing") => AiringStatus::FinishedAiring,
        Some("not_yet_aired") => AiringStatus::NotYetAired,
        _ => AiringStatus::Unknown,
    };
    anime.kind = match m.media_type.as_deref() {
        Some("tv") => AnimeType::Tv,
        Some("ova") => AnimeType::Ova,
        Some("movie") => AnimeType::Movie,
        Some("special" | "cm" | "pv" | "tv_special") => AnimeType::Special,
        Some("ona") => AnimeType::Ona,
        Some("music") => AnimeType::Music,
        _ => AnimeType::Unknown,
    };
    anime.date_started = parse_date(m.start_date.as_deref());
    anime.date_finished = parse_date(m.end_date.as_deref());
    anime.score = m.mean.unwrap_or(0.0);
    anime.popularity_rank = m.popularity.unwrap_or(0);
    anime.image_url = m
        .main_picture
        .and_then(|p| p.large.or(p.medium))
        .unwrap_or_default();
    anime.synopsis = m.synopsis.unwrap_or_default();
    anime.titles.romaji = m.title.unwrap_or_default();
    let alt = m.alternative_titles.unwrap_or_default();
    anime.titles.english = alt.en.unwrap_or_default();
    anime.titles.japanese = alt.ja.unwrap_or_default();
    anime.titles.synonyms = alt
        .synonyms
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    anime.genres = names(m.genres);
    anime.studios = names(m.studios);
    Some(anime)
}

// ---- List entries -------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct ListStatusJson {
    status: Option<String>,
    score: Option<u8>,
    // The API returns this spelling but takes `num_watched_episodes`.
    num_episodes_watched: Option<u32>,
    is_rewatching: Option<bool>,
    num_times_rewatched: Option<u32>,
    start_date: Option<String>,
    finish_date: Option<String>,
    updated_at: Option<String>,
    comments: Option<String>,
}

/// MyAnimeList entries have no id of their own; the anime id is the key.
pub(crate) fn to_entry(anime_id: AnimeId, s: ListStatusJson) -> ListEntry {
    let mut entry = ListEntry::new(anime_id);
    entry.watched_episodes = s.num_episodes_watched.unwrap_or(0);
    entry.score = parse_list_score(s.score.unwrap_or(0));
    entry.status = match s.status.as_deref() {
        Some("watching") => ListStatus::Watching,
        Some("completed") => ListStatus::Completed,
        Some("on_hold") => ListStatus::OnHold,
        Some("dropped") => ListStatus::Dropped,
        Some("plan_to_watch") => ListStatus::PlanToWatch,
        _ => ListStatus::NotInList,
    };
    entry.rewatching = s.is_rewatching.unwrap_or(false);
    entry.rewatched_times = s.num_times_rewatched.unwrap_or(0);
    entry.date_started = parse_date(s.start_date.as_deref());
    entry.date_completed = parse_date(s.finish_date.as_deref());
    entry.last_updated = s
        .updated_at
        .and_then(|t| t.parse().ok())
        .unwrap_or(Timestamp::UNIX_EPOCH);
    entry.notes = s.comments.unwrap_or_default();
    entry
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct ListPageJson {
    pub data: Vec<ListItemJson>,
    pub paging: PagingJson,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct ListItemJson {
    pub node: AnimeJson,
    pub list_status: Option<ListStatusJson>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct PagingJson {
    pub next: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct SearchPageJson {
    pub data: Vec<SearchItemJson>,
    pub paging: PagingJson,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct SearchItemJson {
    pub node: AnimeJson,
}

fn status_name(status: ListStatus) -> &'static str {
    match status {
        ListStatus::Watching | ListStatus::NotInList => "watching",
        ListStatus::Completed => "completed",
        ListStatus::OnHold => "on_hold",
        ListStatus::Dropped => "dropped",
        ListStatus::PlanToWatch => "plan_to_watch",
    }
}

/// As much of the date as is known; empty clears it.
fn date_text(date: FuzzyDate) -> String {
    match (date.year, date.month, date.day) {
        (0, _, _) => String::new(),
        (y, 0, _) => format!("{y:04}"),
        (y, m, 0) => format!("{y:04}-{m:02}"),
        (y, m, d) => format!("{y:04}-{m:02}-{d:02}"),
    }
}

/// The `PATCH my_list_status` body. Only dirty fields are sent so that an
/// edit made on the website in the meantime is not overwritten.
pub(crate) fn save_form(entry: &ListEntry, dirty: Fields) -> String {
    let mut pairs: Vec<(&str, String)> = Vec::new();
    if dirty.contains(Fields::EPISODE) {
        pairs.push(("num_watched_episodes", entry.watched_episodes.to_string()));
    }
    if dirty.intersects(Fields::STATUS | Fields::REWATCHING) {
        pairs.push(("status", status_name(entry.status).to_owned()));
        pairs.push(("is_rewatching", entry.rewatching.to_string()));
    }
    if dirty.contains(Fields::SCORE) {
        pairs.push(("score", to_list_score(entry.score).to_string()));
    }
    if dirty.contains(Fields::REWATCHED_TIMES) {
        pairs.push(("num_times_rewatched", entry.rewatched_times.to_string()));
    }
    if dirty.contains(Fields::DATE_STARTED) {
        pairs.push(("start_date", date_text(entry.date_started)));
    }
    if dirty.contains(Fields::DATE_COMPLETED) {
        pairs.push(("finish_date", date_text(entry.date_completed)));
    }
    if dirty.contains(Fields::NOTES) {
        pairs.push(("comments", entry.notes.clone()));
    }
    form(&pairs)
}

/// `application/x-www-form-urlencoded`. Goes through `Url`'s serializer,
/// which encodes `+` and re-encodes existing `%XX` as literal text, as
/// docs/sync.md asks, without another dependency.
pub(crate) fn form(pairs: &[(&str, String)]) -> String {
    let mut url = Url::parse("http://form.invalid/").expect("constant URL is valid");
    url.query_pairs_mut()
        .extend_pairs(pairs.iter().map(|(k, v)| (*k, v.as_str())));
    url.query().unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(status: u16, www: Option<&str>, body: &str) -> Reply {
        Reply {
            status: StatusCode::from_u16(status).unwrap(),
            server: None,
            www_authenticate: www.map(str::to_owned),
            body: body.as_bytes().to_vec(),
        }
    }

    fn now() -> Timestamp {
        "2026-11-22T12:00:00Z".parse().unwrap()
    }

    #[test]
    fn form_encodes_plus_and_percent() {
        let body = form(&[
            ("comments", "1+1 = 2 & 50%20 off".to_owned()),
            ("x", "ä".to_owned()),
        ]);
        assert_eq!(body, "comments=1%2B1+%3D+2+%26+50%2520+off&x=%C3%A4");
    }

    #[test]
    fn save_form_sends_only_dirty_fields() {
        let mut entry = ListEntry::new(AnimeId(1));
        entry.watched_episodes = 7;
        entry.status = ListStatus::Completed;
        entry.rewatching = true;
        entry.score = 85;
        entry.date_started = FuzzyDate::new(2024, 3, 0);
        assert_eq!(save_form(&entry, Fields::EPISODE), "num_watched_episodes=7");
        assert_eq!(
            save_form(&entry, Fields::REWATCHING | Fields::SCORE),
            "status=completed&is_rewatching=true&score=8"
        );
        assert_eq!(
            save_form(&entry, Fields::DATE_STARTED),
            "start_date=2024-03"
        );
        assert_eq!(save_form(&entry, Fields::DATE_COMPLETED), "finish_date=");
    }

    #[test]
    fn dates_with_missing_parts() {
        assert_eq!(parse_date(Some("2008-10-02")), FuzzyDate::new(2008, 10, 2));
        assert_eq!(parse_date(Some("2008-10")), FuzzyDate::new(2008, 10, 0));
        assert_eq!(parse_date(Some("2008")), FuzzyDate::new(2008, 0, 0));
        assert_eq!(parse_date(Some("")), FuzzyDate::default());
        assert_eq!(parse_date(None), FuzzyDate::default());
    }

    #[test]
    fn anime_mapping() {
        let json = r#"{"id":4224,"title":"Toradora!","main_picture":{"medium":"m.jpg","large":"l.jpg"},
            "alternative_titles":{"synonyms":["Tiger X Dragon",""],"en":"Toradora!","ja":"とらドラ！"},
            "start_date":"2008-10-02","end_date":"2009-03","synopsis":"Story.","mean":8.08,
            "popularity":30,"media_type":"tv_special","status":"finished_airing",
            "genres":[{"id":4,"name":"Comedy"},{"id":22,"name":"Romance"}],"num_episodes":25,
            "average_episode_duration":1395,"rating":"r+","studios":[{"id":7,"name":"J.C.Staff"}]}"#;
        let anime = to_anime(serde_json::from_str(json).unwrap(), now()).unwrap();
        assert_eq!(anime.id, AnimeId(4224));
        assert_eq!(anime.mal_id, Some(4224));
        assert_eq!(anime.titles.romaji, "Toradora!");
        assert_eq!(anime.titles.japanese, "とらドラ！");
        assert_eq!(anime.titles.synonyms, ["Tiger X Dragon"]);
        assert_eq!(anime.kind, AnimeType::Special);
        assert_eq!(anime.status, AiringStatus::FinishedAiring);
        assert_eq!(anime.age_rating, AgeRating::R17);
        assert_eq!(anime.episode_count, Some(25));
        assert_eq!(anime.episode_length, Some(23));
        assert_eq!(anime.date_finished, FuzzyDate::new(2009, 3, 0));
        assert_eq!(anime.image_url, "l.jpg");
        assert_eq!(anime.genres, ["Comedy", "Romance"]);
        assert_eq!(anime.studios, ["J.C.Staff"]);
        assert!((anime.score - 8.08).abs() < 0.001);
        assert!(to_anime(serde_json::from_str(r#"{"id":0}"#).unwrap(), now()).is_none());
    }

    #[test]
    fn entry_mapping() {
        let json = r#"{"status":"completed","score":8,"num_episodes_watched":25,"is_rewatching":true,
            "updated_at":"2017-11-11T19:51:22+00:00","start_date":"2017-01-01","finish_date":"2017-11",
            "num_times_rewatched":2,"comments":"Good"}"#;
        let entry = to_entry(AnimeId(4224), serde_json::from_str(json).unwrap());
        assert_eq!(entry.id, None);
        assert_eq!(entry.status, ListStatus::Completed);
        assert_eq!(entry.score, 80);
        assert_eq!(entry.watched_episodes, 25);
        assert!(entry.rewatching);
        assert_eq!(entry.rewatched_times, 2);
        assert_eq!(entry.date_completed, FuzzyDate::new(2017, 11, 0));
        assert_eq!(
            entry.last_updated,
            "2017-11-11T19:51:22Z".parse::<Timestamp>().unwrap()
        );
        assert_eq!(entry.notes, "Good");
    }

    #[test]
    fn error_messages_in_order() {
        let header = r#"Bearer realm="api", error="invalid_token", error_description="The access token expired""#;
        let r = reply(401, Some(header), r#"{"message":"ignored"}"#);
        assert!(r.token_expired());
        assert_eq!(
            check(&r, "fallback"),
            Err(ServiceError::Unauthorized(
                "The access token expired".into()
            ))
        );
        let r = reply(
            400,
            None,
            r#"{"error":"invalid_request","message":"Bad","hint":"Fix it"}"#,
        );
        assert_eq!(
            check(&r, "fallback"),
            Err(ServiceError::Api("Bad (Fix it)".into()))
        );
        let r = reply(400, None, r#"{"hint":"Only hint"}"#);
        assert_eq!(
            check(&r, "fallback"),
            Err(ServiceError::Api("Only hint".into()))
        );
        let r = reply(403, None, r#"{"error":"forbidden"}"#);
        assert_eq!(
            check(&r, "fallback"),
            Err(ServiceError::Api(
                "Please set up your account via Settings.".into()
            ))
        );
        let r = reply(500, None, "<html>");
        assert_eq!(
            check(&r, "fallback"),
            Err(ServiceError::Api("fallback".into()))
        );
        assert_eq!(
            check(&reply(404, None, ""), "x"),
            Err(ServiceError::NotFound)
        );
        assert!(!reply(401, Some(r#"Bearer error="invalid_request""#), "").token_expired());
    }

    #[test]
    fn next_page_offset() {
        let paging = PagingJson {
            next: Some(
                "https://api.myanimelist.net/v2/users/x/animelist?offset=1000&limit=1000".into(),
            ),
        };
        assert_eq!(next_offset(&paging), Some(1000));
        assert_eq!(next_offset(&PagingJson::default()), None);
    }
}
