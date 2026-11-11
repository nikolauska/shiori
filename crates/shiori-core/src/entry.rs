//! The user's list entries, per-anime settings and watch history.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::anime::AnimeId;
use crate::date::FuzzyDate;

/// Id of a list entry on the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ListEntryId(pub i64);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ListStatus {
    #[default]
    NotInList,
    Watching,
    Completed,
    OnHold,
    Dropped,
    PlanToWatch,
}

impl ListStatus {
    /// The statuses shown to the user, in display order.
    pub const SHOWN: [ListStatus; 5] = [
        ListStatus::Watching,
        ListStatus::Completed,
        ListStatus::OnHold,
        ListStatus::Dropped,
        ListStatus::PlanToWatch,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListEntry {
    /// `None` until the service has created the entry.
    pub id: Option<ListEntryId>,
    pub anime_id: AnimeId,
    pub watched_episodes: u32,
    /// 0 to 100 whatever the service uses; 0 is no score.
    pub score: u8,
    pub status: ListStatus,
    pub is_private: bool,
    pub rewatched_times: u32,
    pub rewatching: bool,
    pub rewatching_ep: u32,
    pub date_started: FuzzyDate,
    pub date_completed: FuzzyDate,
    pub last_updated: Timestamp,
    pub notes: String,
    /// The user removed it and the service has not confirmed yet.
    pub pending_delete: bool,
}

impl ListEntry {
    /// An empty entry for an anime, used as the "before" of a new entry.
    pub fn new(anime_id: AnimeId) -> Self {
        Self {
            id: None,
            anime_id,
            watched_episodes: 0,
            score: 0,
            status: ListStatus::NotInList,
            is_private: false,
            rewatched_times: 0,
            rewatching: false,
            rewatching_ep: 0,
            date_started: FuzzyDate::default(),
            date_completed: FuzzyDate::default(),
            last_updated: Timestamp::UNIX_EPOCH,
            notes: String::new(),
            pending_delete: false,
        }
    }
}

bitflags::bitflags! {
    /// Fields of a list entry that changed. The bit values are stored in the
    /// queue table, so they must never change.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
    pub struct Fields: u8 {
        const EPISODE         = 1 << 0;
        const SCORE           = 1 << 1;
        const STATUS          = 1 << 2;
        const REWATCHING      = 1 << 3;
        const REWATCHED_TIMES = 1 << 4;
        const DATE_STARTED    = 1 << 5;
        const DATE_COMPLETED  = 1 << 6;
        const NOTES           = 1 << 7;
    }
}

/// Which of the synchronized fields differ. With no previous entry, compares
/// against an empty entry for the same anime.
pub fn changed_fields(before: Option<&ListEntry>, after: &ListEntry) -> Fields {
    let empty;
    let before = match before {
        Some(entry) => entry,
        None => {
            empty = ListEntry::new(after.anime_id);
            &empty
        }
    };
    let mut fields = Fields::empty();
    fields.set(
        Fields::EPISODE,
        before.watched_episodes != after.watched_episodes,
    );
    fields.set(Fields::SCORE, before.score != after.score);
    fields.set(Fields::STATUS, before.status != after.status);
    fields.set(Fields::REWATCHING, before.rewatching != after.rewatching);
    fields.set(
        Fields::REWATCHED_TIMES,
        before.rewatched_times != after.rewatched_times,
    );
    fields.set(
        Fields::DATE_STARTED,
        before.date_started != after.date_started,
    );
    fields.set(
        Fields::DATE_COMPLETED,
        before.date_completed != after.date_completed,
    );
    fields.set(Fields::NOTES, before.notes != after.notes);
    fields
}

/// An entry waiting to be deleted counts as not in the list everywhere.
pub fn is_in_list(entry: Option<&ListEntry>) -> bool {
    entry.is_some_and(|e| e.status != ListStatus::NotInList && !e.pending_delete)
}

/// Share of episodes watched for a progress bar, at most 1.0.
pub fn progress_ratio(watched: u32, episode_count: Option<u32>) -> f32 {
    match episode_count {
        Some(count) if count > 0 => (watched as f32 / count as f32).min(1.0),
        // Unknown length: show the bar mostly full rather than empty, since
        // the series is in progress.
        _ => 0.8,
    }
}

/// Settings the user keeps for one anime.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimeSettings {
    pub id: AnimeId,
    /// Overrides the title shown everywhere.
    pub display_title: String,
    /// Where this anime's files are.
    pub folder: String,
    /// Extra titles for recognition, with the strongest weight.
    pub synonyms: Vec<String>,
}

impl AnimeSettings {
    pub fn new(id: AnimeId) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryItem {
    pub id: i64,
    pub anime_id: AnimeId,
    pub episode: u32,
    pub time: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_bits_are_stable() {
        assert_eq!(Fields::EPISODE.bits(), 1);
        assert_eq!(Fields::NOTES.bits(), 128);
        assert_eq!(Fields::all().bits(), 255);
    }

    #[test]
    fn changed_fields_compares_each_field() {
        let before = ListEntry::new(AnimeId(1));
        assert_eq!(changed_fields(Some(&before), &before), Fields::empty());

        let mut after = before.clone();
        after.watched_episodes = 3;
        after.notes = "good".into();
        after.is_private = true; // not synchronized as a changed field
        assert_eq!(
            changed_fields(Some(&before), &after),
            Fields::EPISODE | Fields::NOTES
        );

        let mut after = before.clone();
        after.status = ListStatus::Watching;
        after.score = 80;
        after.rewatching = true;
        after.rewatched_times = 1;
        after.date_started = FuzzyDate::new(2026, 1, 1);
        after.date_completed = FuzzyDate::new(2026, 2, 1);
        assert_eq!(
            changed_fields(None, &after),
            Fields::STATUS
                | Fields::SCORE
                | Fields::REWATCHING
                | Fields::REWATCHED_TIMES
                | Fields::DATE_STARTED
                | Fields::DATE_COMPLETED
        );
    }

    #[test]
    fn in_list() {
        let mut e = ListEntry::new(AnimeId(1));
        assert!(!is_in_list(None));
        assert!(!is_in_list(Some(&e)));
        e.status = ListStatus::Watching;
        assert!(is_in_list(Some(&e)));
        e.pending_delete = true;
        assert!(!is_in_list(Some(&e)));
    }

    #[test]
    fn progress() {
        assert_eq!(progress_ratio(6, Some(12)), 0.5);
        assert_eq!(progress_ratio(14, Some(12)), 1.0);
        assert_eq!(progress_ratio(3, None), 0.8);
    }
}
