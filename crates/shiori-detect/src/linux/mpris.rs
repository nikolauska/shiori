//! What players report about themselves over MPRIS on the session bus.

use std::collections::HashMap;
use std::time::Duration;

use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::{Connection, OwnedVariant, Proxy, block_on, connection};

const PREFIX: &str = "org.mpris.MediaPlayer2.";

/// A hung player must not stall detection for zbus's default 25 seconds.
const TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub pid: u32,
    pub url: Option<String>,
    pub title: Option<String>,
    pub playing: Option<bool>,
}

// zbus 6 has no blocking API any more. Detection runs on a plain thread,
// so each call drives zbus's own per-thread runtime with `block_on`
// instead of bringing in an async runtime just for MPRIS.
pub fn connect() -> Option<Connection> {
    match block_on(
        connection::Builder::session()
            .method_timeout(TIMEOUT)
            .build(),
    ) {
        Ok(connection) => Some(connection),
        Err(error) => {
            tracing::debug!("No session bus, so no MPRIS: {error}");
            None
        }
    }
}

/// One report per player on the bus. `Err` when the bus itself failed, so
/// the caller can reconnect; a single player that does not answer is
/// skipped.
pub fn reports(connection: &Connection) -> zbus::Result<Vec<Report>> {
    block_on(async {
        let bus = DBusProxy::new(connection).await?;
        let names = bus.list_names().await?;
        let mut reports = Vec::new();
        for name in names
            .iter()
            .filter(|name| name.as_str().starts_with(PREFIX))
        {
            let name: BusName<'_> = name.inner().clone();
            let Ok(pid) = bus.get_connection_unix_process_id(name.clone()).await else {
                continue;
            };
            match report(connection, name.clone(), pid).await {
                Ok(report) => reports.push(report),
                Err(error) => tracing::debug!("MPRIS player {name} did not answer: {error}"),
            }
        }
        Ok(reports)
    })
}

async fn report(connection: &Connection, name: BusName<'_>, pid: u32) -> zbus::Result<Report> {
    let player = Proxy::new(
        connection,
        name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
    .await?;
    let metadata: HashMap<String, OwnedVariant> = player.get_property("Metadata").await?;
    let text = |key: &str| {
        metadata
            .get(key)
            .and_then(|value| String::try_from(value.try_clone().ok()?).ok())
            .filter(|text| !text.is_empty())
    };
    // Optional in the specification; some players leave it out.
    let playing = player
        .get_property::<String>("PlaybackStatus")
        .await
        .ok()
        .map(|status| status == "Playing");
    Ok(Report {
        pid,
        url: text("xesam:url"),
        title: text("xesam:title"),
        playing,
    })
}

/// The path in a `file://` URL. `None` for other URLs and for files on
/// another host.
pub fn file_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    Some(percent_decode(path))
}

/// File URLs escape every byte that is not plain ASCII, and the bytes are
/// UTF-8 in practice. Anything else is kept as it is.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = bytes.get(i + 1..i + 3)
            && let Ok(hex) = std::str::from_utf8(hex)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(
            file_path("file:///home/a/%5BGroup%5D%20Show%20-%2001.mkv").as_deref(),
            Some("/home/a/[Group] Show - 01.mkv")
        );
        assert_eq!(
            file_path("file://localhost/a/%E3%81%82.mkv").as_deref(),
            Some("/a/あ.mkv")
        );
        assert_eq!(
            file_path("file:///a/100%.mkv").as_deref(),
            Some("/a/100%.mkv")
        );
        assert_eq!(file_path("file://nas/a.mkv"), None);
        assert_eq!(file_path("https://example.com/a"), None);
    }
}
