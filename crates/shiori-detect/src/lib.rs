//! Finds running media players and what they have open.
//!
//! This crate only reports what it sees. Picking the video among the files
//! and matching it to an anime is the app's job. See docs/detection.md.

mod players;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod win;

use std::marker::PhantomData;

pub use players::{
    ParseError, ParseErrorKind, Pattern, Player, PlayerType, Strategy, TitleFormat,
    builtin_players, parse_players,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaInfoKind {
    /// A window title that does not look like a path.
    Unknown,
    File,
    /// A browser tab name.
    Tab,
    /// A browser page title, or a title the player reports.
    Title,
    Url,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfo {
    pub kind: MediaInfoKind,
    pub value: String,
}

/// A window, as the system names it: an `HWND` on Windows, an X11 window
/// id on Linux. Only meaningful to the `Detector` that returned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

/// A player that has something open.
#[derive(Debug, Clone)]
pub struct Detected {
    pub player: Player,
    pub process_id: u32,
    /// `None` when the player was found without a window, as under Wayland.
    pub window: Option<WindowId>,
    /// Empty when there is no window.
    pub window_title: String,
    /// In the order found; never empty.
    pub media: Vec<MediaInfo>,
    /// `None` when the player does not say.
    pub playing: Option<bool>,
}

/// Finds players. Keeps connections between polls (the X11 display and the
/// session bus on Linux, UI Automation on Windows), so make one and reuse
/// it.
///
/// Create and use it on one thread: UI Automation is a COM object tied to
/// the thread that made it. It is not `Send` on any system, so that code
/// written on Linux does not break on Windows.
#[derive(Debug)]
pub struct Detector {
    backend: Backend,
    _one_thread: PhantomData<*const ()>,
}

#[cfg(target_os = "linux")]
type Backend = linux::Backend;
#[cfg(windows)]
type Backend = win::Backend;

/// Systems without a backend yet (macOS, decision D2) find nothing.
#[cfg(not(any(target_os = "linux", windows)))]
#[derive(Debug, Default)]
struct Backend;

#[cfg(not(any(target_os = "linux", windows)))]
impl Backend {
    fn detect(&mut self, _: &[Player]) -> Vec<Detected> {
        Vec::new()
    }

    fn is_focused(&self, _: &Detected) -> Option<bool> {
        None
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}

impl Detector {
    pub fn new() -> Self {
        Self {
            backend: Backend::default(),
            _one_thread: PhantomData,
        }
    }

    /// Every running player in `players` that has something open.
    pub fn detect(&mut self, players: &[Player]) -> Vec<Detected> {
        self.backend.detect(players)
    }

    /// Whether the player's window has the keyboard focus. `None` when the
    /// system cannot tell, as under Wayland, or the player has no window.
    pub fn is_focused(&self, detected: &Detected) -> Option<bool> {
        self.backend.is_focused(detected)
    }
}

/// Adds a piece of media information, dropping empty values as Anisthesia
/// does: an empty title means the player is open with nothing playing.
fn push_media(media: &mut Vec<MediaInfo>, kind: MediaInfoKind, value: String) {
    if !value.is_empty() {
        media.push(MediaInfo { kind, value });
    }
}

/// The kind of a window title after the title format: `File` when it is a
/// full path, which some players show instead of the file name.
fn title_kind(title: &str) -> MediaInfoKind {
    if looks_like_windows_path(title) || looks_like_unix_path(title) {
        MediaInfoKind::File
    } else {
        MediaInfoKind::Unknown
    }
}

/// `C:\...`, `C:/...` or `\\server\...`, followed by a valid name
/// character. Anisthesia's test, kept as it is on every system.
fn looks_like_windows_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    let rest = match bytes {
        [drive, b':', b'/' | b'\\', rest @ ..] if drive.is_ascii_alphabetic() => rest,
        [b'\\', b'\\', rest @ ..] => rest,
        _ => return false,
    };
    rest.first()
        .is_some_and(|c| !br#"<>:"/\|?*"#.contains(c) && !c.is_ascii_control())
}

/// An absolute path to a file that exists. A bare leading `/` is not
/// enough: page titles like "/r/anime" start with one too.
fn looks_like_unix_path(text: &str) -> bool {
    cfg!(unix) && text.starts_with('/') && std::path::Path::new(text).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_in_window_titles() {
        assert_eq!(title_kind(r"C:\Anime\Show 01.mkv"), MediaInfoKind::File);
        assert_eq!(title_kind("D:/Anime/Show 01.mkv"), MediaInfoKind::File);
        assert_eq!(title_kind(r"\\nas\anime\Show.mkv"), MediaInfoKind::File);
        assert_eq!(title_kind("Show 01.mkv"), MediaInfoKind::Unknown);
        assert_eq!(title_kind(r"C:\"), MediaInfoKind::Unknown);
        assert_eq!(title_kind(r"C:\|"), MediaInfoKind::Unknown);
        assert_eq!(title_kind("/r/anime"), MediaInfoKind::Unknown);
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_unix_path_is_a_file() {
        let file = std::env::current_exe().unwrap();
        assert_eq!(title_kind(file.to_str().unwrap()), MediaInfoKind::File);
    }

    #[test]
    fn empty_values_are_dropped() {
        let mut media = Vec::new();
        push_media(&mut media, MediaInfoKind::Title, String::new());
        push_media(&mut media, MediaInfoKind::Title, "Show".into());
        assert_eq!(media.len(), 1);
    }
}
