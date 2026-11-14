//! Automatic backups of a list kept on this computer: one a week, named by
//! date, the last few kept. Without an account this is the only other copy
//! of the list.

use std::path::{Path, PathBuf};

use anyhow::Result;
use jiff::ToSpan as _;
use jiff::civil::Date;

use crate::state::AppState;

/// How many automatic backups are kept.
pub const KEEP: usize = 4;

const PREFIX: &str = "local-";
const EXTENSION: &str = "sqlite";

/// The automatic backups in `dir`, newest first. Other files are ignored,
/// so a backup the user saved there by hand is never pruned.
pub fn backups(dir: &Path) -> Vec<(Date, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(Date, PathBuf)> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != EXTENSION {
                return None;
            }
            let date = path
                .file_stem()?
                .to_str()?
                .strip_prefix(PREFIX)?
                .parse()
                .ok()?;
            Some((date, path))
        })
        .collect();
    found.sort_by_key(|(date, _)| std::cmp::Reverse(*date));
    found
}

/// The day of the newest automatic backup.
pub fn latest(dir: &Path) -> Option<Date> {
    backups(dir).first().map(|(date, _)| *date)
}

/// None yet, or the newest is a week old.
pub fn is_due(dir: &Path, today: Date) -> bool {
    latest(dir).is_none_or(|last| last.saturating_add(7.days()) <= today)
}

/// Backs up `state` when a backup is due and prunes the oldest ones.
/// Returns the new backup's path.
pub fn back_up_weekly(state: &AppState, dir: &Path, today: Date) -> Result<Option<PathBuf>> {
    if !is_due(dir, today) {
        return Ok(None);
    }
    let path = dir.join(format!("{PREFIX}{today}.{EXTENSION}"));
    state.back_up(&path)?;
    for (_, old) in backups(dir).into_iter().skip(KEEP) {
        if let Err(error) = std::fs::remove_file(&old) {
            tracing::warn!("Could not remove old backup {}: {error}", old.display());
        }
    }
    Ok(Some(path))
}

/// Where the list is saved just before a restore replaces it, so a restore
/// of the wrong file can be undone. Only the latest one is kept.
pub fn before_restore(dir: &Path) -> PathBuf {
    dir.join(format!("before-restore.{EXTENSION}"))
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use shiori_store::{Accounts, Database, Settings};

    use super::*;

    fn state() -> AppState {
        AppState::new(
            Database::open_in_memory().unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap()
    }

    #[test]
    fn one_backup_a_week_and_the_last_four_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let state = state();
        std::fs::write(dir.path().join("mine.sqlite"), "kept").unwrap();

        let first = date(2026, 10, 1);
        assert!(back_up_weekly(&state, dir.path(), first).unwrap().is_some());
        // Six days later nothing is due.
        let soon = first.saturating_add(6.days());
        assert!(back_up_weekly(&state, dir.path(), soon).unwrap().is_none());
        for week in 1..=5 {
            let day = first.saturating_add((7 * week).days());
            assert!(back_up_weekly(&state, dir.path(), day).unwrap().is_some());
        }
        let kept: Vec<Date> = backups(dir.path()).into_iter().map(|(d, _)| d).collect();
        assert_eq!(
            kept,
            [
                date(2026, 11, 5),
                date(2026, 10, 29),
                date(2026, 10, 22),
                date(2026, 10, 15)
            ]
        );
        assert!(dir.path().join("mine.sqlite").exists());
        assert_eq!(latest(dir.path()), Some(date(2026, 11, 5)));
    }

    #[test]
    fn a_missing_folder_has_no_backups() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("backups");
        assert!(backups(&missing).is_empty());
        assert!(is_due(&missing, date(2026, 11, 14)));
    }
}
