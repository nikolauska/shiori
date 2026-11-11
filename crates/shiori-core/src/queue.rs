//! A pending change waiting to be sent to the list service.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::anime::AnimeId;
use crate::entry::Fields;

/// At most one item exists per anime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueItem {
    pub anime_id: AnimeId,
    /// Empty means "delete this entry".
    pub dirty: Fields,
    /// Last time this item was touched.
    pub time: Timestamp,
    pub retry_count: u32,
    pub last_error: Option<String>,
}

impl QueueItem {
    pub fn is_delete(&self) -> bool {
        self.dirty.is_empty()
    }
}
