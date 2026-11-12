//! Rules of the poster cache: where a poster is stored, when it is
//! downloaded again, and what kind of image a download is.

use std::path::{Path, PathBuf};

use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use shiori_core::anime::airing_status;
use shiori_core::{AiringStatus, Anime, AnimeId};

/// The extensions a cached poster can have.
const EXTENSIONS: [&str; 4] = ["jpg", "png", "webp", "gif"];

/// How long a poster of an anime that is still airing is kept before it is
/// downloaded again. Posters of finished anime do not change.
const REFRESH_AFTER: SignedDuration = SignedDuration::from_hours(24 * 7);

/// The cached poster of an anime, whatever its extension.
pub fn cached_file(folder: &Path, id: AnimeId) -> Option<PathBuf> {
    EXTENSIONS
        .iter()
        .map(|ext| folder.join(format!("{id}.{ext}")))
        .find(|path| path.is_file())
}

/// Whether a cached poster written at `written` should be replaced.
pub fn is_outdated(
    anime: &Anime,
    written: Timestamp,
    now: Timestamp,
    today_in_japan: Date,
) -> bool {
    airing_status(anime, today_in_japan) != AiringStatus::FinishedAiring
        && now.duration_since(written) >= REFRESH_AFTER
}

/// The file extension for downloaded image bytes, from their first bytes;
/// the address does not always end in one.
pub fn extension(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some("webp"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        _ => None,
    }
}

/// Writes a downloaded poster, replacing one with another extension.
pub fn store(folder: &Path, id: AnimeId, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let ext = extension(bytes).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "not a known image format")
    })?;
    std::fs::create_dir_all(folder)?;
    if let Some(old) = cached_file(folder, id) {
        std::fs::remove_file(old)?;
    }
    let path = folder.join(format!("{id}.{ext}"));
    // Written beside the target first so that a crash never leaves half an
    // image that would be read as the cached poster.
    let partial = folder.join(format!("{id}.{ext}.part"));
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use shiori_core::FuzzyDate;

    use super::*;

    #[test]
    fn finished_anime_keep_their_poster() {
        let today = jiff::civil::date(2026, 11, 12);
        let now: Timestamp = "2026-11-12T00:00:00Z".parse().unwrap();
        let old = now - SignedDuration::from_hours(24 * 8);
        let recent = now - SignedDuration::from_hours(24);
        let mut anime = Anime::new(AnimeId(1));
        anime.date_started = FuzzyDate::new(2026, 10, 1);
        assert!(is_outdated(&anime, old, now, today));
        assert!(!is_outdated(&anime, recent, now, today));
        anime.status = AiringStatus::FinishedAiring;
        assert!(!is_outdated(&anime, old, now, today));
    }

    #[test]
    fn stored_poster_replaces_another_format() {
        let dir = tempfile::tempdir().unwrap();
        let png = [0x89, b'P', b'N', b'G', 0, 0];
        let jpg = [0xFF, 0xD8, 0xFF, 0xE0];
        let first = store(dir.path(), AnimeId(5), &png).unwrap();
        assert_eq!(first.extension().unwrap(), "png");
        let second = store(dir.path(), AnimeId(5), &jpg).unwrap();
        assert_eq!(cached_file(dir.path(), AnimeId(5)), Some(second));
        assert!(!first.exists());
        assert!(store(dir.path(), AnimeId(6), b"<html>").is_err());
        assert_eq!(cached_file(dir.path(), AnimeId(6)), None);
    }
}
