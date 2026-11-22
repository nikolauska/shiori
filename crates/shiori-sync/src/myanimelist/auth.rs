//! Signing in to MyAnimeList: OAuth 2 authorization code with PKCE, the
//! code caught by a listener on the loopback address (decision I35).

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use reqwest::Url;

use super::CLIENT_ID;

/// Must match the address registered for Shiori's client exactly; MyAnimeList
/// compares the whole string, port included.
pub const REDIRECT_URI: &str = "http://127.0.0.1:47391/callback";
const LISTEN_ADDRESS: &str = "127.0.0.1:47391";
const AUTHORIZE_URL: &str = "https://myanimelist.net/v1/oauth2/authorize";

/// One sign-in attempt: the page to open and what to check the answer with.
#[derive(Clone)]
pub struct Authorization {
    pub url: String,
    pub verifier: String,
    pub state: String,
}

// The verifier is half of the proof that this app started the sign-in.
impl std::fmt::Debug for Authorization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authorization")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Authorization {
    pub fn new() -> Self {
        // MyAnimeList only supports the `plain` method, so the verifier is
        // sent as the challenge in the authorize URL and is not a secret from
        // anyone who sees that URL. `fastrand` (seeded from the OS) is enough
        // for that; the `state` value only has to be unguessable by a web page
        // trying to forge a callback.
        let verifier = random_text(64);
        let state = random_text(24);
        let mut url = Url::parse(AUTHORIZE_URL).expect("constant URL is valid");
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("code_challenge", &verifier)
            .append_pair("code_challenge_method", "plain")
            .append_pair("state", &state);
        Self {
            url: url.into(),
            verifier,
            state,
        }
    }
}

impl Default for Authorization {
    fn default() -> Self {
        Self::new()
    }
}

/// From the characters RFC 7636 allows in a verifier.
fn random_text(len: usize) -> String {
    const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    (0..len)
        .map(|_| char::from(UNRESERVED[fastrand::usize(..UNRESERVED.len())]))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("Sign-in was cancelled.")]
    Cancelled,
    #[error("MyAnimeList did not answer in time.")]
    TimedOut,
    /// MyAnimeList sent an error instead of a code, for example when the
    /// user pressed "Deny".
    #[error("{0}")]
    Denied(String),
    /// The answer belongs to another sign-in attempt, or was forged.
    #[error("The answer from MyAnimeList did not match this sign-in. Try again.")]
    StateMismatch,
    #[error("That is not a MyAnimeList sign-in code or address.")]
    NotACode,
    #[error("Could not listen for MyAnimeList's answer: {0}")]
    Listen(String),
}

/// What the user pasted: either the code itself, or the whole address the
/// browser ended up on. Accepting the address saves digging the code out of
/// it when the listener could not run.
pub fn code_from_input(input: &str, state: &str) -> Result<String, AuthError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(AuthError::NotACode);
    }
    if input.contains("code=") || input.contains("error=") || input.contains("://") {
        let query = input.split_once('?').map_or(input, |(_, q)| q);
        return code_from_query(query, state);
    }
    if input.chars().any(char::is_whitespace) {
        return Err(AuthError::NotACode);
    }
    Ok(input.to_owned())
}

fn code_from_query(query: &str, state: &str) -> Result<String, AuthError> {
    let mut url = Url::parse("http://query.invalid/").expect("constant URL is valid");
    url.set_query(Some(query));
    let get = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
    };
    if let Some(error) = get("error") {
        let message = get("message")
            .or_else(|| get("error_description"))
            .filter(|m| !m.is_empty());
        return Err(AuthError::Denied(match error.as_str() {
            "access_denied" => "Sign-in was cancelled on MyAnimeList.".to_owned(),
            _ => message.unwrap_or(error),
        }));
    }
    // A pasted address carries the state; a missing one is accepted so that
    // a cut-off paste still works, but a different one never is.
    if get("state").is_some_and(|s| s != state) {
        return Err(AuthError::StateMismatch);
    }
    get("code")
        .filter(|c| !c.is_empty())
        .ok_or(AuthError::NotACode)
}

/// Waits for the browser to come back to [`REDIRECT_URI`].
#[derive(Debug)]
pub struct Listener(TcpListener);

impl Listener {
    /// Fails when another program holds the port; the user can then paste
    /// the address instead.
    pub fn bind() -> Result<Self, AuthError> {
        let listener =
            TcpListener::bind(LISTEN_ADDRESS).map_err(|e| AuthError::Listen(e.to_string()))?;
        // Polled so that `cancel` and the timeout are noticed without a
        // second thread to wake the accept.
        listener
            .set_nonblocking(true)
            .map_err(|e| AuthError::Listen(e.to_string()))?;
        Ok(Self(listener))
    }

    /// Blocks until the code arrives, `cancel` is set, or `timeout` passes.
    /// Run it off the UI thread.
    pub fn wait(
        self,
        state: &str,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<String, AuthError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AuthError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(AuthError::TimedOut);
            }
            match self.0.accept() {
                Ok((stream, _)) => {
                    if let Some(result) = answer(stream, state) {
                        return result;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(AuthError::Listen(e.to_string())),
            }
        }
    }
}

/// Reads one request and answers it. `None` for requests that are not the
/// callback, such as the browser asking for a favicon.
fn answer(stream: TcpStream, state: &str) -> Option<Result<String, AuthError>> {
    // The listener is non-blocking; this one connection should not be, but
    // a browser that never finishes its request must not hang the sign-in.
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    // "GET /callback?code=...&state=... HTTP/1.1"
    let target = line.split_whitespace().nth(1).unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut stream = &stream;
    if path != "/callback" {
        let _ = stream.write_all(response("404 Not Found", "Not found.").as_bytes());
        return None;
    }
    let result = code_from_query(query, state).and_then(|code| {
        // Unlike a paste, the browser always brings the state back.
        if query.contains("state=") {
            Ok(code)
        } else {
            Err(AuthError::StateMismatch)
        }
    });
    let page = match &result {
        // The code still has to be exchanged, which can fail; the app says
        // how it went.
        Ok(_) => page(
            "Back to Shiori",
            "MyAnimeList answered. You can close this tab; Shiori finishes signing in.",
        ),
        Err(error) => page("Shiori could not sign in", &error.to_string()),
    };
    let _ = stream.write_all(response("200 OK", &page).as_bytes());
    Some(result)
}

fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\n\
         cache-control: no-store\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn page(title: &str, text: &str) -> String {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    format!(
        "<!doctype html><meta charset=utf-8><title>Shiori</title>\
         <body style=\"margin:0;min-height:100vh;display:grid;place-items:center;\
         background:#0a0a0a;color:#fafafa;font-family:system-ui,sans-serif;text-align:center\">\
         <div><h1 style=\"font-size:20px;font-weight:600\">{}</h1>\
         <p style=\"color:#a1a1aa\">{}</p></div>",
        escape(title),
        escape(text)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorize_url_has_everything() {
        let auth = Authorization::new();
        assert_eq!(auth.verifier.len(), 64);
        let url = Url::parse(&auth.url).unwrap();
        let get = |k: &str| {
            url.query_pairs()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(get("client_id").as_deref(), Some(CLIENT_ID));
        assert_eq!(get("redirect_uri").as_deref(), Some(REDIRECT_URI));
        assert_eq!(get("code_challenge"), Some(auth.verifier.clone()));
        assert_eq!(get("code_challenge_method").as_deref(), Some("plain"));
        assert_eq!(get("state"), Some(auth.state.clone()));
        assert_ne!(Authorization::new().verifier, auth.verifier);
    }

    #[test]
    fn pasted_code_or_address() {
        assert_eq!(code_from_input("  abc123 ", "s").unwrap(), "abc123");
        assert_eq!(
            code_from_input("http://127.0.0.1:47391/callback?code=def&state=s", "s").unwrap(),
            "def"
        );
        // Cut off before the state: still usable.
        assert_eq!(
            code_from_input("127.0.0.1:47391/callback?code=def", "s").unwrap(),
            "def"
        );
        assert_eq!(
            code_from_input("http://127.0.0.1:47391/callback?code=def&state=other", "s"),
            Err(AuthError::StateMismatch)
        );
        assert_eq!(
            code_from_input(
                "http://127.0.0.1:47391/callback?error=access_denied&state=s",
                "s"
            ),
            Err(AuthError::Denied(
                "Sign-in was cancelled on MyAnimeList.".into()
            ))
        );
        assert_eq!(code_from_input("", "s"), Err(AuthError::NotACode));
        assert_eq!(code_from_input("two words", "s"), Err(AuthError::NotACode));
    }

    #[test]
    fn listener_answers_the_browser() {
        // The real port may be taken on a developer machine; the logic is the
        // same on any port.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let browser = std::thread::spawn(move || {
            let get = |path: &str| {
                let mut s = TcpStream::connect(address).unwrap();
                write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut out = String::new();
                std::io::Read::read_to_string(&mut s, &mut out).unwrap();
                out
            };
            let favicon = get("/favicon.ico");
            let callback = get("/callback?code=the-code&state=st");
            (favicon, callback)
        });
        let cancel = AtomicBool::new(false);
        let code = Listener(listener)
            .wait("st", Duration::from_secs(10), &cancel)
            .unwrap();
        assert_eq!(code, "the-code");
        let (favicon, callback) = browser.join().unwrap();
        assert!(favicon.starts_with("HTTP/1.1 404"), "{favicon}");
        assert!(callback.contains("Back to Shiori"), "{callback}");
    }

    #[test]
    fn listener_stops_when_cancelled() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let cancel = AtomicBool::new(true);
        assert_eq!(
            Listener(listener).wait("st", Duration::from_secs(10), &cancel),
            Err(AuthError::Cancelled)
        );
    }
}
