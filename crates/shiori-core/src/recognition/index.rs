//! The title index: normalized titles to the anime that have them.

use std::collections::HashMap;

use crate::anime::{Anime, AnimeId};
use crate::entry::AnimeSettings;
use crate::recognition::normalize;

/// Where a title in the index came from. Kept so that the UI can say why
/// a file matched an anime, not only which one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TitleKind {
    /// One of the user's alternative titles in the per-anime settings.
    UserSynonym,
    /// The romaji, English or Japanese title.
    MainTitle,
    /// The romaji title with the start year, "Title (2019)".
    TitleWithYear,
    /// A synonym from the service.
    ServiceSynonym,
}

impl TitleKind {
    /// Weights decide which anime wins when several share a title.
    pub fn weight(self) -> f32 {
        match self {
            Self::UserSynonym => 2.0,
            Self::MainTitle => 1.0,
            Self::TitleWithYear | Self::ServiceSynonym => 0.5,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TitleIndex {
    titles: HashMap<String, HashMap<AnimeId, TitleKind>>,
}

impl TitleIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the index from every stored anime and its per-anime settings.
    pub fn build<'a>(
        anime: impl IntoIterator<Item = &'a Anime>,
        settings: impl Fn(AnimeId) -> Option<&'a AnimeSettings>,
    ) -> Self {
        let mut index = Self::new();
        for item in anime {
            index.add(item, settings(item.id));
        }
        index
    }

    pub fn add(&mut self, anime: &Anime, settings: Option<&AnimeSettings>) {
        if let Some(settings) = settings {
            for synonym in &settings.synonyms {
                self.insert(synonym, anime.id, TitleKind::UserSynonym);
            }
        }
        let titles = &anime.titles;
        for title in [&titles.romaji, &titles.english, &titles.japanese] {
            self.insert(title, anime.id, TitleKind::MainTitle);
        }
        // Lets "Title (2019)" pick the right one of two anime with the same
        // title, as release names often add the year to tell them apart.
        let year = anime.date_started.year;
        if year != 0 {
            let year = year.to_string();
            if !titles.romaji.contains(&year) {
                let with_year = format!("{} ({year})", titles.romaji);
                self.insert(&with_year, anime.id, TitleKind::TitleWithYear);
            }
        }
        for synonym in &titles.synonyms {
            self.insert(synonym, anime.id, TitleKind::ServiceSynonym);
        }
    }

    pub fn remove(&mut self, id: AnimeId) {
        self.titles.retain(|_, matches| {
            matches.remove(&id);
            !matches.is_empty()
        });
    }

    /// Call when an anime or its per-anime settings changed.
    pub fn update(&mut self, anime: &Anime, settings: Option<&AnimeSettings>) {
        self.remove(anime.id);
        self.add(anime, settings);
    }

    /// Anime with this title, strongest first. Equal weights are ordered by
    /// id so the result does not depend on hash order.
    pub fn lookup(&self, title: &str) -> Vec<(AnimeId, f32)> {
        self.lookup_kinds(title)
            .into_iter()
            .map(|(id, kind)| (id, kind.weight()))
            .collect()
    }

    /// Like `lookup`, with the kind of title each anime matched by. An anime
    /// that has the title several ways gets the strongest one.
    pub fn lookup_kinds(&self, title: &str) -> Vec<(AnimeId, TitleKind)> {
        let Some(matches) = self.titles.get(&normalize(title)) else {
            return Vec::new();
        };
        let mut matches: Vec<_> = matches.iter().map(|(id, k)| (*id, *k)).collect();
        matches.sort_by(|a, b| b.1.weight().total_cmp(&a.1.weight()).then(a.0.cmp(&b.0)));
        matches
    }

    fn insert(&mut self, title: &str, id: AnimeId, kind: TitleKind) {
        let normalized = normalize(title);
        if normalized.is_empty() {
            return;
        }
        let entry = self
            .titles
            .entry(normalized)
            .or_default()
            .entry(id)
            .or_insert(kind);
        if entry.weight() < kind.weight() {
            *entry = kind;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::FuzzyDate;

    fn anime(id: u32, romaji: &str) -> Anime {
        let mut a = Anime::new(AnimeId(id));
        a.titles.romaji = romaji.into();
        a
    }

    #[test]
    fn lookup_normalizes_and_orders_by_weight() {
        let mut a = anime(1, "Toradora!");
        a.titles.synonyms = vec!["Tiger x Dragon".into()];
        let b = anime(2, "Tiger × Dragon");
        let mut index = TitleIndex::new();
        index.add(&a, None);
        index.add(&b, None);
        assert_eq!(index.lookup("toradora"), [(AnimeId(1), 1.0)]);
        assert_eq!(
            index.lookup("Tiger x Dragon"),
            [(AnimeId(2), 1.0), (AnimeId(1), 0.5)]
        );
        assert!(index.lookup("Unknown").is_empty());
    }

    #[test]
    fn user_synonyms_are_strongest() {
        let a = anime(1, "Shingeki no Kyojin");
        let settings = AnimeSettings {
            synonyms: vec!["SnK".into()],
            ..AnimeSettings::new(AnimeId(1))
        };
        let mut b = anime(2, "Other");
        b.titles.synonyms = vec!["SnK".into()];
        let index = TitleIndex::build([&a, &b], |id| (id == AnimeId(1)).then_some(&settings));
        assert_eq!(index.lookup("snk"), [(AnimeId(1), 2.0), (AnimeId(2), 0.5)]);
    }

    #[test]
    fn year_title_and_highest_weight_wins() {
        let mut a = anime(1, "Hunter x Hunter");
        a.date_started = FuzzyDate::new(2011, 10, 2);
        a.titles.english = "Hunter x Hunter".into();
        let mut index = TitleIndex::new();
        index.add(&a, None);
        assert_eq!(index.lookup("Hunter x Hunter (2011)"), [(AnimeId(1), 0.5)]);
        assert_eq!(index.lookup("Hunter x Hunter"), [(AnimeId(1), 1.0)]);

        let mut b = anime(2, "Fate/Zero 2011");
        b.date_started = FuzzyDate::new(2011, 10, 1);
        index.add(&b, None);
        assert!(index.lookup("Fate/Zero 2011 (2011)").is_empty());
    }

    #[test]
    fn update_and_remove() {
        let mut a = anime(1, "Old Title");
        let mut index = TitleIndex::new();
        index.add(&a, None);
        a.titles.romaji = "New Title".into();
        index.update(&a, None);
        assert!(index.lookup("Old Title").is_empty());
        assert_eq!(index.lookup("New Title").len(), 1);
        index.remove(AnimeId(1));
        assert!(index.lookup("New Title").is_empty());
    }

    #[test]
    fn empty_titles_are_skipped() {
        let a = anime(1, "");
        let mut index = TitleIndex::new();
        index.add(&a, None);
        assert!(index.titles.is_empty());
    }
}
