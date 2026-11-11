//! Matching a parsed episode to an anime in the local database.

use std::collections::BTreeMap;

use crate::anime::{Anime, AnimeId};
use crate::episode::{ElementKind, Episode};
use crate::recognition::{Relations, TitleIndex};

/// Finds the anime a parsed episode belongs to. On a relations redirect the
/// episode's numbers are rewritten to the destination's numbering. Also sets
/// `episode.anime_id`.
pub fn identify(
    episode: &mut Episode,
    index: &TitleIndex,
    anime: &BTreeMap<AnimeId, Anime>,
    relations: &Relations,
) -> Option<AnimeId> {
    let id = find(episode, index, anime, relations);
    episode.anime_id = id;
    id
}

fn find(
    episode: &mut Episode,
    index: &TitleIndex,
    anime: &BTreeMap<AnimeId, Anime>,
    relations: &Relations,
) -> Option<AnimeId> {
    // Openings, endings and previews never count as episodes.
    if !is_valid_episode_type(episode) {
        return None;
    }
    let title = episode.title().unwrap_or_default().to_owned();
    for (id, _) in index.lookup(&title) {
        let Some(item) = anime.get(&id) else {
            continue;
        };
        if is_valid_episode_number(episode, item) {
            return Some(id);
        }
        let Some(range) = episode.episode_number_range() else {
            continue;
        };
        if let Some(redirect) = relations.find_redirection(id, range) {
            tracing::debug!(
                "Redirection: {id}:{} -> {}:{}",
                format_range(range),
                redirect.id,
                format_range(redirect.episode_range)
            );
            episode.set_episode_number_range(redirect.episode_range);
            return Some(redirect.id);
        }
    }
    None
}

fn format_range((first, last): (u32, u32)) -> String {
    if last > first {
        format!("{first}-{last}")
    } else {
        first.to_string()
    }
}

fn is_valid_episode_type(episode: &Episode) -> bool {
    const NOT_EPISODES: [&str; 8] = [
        "op", "opening", "ncop", "ed", "ending", "nced", "preview", "pv",
    ];
    !episode
        .all(ElementKind::Type)
        .any(|t| NOT_EPISODES.iter().any(|k| k.eq_ignore_ascii_case(t)))
}

fn is_valid_episode_number(episode: &Episode, anime: &Anime) -> bool {
    let Some(count) = anime.episode_count.filter(|&c| c > 0) else {
        return true;
    };
    let Some((_, last)) = episode.episode_number_range() else {
        // A single-episode anime needs no number, and a name without an
        // extension is a batch or folder, not one episode.
        return count == 1 || !episode.is_video_file();
    };
    // Episode 0 is let through to the relations redirect.
    last != 0 && last <= count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recognition::{ParseOptions, parse};
    use crate::service::ServiceId;

    struct Fixture {
        index: TitleIndex,
        anime: BTreeMap<AnimeId, Anime>,
        relations: Relations,
    }

    fn fixture() -> Fixture {
        let mut list = Vec::new();
        let mut add = |id, title: &str, count| {
            let mut a = Anime::new(AnimeId(id));
            a.titles.romaji = title.into();
            a.episode_count = count;
            list.push(a);
        };
        add(116242, "Tensei Shitara Slime Datta Ken", Some(12));
        add(
            127366,
            "Tensei Shitara Slime Datta Ken 2nd Season",
            Some(12),
        );
        add(1, "Kimi no Na wa.", Some(1));
        add(2, "Toradora!", Some(25));
        add(3, "One Piece", None);
        let anime: BTreeMap<_, _> = list.into_iter().map(|a| (a.id, a)).collect();
        let index = TitleIndex::build(anime.values(), |_| None);
        let relations = Relations::parse(
            "::rules\n- 41380|43367|116242:13-24 -> 44881|43883|127366:1-12!\n",
            ServiceId::AniList,
        );
        Fixture {
            index,
            anime,
            relations,
        }
    }

    fn run(f: &Fixture, name: &str) -> (Option<u32>, Option<(u32, u32)>) {
        let mut episode = parse(name, ParseOptions::default());
        let id = identify(&mut episode, &f.index, &f.anime, &f.relations);
        assert_eq!(episode.anime_id, id);
        (id.map(|i| i.0), episode.episode_number_range())
    }

    #[test]
    fn plain_match() {
        let f = fixture();
        assert_eq!(
            run(&f, "[Group] Toradora! - 05 [720p].mkv"),
            (Some(2), Some((5, 5)))
        );
        assert_eq!(
            run(&f, "[Group] One Piece - 1100 [720p].mkv"),
            (Some(3), Some((1100, 1100)))
        );
        assert_eq!(
            run(&f, "[Group] Unknown Show - 01.mkv"),
            (None, Some((1, 1)))
        );
    }

    #[test]
    fn redirect_continuous_numbering() {
        let f = fixture();
        assert_eq!(
            run(
                &f,
                "[Group] Tensei Shitara Slime Datta Ken - 14 [1080p].mkv"
            ),
            (Some(127366), Some((2, 2)))
        );
        assert_eq!(
            run(
                &f,
                "[Group] Tensei Shitara Slime Datta Ken 2nd Season - 13 [1080p].mkv"
            ),
            (Some(127366), Some((1, 1)))
        );
    }

    #[test]
    fn number_too_high_without_redirect() {
        let f = fixture();
        assert_eq!(
            run(&f, "[Group] Toradora! - 26.mkv"),
            (None, Some((26, 26)))
        );
        assert_eq!(run(&f, "[Group] Toradora! - 00.mkv"), (None, Some((0, 0))));
    }

    #[test]
    fn missing_number() {
        let f = fixture();
        assert_eq!(run(&f, "[Group] Kimi no Na wa. [1080p].mkv").0, Some(1));
        assert_eq!(run(&f, "[Group] Toradora! [1080p].mkv").0, None);
        assert_eq!(
            run(&f, "[Group] Toradora! [1080p]").0,
            Some(2),
            "batch name"
        );
    }

    #[test]
    fn openings_and_endings_are_not_episodes() {
        let f = fixture();
        assert_eq!(run(&f, "[Group] Toradora! - NCOP [1080p].mkv").0, None);
        assert_eq!(run(&f, "[Group] Toradora! - ED [1080p].mkv").0, None);
    }
}
