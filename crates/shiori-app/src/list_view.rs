//! Which anime the list and search pages show, and in what order. Kept
//! apart from the views so the rules can be tested without a window.

use std::cmp::Ordering;
use std::collections::HashMap;

use shiori_core::date::Season;
use shiori_core::entry::is_in_list;
use shiori_core::{Anime, AnimeId, ListEntry, ListStatus};

use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListColumn {
    Title,
    Progress,
    Duration,
    Rewatches,
    Score,
    Average,
    Type,
    Season,
    Started,
    Completed,
    LastUpdated,
    Notes,
}

impl ListColumn {
    pub const ALL: [ListColumn; 12] = [
        ListColumn::Title,
        ListColumn::Progress,
        ListColumn::Duration,
        ListColumn::Rewatches,
        ListColumn::Score,
        ListColumn::Average,
        ListColumn::Type,
        ListColumn::Season,
        ListColumn::Started,
        ListColumn::Completed,
        ListColumn::LastUpdated,
        ListColumn::Notes,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ListColumn::Title => "Title",
            ListColumn::Progress => "Progress",
            ListColumn::Duration => "Duration",
            ListColumn::Rewatches => "Rewatches",
            ListColumn::Score => "Score",
            ListColumn::Average => "Average",
            ListColumn::Type => "Type",
            ListColumn::Season => "Season",
            ListColumn::Started => "Started",
            ListColumn::Completed => "Completed",
            ListColumn::LastUpdated => "Last updated",
            ListColumn::Notes => "Notes",
        }
    }

    /// The key used in `session.toml`.
    pub fn key(self) -> &'static str {
        match self {
            ListColumn::Title => "title",
            ListColumn::Progress => "progress",
            ListColumn::Duration => "duration",
            ListColumn::Rewatches => "rewatches",
            ListColumn::Score => "score",
            ListColumn::Average => "average",
            ListColumn::Type => "type",
            ListColumn::Season => "season",
            ListColumn::Started => "started",
            ListColumn::Completed => "completed",
            ListColumn::LastUpdated => "last_updated",
            ListColumn::Notes => "notes",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    pub column: ListColumn,
    pub descending: bool,
}

/// The primary sort and the one before it, which breaks ties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sort {
    pub primary: Option<SortKey>,
    pub secondary: Option<SortKey>,
}

impl Sort {
    /// Sorting by a new column keeps the old one as the tie breaker, as
    /// clicking one header after another does in Taiga.
    pub fn by(&mut self, key: SortKey) {
        if self.primary.is_some_and(|p| p.column != key.column) {
            self.secondary = self.primary;
        }
        self.primary = Some(key);
    }
}

/// Whether the filter text appears in any of the anime's titles.
pub fn matches_filter(anime: &Anime, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    if filter.is_empty() {
        return true;
    }
    let titles = &anime.titles;
    [&titles.romaji, &titles.english, &titles.japanese]
        .into_iter()
        .chain(&titles.synonyms)
        .any(|t| t.to_lowercase().contains(&filter))
}

/// The rows of the anime list page.
pub fn list_rows(
    state: &AppState,
    status: Option<ListStatus>,
    filter: &str,
    sort: Sort,
) -> Vec<AnimeId> {
    let mut rows: Vec<AnimeId> = state
        .entries
        .values()
        .filter(|e| is_in_list(Some(e)))
        .filter(|e| status.is_none_or(|s| e.status == s))
        .filter(|e| {
            state
                .anime
                .get(&e.anime_id)
                .is_some_and(|a| matches_filter(a, filter))
        })
        .map(|e| e.anime_id)
        .collect();
    sort_rows(state, &mut rows, sort);
    rows
}

pub fn sort_rows(state: &AppState, rows: &mut [AnimeId], sort: Sort) {
    // Titles are compared lower-cased once, not on every comparison.
    let titles: HashMap<AnimeId, String> = rows
        .iter()
        .map(|&id| (id, state.title(id).to_lowercase()))
        .collect();
    rows.sort_by(|a, b| {
        let by_key = |key: Option<SortKey>| {
            key.map_or(Ordering::Equal, |key| {
                let ordering = compare(state, &titles, key.column, *a, *b);
                if key.descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            })
        };
        by_key(sort.primary)
            .then_with(|| by_key(sort.secondary))
            // The title, then the id, settle the rest, so the order never
            // depends on the order things were loaded in.
            .then_with(|| titles[a].cmp(&titles[b]))
            .then_with(|| a.cmp(b))
    });
}

fn compare(
    state: &AppState,
    titles: &HashMap<AnimeId, String>,
    column: ListColumn,
    a: AnimeId,
    b: AnimeId,
) -> Ordering {
    let (anime_a, anime_b) = (state.anime.get(&a), state.anime.get(&b));
    let (entry_a, entry_b) = (state.entries.get(&a), state.entries.get(&b));
    let by_entry = |key: fn(&ListEntry) -> i64| entry_a.map(key).cmp(&entry_b.map(key));
    match column {
        ListColumn::Title => titles[&a].cmp(&titles[&b]),
        ListColumn::Progress => by_entry(|e| e.watched_episodes.into()),
        ListColumn::Rewatches => by_entry(|e| e.rewatched_times.into()),
        ListColumn::Score => by_entry(|e| e.score.into()),
        ListColumn::LastUpdated => by_entry(|e| e.last_updated.as_second()),
        ListColumn::Started => entry_a
            .map(|e| e.date_started)
            .cmp(&entry_b.map(|e| e.date_started)),
        ListColumn::Completed => entry_a
            .map(|e| e.date_completed)
            .cmp(&entry_b.map(|e| e.date_completed)),
        ListColumn::Notes => entry_a
            .map(|e| e.notes.to_lowercase())
            .cmp(&entry_b.map(|e| e.notes.to_lowercase())),
        ListColumn::Duration => anime_a
            .and_then(|x| x.episode_length)
            .cmp(&anime_b.and_then(|x| x.episode_length)),
        ListColumn::Average => anime_a
            .map_or(0.0, |x| x.score)
            .total_cmp(&anime_b.map_or(0.0, |x| x.score)),
        ListColumn::Type => anime_a
            .map(|x| x.kind as u8)
            .cmp(&anime_b.map(|x| x.kind as u8)),
        ListColumn::Season => anime_a
            .map(|x| Season::from_date(x.date_started))
            .cmp(&anime_b.map(|x| Season::from_date(x.date_started))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;
    use shiori_core::FuzzyDate;
    use shiori_store::{Accounts, Database, Settings};

    fn state() -> AppState {
        let mut state = AppState::new(
            Database::open_in_memory().unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap();
        let mut add = |id: u32, title: &str, status, score, synonym: &str| {
            let mut anime = Anime::new(AnimeId(id));
            anime.titles.romaji = title.into();
            if !synonym.is_empty() {
                anime.titles.synonyms.push(synonym.into());
            }
            anime.date_started = FuzzyDate::new(2000 + id as u16, 1, 1);
            state.store_anime(vec![anime]).unwrap();
            let mut entry = ListEntry::new(AnimeId(id));
            entry.status = status;
            entry.score = score;
            state.save_entry(entry, Timestamp::UNIX_EPOCH).unwrap();
        };
        add(1, "bravo", ListStatus::Watching, 80, "");
        add(2, "Alpha", ListStatus::Watching, 80, "Tiger X Dragon");
        add(3, "charlie", ListStatus::Completed, 60, "");
        add(4, "delta", ListStatus::Watching, 90, "");
        state
    }

    #[test]
    fn filters_by_status_and_title() {
        let state = state();
        let all = list_rows(&state, None, "", Sort::default());
        assert_eq!(all, [AnimeId(2), AnimeId(1), AnimeId(3), AnimeId(4)]);
        let watching = list_rows(&state, Some(ListStatus::Watching), "", Sort::default());
        assert_eq!(watching, [AnimeId(2), AnimeId(1), AnimeId(4)]);
        // Matches synonyms too, ignoring case.
        let tiger = list_rows(&state, None, "tiger", Sort::default());
        assert_eq!(tiger, [AnimeId(2)]);
    }

    #[test]
    fn removed_entries_are_hidden() {
        let mut state = state();
        state
            .remove_entry(AnimeId(1), Timestamp::UNIX_EPOCH)
            .unwrap();
        let rows = list_rows(&state, None, "", Sort::default());
        assert!(!rows.contains(&AnimeId(1)));
    }

    #[test]
    fn second_sort_breaks_ties_then_title() {
        let state = state();
        let mut sort = Sort::default();
        sort.by(SortKey {
            column: ListColumn::Season,
            descending: true,
        });
        sort.by(SortKey {
            column: ListColumn::Score,
            descending: true,
        });
        assert_eq!(sort.secondary.map(|k| k.column), Some(ListColumn::Season));
        let rows = list_rows(&state, None, "", sort);
        // 90, then the two 80s by season (newest first), then 60.
        assert_eq!(rows, [AnimeId(4), AnimeId(2), AnimeId(1), AnimeId(3)]);

        let by_score = Sort {
            primary: Some(SortKey {
                column: ListColumn::Score,
                descending: false,
            }),
            secondary: None,
        };
        // Equal scores fall back to the title: Alpha before bravo.
        let rows = list_rows(&state, None, "", by_score);
        assert_eq!(rows, [AnimeId(3), AnimeId(2), AnimeId(1), AnimeId(4)]);
    }
}
