//! Pure logic for Shiori: types, recognition and tracking decisions.
//!
//! Nothing here touches files, the network, a database, the clock or windows.
//! "Now" and "today" are always passed in, so every rule can be tested
//! without setup.

pub mod anime;
pub mod date;
pub mod entry;
pub mod episode;
pub mod queue;
pub mod recognition;
pub mod search;
pub mod service;
pub mod tracking;

pub use anime::{AgeRating, AiringStatus, Anime, AnimeId, AnimeType, TitleLanguage, Titles};
pub use date::{FuzzyDate, Season, SeasonName};
pub use entry::{AnimeSettings, Fields, HistoryItem, ListEntry, ListEntryId, ListStatus};
pub use episode::{Element, ElementKind, Episode};
pub use queue::QueueItem;
pub use search::{SearchParams, SearchSort};
pub use service::ServiceId;
pub use tracking::UpdateTrigger;
