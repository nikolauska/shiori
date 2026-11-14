//! Keeping a local list's anime up to date. With an account the list fetch
//! refreshes every anime in the list; a list kept on this computer has no
//! fetch, so airing times, new episode counts and episode totals would
//! otherwise stay as they were when each anime was stored.

use jiff::Timestamp;
use jiff::civil::Date;
use shiori_core::anime::{airing_status, is_stale};
use shiori_core::entry::is_in_list;
use shiori_core::{AiringStatus, AnimeId, ListStatus};

use crate::state::AppState;

/// The list's anime worth asking about: those being watched or planned,
/// and anything still airing, since those are the ones whose schedule and
/// counts change. With `all_due` unset only stale ones are picked, so the
/// hourly refresh asks about a finished series at most once a week.
pub fn anime_to_refresh(
    state: &AppState,
    now: Timestamp,
    today_in_japan: Date,
    all_due: bool,
) -> Vec<AnimeId> {
    state
        .entries
        .values()
        .filter(|e| is_in_list(Some(e)))
        .filter_map(|entry| {
            let anime = state.anime.get(&entry.anime_id)?;
            let followed = matches!(entry.status, ListStatus::Watching | ListStatus::PlanToWatch)
                || airing_status(anime, today_in_japan) == AiringStatus::Airing;
            (followed && (all_due || is_stale(anime, now, today_in_japan))).then_some(anime.id)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use shiori_core::{Anime, FuzzyDate, ListEntry};
    use shiori_store::{Accounts, Database, Settings};

    use super::*;

    fn now() -> Timestamp {
        "2026-11-14T12:00:00Z".parse().unwrap()
    }

    /// Fresh, complete data for a finished anime.
    fn anime(id: u32) -> Anime {
        let mut a = Anime::new(AnimeId(id));
        a.last_modified = Some(now());
        a.synopsis = "x".into();
        a.genres = vec!["Action".into()];
        a.score = 7.0;
        a.status = AiringStatus::FinishedAiring;
        a
    }

    fn airing(id: u32) -> Anime {
        let mut a = anime(id);
        a.status = AiringStatus::Airing;
        a.date_started = FuzzyDate::new(2026, 10, 1);
        a
    }

    fn state(list: &[(Anime, ListStatus)]) -> AppState {
        let mut state = AppState::new(
            Database::open_in_memory().unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap();
        state
            .store_anime(list.iter().map(|(a, _)| a.clone()).collect())
            .unwrap();
        for (a, status) in list {
            let mut entry = ListEntry::new(a.id);
            entry.status = *status;
            state.save_entry(entry, now()).unwrap();
        }
        state
    }

    #[test]
    fn watched_planned_and_airing_anime_are_followed() {
        let state = state(&[
            (anime(1), ListStatus::Watching),
            (anime(2), ListStatus::PlanToWatch),
            (anime(3), ListStatus::Completed),
            (airing(4), ListStatus::OnHold),
            (anime(5), ListStatus::Dropped),
        ]);
        let today = date(2026, 11, 14);
        let mut ids = anime_to_refresh(&state, now(), today, true);
        ids.sort();
        assert_eq!(ids, [AnimeId(1), AnimeId(2), AnimeId(4)]);
        // Everything was just stored, so nothing is due yet.
        assert!(anime_to_refresh(&state, now(), today, false).is_empty());
    }

    #[test]
    fn stale_anime_are_due() {
        let mut old_airing = airing(1);
        old_airing.last_modified = Some(now() - jiff::SignedDuration::from_hours(2));
        let mut old_finished = anime(2);
        old_finished.last_modified = Some(now() - jiff::SignedDuration::from_hours(2));
        let state = state(&[
            (old_airing, ListStatus::Watching),
            (old_finished, ListStatus::Watching),
        ]);
        // A finished anime two hours old is fine for another week.
        assert_eq!(
            anime_to_refresh(&state, now(), date(2026, 11, 14), false),
            [AnimeId(1)]
        );
    }
}
