//! How values are written in the UI. Every user-visible string the views
//! build goes through here or `text`, so translations can be added later.

use shiori_core::anime::airing_status;
use shiori_core::date::Season;
use shiori_core::{AiringStatus, Anime, AnimeType, FuzzyDate, ListEntry, SeasonName};
use shiori_sync::anilist::score::{ScoreFormat, format_rating};

use shiori_app::list_view::ListColumn;
use shiori_app::state::AppState;

pub use shiori_app::list_view::status_label;

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

/// Today's date in Japan, where airing dates are given. Japan has no
/// daylight saving time, so a fixed offset is exact and needs no time zone
/// database.
pub fn today_in_japan() -> jiff::civil::Date {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(9)))
        .date()
}

pub fn airing_label(anime: &Anime, today_in_japan: jiff::civil::Date) -> &'static str {
    match airing_status(anime, today_in_japan) {
        AiringStatus::FinishedAiring => "Finished airing",
        AiringStatus::Airing => "Currently airing",
        AiringStatus::NotYetAired => "Not yet aired",
        AiringStatus::Unknown => "",
    }
}

/// A synopsis as plain text. Services send some HTML: line breaks become
/// new lines, other tags are dropped, and the common entities are decoded.
pub fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            // A lone "<" is text, not a tag.
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = rest[start + 1..start + end].trim().to_ascii_lowercase();
        if tag.starts_with("br") || tag == "/p" {
            out.push('\n');
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    let out = out
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&amp;", "&");
    // AniList often sends "<br>\n", which would leave blank lines doubled.
    let mut text = String::with_capacity(out.len());
    let mut newlines = 0;
    for c in out.trim().chars() {
        if c == '\n' {
            newlines += 1;
            if newlines > 2 {
                continue;
            }
        } else {
            newlines = 0;
        }
        text.push(c);
    }
    text
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
        // Sorts only; the table never shows them as columns.
        ListColumn::NewEpisodes | ListColumn::NextAiring => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synopsis_html_becomes_text() {
        let html = "A <i>story</i> of &quot;Tom &amp; Jerry&quot;.<br>\n<br>\n(Source: AniList)";
        assert_eq!(
            plain_text(html),
            "A story of \"Tom & Jerry\".\n\n(Source: AniList)"
        );
        assert_eq!(plain_text("1 < 2"), "1 < 2");
    }
}
