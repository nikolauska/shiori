//! Exporting the list as a file.

use shiori_core::entry::is_in_list;
use shiori_core::{ListEntry, ListStatus};

use crate::list_view::status_label;
use crate::state::AppState;

/// The list as Markdown: one section per status, each line
/// `<romaji title> (<watched>/<episode count>)`, sorted by title ignoring
/// case. Statuses with nothing in them are left out.
pub fn markdown(state: &AppState) -> String {
    let mut out = String::new();
    for status in ListStatus::SHOWN {
        let mut lines: Vec<(String, String)> = state
            .entries
            .values()
            .filter(|e| e.status == status && is_in_list(Some(e)))
            .map(|entry| {
                let line = line(state, entry);
                (line.to_lowercase(), line)
            })
            .collect();
        if lines.is_empty() {
            continue;
        }
        lines.sort();
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("## {}\n\n", status_label(status)));
        for (_, line) in lines {
            out.push_str(&format!("- {line}\n"));
        }
    }
    out
}

fn line(state: &AppState, entry: &ListEntry) -> String {
    let anime = state.anime.get(&entry.anime_id);
    // The romaji title is the one every anime has; it falls back to the
    // shown title for an anime without one.
    let title = anime
        .map(|a| a.titles.romaji.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| state.title(entry.anime_id));
    let total = anime
        .and_then(|a| a.episode_count)
        .map_or("?".to_owned(), |n| n.to_string());
    format!("{title} ({}/{total})", entry.watched_episodes)
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use shiori_core::{Anime, AnimeId};
    use shiori_store::{Accounts, Database, Settings};

    use super::*;

    #[test]
    fn sections_sorted_by_title() {
        let mut state = AppState::new(
            Database::open_in_memory().unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap();
        let mut add = |id: u32, title: &str, count: Option<u32>, status, watched| {
            let mut anime = Anime::new(AnimeId(id));
            anime.titles.romaji = title.into();
            anime.episode_count = count;
            state.store_anime(vec![anime]).unwrap();
            let mut entry = ListEntry::new(AnimeId(id));
            entry.status = status;
            entry.watched_episodes = watched;
            state.save_entry(entry, Timestamp::UNIX_EPOCH).unwrap();
        };
        add(1, "bocchi the Rock!", Some(12), ListStatus::Completed, 12);
        add(2, "Frieren", Some(28), ListStatus::Watching, 5);
        add(3, "Akira", Some(1), ListStatus::Completed, 1);
        add(4, "One Piece", None, ListStatus::Watching, 1100);
        assert_eq!(
            markdown(&state),
            "## Watching\n\n- Frieren (5/28)\n- One Piece (1100/?)\n\n\
             ## Completed\n\n- Akira (1/1)\n- bocchi the Rock! (12/12)\n"
        );
    }
}
