//! The interface every list service implements.

use std::future::Future;

use shiori_core::{Anime, AnimeId, Fields, ListEntry, SearchParams, ServiceId};

use crate::ServiceError;

/// What a service needs to act as the user.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Account {
    pub username: String,
    pub token: Option<String>,
}

// Written by hand so that a token never ends up in a log line.
impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("username", &self.username)
            .field("token", &self.token.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

/// Who the user is, as the service told us at sign-in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountInfo {
    pub user_id: String,
    pub username: String,
    /// The service's name for the user's score format, for example
    /// `POINT_10` on AniList.
    pub rating_system: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FetchedList {
    pub anime: Vec<Anime>,
    pub entries: Vec<ListEntry>,
}

/// The entry after an add or update, with the anime when the service sent
/// it along.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedEntry {
    pub entry: ListEntry,
    pub anime: Option<Anime>,
}

/// Where the next search page starts. Opaque to the caller because each
/// service pages differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageToken(pub(crate) String);

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchPage {
    pub anime: Vec<Anime>,
    pub next: Option<PageToken>,
}

/// The futures are `Send` so that the app can run them on its network
/// thread's runtime.
pub trait Service {
    fn id(&self) -> ServiceId;

    /// Checks the stored credentials and returns who the user is.
    fn authenticate(
        &self,
        account: &Account,
    ) -> impl Future<Output = Result<AccountInfo, ServiceError>> + Send;

    /// `ServiceError::NotFound` means the service removed the anime.
    fn fetch_anime(&self, id: AnimeId) -> impl Future<Output = Result<Anime, ServiceError>> + Send;

    /// The whole list. Reports progress as (done, total) through the
    /// callback; total is 0 when unknown.
    fn fetch_list(
        &self,
        account: &Account,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> impl Future<Output = Result<FetchedList, ServiceError>> + Send;

    fn search(
        &self,
        params: &SearchParams,
        page: Option<PageToken>,
    ) -> impl Future<Output = Result<SearchPage, ServiceError>> + Send;

    /// Sends only the fields in `dirty`. Returns the entry as the service now
    /// has it, when the service returns one.
    fn add_entry(
        &self,
        account: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> impl Future<Output = Result<Option<SavedEntry>, ServiceError>> + Send;

    fn update_entry(
        &self,
        account: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> impl Future<Output = Result<Option<SavedEntry>, ServiceError>> + Send;

    /// An entry the service does not have counts as deleted.
    fn delete_entry(
        &self,
        account: &Account,
        entry: &ListEntry,
    ) -> impl Future<Output = Result<(), ServiceError>> + Send;
}
