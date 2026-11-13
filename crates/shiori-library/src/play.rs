//! Which episode each Play choice means (docs/ui.md, "Playing an episode").

use jiff::civil::Date;
use shiori_core::anime::airing_status;
use shiori_core::{AiringStatus, Anime, ListEntry, ListStatus};

/// The episode after the user's progress, or episode 1 once every episode
/// is watched, to start over.
pub fn next_episode(entry: Option<&ListEntry>, anime: Option<&Anime>) -> u32 {
    let next = entry.map_or(0, |e| e.watched_episodes) + 1;
    match anime.and_then(|a| a.episode_count).filter(|&c| c > 0) {
        Some(count) if next > count => 1,
        _ => next,
    }
}

/// The episode watched most recently; `None` before the first.
pub fn last_episode(entry: Option<&ListEntry>) -> Option<u32> {
    entry.map(|e| e.watched_episodes).filter(|&n| n > 0)
}

/// The highest episode Random may pick: the next one, so nothing ahead of
/// it is spoiled, or any episode of a completed anime. `None` when there
/// is nothing to pick from.
pub fn random_episode_limit(entry: Option<&ListEntry>, anime: Option<&Anime>) -> Option<u32> {
    let count = anime.and_then(|a| a.episode_count).filter(|&c| c > 0);
    let completed = entry.is_some_and(|e| e.status == ListStatus::Completed);
    let limit = match (completed, count) {
        (true, Some(count)) => count,
        // A completed entry with no known count: what was watched.
        (true, None) => entry.map_or(0, |e| e.watched_episodes),
        (false, _) => {
            let next = entry.map_or(0, |e| e.watched_episodes) + 1;
            count.map_or(next, |c| next.min(c))
        }
    };
    (limit >= 1).then_some(limit)
}

/// Whether "Start new rewatch" is offered: every episode watched, not
/// rewatching already, and the anime finished airing, so that there is a
/// complete run to watch again.
pub fn can_start_rewatch(
    entry: Option<&ListEntry>,
    anime: Option<&Anime>,
    today_in_japan: Date,
) -> bool {
    let (Some(entry), Some(anime)) = (entry, anime) else {
        return false;
    };
    let all_watched = anime
        .episode_count
        .is_some_and(|c| c > 0 && entry.watched_episodes >= c);
    !entry.rewatching
        && all_watched
        && airing_status(anime, today_in_japan) == AiringStatus::FinishedAiring
}

#[cfg(test)]
mod tests {
    use shiori_core::AnimeId;

    use super::*;

    fn anime(count: Option<u32>) -> Anime {
        let mut anime = Anime::new(AnimeId(1));
        anime.episode_count = count;
        anime.status = AiringStatus::FinishedAiring;
        anime
    }

    fn entry(watched: u32, status: ListStatus) -> ListEntry {
        let mut entry = ListEntry::new(AnimeId(1));
        entry.watched_episodes = watched;
        entry.status = status;
        entry
    }

    #[test]
    fn next_and_last() {
        let a = anime(Some(12));
        assert_eq!(next_episode(None, Some(&a)), 1);
        assert_eq!(
            next_episode(Some(&entry(4, ListStatus::Watching)), Some(&a)),
            5
        );
        assert_eq!(
            next_episode(Some(&entry(12, ListStatus::Completed)), Some(&a)),
            1
        );
        assert_eq!(
            next_episode(Some(&entry(30, ListStatus::Watching)), Some(&anime(None))),
            31
        );
        assert_eq!(last_episode(Some(&entry(0, ListStatus::Watching))), None);
        assert_eq!(last_episode(Some(&entry(3, ListStatus::Watching))), Some(3));
    }

    #[test]
    fn random_never_spoils() {
        let a = anime(Some(12));
        assert_eq!(random_episode_limit(None, Some(&a)), Some(1));
        assert_eq!(
            random_episode_limit(Some(&entry(4, ListStatus::Watching)), Some(&a)),
            Some(5)
        );
        assert_eq!(
            random_episode_limit(Some(&entry(12, ListStatus::Watching)), Some(&a)),
            Some(12)
        );
        assert_eq!(
            random_episode_limit(Some(&entry(3, ListStatus::Completed)), Some(&a)),
            Some(12)
        );
        assert_eq!(
            random_episode_limit(Some(&entry(0, ListStatus::Completed)), Some(&anime(None))),
            None
        );
    }

    #[test]
    fn rewatch_needs_a_finished_complete_run() {
        let today = jiff::civil::date(2026, 11, 12);
        let a = anime(Some(12));
        let done = entry(12, ListStatus::Completed);
        assert!(can_start_rewatch(Some(&done), Some(&a), today));
        let mut rewatching = done.clone();
        rewatching.rewatching = true;
        assert!(!can_start_rewatch(Some(&rewatching), Some(&a), today));
        assert!(!can_start_rewatch(
            Some(&entry(11, ListStatus::Watching)),
            Some(&a),
            today
        ));
        let mut airing = anime(Some(12));
        airing.status = AiringStatus::Airing;
        airing.date_started = shiori_core::FuzzyDate::parse("2026-10-01");
        assert!(!can_start_rewatch(Some(&done), Some(&airing), today));
    }
}
