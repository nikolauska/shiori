//! MyAnimeList, through its REST API v2. See docs/sync-myanimelist.md.

pub mod auth;
mod json;
pub mod score;

use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, SERVER, WWW_AUTHENTICATE};
use reqwest::{Method, Url};
use serde::de::DeserializeOwned;
use shiori_core::{
    Anime, AnimeId, Fields, ListEntry, SearchParams, SearchSort, SeasonName, ServiceId,
};

use crate::{
    Account, AccountInfo, FetchedList, PageToken, SavedEntry, SearchPage, Service, ServiceError,
    Tokens,
};
use json::{
    ANIME_FIELDS, AnimeJson, LIST_STATUS_FIELDS, ListPageJson, ListStatusJson, Reply,
    SearchPageJson, TokenJson, UserJson, check, decode, form, next_offset, save_form, to_anime,
    to_entry,
};

/// Shiori's API client, registered at <https://myanimelist.net/apiconfig> as
/// app type "other", which has no client secret (decision I35).
pub const CLIENT_ID: &str = "b77ce5295fd04e468863d09d275c19b7";

const API_URL: &str = "https://api.myanimelist.net/v2";
const TOKEN_URL: &str = "https://myanimelist.net/v1/oauth2/token";

const LIST_PAGE_LIMIT: u32 = 1000;
const SEARCH_PAGE_LIMIT: u32 = 100;
const SEASON_PAGE_LIMIT: u32 = 500;

const NO_REFRESH_TOKEN: &str = "Refresh token is unavailable.";

pub fn anime_page_url(id: AnimeId) -> String {
    format!("https://myanimelist.net/anime/{id}/")
}

/// The newest tokens this service knows of. Kept here, not only handed to
/// the app, because the next request may be sent before the app has stored
/// them, and MyAnimeList may reject the old refresh token once it issued a
/// new one.
#[derive(Default)]
struct TokenState {
    current: Option<Tokens>,
    unsaved: bool,
}

#[derive(Clone)]
pub struct MyAnimeList {
    client: reqwest::Client,
    api: String,
    token_url: String,
    tokens: Arc<Mutex<TokenState>>,
}

impl std::fmt::Debug for MyAnimeList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MyAnimeList")
            .field("api", &self.api)
            .finish_non_exhaustive()
    }
}

impl MyAnimeList {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_urls(client, API_URL, TOKEN_URL)
    }

    /// For tests against a local server.
    pub fn with_urls(
        client: reqwest::Client,
        api: impl Into<String>,
        token_url: impl Into<String>,
    ) -> Self {
        Self {
            client,
            api: api.into().trim_end_matches('/').to_owned(),
            token_url: token_url.into(),
            tokens: Arc::default(),
        }
    }

    /// Finishes signing in: trades the code from the browser for tokens.
    /// The app stores them and then calls `authenticate`.
    pub async fn exchange_code(&self, code: &str, verifier: &str) -> Result<Tokens, ServiceError> {
        let body = form(&[
            ("client_id", CLIENT_ID.to_owned()),
            ("grant_type", "authorization_code".to_owned()),
            ("code", code.to_owned()),
            ("redirect_uri", auth::REDIRECT_URI.to_owned()),
            ("code_verifier", verifier.to_owned()),
        ]);
        let reply = self
            .send(Method::POST, self.token_url.clone(), Some(body), None)
            .await?;
        let json: TokenJson = decode(&reply, "Could not parse authentication data.")?;
        let tokens = Tokens {
            access: json.access_token,
            refresh: json.refresh_token,
        };
        let mut state = self.lock();
        state.current = Some(tokens.clone());
        state.unsaved = false;
        Ok(tokens)
    }

    /// After signing out, so a later account does not get the old tokens.
    pub fn forget_tokens(&self) {
        *self.lock() = TokenState::default();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TokenState> {
        // A panic while holding the lock leaves nothing half-written: the
        // state is replaced whole.
        self.tokens.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn access_token(&self, account: &Account) -> Option<String> {
        let state = self.lock();
        state
            .current
            .as_ref()
            .map(|t| t.access.clone())
            .or_else(|| account.token.clone())
    }

    /// Gets a new access token. `used` is the token that was just rejected:
    /// when another request has refreshed in the meantime, its token is used
    /// instead of refreshing again with a refresh token that may be spent.
    async fn refresh(&self, account: &Account, used: Option<&str>) -> Result<String, ServiceError> {
        let refresh_token = {
            let state = self.lock();
            if let Some(current) = &state.current
                && Some(current.access.as_str()) != used
            {
                return Ok(current.access.clone());
            }
            state
                .current
                .as_ref()
                .map(|t| t.refresh.clone())
                .or_else(|| account.refresh_token.clone())
        };
        let refresh_token =
            refresh_token.ok_or_else(|| ServiceError::Unauthorized(NO_REFRESH_TOKEN.to_owned()))?;
        let body = form(&[
            ("client_id", CLIENT_ID.to_owned()),
            ("grant_type", "refresh_token".to_owned()),
            ("refresh_token", refresh_token),
        ]);
        let reply = self
            .send(Method::POST, self.token_url.clone(), Some(body), None)
            .await?;
        // Any refusal here means the sign-in is over; the user signs in again.
        let json: TokenJson =
            decode(&reply, "Could not refresh the access token.").map_err(|error| match error {
                ServiceError::Api(message) => ServiceError::Unauthorized(message),
                other => other,
            })?;
        let access = json.access_token.clone();
        let mut state = self.lock();
        state.current = Some(Tokens {
            access: json.access_token,
            refresh: json.refresh_token,
        });
        state.unsaved = true;
        Ok(access)
    }

    async fn send(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        token: Option<&str>,
    ) -> Result<Reply, ServiceError> {
        let mut request = self
            .client
            .request(method, &url)
            .header("Accept", "application/json");
        request = match token {
            Some(token) => request.header(AUTHORIZATION, format!("Bearer {token}")),
            // Requests without a user still have to say which app is asking.
            None => request.header("X-MAL-CLIENT-ID", CLIENT_ID),
        };
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(body);
        }
        let response = request.send().await?;
        let status = response.status();
        tracing::debug!(
            "[MyAnimeList] HTTP {status}, headers: {:?}",
            response.headers()
        );
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let server = header(SERVER);
        let www_authenticate = header(WWW_AUTHENTICATE);
        let body = response.bytes().await?.to_vec();
        Ok(Reply {
            status,
            server,
            www_authenticate,
            body,
        })
    }

    /// A request as the user, refreshing the token once if it expired.
    async fn call_as(
        &self,
        account: &Account,
        method: Method,
        url: String,
        body: Option<String>,
    ) -> Result<Reply, ServiceError> {
        let mut token = self.access_token(account);
        if token.is_none() && account.refresh_token.is_some() {
            token = Some(self.refresh(account, None).await?);
        }
        let reply = self
            .send(method.clone(), url.clone(), body.clone(), token.as_deref())
            .await?;
        if !reply.token_expired() || token.is_none() {
            return Ok(reply);
        }
        let token = self.refresh(account, token.as_deref()).await?;
        self.send(method, url, body, Some(&token)).await
    }

    async fn get_public<T: DeserializeOwned>(
        &self,
        url: Url,
        fallback: &str,
    ) -> Result<T, ServiceError> {
        let reply = self.send(Method::GET, url.into(), None, None).await?;
        decode(&reply, fallback)
    }

    fn url(&self, path: &str, query: &[(&str, String)]) -> Url {
        let mut url = Url::parse(&format!("{}{path}", self.api)).expect("API URL is valid");
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        url
    }

    async fn save(
        &self,
        account: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        let url = self.url(&format!("/anime/{}/my_list_status", entry.anime_id), &[]);
        let reply = self
            .call_as(
                account,
                Method::PATCH,
                url.into(),
                Some(save_form(entry, dirty)),
            )
            .await?;
        let status: ListStatusJson =
            decode(&reply, "Could not parse list entry.").map_err(|error| match error {
                ServiceError::NotFound => {
                    ServiceError::Api("Anime list entry does not exist.".to_owned())
                }
                other => other,
            })?;
        Ok(Some(SavedEntry {
            entry: to_entry(entry.anime_id, status),
            anime: None,
        }))
    }
}

impl Service for MyAnimeList {
    fn id(&self) -> ServiceId {
        ServiceId::MyAnimeList
    }

    fn take_refreshed_tokens(&self) -> Option<Tokens> {
        let mut state = self.lock();
        if !state.unsaved {
            return None;
        }
        state.unsaved = false;
        state.current.clone()
    }

    async fn authenticate(&self, account: &Account) -> Result<AccountInfo, ServiceError> {
        if self.access_token(account).is_none() && account.refresh_token.is_none() {
            return Err(ServiceError::Unauthorized(
                "Please sign in to MyAnimeList via Settings.".to_owned(),
            ));
        }
        let url = self.url("/users/@me", &[]);
        let reply = self.call_as(account, Method::GET, url.into(), None).await?;
        let user: UserJson = decode(&reply, "Could not parse user information.")?;
        Ok(AccountInfo {
            user_id: user.id.to_string(),
            username: user.name,
            rating_system: String::new(),
        })
    }

    async fn fetch_anime(&self, id: AnimeId) -> Result<Anime, ServiceError> {
        const FALLBACK: &str = "Could not parse anime details.";
        let url = self.url(
            &format!("/anime/{id}"),
            &[("fields", ANIME_FIELDS.to_owned())],
        );
        let json: AnimeJson = self.get_public(url, FALLBACK).await?;
        to_anime(json, Timestamp::now()).ok_or_else(|| ServiceError::Parse(FALLBACK.to_owned()))
    }

    async fn fetch_list(
        &self,
        account: &Account,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<FetchedList, ServiceError> {
        const FALLBACK: &str = "Could not parse anime list.";
        // `@me` works for the signed-in user even before the username is
        // known; a public list needs the name.
        let user = if account.username.is_empty() {
            "@me"
        } else {
            account.username.as_str()
        };
        let path = format!("/users/{user}/animelist");
        let fields = format!("{ANIME_FIELDS},list_status{{{LIST_STATUS_FIELDS}}}");
        let now = Timestamp::now();
        let mut list = FetchedList::default();
        let mut offset = 0;
        loop {
            let url = self.url(
                &path,
                &[
                    ("limit", LIST_PAGE_LIMIT.to_string()),
                    ("offset", offset.to_string()),
                    ("nsfw", "true".to_owned()),
                    ("fields", fields.clone()),
                ],
            );
            // A public list is read without a token, as the client.
            let reply = if self.access_token(account).is_some() || account.refresh_token.is_some() {
                self.call_as(account, Method::GET, url.into(), None).await?
            } else {
                self.send(Method::GET, url.into(), None, None).await?
            };
            let page: ListPageJson = decode(&reply, FALLBACK)?;
            for item in page.data {
                let Some(anime) = to_anime(item.node, now) else {
                    continue;
                };
                if let Some(status) = item.list_status {
                    list.entries.push(to_entry(anime.id, status));
                }
                list.anime.push(anime);
            }
            // The total is unknown until the last page; report what we have.
            progress(list.entries.len() as u64, 0);
            match next_offset(&page.paging) {
                Some(next) if next > offset => offset = next,
                _ => break,
            }
        }
        Ok(list)
    }

    async fn search(
        &self,
        params: &SearchParams,
        page: Option<PageToken>,
    ) -> Result<SearchPage, ServiceError> {
        let offset: u32 = page.and_then(|p| p.0.parse().ok()).unwrap_or(0);
        let season = match params.season {
            Some(SeasonName::Winter) => Some("winter"),
            Some(SeasonName::Spring) => Some("spring"),
            Some(SeasonName::Summer) => Some("summer"),
            Some(SeasonName::Fall) => Some("fall"),
            Some(SeasonName::Unknown) | None => None,
        };
        let text = params.text.trim();
        let url = match (season, params.year) {
            (Some(season), Some(year)) => self.url(
                &format!("/anime/season/{year}/{season}"),
                &[
                    ("limit", SEASON_PAGE_LIMIT.to_string()),
                    ("offset", offset.to_string()),
                    ("nsfw", "true".to_owned()),
                    ("fields", ANIME_FIELDS.to_owned()),
                ],
            ),
            // MyAnimeList can only browse by season; without one there must
            // be text to search for.
            _ if text.is_empty() => return Ok(SearchPage::default()),
            _ => self.url(
                "/anime",
                &[
                    ("q", text.to_owned()),
                    ("limit", SEARCH_PAGE_LIMIT.to_string()),
                    ("offset", offset.to_string()),
                    ("nsfw", "true".to_owned()),
                    ("fields", ANIME_FIELDS.to_owned()),
                ],
            ),
        };
        let json: SearchPageJson = self
            .get_public(url, "Could not parse search results.")
            .await?;
        let next = next_offset(&json.paging)
            .filter(|&n| n > offset)
            .map(|n| PageToken(n.to_string()));
        let now = Timestamp::now();
        let mut anime: Vec<Anime> = json
            .data
            .into_iter()
            .filter_map(|item| to_anime(item.node, now))
            // MyAnimeList takes no type or status filter; they are applied
            // here instead.
            .filter(|a| params.kind.is_none_or(|k| a.kind == k))
            .filter(|a| params.status.is_none_or(|s| a.status == s))
            .collect();
        sort_locally(&mut anime, params);
        Ok(SearchPage { anime, next })
    }

    async fn add_entry(
        &self,
        account: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        // Adding is the same request as updating on MyAnimeList.
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
        let url = self.url(&format!("/anime/{}/my_list_status", entry.anime_id), &[]);
        let reply = self
            .call_as(account, Method::DELETE, url.into(), None)
            .await?;
        match check(&reply, "Could not delete list entry.") {
            Ok(()) | Err(ServiceError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// The column sort from the search page. With search text, MyAnimeList's
/// own relevance order is kept, as for AniList.
fn sort_locally(anime: &mut [Anime], params: &SearchParams) {
    if !params.text.trim().is_empty() {
        return;
    }
    let Some(sort) = params.sort else {
        return;
    };
    anime.sort_by(|a, b| {
        let order = match sort {
            SearchSort::Title => a.titles.romaji.cmp(&b.titles.romaji),
            SearchSort::Duration => a.episode_length.cmp(&b.episode_length),
            SearchSort::Score => a.score.total_cmp(&b.score),
            SearchSort::Type => (a.kind as u8).cmp(&(b.kind as u8)),
            SearchSort::StartDate => a.date_started.cmp(&b.date_started),
        };
        if params.descending {
            order.reverse()
        } else {
            order
        }
    });
}
