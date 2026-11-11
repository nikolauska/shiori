//! What Synchronize does, and the rules around a list fetch.

use std::collections::HashSet;

use shiori_core::AnimeId;

use crate::SyncQueue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    /// Check the stored token; on success, Synchronize again.
    Authenticate,
    FetchList,
    ProcessQueue,
    /// No username and no token: nothing can be done.
    NotSetUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncState {
    /// Set by a successful sign-in in this run; never stored.
    pub signed_in: bool,
    pub has_token: bool,
    pub has_username: bool,
    pub queue_empty: bool,
}

pub fn sync_action(state: SyncState) -> SyncAction {
    if !state.signed_in {
        return if state.has_token {
            SyncAction::Authenticate
        } else if state.has_username {
            // A public list can be read without signing in.
            SyncAction::FetchList
        } else {
            SyncAction::NotSetUp
        };
    }
    // Local edits go out before the list is fetched, or the fetch would
    // bring back the old values and the user would see their edits revert.
    if state.queue_empty {
        SyncAction::FetchList
    } else {
        SyncAction::ProcessQueue
    }
}

/// Whether the settle and retry timers may send the queue.
pub fn should_auto_process(sync_enabled: bool, signed_in: bool, has_token: bool) -> bool {
    sync_enabled && (signed_in || has_token)
}

/// Local entries the service no longer has. Entries with a queue item are
/// kept: they are local additions the service has not seen yet.
pub fn entries_to_prune(
    local: impl IntoIterator<Item = AnimeId>,
    fetched: &HashSet<AnimeId>,
    queue: &SyncQueue,
) -> Vec<AnimeId> {
    local
        .into_iter()
        .filter(|id| !fetched.contains(id) && !queue.contains(*id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;
    use shiori_core::Fields;

    #[test]
    fn actions() {
        let state = |signed_in, has_token, has_username, queue_empty| SyncState {
            signed_in,
            has_token,
            has_username,
            queue_empty,
        };
        use SyncAction::*;
        assert_eq!(sync_action(state(false, true, true, false)), Authenticate);
        assert_eq!(sync_action(state(false, false, true, false)), FetchList);
        assert_eq!(sync_action(state(false, false, false, true)), NotSetUp);
        assert_eq!(sync_action(state(true, true, true, false)), ProcessQueue);
        assert_eq!(sync_action(state(true, true, true, true)), FetchList);
    }

    #[test]
    fn auto_processing() {
        assert!(!should_auto_process(false, true, true));
        assert!(!should_auto_process(true, false, false));
        assert!(should_auto_process(true, false, true));
        assert!(should_auto_process(true, true, false));
    }

    #[test]
    fn prune_keeps_queued_entries() {
        let mut queue = SyncQueue::default();
        queue.push(AnimeId(3), Fields::EPISODE, Timestamp::UNIX_EPOCH);
        let fetched = HashSet::from([AnimeId(1)]);
        let pruned = entries_to_prune([AnimeId(1), AnimeId(2), AnimeId(3)], &fetched, &queue);
        assert_eq!(pruned, [AnimeId(2)]);
    }
}
