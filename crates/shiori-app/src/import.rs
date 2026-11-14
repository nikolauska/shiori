//! Adding an imported list to a list kept on this computer.

use anyhow::Result;
use jiff::Timestamp;
use shiori_core::entry::is_in_list;
use shiori_core::{Anime, ListEntry};

use crate::state::{AppEvent, AppState};

/// What an import did, for the summary shown afterwards.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub added: usize,
    /// Already in the list; left as they were.
    pub already: usize,
    /// Titles the service has no anime for.
    pub not_found: Vec<String>,
}

/// Adds the entries whose anime is not in the list yet. An entry already in
/// the list is kept as it is: it may hold progress made since the export.
pub fn add_missing(
    state: &mut AppState,
    anime: Vec<Anime>,
    entries: Vec<ListEntry>,
    now: Timestamp,
    summary: &mut Summary,
) -> Result<Vec<AppEvent>> {
    let mut events = state.store_anime(anime)?;
    for mut entry in entries {
        if is_in_list(state.entries.get(&entry.anime_id)) {
            summary.already += 1;
            continue;
        }
        // An id belongs to whoever's list it came from; this list has none.
        entry.id = None;
        entry.pending_delete = false;
        entry.last_updated = now;
        events.extend(state.save_entry(entry, now)?);
        summary.added += 1;
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use shiori_core::{AnimeId, ListEntryId, ListStatus};
    use shiori_store::{Accounts, Database, ListLocation, Settings};

    use super::*;

    #[test]
    fn adds_new_entries_and_keeps_existing_ones() {
        let mut state = AppState::new(
            Database::open_in_memory().unwrap(),
            Settings {
                list: Some(ListLocation::Local),
                ..Settings::default()
            },
            Accounts::default(),
        )
        .unwrap();
        state.store_anime(vec![Anime::new(AnimeId(1))]).unwrap();
        let mut mine = ListEntry::new(AnimeId(1));
        mine.status = ListStatus::Watching;
        mine.watched_episodes = 8;
        state.save_entry(mine, Timestamp::UNIX_EPOCH).unwrap();

        let imported = |id, watched| {
            let mut e = ListEntry::new(AnimeId(id));
            e.id = Some(ListEntryId(99));
            e.status = ListStatus::Completed;
            e.watched_episodes = watched;
            e
        };
        let mut summary = Summary::default();
        add_missing(
            &mut state,
            vec![Anime::new(AnimeId(1)), Anime::new(AnimeId(2))],
            vec![imported(1, 12), imported(2, 24)],
            Timestamp::UNIX_EPOCH,
            &mut summary,
        )
        .unwrap();
        assert_eq!((summary.added, summary.already), (1, 1));
        assert_eq!(state.entries[&AnimeId(1)].watched_episodes, 8);
        assert_eq!(state.entries[&AnimeId(2)].watched_episodes, 24);
        assert_eq!(state.entries[&AnimeId(2)].id, None);
        assert!(state.queue.is_empty());
    }
}
