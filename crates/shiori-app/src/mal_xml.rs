//! MyAnimeList's XML list format: what MyAnimeList exports and imports, what
//! AniList imports, and what Taiga exports. A list kept on this computer
//! moves in and out through it.

use std::io::Read as _;

use anyhow::{Context as _, Result, bail};
use jiff::Timestamp;
use shiori_core::entry::is_in_list;
use shiori_core::{AnimeId, AnimeType, FuzzyDate, ListEntry, ListStatus};

use crate::state::AppState;

/// One anime in an XML list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlEntry {
    pub mal_id: u32,
    pub title: String,
    pub watched: u32,
    pub status: ListStatus,
    /// 0 to 10, as MyAnimeList scores; 0 is no score.
    pub score: u8,
    pub started: FuzzyDate,
    pub finished: FuzzyDate,
    pub times_watched: u32,
    pub rewatching: bool,
    pub rewatching_ep: u32,
    pub comments: String,
}

impl XmlEntry {
    /// The list entry for the same anime on the service.
    pub fn to_entry(&self, anime: AnimeId, now: Timestamp) -> ListEntry {
        let mut entry = ListEntry::new(anime);
        entry.watched_episodes = self.watched;
        entry.status = self.status;
        entry.score = self.score.min(10) * 10;
        entry.date_started = self.started;
        entry.date_completed = self.finished;
        entry.rewatched_times = self.times_watched;
        entry.rewatching = self.rewatching;
        entry.rewatching_ep = self.rewatching_ep;
        entry.notes = self.comments.clone();
        entry.last_updated = now;
        entry
    }
}

/// Reads an XML list, gzipped (as MyAnimeList exports it) or not.
pub fn parse(bytes: &[u8]) -> Result<Vec<XmlEntry>> {
    let text = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut text = String::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_string(&mut text)
            .context("the file looks gzipped but could not be unpacked")?;
        text
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let doc = roxmltree::Document::parse(&text).context("the file is not valid XML")?;
    let root = doc.root_element();
    if !root.has_tag_name("myanimelist") {
        bail!("the file is not a MyAnimeList list export");
    }
    let entries = root
        .children()
        .filter(|n| n.has_tag_name("anime"))
        .filter_map(|anime| {
            let text = |tag: &str| -> String {
                anime
                    .children()
                    .find(|n| n.has_tag_name(tag))
                    .and_then(|n| n.text())
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            };
            let number = |tag: &str| text(tag).parse::<u32>().unwrap_or(0);
            let mal_id = number("series_animedb_id");
            if mal_id == 0 {
                return None;
            }
            Some(XmlEntry {
                mal_id,
                title: text("series_title"),
                watched: number("my_watched_episodes"),
                status: status_from_xml(&text("my_status")),
                score: u8::try_from(number("my_score")).unwrap_or(0).min(10),
                started: FuzzyDate::parse(&text("my_start_date")),
                finished: FuzzyDate::parse(&text("my_finish_date")),
                times_watched: number("my_times_watched"),
                rewatching: number("my_rewatching") != 0,
                rewatching_ep: number("my_rewatching_ep"),
                comments: text("my_comments"),
            })
        })
        .collect();
    Ok(entries)
}

/// Names as MyAnimeList writes them; older exports, Taiga's among them,
/// use numbers.
fn status_from_xml(text: &str) -> ListStatus {
    match text.to_ascii_lowercase().as_str() {
        "watching" | "1" => ListStatus::Watching,
        "completed" | "2" => ListStatus::Completed,
        "on-hold" | "on hold" | "3" => ListStatus::OnHold,
        "dropped" | "4" => ListStatus::Dropped,
        "plan to watch" | "6" => ListStatus::PlanToWatch,
        // An entry without a known status still belongs in the list.
        _ => ListStatus::PlanToWatch,
    }
}

fn status_to_xml(status: ListStatus) -> &'static str {
    match status {
        ListStatus::Watching => "Watching",
        ListStatus::Completed => "Completed",
        ListStatus::OnHold => "On-Hold",
        ListStatus::Dropped => "Dropped",
        ListStatus::PlanToWatch | ListStatus::NotInList => "Plan to Watch",
    }
}

fn type_to_xml(kind: AnimeType) -> &'static str {
    match kind {
        AnimeType::Tv => "TV",
        AnimeType::Movie => "Movie",
        AnimeType::Ova => "OVA",
        AnimeType::Ona => "ONA",
        AnimeType::Special => "Special",
        AnimeType::Music => "Music",
        _ => "Unknown",
    }
}

/// Text as CDATA. "]]>" would end the section early, so it is split across
/// two sections.
fn cdata(text: &str) -> String {
    format!("<![CDATA[{}]]>", text.replace("]]>", "]]]]><![CDATA[>"))
}

/// The list as an XML export. Anime without a known MyAnimeList id cannot be
/// written; they are returned so the user can be told.
pub fn write(state: &AppState) -> (String, Vec<AnimeId>) {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<myanimelist>\n  <myinfo>\n    \
         <user_export_type>1</user_export_type>\n  </myinfo>\n",
    );
    let mut missing = Vec::new();
    for entry in state.entries.values().filter(|e| is_in_list(Some(e))) {
        let anime = state.anime.get(&entry.anime_id);
        let Some(mal_id) = anime.and_then(|a| a.mal_id) else {
            missing.push(entry.anime_id);
            continue;
        };
        let kind = anime.map_or(AnimeType::Unknown, |a| a.kind);
        let episodes = anime.and_then(|a| a.episode_count).unwrap_or(0);
        // Rounded to the nearest whole point, as MyAnimeList scores are.
        let score = (u32::from(entry.score) + 5) / 10;
        out.push_str(&format!(
            "  <anime>\n    <series_animedb_id>{mal_id}</series_animedb_id>\n    \
             <series_title>{}</series_title>\n    <series_type>{}</series_type>\n    \
             <series_episodes>{episodes}</series_episodes>\n    <my_id>0</my_id>\n    \
             <my_watched_episodes>{}</my_watched_episodes>\n    \
             <my_start_date>{}</my_start_date>\n    <my_finish_date>{}</my_finish_date>\n    \
             <my_score>{score}</my_score>\n    <my_status>{}</my_status>\n    \
             <my_comments>{}</my_comments>\n    <my_times_watched>{}</my_times_watched>\n    \
             <my_rewatching>{}</my_rewatching>\n    <my_rewatching_ep>{}</my_rewatching_ep>\n    \
             <update_on_import>1</update_on_import>\n  </anime>\n",
            cdata(&state.title(entry.anime_id)),
            type_to_xml(kind),
            entry.watched_episodes,
            entry.date_started,
            entry.date_completed,
            status_to_xml(entry.status),
            cdata(&entry.notes),
            entry.rewatched_times,
            u8::from(entry.rewatching),
            entry.rewatching_ep,
        ));
    }
    out.push_str("</myanimelist>\n");
    (out, missing)
}

#[cfg(test)]
mod tests {
    use shiori_core::Anime;
    use shiori_store::{Accounts, Database, Settings};

    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<myanimelist>
  <myinfo><user_export_type>1</user_export_type></myinfo>
  <anime>
    <series_animedb_id>4224</series_animedb_id>
    <series_title><![CDATA[Toradora!]]></series_title>
    <my_watched_episodes>25</my_watched_episodes>
    <my_start_date>2020-01-05</my_start_date>
    <my_finish_date>0000-00-00</my_finish_date>
    <my_score>9</my_score>
    <my_status>Completed</my_status>
    <my_comments><![CDATA[Taiga & Ryuuji]]></my_comments>
    <my_times_watched>1</my_times_watched>
  </anime>
  <anime>
    <series_animedb_id>5081</series_animedb_id>
    <series_title>Bakemonogatari</series_title>
    <my_watched_episodes>3</my_watched_episodes>
    <my_status>3</my_status>
  </anime>
  <anime><series_title>No id</series_title></anime>
</myanimelist>"#;

    #[test]
    fn reads_names_numbers_and_cdata() {
        let entries = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(entries.len(), 2, "the anime without an id is skipped");
        let toradora = &entries[0];
        assert_eq!(toradora.mal_id, 4224);
        assert_eq!(toradora.title, "Toradora!");
        assert_eq!(toradora.status, ListStatus::Completed);
        assert_eq!(toradora.score, 9);
        assert_eq!(toradora.started, FuzzyDate::new(2020, 1, 5));
        assert!(toradora.finished.is_empty());
        assert_eq!(toradora.comments, "Taiga & Ryuuji");
        assert_eq!(entries[1].status, ListStatus::OnHold);

        let entry = toradora.to_entry(AnimeId(1), Timestamp::UNIX_EPOCH);
        assert_eq!(entry.score, 90);
        assert_eq!(entry.watched_episodes, 25);
        assert_eq!(entry.rewatched_times, 1);
    }

    #[test]
    fn reads_gzipped_exports_and_refuses_other_files() {
        use std::io::Write as _;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(SAMPLE.as_bytes()).unwrap();
        let bytes = gz.finish().unwrap();
        assert_eq!(parse(&bytes).unwrap().len(), 2);
        assert!(parse(b"<html></html>").is_err());
        assert!(parse(b"not xml").is_err());
    }

    #[test]
    fn writes_what_it_reads_back() {
        let mut state = AppState::new(
            Database::open_in_memory().unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap();
        let mut known = Anime::new(AnimeId(1));
        known.mal_id = Some(4224);
        known.titles.romaji = "Toradora!".into();
        known.kind = AnimeType::Tv;
        known.episode_count = Some(25);
        state
            .store_anime(vec![known, Anime::new(AnimeId(2))])
            .unwrap();
        let mut entry = ListEntry::new(AnimeId(1));
        entry.status = ListStatus::Watching;
        entry.watched_episodes = 7;
        entry.score = 85;
        entry.notes = "odd ]]> note".into();
        entry.date_started = FuzzyDate::new(2026, 11, 0);
        state.save_entry(entry, Timestamp::UNIX_EPOCH).unwrap();
        let mut other = ListEntry::new(AnimeId(2));
        other.status = ListStatus::PlanToWatch;
        state.save_entry(other, Timestamp::UNIX_EPOCH).unwrap();

        let (xml, missing) = write(&state);
        assert_eq!(missing, [AnimeId(2)]);
        let back = parse(xml.as_bytes()).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].mal_id, 4224);
        assert_eq!(back[0].title, "Toradora!");
        assert_eq!(back[0].status, ListStatus::Watching);
        assert_eq!(back[0].watched, 7);
        assert_eq!(back[0].score, 9);
        assert_eq!(back[0].comments, "odd ]]> note");
        assert_eq!(back[0].started, FuzzyDate::new(2026, 11, 0));
    }
}
