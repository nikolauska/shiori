//! Airing times for a MyAnimeList list, borrowed from AniList (decision
//! I37). MyAnimeList only gives a weekly broadcast slot, with no episode
//! number, which is wrong across breaks and delays; the Home schedule and
//! the "new episodes" badges need the next episode and when it airs.

use std::collections::BTreeMap;

use shiori_core::entry::is_in_list;
use shiori_core::{AiringStatus, Anime, AnimeId};

use crate::state::AppState;

/// The anime in the list that are airing or about to, by MyAnimeList id.
pub fn ids_to_look_up(state: &AppState) -> Vec<u32> {
    state
        .anime
        .values()
        .filter(|a| matches!(a.status, AiringStatus::Airing | AiringStatus::NotYetAired))
        .filter(|a| is_in_list(state.entries.get(&a.id)))
        .map(|a| a.id.0)
        .collect()
}

/// The list's anime with AniList's airing times copied in. `found` is what
/// AniList returned for a lookup by MyAnimeList id. Only those two fields
/// are taken; the rest of the data stays MyAnimeList's.
pub fn with_airing_times(known: &BTreeMap<AnimeId, Anime>, found: &[Anime]) -> Vec<Anime> {
    found
        .iter()
        .filter_map(|from| {
            let mut anime = known.get(&AnimeId(from.mal_id?))?.clone();
            if anime.next_episode_time == from.next_episode_time
                && anime.last_aired_episode == from.last_aired_episode
            {
                return None;
            }
            anime.next_episode_time = from.next_episode_time;
            anime.last_aired_episode = from.last_aired_episode;
            Some(anime)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_airing_times_are_copied() {
        let mut mal = Anime::new(AnimeId(52991));
        mal.titles.romaji = "Sousou no Frieren".into();
        mal.image_url = "mal.jpg".into();
        let known = BTreeMap::from([(mal.id, mal)]);

        let mut anilist = Anime::new(AnimeId(154587));
        anilist.mal_id = Some(52991);
        anilist.image_url = "anilist.jpg".into();
        anilist.last_aired_episode = 7;
        anilist.next_episode_time = Some("2026-11-28T15:00:00Z".parse().unwrap());
        let mut unknown = Anime::new(AnimeId(1));
        unknown.mal_id = Some(999);
        let no_mal_id = Anime::new(AnimeId(2));

        let updated = with_airing_times(&known, &[anilist.clone(), unknown, no_mal_id]);
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].id, AnimeId(52991));
        assert_eq!(updated[0].image_url, "mal.jpg");
        assert_eq!(updated[0].last_aired_episode, 7);
        assert_eq!(updated[0].next_episode_time, anilist.next_episode_time);

        // Nothing changed: nothing to store.
        let known = BTreeMap::from([(updated[0].id, updated[0].clone())]);
        assert!(with_airing_times(&known, &[anilist]).is_empty());
    }
}
