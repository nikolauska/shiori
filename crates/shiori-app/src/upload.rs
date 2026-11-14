//! Uploading a list kept on this computer to an account: merging it into
//! the account's freshly fetched list, so the queue sends the difference.

use std::collections::BTreeMap;

use anyhow::Result;
use jiff::Timestamp;
use shiori_core::entry::is_in_list;
use shiori_core::{Anime, AnimeId, ListEntry, ListStatus};

use crate::state::{AppEvent, AppState};

/// What an upload changed in the account's list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Merged {
    pub added: usize,
    pub updated: usize,
}

/// How far along a status is. Statuses of the same rank are different
/// choices rather than progress, so neither replaces the other.
fn rank(status: ListStatus) -> u8 {
    match status {
        ListStatus::NotInList => 0,
        ListStatus::PlanToWatch => 1,
        ListStatus::Watching | ListStatus::OnHold | ListStatus::Dropped => 2,
        ListStatus::Completed => 3,
    }
}

/// The account's entry with what the local one adds: progress and status
/// only move forward, and the rest only fills what the account lacks, so
/// an upload never undoes something done on the website.
fn combine(account: &ListEntry, local: &ListEntry) -> ListEntry {
    let mut entry = account.clone();
    entry.watched_episodes = entry.watched_episodes.max(local.watched_episodes);
    if rank(local.status) > rank(entry.status) {
        entry.status = local.status;
    }
    if entry.score == 0 {
        entry.score = local.score;
    }
    if entry.notes.is_empty() {
        entry.notes = local.notes.clone();
    }
    if entry.date_started.is_empty() {
        entry.date_started = local.date_started;
    }
    if entry.date_completed.is_empty() {
        entry.date_completed = local.date_completed;
    }
    entry.rewatched_times = entry.rewatched_times.max(local.rewatched_times);
    entry
}

/// Merges a local list into `state`, the account's list. Nothing in the
/// account's list is removed. The changes are queued like any edit.
pub fn merge(
    state: &mut AppState,
    local_anime: &BTreeMap<AnimeId, Anime>,
    local_entries: impl IntoIterator<Item = ListEntry>,
    now: Timestamp,
) -> Result<(Merged, Vec<AppEvent>)> {
    let mut merged = Merged::default();
    let mut events = Vec::new();
    let entries: Vec<ListEntry> = local_entries
        .into_iter()
        .filter(|e| is_in_list(Some(e)))
        .collect();
    // Entries need their anime stored; the account's copy of an anime is
    // kept when it has one, being the fresher.
    let missing: Vec<Anime> = entries
        .iter()
        .filter(|e| !state.anime.contains_key(&e.anime_id))
        .filter_map(|e| local_anime.get(&e.anime_id).cloned())
        .collect();
    events.extend(state.store_anime(missing)?);
    for local in entries {
        if !state.anime.contains_key(&local.anime_id) {
            continue;
        }
        let next = match state.entries.get(&local.anime_id) {
            Some(account) if is_in_list(Some(account)) => {
                let next = combine(account, &local);
                if &next == account {
                    continue;
                }
                merged.updated += 1;
                next
            }
            _ => {
                merged.added += 1;
                ListEntry {
                    // Created by the service when the add is sent.
                    id: None,
                    pending_delete: false,
                    ..local
                }
            }
        };
        events.extend(state.save_entry(next, now)?);
    }
    Ok((merged, events))
}

#[cfg(test)]
mod tests {
    use shiori_core::ListEntryId;
    use shiori_store::{Accounts, Database, Settings};

    use super::*;

    fn entry(id: u32, status: ListStatus, watched: u32) -> ListEntry {
        let mut e = ListEntry::new(AnimeId(id));
        e.status = status;
        e.watched_episodes = watched;
        e
    }

    #[test]
    fn adds_missing_and_only_moves_entries_forward() {
        let mut state = AppState::new(
            Database::open_in_memory().unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap();
        state
            .store_anime((1..=3).map(|id| Anime::new(AnimeId(id))).collect())
            .unwrap();
        let mut ahead = entry(1, ListStatus::Completed, 12);
        ahead.id = Some(ListEntryId(10));
        ahead.score = 70;
        let mut behind = entry(2, ListStatus::Watching, 3);
        behind.id = Some(ListEntryId(20));
        for e in [ahead, behind] {
            state.save_entry(e, Timestamp::UNIX_EPOCH).unwrap();
        }
        // As if the list had just been fetched: nothing waits to be sent.
        state.clear_queue().unwrap();

        let local_anime: BTreeMap<AnimeId, Anime> = (1..=4)
            .map(|id| (AnimeId(id), Anime::new(AnimeId(id))))
            .collect();
        let mut local_1 = entry(1, ListStatus::Watching, 5);
        local_1.score = 90;
        let mut local_2 = entry(2, ListStatus::Completed, 12);
        local_2.notes = "great".into();
        let mut local_4 = entry(4, ListStatus::PlanToWatch, 0);
        local_4.id = Some(ListEntryId(99));
        let (merged, _) = merge(
            &mut state,
            &local_anime,
            [local_1, local_2, local_4],
            Timestamp::UNIX_EPOCH,
        )
        .unwrap();

        assert_eq!(
            merged,
            Merged {
                added: 1,
                updated: 1
            }
        );
        // The account was further along with anime 1 and had a score.
        let one = &state.entries[&AnimeId(1)];
        assert_eq!(
            (one.status, one.watched_episodes, one.score),
            (ListStatus::Completed, 12, 70)
        );
        let two = &state.entries[&AnimeId(2)];
        assert_eq!(
            (two.status, two.watched_episodes),
            (ListStatus::Completed, 12)
        );
        assert_eq!(two.notes, "great");
        assert_eq!(two.id, Some(ListEntryId(20)));
        let four = &state.entries[&AnimeId(4)];
        assert_eq!(four.id, None);
        // Anime 4 came along from the local list.
        assert!(state.anime.contains_key(&AnimeId(4)));
        assert!(state.queue.contains(AnimeId(2)));
        assert!(state.queue.contains(AnimeId(4)));
        assert!(!state.queue.contains(AnimeId(1)));
    }
}
