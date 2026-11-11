//! The AniList client against a local server that answers with recorded
//! responses, to check what is sent and how answers are read.

use shiori_core::{AnimeId, Fields, ListEntry, ListEntryId, ListStatus};
use shiori_sync::anilist::AniList;
use shiori_sync::{Account, NetworkConfig, Service, ServiceError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Answers one request with `status` and `body`, and returns the request as
/// text once it has been read.
async fn serve_once(status: u16, body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
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
            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&request).into_owned()
    });
    (url, handle)
}

fn client(url: &str) -> AniList {
    AniList::with_endpoint(NetworkConfig::default().client().unwrap(), url)
}

fn account() -> Account {
    Account {
        username: "someone".into(),
        token: Some("tok".into()),
    }
}

#[tokio::test]
async fn authenticate_sends_the_token() {
    let (url, server) = serve_once(200, include_str!("data/anilist/viewer.json")).await;
    let info = client(&url).authenticate(&account()).await.unwrap();
    assert_eq!(info.username, "someone");
    assert_eq!(info.user_id, "123");
    assert_eq!(info.rating_system, "POINT_10_DECIMAL");
    let request = server.await.unwrap().to_lowercase();
    assert!(request.contains("authorization: bearer tok"), "{request}");
    assert!(request.contains("user-agent: shiori/0.1"), "{request}");
    assert!(request.contains("viewer"), "{request}");
}

#[tokio::test]
async fn invalid_token_asks_to_sign_in_again() {
    let (url, _server) = serve_once(400, include_str!("data/anilist/invalid-token.json")).await;
    let error = client(&url).authenticate(&account()).await.unwrap_err();
    assert!(matches!(error, ServiceError::Unauthorized(_)));
    assert_eq!(
        error.to_string(),
        "Access token has expired. Please re-authorize your account via Settings."
    );
}

#[tokio::test]
async fn fetch_list_reports_progress() {
    let (url, server) = serve_once(200, include_str!("data/anilist/list.json")).await;
    let progress = std::sync::Mutex::new(Vec::new());
    let list = client(&url)
        .fetch_list(&account(), &|done, total| {
            progress.lock().unwrap().push((done, total))
        })
        .await
        .unwrap();
    assert_eq!(list.entries.len(), 3);
    assert_eq!(list.anime.len(), 3);
    let progress = progress.into_inner().unwrap();
    let total = include_str!("data/anilist/list.json").len() as u64;
    assert_eq!(progress.last(), Some(&(total, total)));
    let request = server.await.unwrap();
    assert!(request.contains(r#""userName":"someone""#), "{request}");
    assert!(request.contains("fragment MediaListFragment"), "{request}");
}

#[tokio::test]
async fn missing_anime_is_not_found() {
    let (url, _server) = serve_once(404, include_str!("data/anilist/media-not-found.json")).await;
    let error = client(&url)
        .fetch_anime(AnimeId(999_999_999))
        .await
        .unwrap_err();
    assert_eq!(error, ServiceError::NotFound);
}

#[tokio::test]
async fn delete_of_missing_entry_succeeds() {
    let (url, server) = serve_once(
        404,
        r#"{"errors":[{"message":"Not Found.","status":404}],"data":null}"#,
    )
    .await;
    let mut entry = ListEntry::new(AnimeId(1));
    entry.id = Some(ListEntryId(77));
    client(&url).delete_entry(&account(), &entry).await.unwrap();
    let request = server.await.unwrap();
    assert!(request.contains(r#""variables":{"id":77}"#), "{request}");
}

#[tokio::test]
async fn save_sends_dirty_fields_and_reads_answer() {
    let body = r#"{"data":{"SaveMediaListEntry":{"id":5,"status":"CURRENT","score":0,"progress":3,
        "repeat":0,"private":false,"notes":null,"startedAt":{},"completedAt":{},"updatedAt":1790000000,
        "media":{"id":4224,"title":{"romaji":"Toradora!"}}}}}"#;
    let (url, server) = serve_once(200, body).await;
    let mut entry = ListEntry::new(AnimeId(4224));
    entry.status = ListStatus::Watching;
    entry.watched_episodes = 3;
    let saved = client(&url)
        .add_entry(&account(), &entry, Fields::EPISODE)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.entry.id, Some(ListEntryId(5)));
    assert_eq!(saved.entry.watched_episodes, 3);
    assert_eq!(saved.anime.unwrap().titles.romaji, "Toradora!");
    let request = server.await.unwrap();
    assert!(
        request.contains(r#""variables":{"mediaId":4224,"progress":3}"#),
        "{request}"
    );
}
