//! The offline update queue: every local edit is stored first and sent later.
//!
//! This is only the decision logic. The app sends the requests it hands out,
//! applies the reconcile results to its entries, and mirrors the items to the
//! database after each call.

use std::collections::BTreeMap;

use jiff::Timestamp;
use shiori_core::{AnimeId, Fields, ListEntry, ListEntryId, QueueItem};

/// What to send for one queue item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Add { entry: ListEntry, dirty: Fields },
    Update { entry: ListEntry, dirty: Fields },
    Delete { entry: ListEntry },
}

impl Request {
    pub fn anime_id(&self) -> AnimeId {
        match self {
            Request::Add { entry, .. }
            | Request::Update { entry, .. }
            | Request::Delete { entry } => entry.anime_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing to send, or a request is already in flight.
    Idle,
    /// The user must be signed in first. A successful sign-in runs
    /// Synchronize, which comes back to the queue.
    Authenticate,
    Send(Request),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Next {
    pub step: Step,
    /// Items removed because their entry no longer exists locally.
    pub dropped: Vec<AnimeId>,
}

/// How to bring the local entry in line after a successful request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconcile {
    Nothing,
    /// The delete went through and the user still wants the entry gone.
    DeleteLocal,
    /// The delete went through but the user added the entry again
    /// meanwhile; it must be sent as a new entry.
    ClearServiceId,
    /// Replace the local entry with the service's version.
    Store(ListEntry),
    /// The local entry has newer edits than the answer; keep it and only
    /// take the id the service gave it.
    CopyServiceId(ListEntryId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub anime_id: AnimeId,
    /// Only set on success.
    pub reconcile: Reconcile,
    /// The item after the request: `None` when it was removed.
    pub item: Option<QueueItem>,
    pub succeeded: bool,
}

#[derive(Debug, Clone, Copy)]
struct InFlight {
    anime_id: AnimeId,
    is_delete: bool,
    modified: bool,
}

#[derive(Debug, Default)]
pub struct SyncQueue {
    items: BTreeMap<AnimeId, QueueItem>,
    in_flight: Option<InFlight>,
}

impl SyncQueue {
    pub fn new(items: impl IntoIterator<Item = QueueItem>) -> Self {
        Self {
            items: items.into_iter().map(|i| (i.anime_id, i)).collect(),
            in_flight: None,
        }
    }

    pub fn get(&self, anime_id: AnimeId) -> Option<&QueueItem> {
        self.items.get(&anime_id)
    }

    pub fn contains(&self, anime_id: AnimeId) -> bool {
        self.items.contains_key(&anime_id)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Oldest first, the order they are sent in.
    pub fn items(&self) -> Vec<&QueueItem> {
        let mut items: Vec<_> = self.items.values().collect();
        items.sort_by_key(|i| (i.time, i.anime_id));
        items
    }

    /// The anime whose request is in flight.
    pub fn in_flight(&self) -> Option<AnimeId> {
        self.in_flight.map(|f| f.anime_id)
    }

    /// Records an edit. Returns the item as it now is.
    pub fn push(&mut self, anime_id: AnimeId, dirty: Fields, now: Timestamp) -> &QueueItem {
        self.mark_modified(anime_id);
        let item = self.items.entry(anime_id).or_insert_with(|| QueueItem {
            anime_id,
            dirty: Fields::empty(),
            time: now,
            retry_count: 0,
            last_error: None,
        });
        item.dirty |= dirty;
        item.time = now;
        item
    }

    /// Records that the entry should be deleted. Any edits waiting for it are
    /// dropped, since there will be nothing left to edit.
    pub fn push_delete(&mut self, anime_id: AnimeId, now: Timestamp) -> &QueueItem {
        self.mark_modified(anime_id);
        self.items.insert(
            anime_id,
            QueueItem {
                anime_id,
                dirty: Fields::empty(),
                time: now,
                retry_count: 0,
                last_error: None,
            },
        );
        &self.items[&anime_id]
    }

    fn mark_modified(&mut self, anime_id: AnimeId) {
        if let Some(flight) = self.in_flight.as_mut().filter(|f| f.anime_id == anime_id) {
            flight.modified = true;
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Picks the next request. Only one is in flight at a time, which keeps
    /// the answers in the order the edits were made.
    pub fn next(&mut self, entries: &BTreeMap<AnimeId, ListEntry>, signed_in: bool) -> Next {
        let mut dropped = Vec::new();
        let step = loop {
            if self.in_flight.is_some() {
                break Step::Idle;
            }
            let Some(item) = self.items().first().map(|i| (*i).clone()) else {
                break Step::Idle;
            };
            let Some(entry) = entries.get(&item.anime_id) else {
                self.items.remove(&item.anime_id);
                dropped.push(item.anime_id);
                continue;
            };
            if !signed_in {
                break Step::Authenticate;
            }
            let is_delete = entry.pending_delete;
            self.in_flight = Some(InFlight {
                anime_id: item.anime_id,
                is_delete,
                modified: false,
            });
            let entry = entry.clone();
            break Step::Send(if is_delete {
                Request::Delete { entry }
            } else if entry.id.is_none() {
                Request::Add {
                    entry,
                    dirty: item.dirty,
                }
            } else {
                Request::Update {
                    entry,
                    dirty: item.dirty,
                }
            });
        };
        Next { step, dropped }
    }

    /// Applies the answer to the request in flight. `answer` is the service's
    /// version of the entry on success, and the error text on failure.
    /// `local` is the local entry as it is now.
    ///
    /// # Panics
    /// When no request is in flight.
    pub fn finish(
        &mut self,
        answer: Result<Option<ListEntry>, String>,
        local: Option<&ListEntry>,
    ) -> Finished {
        let flight = self.in_flight.take().expect("no request in flight");
        let anime_id = flight.anime_id;
        let answer = match answer {
            Ok(answer) => answer,
            Err(error) => {
                // The item stays where it is; the retry timer tries again.
                let item = self.items.get_mut(&anime_id).map(|item| {
                    item.retry_count += 1;
                    item.last_error = Some(error);
                    item.clone()
                });
                return Finished {
                    anime_id,
                    reconcile: Reconcile::Nothing,
                    item,
                    succeeded: false,
                };
            }
        };

        let reconcile = if flight.is_delete {
            match local {
                Some(entry) if entry.pending_delete => Reconcile::DeleteLocal,
                Some(_) => Reconcile::ClearServiceId,
                None => Reconcile::Nothing,
            }
        } else {
            match answer {
                Some(entry) if !flight.modified => Reconcile::Store(entry),
                Some(ListEntry { id: Some(id), .. }) => Reconcile::CopyServiceId(id),
                _ => Reconcile::Nothing,
            }
        };

        let item = if flight.modified {
            // Newer edits arrived while this one was on its way; keep the
            // item so they are sent next, with a clean slate.
            self.items.get_mut(&anime_id).map(|item| {
                item.retry_count = 0;
                item.last_error = None;
                item.clone()
            })
        } else {
            self.items.remove(&anime_id);
            None
        };
        Finished {
            anime_id,
            reconcile,
            item,
            succeeded: true,
        }
    }
}
