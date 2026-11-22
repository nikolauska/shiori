//! The MyAnimeList client against a local server that answers with canned
//! responses, to check what is sent, how answers are read, and that an
//! expired token is refreshed once.

use shiori_core::{AnimeId, Fields, ListEntry, ListStatus};
use shiori_sync::myanimelist::MyAnimeList;
use shiori_sync::{Account, NetworkConfig, Service, ServiceError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct Answer {
    status: u16,
    headers: &'static str,
    body: String,
}

fn answer(status: u16, body: impl Into<String>) -> Answer {
    Answer {
        status,
        headers: "",
        body: body.into(),
    }
}

/// Answers requests in order, one per connection, and returns the
/// requests as text.
async fn serve(
    answers: impl FnOnce(&str) -> Vec<Answer>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    // Built once the address is known, so answers can link back here.
    let answers = answers(&url);
    let handle = tokio::spawn(async move {
        let mut requests = Vec::new();
        for a in answers {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 8192];
            loop {
                let n = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 {} X\r\ncontent-type: application/json\r\n{}content-length: {}\r\nconnection: close\r\n\r\n{}",
                a.status,
                a.headers,
                a.body.len(),
                a.body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            requests.push(String::from_utf8_lossy(&request).into_owned());
        }
        requests
    });
    (url, handle)
}

fn client(url: &str) -> MyAnimeList {
    MyAnimeList::with_urls(
        NetworkConfig::default().client().unwrap(),
        format!("{url}/v2"),
        format!("{url}/token"),
    )
}

fn account() -> Account {
    Account {
        username: "someone".into(),
        token: Some("old-access".into()),
        refresh_token: Some("old-refresh".into()),
    }
}

const USER: &str = r#"{"id":42,"name":"someone"}"#;
const TOKENS: &str = r#"{"token_type":"Bearer","expires_in":3600,"access_token":"new-access","refresh_token":"new-refresh"}"#;
const EXPIRED: &str = "www-authenticate: Bearer error=\"invalid_token\", error_description=\"The access token expired\"\r\n";

#[tokio::test]
async fn expired_token_is_refreshed_once() {
    let (url, server) = serve(|_| {
        vec![
            Answer {
                status: 401,
                headers: EXPIRED,
                body: r#"{"error":"invalid_token"}"#.into(),
            },
            answer(200, TOKENS),
            answer(200, USER),
        ]
    })
    .await;
    let mal = client(&url);
    let info = mal.authenticate(&account()).await.unwrap();
    assert_eq!(info.username, "someone");
    assert_eq!(info.user_id, "42");

    let requests = server.await.unwrap();
    let first = requests[0].to_lowercase();
    assert!(first.starts_with("get /v2/users/@me"), "{first}");
    assert!(
        first.contains("authorization: bearer old-access"),
        "{first}"
    );
    let refresh = &requests[1];
    assert!(refresh.starts_with("POST /token"), "{refresh}");
    assert!(
        refresh.ends_with(
            "client_id=b77ce5295fd04e468863d09d275c19b7&grant_type=refresh_token&refresh_token=old-refresh"
        ),
        "{refresh}"
    );
    assert!(
        requests[2]
            .to_lowercase()
            .contains("authorization: bearer new-access"),
        "{}",
        requests[2]
    );

    let tokens = mal.take_refreshed_tokens().unwrap();
    assert_eq!(tokens.access, "new-access");
    assert_eq!(tokens.refresh, "new-refresh");
    assert_eq!(mal.take_refreshed_tokens(), None);
}

#[tokio::test]
async fn missing_refresh_token_signs_out() {
    let (url, _server) = serve(|_| {
        vec![Answer {
            status: 401,
            headers: EXPIRED,
            body: String::new(),
        }]
    })
    .await;
    let mut account = account();
    account.refresh_token = None;
    let error = client(&url).authenticate(&account).await.unwrap_err();
    assert_eq!(
        error,
        ServiceError::Unauthorized("Refresh token is unavailable.".into())
    );
}

#[tokio::test]
async fn refused_refresh_signs_out() {
    let (url, _server) = serve(|_| {
        vec![
            Answer {
                status: 401,
                headers: EXPIRED,
                body: String::new(),
            },
            answer(
                400,
                r#"{"error":"invalid_grant","message":"The refresh token is invalid."}"#,
            ),
        ]
    })
    .await;
    let error = client(&url).authenticate(&account()).await.unwrap_err();
    assert_eq!(
        error,
        ServiceError::Unauthorized("The refresh token is invalid.".into())
    );
}

#[tokio::test]
async fn list_is_fetched_page_by_page() {
    let item = |id: u32, watched: u32| {
        format!(
            r#"{{"node":{{"id":{id},"title":"Anime {id}"}},"list_status":{{"status":"watching","score":7,"num_episodes_watched":{watched},"is_rewatching":false,"updated_at":"2026-11-01T10:00:00+00:00"}}}}"#
        )
    };
    let (url, server) = serve(|url| {
        vec![
            answer(
                200,
                format!(
                    r#"{{"data":[{},{}],"paging":{{"next":"{url}/v2/users/someone/animelist?offset=2&limit=2"}}}}"#,
                    item(1, 3),
                    item(2, 5)
                ),
            ),
            answer(200, format!(r#"{{"data":[{}],"paging":{{}}}}"#, item(3, 1))),
        ]
    })
    .await;
    let list = client(&url)
        .fetch_list(&account(), &|_, _| {})
        .await
        .unwrap();
    assert_eq!(list.entries.len(), 3);
    assert_eq!(list.anime.len(), 3);
    assert_eq!(list.entries[1].watched_episodes, 5);
    assert_eq!(list.entries[0].score, 70);
    let requests = server.await.unwrap();
    assert!(requests[0].contains("offset=0"), "{}", requests[0]);
    assert!(requests[0].contains("nsfw=true"), "{}", requests[0]);
    assert!(requests[0].contains("list_status%7B"), "{}", requests[0]);
    assert!(requests[1].contains("offset=2"), "{}", requests[1]);
}

#[tokio::test]
async fn public_list_is_read_as_the_client() {
    let (url, server) = serve(|_| vec![answer(200, r#"{"data":[],"paging":{}}"#)]).await;
    let account = Account {
        username: "someone".into(),
        ..Account::default()
    };
    client(&url).fetch_list(&account, &|_, _| {}).await.unwrap();
    let request = server.await.unwrap()[0].to_lowercase();
    assert!(
        request.contains("x-mal-client-id: b77ce5295fd04e468863d09d275c19b7"),
        "{request}"
    );
    assert!(!request.contains("authorization"), "{request}");
}

#[tokio::test]
async fn save_sends_dirty_fields_as_a_form() {
    let (url, server) = serve(|_| vec![answer(
        200,
        r#"{"status":"watching","score":0,"num_episodes_watched":4,"is_rewatching":false,"updated_at":"2026-11-22T10:00:00+00:00"}"#,
    )])
    .await;
    let mut entry = ListEntry::new(AnimeId(4224));
    entry.status = ListStatus::Watching;
    entry.watched_episodes = 4;
    let saved = client(&url)
        .add_entry(&account(), &entry, Fields::EPISODE)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.entry.watched_episodes, 4);
    assert_eq!(saved.entry.id, None);
    let request = server.await.unwrap()[0].clone();
    assert!(
        request.starts_with("PATCH /v2/anime/4224/my_list_status"),
        "{request}"
    );
    assert!(
        request
            .to_lowercase()
            .contains("content-type: application/x-www-form-urlencoded"),
        "{request}"
    );
    assert!(
        request.ends_with("\r\n\r\nnum_watched_episodes=4"),
        "{request}"
    );
}

#[tokio::test]
async fn saving_a_missing_entry_says_so() {
    let (url, _server) = serve(|_| vec![answer(404, r#"{"error":"not_found"}"#)]).await;
    let error = client(&url)
        .update_entry(&account(), &ListEntry::new(AnimeId(1)), Fields::SCORE)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ServiceError::Api("Anime list entry does not exist.".into())
    );
}

#[tokio::test]
async fn delete_of_missing_entry_succeeds() {
    let (url, server) = serve(|_| vec![answer(404, r#"{"error":"not_found"}"#)]).await;
    client(&url)
        .delete_entry(&account(), &ListEntry::new(AnimeId(9)))
        .await
        .unwrap();
    let request = server.await.unwrap()[0].clone();
    assert!(
        request.starts_with("DELETE /v2/anime/9/my_list_status"),
        "{request}"
    );
}

#[tokio::test]
async fn code_is_exchanged_with_the_verifier() {
    let (url, server) = serve(|_| vec![answer(200, TOKENS)]).await;
    let mal = client(&url);
    let tokens = mal.exchange_code("the-code", "the-verifier").await.unwrap();
    assert_eq!(tokens.access, "new-access");
    // Stored by the app itself after sign-in, so not handed back again.
    assert_eq!(mal.take_refreshed_tokens(), None);
    let request = server.await.unwrap()[0].clone();
    assert!(
        request.ends_with(
            "client_id=b77ce5295fd04e468863d09d275c19b7&grant_type=authorization_code&code=the-code&redirect_uri=http%3A%2F%2F127.0.0.1%3A47391%2Fcallback&code_verifier=the-verifier"
        ),
        "{request}"
    );
}

#[tokio::test]
async fn missing_anime_is_not_found() {
    let (url, _server) = serve(|_| vec![answer(404, r#"{"error":"not_found"}"#)]).await;
    let error = client(&url).fetch_anime(AnimeId(1)).await.unwrap_err();
    assert_eq!(error, ServiceError::NotFound);
}
