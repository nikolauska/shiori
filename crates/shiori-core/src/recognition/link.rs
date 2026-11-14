//! Linking a title to an anime by hand: finding anime whose titles are close
//! to one that matched nothing, and remembering the user's choice as an
//! alternative title.

use std::collections::BTreeMap;

use crate::anime::AnimeId;
use crate::date::FuzzyDate;
use crate::entry::{AnimeSettings, LinkedTitle};
use crate::episode::{ElementKind, Episode};
use crate::recognition::{normalize, normalize_words};

/// Words too common in titles to say anything about which anime it is.
const FILLER: [&str; 8] = ["no", "to", "wa", "ga", "ni", "de", "a", "of"];

/// Words that on their own name a part of a series, not a series.
const PART_WORDS: [&str; 8] = ["season", "s", "part", "cour", "ova", "sp", "tv", "movie"];

/// How alike `query` is to `title`, from 0 to about 1.2. Every query word
/// found in the title counts fully, a word that starts the other counts
/// most of the way (for shortened titles: "kusuriya" and "kusuriyano"); a
/// matching `season` adds a little, and each title word left over takes a
/// little away, so the shortest fitting title ranks first.
fn closeness(query: &[String], season: Option<u32>, title: &[String]) -> f32 {
    if query.is_empty() {
        return 0.0;
    }
    let mut found = 0.0;
    let mut used = 0;
    for q in query {
        if title.contains(q) {
            found += 1.0;
            used += 1;
        } else if q.chars().count() >= 3
            && title.iter().any(|t| {
                t.chars().count() >= 3 && (t.starts_with(q.as_str()) || q.starts_with(t.as_str()))
            })
        {
            found += 0.8;
            used += 1;
        }
    }
    let mut score = found / query.len() as f32;
    if let Some(season) = season
        && title.contains(&season.to_string())
    {
        score += 0.2;
    }
    score - 0.01 * title.len().saturating_sub(used) as f32
}

fn meaningful_words(title: &str) -> Vec<String> {
    normalize_words(title)
        .into_iter()
        .filter(|w| !FILLER.contains(&w.as_str()))
        .collect()
}

/// Anime with a title close to `query`, best first, at most `limit`.
/// `titles` gives every title of every anime to consider; an anime counts
/// with its best title. Below half the query's words matched, an anime is
/// not close.
pub fn close_matches<'a>(
    query: &str,
    season: Option<u32>,
    titles: impl IntoIterator<Item = (AnimeId, &'a str)>,
    limit: usize,
) -> Vec<(AnimeId, f32)> {
    let query = meaningful_words(query);
    let mut best: BTreeMap<AnimeId, f32> = BTreeMap::new();
    for (id, title) in titles {
        if title.is_empty() {
            continue;
        }
        let score = closeness(&query, season, &meaningful_words(title));
        if score >= 0.5 {
            let entry = best.entry(id).or_insert(score);
            *entry = entry.max(score);
        }
    }
    let mut found: Vec<_> = best.into_iter().collect();
    found.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    found.truncate(limit);
    found
}

/// Too short or too vague to remember as a title: matching every file with
/// it would link files of other anime too ("S2", "Season 2", "OVA").
pub fn is_generic_title(title: &str) -> bool {
    if normalize(title).chars().count() < 4 {
        return true;
    }
    normalize_words(title)
        .iter()
        .all(|w| PART_WORDS.contains(&w.as_str()) || w.chars().all(|c| c.is_ascii_digit()))
}

/// The title to remember for a parsed episode: its title, with the season
/// when the file names one above 1. Identification looks up "<title> season
/// <n>" first, so "Kusuriya S3 - 07" remembered as "Kusuriya Season 3"
/// matches that season's files without taking "Kusuriya - 05" of the first.
pub fn title_to_remember(episode: &Episode) -> Option<String> {
    let title = episode.title().filter(|t| !t.trim().is_empty())?;
    let season = episode
        .get(ElementKind::Season)
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&s| s > 1);
    Some(match season {
        Some(season) => format!("{title} Season {season}"),
        None => title.to_owned(),
    })
}

/// Makes `title` an alternative title of `id`, and of no other anime: a
/// title links to one anime. Returns the settings that changed, the other
/// anime's first. Titles compare as normalized, so "Kusuriya S3" and
/// "kusuriya s3" are the same link.
pub fn link_title(
    settings: &BTreeMap<AnimeId, AnimeSettings>,
    id: AnimeId,
    title: &str,
    date: FuzzyDate,
) -> Vec<AnimeSettings> {
    let key = normalize(title);
    let same = |other: &str| normalize(other) == key;
    let mut changed = Vec::new();
    for (other_id, other) in settings {
        if *other_id == id || !other.synonyms.iter().any(|s| same(s)) {
            continue;
        }
        let mut other = other.clone();
        other.synonyms.retain(|s| !same(s));
        other.linked.retain(|l| !same(&l.title));
        changed.push(other);
    }
    let mut target = settings
        .get(&id)
        .cloned()
        .unwrap_or_else(|| AnimeSettings::new(id));
    let before = target.clone();
    if !target.synonyms.iter().any(|s| same(s)) {
        target.synonyms.push(title.to_owned());
    }
    if !target.linked.iter().any(|l| same(&l.title)) {
        target.linked.push(LinkedTitle {
            title: title.to_owned(),
            date,
        });
    }
    if target != before {
        changed.push(target);
    }
    changed
}

/// The anime `title` is linked to, if the user linked it to one.
pub fn linked_to(settings: &BTreeMap<AnimeId, AnimeSettings>, title: &str) -> Option<AnimeId> {
    let key = normalize(title);
    settings
        .values()
        .find(|s| s.synonyms.iter().any(|t| normalize(t) == key))
        .map(|s| s.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles() -> Vec<(AnimeId, &'static str)> {
        vec![
            (AnimeId(1), "Kusuriya no Hitorigoto"),
            (AnimeId(1), "The Apothecary Diaries"),
            (AnimeId(2), "Kusuriya no Hitorigoto 2nd Season"),
            (AnimeId(3), "Kusuriya no Hitorigoto 3rd Season"),
            (AnimeId(4), "Sasaki to Pii-chan"),
            (AnimeId(5), "Yani Neko"),
        ]
    }

    #[test]
    fn a_shortened_title_finds_the_series_and_the_season_picks_one() {
        let found = close_matches("Kusuriya", Some(3), titles(), 5);
        let ids: Vec<u32> = found.iter().map(|(id, _)| id.0).collect();
        assert_eq!(ids[0], 3, "{found:?}");
        assert_eq!(ids.len(), 3);
        // Without a season, the shortest title, the first season, wins.
        let found = close_matches("Kusuriya", None, titles(), 5);
        assert_eq!(found[0].0, AnimeId(1), "{found:?}");
    }

    #[test]
    fn other_titles_count_and_unrelated_ones_do_not() {
        let found = close_matches("Apothecary Diaries", None, titles(), 5);
        assert_eq!(found.first().map(|f| f.0), Some(AnimeId(1)));
        assert!(close_matches("Frieren", None, titles(), 5).is_empty());
        assert_eq!(close_matches("Kusuriya", None, titles(), 2).len(), 2);
    }

    #[test]
    fn a_remembered_title_keeps_the_season_and_matches_only_that_season() {
        use crate::anime::Anime;
        use crate::recognition::{ParseOptions, Relations, TitleIndex, identify, parse};
        use crate::service::ServiceId;

        let season3 = parse(
            "[SubsPlease] Kusuriya S3 - 07 (1080p).mkv",
            ParseOptions::default(),
        );
        let title = title_to_remember(&season3).unwrap();
        assert_eq!(title, "Kusuriya Season 3");
        let plain = parse(
            "[SubsPlease] Kusuriya - 05 (1080p).mkv",
            ParseOptions::default(),
        );
        assert_eq!(title_to_remember(&plain).unwrap(), "Kusuriya");

        let mut anime = Anime::new(AnimeId(3));
        anime.titles.romaji = "Kusuriya no Hitorigoto 3rd Season".into();
        anime.episode_count = Some(12);
        let anime = BTreeMap::from([(anime.id, anime)]);
        let changed = link_title(&BTreeMap::new(), AnimeId(3), &title, FuzzyDate::default());
        let settings = &changed[0];
        let index = TitleIndex::build(anime.values(), |_| Some(settings));
        let relations = Relations::parse("", ServiceId::AniList);

        let mut episode = parse("[Other] Kusuriya S3 - 08.mkv", ParseOptions::default());
        assert_eq!(
            identify(
                &mut episode,
                &index,
                &anime,
                &relations,
                &Default::default()
            ),
            Some(AnimeId(3))
        );
        let mut episode = parse("[Other] Kusuriya - 05.mkv", ParseOptions::default());
        assert_eq!(
            identify(
                &mut episode,
                &index,
                &anime,
                &relations,
                &Default::default()
            ),
            None
        );
    }

    #[test]
    fn generic_titles() {
        assert!(is_generic_title("S2"));
        assert!(is_generic_title("Season 2"));
        assert!(is_generic_title("OVA"));
        assert!(!is_generic_title("Kusuriya S3"));
        assert!(!is_generic_title("Yani Neko"));
    }

    #[test]
    fn linking_moves_a_title_from_another_anime() {
        let date = FuzzyDate::new(2026, 11, 14);
        let mut settings = BTreeMap::new();
        let mut two = AnimeSettings::new(AnimeId(2));
        two.synonyms = vec!["Kusuriya S3".into(), "Keep me".into()];
        two.linked = vec![LinkedTitle {
            title: "Kusuriya S3".into(),
            date,
        }];
        settings.insert(AnimeId(2), two);

        let changed = link_title(&settings, AnimeId(3), "kusuriya s3", date);
        assert_eq!(changed.len(), 2);
        assert_eq!(changed[0].id, AnimeId(2));
        assert_eq!(changed[0].synonyms, ["Keep me"]);
        assert!(changed[0].linked.is_empty());
        assert_eq!(changed[1].id, AnimeId(3));
        assert_eq!(changed[1].synonyms, ["kusuriya s3"]);
        assert_eq!(changed[1].linked[0].title, "kusuriya s3");

        for s in changed {
            settings.insert(s.id, s);
        }
        assert_eq!(linked_to(&settings, "Kusuriya S3"), Some(AnimeId(3)));
        // Linking again changes nothing.
        assert!(link_title(&settings, AnimeId(3), "Kusuriya S3", date).is_empty());
    }

    #[test]
    fn linking_a_title_the_user_typed_in_marks_it_as_linked() {
        let date = FuzzyDate::new(2026, 11, 14);
        let mut settings = BTreeMap::new();
        let mut one = AnimeSettings::new(AnimeId(1));
        one.synonyms = vec!["Kusuriya".into()];
        settings.insert(AnimeId(1), one);
        let changed = link_title(&settings, AnimeId(1), "Kusuriya", date);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].synonyms, ["Kusuriya"]);
        assert_eq!(changed[0].linked.len(), 1);
    }
}
