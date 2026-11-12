//! Which anime the list and search pages show, and in what order. Kept
//! apart from the views so the rules can be tested without a window.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use shiori_core::anime::airing_status;
use shiori_core::date::Season;
use shiori_core::entry::is_in_list;
use shiori_core::{AiringStatus, Anime, AnimeId, ListEntry, ListStatus};

use crate::state::AppState;

pub fn status_label(status: ListStatus) -> &'static str {
    match status {
        ListStatus::Watching => "Watching",
        ListStatus::Completed => "Completed",
        ListStatus::OnHold => "On hold",
        ListStatus::Dropped => "Dropped",
        ListStatus::PlanToWatch => "Plan to watch",
        ListStatus::NotInList => "Not in list",
    }
}

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
    /// Not a table column; a sort for the rows and cards views.
    NewEpisodes,
    /// Not a table column; soonest next episode first.
    NextAiring,
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
            ListColumn::NewEpisodes => "New episodes",
            ListColumn::NextAiring => "Next airing",
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
            ListColumn::NewEpisodes => "new_episodes",
            ListColumn::NextAiring => "next_airing",
        }
    }

    /// What the Sort menu offers, most useful first.
    pub const SORTS: [ListColumn; 8] = [
        ListColumn::NewEpisodes,
        ListColumn::NextAiring,
        ListColumn::Title,
        ListColumn::Progress,
        ListColumn::Score,
        ListColumn::Average,
        ListColumn::Season,
        ListColumn::LastUpdated,
    ];

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .chain([ListColumn::NewEpisodes, ListColumn::NextAiring])
            .find(|c| c.key() == key)
    }

    /// Whether larger values come first when this sort is picked, so that
    /// "New episodes" shows the most behind first and "Score" the best.
    pub fn descending_by_default(self) -> bool {
        matches!(
            self,
            ListColumn::NewEpisodes
                | ListColumn::Score
                | ListColumn::Average
                | ListColumn::LastUpdated
                | ListColumn::Season
        )
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

impl Sort {
    /// The sort remembered in `session.toml`. Unknown column names give no
    /// sort, so an old or edited file cannot break the list.
    pub fn from_session(view: &shiori_store::ListView) -> Self {
        let key = |column: &str, descending| {
            ListColumn::from_key(column).map(|column| SortKey { column, descending })
        };
        Self {
            primary: key(&view.sort_column, view.sort_descending),
            secondary: key(&view.second_sort_column, view.second_sort_descending),
        }
    }

    pub fn to_session(self, view: &mut shiori_store::ListView) {
        let parts = |key: Option<SortKey>| {
            key.map_or((String::new(), false), |k| {
                (k.column.key().to_owned(), k.descending)
            })
        };
        (view.sort_column, view.sort_descending) = parts(self.primary);
        (view.second_sort_column, view.second_sort_descending) = parts(self.secondary);
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
                // An anime with no next episode stays last either way.
                let unknown_time = |id: &AnimeId| {
                    state
                        .anime
                        .get(id)
                        .is_none_or(|x| x.next_episode_time.is_none())
                };
                let keep =
                    key.column == ListColumn::NextAiring && unknown_time(a) != unknown_time(b);
                if key.descending && !keep {
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
        ListColumn::NewEpisodes => {
            let new = |id| new_episodes(state.anime.get(id), state.entries.get(id));
            new(&a).cmp(&new(&b))
        }
        // Unknown times go last whichever way the sort runs, since "no next
        // episode" is not "soonest" or "latest".
        ListColumn::NextAiring => {
            let time = |anime: Option<&Anime>| anime.and_then(|x| x.next_episode_time);
            match (time(anime_a), time(anime_b)) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        }
        ListColumn::Season => anime_a
            .map(|x| Season::from_date(x.date_started))
            .cmp(&anime_b.map(|x| Season::from_date(x.date_started))),
    }
}

/// Aired episodes the user has not watched yet. Only Watching entries
/// count, and only when the service says how many have aired, so a show the
/// user dropped or has not started never nags.
pub fn new_episodes(anime: Option<&Anime>, entry: Option<&ListEntry>) -> u32 {
    match (anime, entry) {
        (Some(anime), Some(entry))
            if entry.status == ListStatus::Watching && is_in_list(Some(entry)) =>
        {
            let aired = match anime.episode_count {
                // A finished count caps a stale aired number.
                Some(count) if anime.last_aired_episode > count => count,
                _ => anime.last_aired_episode,
            };
            aired.saturating_sub(entry.watched_episodes)
        }
        _ => 0,
    }
}

/// The quick filters under the list heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chip {
    #[default]
    All,
    NewEpisodes,
    Airing,
    FinishedAiring,
}

impl Chip {
    pub const ALL: [Chip; 4] = [
        Chip::All,
        Chip::NewEpisodes,
        Chip::Airing,
        Chip::FinishedAiring,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Chip::All => "All",
            Chip::NewEpisodes => "New episodes",
            Chip::Airing => "Airing",
            Chip::FinishedAiring => "Finished airing",
        }
    }

    pub fn matches(self, state: &AppState, id: AnimeId, today_in_japan: Date) -> bool {
        let anime = state.anime.get(&id);
        match self {
            Chip::All => true,
            Chip::NewEpisodes => new_episodes(anime, state.entries.get(&id)) > 0,
            Chip::Airing => {
                anime.is_some_and(|a| airing_status(a, today_in_japan) == AiringStatus::Airing)
            }
            Chip::FinishedAiring => anime
                .is_some_and(|a| airing_status(a, today_in_japan) == AiringStatus::FinishedAiring),
        }
    }
}

/// When the next episode airs, in words: `today 18:30`, `tomorrow 01:00`,
/// `in 3 days`. Days are counted by local calendar date, not 24 hour steps,
/// so 23:00 tonight is "today" and 01:00 is "tomorrow".
pub fn airing_label(time: Timestamp, now: Timestamp, tz: &TimeZone) -> String {
    let when = time.to_zoned(tz.clone());
    let days = (when.date() - now.to_zoned(tz.clone()).date()).get_days();
    match days {
        ..0 => "aired".to_owned(),
        0 => format!("today {}", when.strftime("%H:%M")),
        1 => format!("tomorrow {}", when.strftime("%H:%M")),
        n => format!("in {n} days"),
    }
}

/// The selected rows of a list or card view, kept by anime id so that a
/// re-sort or a sync does not move the selection to other anime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub ids: BTreeSet<AnimeId>,
    /// Where a Shift click range starts.
    anchor: Option<AnimeId>,
}

impl Selection {
    pub fn contains(&self, id: AnimeId) -> bool {
        self.ids.contains(&id)
    }

    /// A click on `rows[ix]`: alone it selects only that row, with Ctrl it
    /// toggles the row, with Shift it selects from the last clicked row.
    pub fn click(&mut self, rows: &[AnimeId], ix: usize, toggle: bool, range: bool) {
        let Some(&id) = rows.get(ix) else {
            return;
        };
        let anchor = self.anchor.and_then(|a| rows.iter().position(|r| *r == a));
        match (range, anchor) {
            (true, Some(start)) => {
                let (from, to) = (start.min(ix), start.max(ix));
                if !toggle {
                    self.ids.clear();
                }
                self.ids.extend(&rows[from..=to]);
                // The anchor stays, so the next Shift click grows or shrinks
                // the same range.
                return;
            }
            _ if toggle => {
                if !self.ids.remove(&id) {
                    self.ids.insert(id);
                }
            }
            _ => {
                self.ids.clear();
                self.ids.insert(id);
            }
        }
        self.anchor = Some(id);
    }

    /// Selects `id` alone unless it is already part of the selection, as a
    /// right click does before opening the menu.
    pub fn select_for_menu(&mut self, id: AnimeId) {
        if !self.ids.contains(&id) {
            self.ids.clear();
            self.ids.insert(id);
            self.anchor = Some(id);
        }
    }

    pub fn invert(&mut self, rows: &[AnimeId]) {
        self.ids = rows
            .iter()
            .copied()
            .filter(|id| !self.ids.contains(id))
            .collect();
    }

    /// Drops anime that are no longer shown.
    pub fn retain(&mut self, rows: &[AnimeId]) {
        let shown: BTreeSet<AnimeId> = rows.iter().copied().collect();
        self.ids.retain(|id| shown.contains(id));
        if self.anchor.is_some_and(|a| !shown.contains(&a)) {
            self.anchor = None;
        }
    }

    /// The selected anime in the order they are shown.
    pub fn in_order(&self, rows: &[AnimeId]) -> Vec<AnimeId> {
        rows.iter()
            .copied()
            .filter(|id| self.ids.contains(id))
            .collect()
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

    #[test]
    fn new_episodes_count_only_watching() {
        let mut anime = Anime::new(AnimeId(1));
        anime.last_aired_episode = 6;
        let mut entry = ListEntry::new(AnimeId(1));
        entry.status = ListStatus::Watching;
        entry.watched_episodes = 3;
        assert_eq!(new_episodes(Some(&anime), Some(&entry)), 3);
        // A stale aired count never goes past the episode count.
        anime.episode_count = Some(4);
        assert_eq!(new_episodes(Some(&anime), Some(&entry)), 1);
        entry.status = ListStatus::OnHold;
        assert_eq!(new_episodes(Some(&anime), Some(&entry)), 0);
        entry.status = ListStatus::Watching;
        anime.last_aired_episode = 0;
        assert_eq!(new_episodes(Some(&anime), Some(&entry)), 0);
    }

    #[test]
    fn sorts_by_new_episodes_and_next_airing() {
        let mut state = state();
        let time = |s: &str| Some(s.parse::<Timestamp>().unwrap());
        let mut set = |id: u32, aired: u32, next: Option<Timestamp>| {
            let mut anime = state.anime[&AnimeId(id)].clone();
            anime.last_aired_episode = aired;
            anime.next_episode_time = next;
            state.store_anime(vec![anime]).unwrap();
        };
        // Entries start at 0 watched; 3 is Completed and never counts.
        set(1, 2, time("2026-11-14T10:00:00Z"));
        set(2, 0, None);
        set(4, 5, time("2026-11-13T10:00:00Z"));
        let by = |column, descending| Sort {
            primary: Some(SortKey { column, descending }),
            secondary: None,
        };
        let rows = list_rows(
            &state,
            Some(ListStatus::Watching),
            "",
            by(ListColumn::NewEpisodes, true),
        );
        assert_eq!(rows, [AnimeId(4), AnimeId(1), AnimeId(2)]);
        // Unknown times last in both directions.
        let rows = list_rows(
            &state,
            Some(ListStatus::Watching),
            "",
            by(ListColumn::NextAiring, false),
        );
        assert_eq!(rows, [AnimeId(4), AnimeId(1), AnimeId(2)]);
        let rows = list_rows(
            &state,
            Some(ListStatus::Watching),
            "",
            by(ListColumn::NextAiring, true),
        );
        assert_eq!(rows, [AnimeId(1), AnimeId(4), AnimeId(2)]);
        let today = jiff::civil::date(2026, 11, 12);
        assert!(Chip::NewEpisodes.matches(&state, AnimeId(4), today));
        assert!(!Chip::NewEpisodes.matches(&state, AnimeId(2), today));
    }

    #[test]
    fn airing_labels_count_calendar_days() {
        let tz = TimeZone::fixed(jiff::tz::offset(2));
        let now: Timestamp = "2026-11-12T20:00:00Z".parse().unwrap(); // 22:00 local
        let at = |s: &str| airing_label(s.parse().unwrap(), now, &tz);
        assert_eq!(at("2026-11-12T21:30:00Z"), "today 23:30");
        assert_eq!(at("2026-11-12T23:00:00Z"), "tomorrow 01:00");
        assert_eq!(at("2026-11-15T12:00:00Z"), "in 3 days");
    }

    #[test]
    fn sort_round_trips_through_the_session() {
        let mut sort = Sort::default();
        sort.by(SortKey {
            column: ListColumn::Score,
            descending: true,
        });
        sort.by(SortKey {
            column: ListColumn::LastUpdated,
            descending: false,
        });
        let mut view = shiori_store::ListView::default();
        sort.to_session(&mut view);
        assert_eq!(view.sort_column, "last_updated");
        assert_eq!(Sort::from_session(&view), sort);
        view.sort_column = "nonsense".into();
        assert_eq!(Sort::from_session(&view).primary, None);
    }

    #[test]
    fn selection_follows_clicks() {
        let rows: Vec<AnimeId> = (1..=6).map(AnimeId).collect();
        let mut selection = Selection::default();
        selection.click(&rows, 1, false, false);
        assert_eq!(selection.in_order(&rows), [AnimeId(2)]);
        // Shift selects from the last clicked row, either way.
        selection.click(&rows, 4, false, true);
        assert_eq!(selection.in_order(&rows), rows[1..=4]);
        selection.click(&rows, 0, false, true);
        assert_eq!(selection.in_order(&rows), rows[0..=1]);
        // Ctrl toggles one row and keeps the rest.
        selection.click(&rows, 5, true, false);
        selection.click(&rows, 0, true, false);
        assert_eq!(selection.in_order(&rows), [AnimeId(2), AnimeId(6)]);
        selection.invert(&rows);
        assert_eq!(
            selection.in_order(&rows),
            [AnimeId(1), AnimeId(3), AnimeId(4), AnimeId(5)]
        );
        // A right click inside the selection keeps it; outside replaces it.
        selection.select_for_menu(AnimeId(3));
        assert_eq!(selection.ids.len(), 4);
        selection.select_for_menu(AnimeId(6));
        assert_eq!(selection.in_order(&rows), [AnimeId(6)]);
        selection.retain(&rows[..3]);
        assert!(selection.ids.is_empty());
    }
}
