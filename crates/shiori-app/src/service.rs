//! The list service in use, chosen at run time.
//!
//! `Service` returns `impl Future`, so it cannot be a trait object; this enum
//! forwards each call to the service it holds instead.

use shiori_core::{AnimeId, Fields, ListEntry, SearchParams, ServiceId};
use shiori_sync::anilist::AniList;
use shiori_sync::myanimelist::MyAnimeList;
use shiori_sync::{
    Account, AccountInfo, FetchedList, PageToken, SavedEntry, SearchPage, Service, ServiceError,
    Tokens,
};

#[derive(Debug, Clone)]
pub enum ActiveService {
    AniList(AniList),
    MyAnimeList(MyAnimeList),
}

impl ActiveService {
    /// Kitsu is not supported yet; callers check before asking for it.
    pub fn new(id: ServiceId, client: reqwest::Client) -> Self {
        match id {
            ServiceId::MyAnimeList => Self::MyAnimeList(MyAnimeList::new(client)),
            ServiceId::AniList | ServiceId::Kitsu => Self::AniList(AniList::new(client)),
        }
    }

    pub fn as_myanimelist(&self) -> Option<&MyAnimeList> {
        match self {
            Self::MyAnimeList(service) => Some(service),
            Self::AniList(_) => None,
        }
    }
}

/// The page of an anime on a service's website.
pub fn anime_page_url(service: ServiceId, id: AnimeId) -> String {
    match service {
        ServiceId::AniList => format!("https://anilist.co/anime/{id}"),
        ServiceId::MyAnimeList => shiori_sync::myanimelist::anime_page_url(id),
        ServiceId::Kitsu => format!("https://kitsu.app/anime/{id}"),
    }
}

macro_rules! forward {
    ($self:ident, $service:ident => $call:expr) => {
        match $self {
            ActiveService::AniList($service) => $call,
            ActiveService::MyAnimeList($service) => $call,
        }
    };
}

impl Service for ActiveService {
    fn id(&self) -> ServiceId {
        forward!(self, s => s.id())
    }

    fn take_refreshed_tokens(&self) -> Option<Tokens> {
        forward!(self, s => s.take_refreshed_tokens())
    }

    async fn authenticate(&self, account: &Account) -> Result<AccountInfo, ServiceError> {
        forward!(self, s => s.authenticate(account).await)
    }

    async fn fetch_anime(&self, id: AnimeId) -> Result<shiori_core::Anime, ServiceError> {
        forward!(self, s => s.fetch_anime(id).await)
    }

    async fn fetch_list(
        &self,
        account: &Account,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<FetchedList, ServiceError> {
        forward!(self, s => s.fetch_list(account, progress).await)
    }

    async fn search(
        &self,
        params: &SearchParams,
        page: Option<PageToken>,
    ) -> Result<SearchPage, ServiceError> {
        forward!(self, s => s.search(params, page).await)
    }

    async fn add_entry(
        &self,
        account: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        forward!(self, s => s.add_entry(account, entry, dirty).await)
    }

    async fn update_entry(
        &self,
        account: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        forward!(self, s => s.update_entry(account, entry, dirty).await)
    }

    async fn delete_entry(&self, account: &Account, entry: &ListEntry) -> Result<(), ServiceError> {
        forward!(self, s => s.delete_entry(account, entry).await)
    }
}

/// How a stored score is shown for the list in use. A list on this computer
/// shows scores as the AniList account would.
pub fn format_rating(state: &crate::state::AppState, value: u8) -> String {
    use shiori_sync::anilist::score::{self, ScoreFormat};
    if state.catalog_service() == ServiceId::MyAnimeList {
        return shiori_sync::myanimelist::score::format_rating(value);
    }
    score::format_rating(
        ScoreFormat::from_name(&state.accounts.anilist.rating_system),
        value,
    )
}
