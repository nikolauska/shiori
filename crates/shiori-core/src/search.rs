//! Search parameters for the service search.

use serde::{Deserialize, Serialize};

use crate::anime::{AiringStatus, AnimeType};
use crate::date::SeasonName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SearchSort {
    Title,
    Duration,
    Score,
    Type,
    StartDate,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SearchParams {
    pub text: String,
    pub year: Option<u16>,
    pub season: Option<SeasonName>,
    pub kind: Option<AnimeType>,
    pub status: Option<AiringStatus>,
    pub sort: Option<SearchSort>,
    pub descending: bool,
}

impl SearchParams {
    /// Season and year both set. Only these fetch every result page.
    pub fn is_season_browse(&self) -> bool {
        self.season.is_some() && self.year.is_some()
    }
}
