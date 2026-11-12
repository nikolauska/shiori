//! What the Home page shows: anime with new episodes to watch, and what
//! airs in the coming week.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan as _};
use shiori_core::entry::is_in_list;
use shiori_core::{AnimeId, ListStatus};

use crate::list_view::new_episodes;
use crate::state::AppState;

/// Watching anime with aired episodes left to watch: most behind first,
/// then by title.
pub fn continue_watching(state: &AppState) -> Vec<(AnimeId, u32)> {
    let mut rows: Vec<(AnimeId, u32, String)> = state
        .entries
        .values()
        .filter_map(|entry| {
            let new = new_episodes(state.anime.get(&entry.anime_id), Some(entry));
            (new > 0).then(|| {
                (
                    entry.anime_id,
                    new,
                    state.title(entry.anime_id).to_lowercase(),
                )
            })
        })
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
    rows.into_iter().map(|(id, new, _)| (id, new)).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Airing {
    pub anime_id: AnimeId,
    pub time: Timestamp,
    /// The episode that airs then.
    pub episode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    pub date: Date,
    /// In airing order.
    pub airings: Vec<Airing>,
}

/// Today and the following `days - 1` days by local date, each with the
/// next episodes of Watching and Plan to watch anime that air on it. Only
/// the next episode of each anime is known, so a weekly show appears once.
pub fn airing_schedule(state: &AppState, now: Timestamp, tz: &TimeZone, days: i64) -> Vec<Day> {
    let today = now.to_zoned(tz.clone()).date();
    let mut schedule: Vec<Day> = (0..days)
        .filter_map(|n| today.checked_add(n.days()).ok())
        .map(|date| Day {
            date,
            airings: Vec::new(),
        })
        .collect();
    for entry in state.entries.values() {
        if !is_in_list(Some(entry))
            || !matches!(entry.status, ListStatus::Watching | ListStatus::PlanToWatch)
        {
            continue;
        }
        let Some(anime) = state.anime.get(&entry.anime_id) else {
            continue;
        };
        let Some(time) = anime.next_episode_time else {
            continue;
        };
        let date = time.to_zoned(tz.clone()).date();
        if let Some(day) = schedule.iter_mut().find(|d| d.date == date) {
            day.airings.push(Airing {
                anime_id: anime.id,
                time,
                episode: anime.last_aired_episode + 1,
            });
        }
    }
    for day in &mut schedule {
        day.airings.sort_by_key(|a| (a.time, a.anime_id));
    }
    schedule
}

#[cfg(test)]
mod tests {
    use shiori_core::{Anime, ListEntry};
    use shiori_store::{Accounts, Database, Settings};

    use super::*;

    fn state() -> AppState {
        let mut state = AppState::new(
            Database::open_in_memory().unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap();
        let mut add = |id: u32, title: &str, status, watched, aired, next: Option<&str>| {
            let mut anime = Anime::new(AnimeId(id));
            anime.titles.romaji = title.into();
            anime.last_aired_episode = aired;
            anime.next_episode_time = next.map(|t| t.parse().unwrap());
            state.store_anime(vec![anime]).unwrap();
            let mut entry = ListEntry::new(AnimeId(id));
            entry.status = status;
            entry.watched_episodes = watched;
            state.save_entry(entry, Timestamp::UNIX_EPOCH).unwrap();
        };
        add(
            1,
            "Bravo",
            ListStatus::Watching,
            3,
            6,
            Some("2026-11-13T10:00:00Z"),
        );
        add(
            2,
            "alpha",
            ListStatus::Watching,
            5,
            6,
            Some("2026-11-12T22:30:00Z"),
        );
        add(
            3,
            "Charlie",
            ListStatus::Watching,
            6,
            6,
            Some("2026-11-12T12:00:00Z"),
        );
        add(
            4,
            "Delta",
            ListStatus::Dropped,
            1,
            6,
            Some("2026-11-12T12:00:00Z"),
        );
        add(
            5,
            "Echo",
            ListStatus::PlanToWatch,
            0,
            0,
            Some("2026-11-25T12:00:00Z"),
        );
        state
    }

    #[test]
    fn most_behind_first() {
        assert_eq!(
            continue_watching(&state()),
            [(AnimeId(1), 3), (AnimeId(2), 1)]
        );
    }

    #[test]
    fn schedule_groups_by_local_date() {
        let state = state();
        let tz = TimeZone::fixed(jiff::tz::offset(2));
        let now: Timestamp = "2026-11-12T08:00:00Z".parse().unwrap();
        let days = airing_schedule(&state, now, &tz, 7);
        assert_eq!(days.len(), 7);
        assert_eq!(days[0].date, jiff::civil::date(2026, 11, 12));
        // 12:00Z is 14:00 local today; 22:30Z is 00:30 tomorrow local.
        let ids = |d: &Day| d.airings.iter().map(|a| a.anime_id).collect::<Vec<_>>();
        assert_eq!(ids(&days[0]), [AnimeId(3)]);
        assert_eq!(ids(&days[1]), [AnimeId(2), AnimeId(1)]);
        assert_eq!(days[1].airings[1].episode, 7);
        // Dropped is left out; Echo airs after the week.
        assert!(days.iter().all(|d| !ids(d).contains(&AnimeId(4))));
        assert!(days.iter().all(|d| !ids(d).contains(&AnimeId(5))));
    }
}
