//! List services and the offline update queue.
//!
//! This crate knows nothing about SQLite or the UI. Services take entries and
//! return results; the queue decides what to send next and how to apply the
//! answer. The app moves data between them and the store.

pub mod anilist;
mod error;
mod http;
pub mod myanimelist;
pub mod queue;
pub mod score;
mod service;
pub mod sync;

pub use error::ServiceError;
pub use http::{NetworkConfig, ProxyConfig, ProxyKind, USER_AGENT};
pub use queue::SyncQueue;
pub use service::{
    Account, AccountInfo, FetchedList, PageToken, SavedEntry, SearchPage, Service, Tokens,
};
