//! AniList, through its GraphQL API.

mod json;
pub mod score;

use jiff::Timestamp;
use reqwest::header::{AUTHORIZATION, SERVER};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use shiori_core::{Anime, AnimeId, Fields, ListEntry, RelationKind, SearchParams, ServiceId};

use crate::{
    Account, AccountInfo, FetchedList, PageToken, SavedEntry, SearchPage, Service, ServiceError,
};
use json::{
    CollectionData, DeleteData, MediaData, PageData, RelationsData, SaveData, ViewerData, decode,
    save_variables, search_variables, to_anime, to_entry, to_relations,
};

/// Shiori's API client, registered at <https://anilist.co/settings/developer>.
pub const CLIENT_ID: u32 = 56678;

const ENDPOINT: &str = "https://graphql.anilist.co";

const MEDIA_FRAGMENT: &str = include_str!("queries/media_fragment.graphql");
const MEDIA_LIST_FRAGMENT: &str = include_str!("queries/media_list_fragment.graphql");

/// Where the user approves Shiori. The client is registered with AniList's
/// "pin" redirect, which shows the token for copying. A local redirect would
/// not work: the implicit flow puts the token after `#`, which browsers never
/// send to a server.
pub fn authorize_url() -> String {
    format!("https://anilist.co/api/v2/oauth/authorize?client_id={CLIENT_ID}&response_type=token")
}

/// A query with the fragments it uses appended, as GraphQL wants them in the
/// same document.
fn with_fragments(query: &str) -> String {
    let mut text = query.to_owned();
    if query.contains("...MediaListFragment") {
        text.push('\n');
        text.push_str(MEDIA_LIST_FRAGMENT);
    }
    if query.contains("...MediaFragment") {
        text.push('\n');
        text.push_str(MEDIA_FRAGMENT);
    }
    text
}

#[derive(Debug, Clone)]
pub struct AniList {
    client: reqwest::Client,
    endpoint: String,
}

impl AniList {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_endpoint(client, ENDPOINT)
    }

    /// For tests against a local server.
    pub fn with_endpoint(client: reqwest::Client, endpoint: impl Into<String>) -> Self {
        Self {
            client,
            endpoint: endpoint.into(),
        }
    }

    async fn request<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Value,
        token: Option<&str>,
        fallback: &str,
        progress: Option<&(dyn Fn(u64, u64) + Send + Sync)>,
    ) -> Result<T, ServiceError> {
        let body = json!({ "query": with_fragments(query), "variables": variables });
        let mut request = self
            .client
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .json(&body);
        if let Some(token) = token {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        let mut response = request.send().await?;
        let status = response.status();
        tracing::debug!("[AniList] HTTP {status}, headers: {:?}", response.headers());
        let server = response
            .headers()
            .get(SERVER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let total = response.content_length().unwrap_or(0);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            bytes.extend_from_slice(&chunk);
            if let Some(progress) = progress {
                progress(bytes.len() as u64, total);
            }
        }
        decode(status, server.as_deref(), &bytes, fallback)
    }
}

impl AniList {
    /// The anime related to `id`, with the related anime's details. Only
    /// AniList has relations Shiori uses, so this is not on `Service`.
    pub async fn fetch_relations(
        &self,
        id: AnimeId,
    ) -> Result<Vec<(RelationKind, Anime)>, ServiceError> {
        const FALLBACK: &str = "Could not parse related anime.";
        let data: RelationsData = self
            .request(
                include_str!("queries/media_relations.graphql"),
                json!({ "id": id.0 }),
                None,
                FALLBACK,
                None,
            )
            .await?;
        let media = data
            .media
            .ok_or_else(|| ServiceError::Parse(FALLBACK.to_owned()))?;
        Ok(to_relations(media, Timestamp::now()))
    }
}

impl AniList {
    /// Most anime `fetch_anime_by_ids` takes at once; AniList pages hold 50.
    pub const IDS_PER_REQUEST: usize = 50;

    /// Several anime in one request, for keeping a local list's anime up to
    /// date without a request per anime. Anime AniList no longer has are
    /// simply missing from the answer.
    pub async fn fetch_anime_by_ids(&self, ids: &[AnimeId]) -> Result<Vec<Anime>, ServiceError> {
        debug_assert!(ids.len() <= Self::IDS_PER_REQUEST);
        let ids: Vec<u32> = ids.iter().map(|id| id.0).collect();
        let data: PageData = self
            .request(
                include_str!("queries/media_by_ids.graphql"),
                json!({ "ids": ids }),
                None,
                "Could not parse anime details.",
                None,
            )
            .await?;
        let now = Timestamp::now();
        Ok(data
            .page
            .media
            .into_iter()
            .filter_map(|m| to_anime(m, now))
            .collect())
    }
}

impl Service for AniList {
    fn id(&self) -> ServiceId {
        ServiceId::AniList
    }

    async fn authenticate(&self, account: &Account) -> Result<AccountInfo, ServiceError> {
        let token = account
            .token
            .as_deref()
            .ok_or_else(|| ServiceError::Unauthorized(json::EXPIRED_TOKEN.to_owned()))?;
        let data: ViewerData = self
            .request(
                include_str!("queries/viewer.graphql"),
                json!({}),
                Some(token),
                "Could not parse user information.",
                None,
            )
            .await?;
        let viewer = data.viewer;
        Ok(AccountInfo {
            user_id: viewer.id.to_string(),
            username: viewer.name,
            rating_system: viewer
                .media_list_options
                .and_then(|o| o.score_format)
                .unwrap_or_default(),
        })
    }

    async fn fetch_anime(&self, id: AnimeId) -> Result<Anime, ServiceError> {
        const FALLBACK: &str = "Could not parse anime details.";
        let data: MediaData = self
            .request(
                include_str!("queries/media.graphql"),
                json!({ "id": id.0 }),
                None,
                FALLBACK,
                None,
            )
            .await?;
        data.media
            .and_then(|m| to_anime(m, Timestamp::now()))
            .ok_or_else(|| ServiceError::Parse(FALLBACK.to_owned()))
    }

    async fn fetch_list(
        &self,
        account: &Account,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<FetchedList, ServiceError> {
        const FALLBACK: &str = "Could not parse anime list.";
        // The token is sent when we have one so that private entries are
        // included; without it only a public list can be read.
        let data: CollectionData = self
            .request(
                include_str!("queries/media_list_collection.graphql"),
                json!({ "userName": account.username }),
                account.token.as_deref(),
                FALLBACK,
                Some(progress),
            )
            .await?;
        let collection = data
            .collection
            .ok_or_else(|| ServiceError::Parse(FALLBACK.to_owned()))?;
        let now = Timestamp::now();
        let mut list = FetchedList::default();
        for item in collection.lists.into_iter().flat_map(|l| l.entries) {
            if let Some((entry, anime)) = to_entry(item, now) {
                list.anime.extend(anime);
                list.entries.push(entry);
            }
        }
        Ok(list)
    }

    async fn search(
        &self,
        params: &SearchParams,
        page: Option<PageToken>,
    ) -> Result<SearchPage, ServiceError> {
        let number = page.and_then(|p| p.0.parse().ok()).unwrap_or(1u32);
        let data: PageData = self
            .request(
                include_str!("queries/media_search.graphql"),
                search_variables(params, number),
                None,
                "Could not parse search results.",
                None,
            )
            .await?;
        let now = Timestamp::now();
        Ok(SearchPage {
            anime: data
                .page
                .media
                .into_iter()
                .filter_map(|m| to_anime(m, now))
                .collect(),
            next: data
                .page
                .page_info
                .has_next_page
                .then(|| PageToken((number + 1).to_string())),
        })
    }

    async fn add_entry(
        &self,
        account: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        self.save(account, entry, dirty).await
    }

    async fn update_entry(
        &self,
        account: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        self.save(account, entry, dirty).await
    }

    async fn delete_entry(&self, account: &Account, entry: &ListEntry) -> Result<(), ServiceError> {
        // An entry that never reached AniList has nothing to delete there.
        let Some(id) = entry.id else {
            return Ok(());
        };
        let result: Result<DeleteData, _> = self
            .request(
                include_str!("queries/delete_media_list_entry.graphql"),
                json!({ "id": id.0 }),
                account.token.as_deref(),
                "Could not delete list entry.",
                None,
            )
            .await;
        match result {
            Ok(_) | Err(ServiceError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl AniList {
    /// Adding and updating are the same mutation on AniList.
    async fn save(
        &self,
        account: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        let data: SaveData = self
            .request(
                include_str!("queries/save_media_list_entry.graphql"),
                save_variables(entry, dirty),
                account.token.as_deref(),
                "Could not update list entry.",
                None,
            )
            .await?;
        Ok(data
            .saved
            .and_then(|saved| to_entry(saved, Timestamp::now()))
            .map(|(entry, anime)| SavedEntry { entry, anime }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_are_appended_once_each() {
        let save = with_fragments(include_str!("queries/save_media_list_entry.graphql"));
        assert_eq!(save.matches("fragment MediaFragment").count(), 1);
        assert_eq!(save.matches("fragment MediaListFragment").count(), 1);
        let viewer = with_fragments(include_str!("queries/viewer.graphql"));
        assert!(!viewer.contains("fragment"));
    }

    #[test]
    fn authorize_url_uses_shiori_client() {
        assert_eq!(
            authorize_url(),
            "https://anilist.co/api/v2/oauth/authorize?client_id=56678&response_type=token"
        );
    }
}
