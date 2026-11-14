//! Remembered episode numbering: the user's confirmed answer to "this
//! release numbers episodes across seasons". A sequence lists anime with
//! the release numbers each one covers, so "Show - 30" with 1-12, 13-24 and
//! 25-36 saved is the third anime's episode 6, whichever of them the title
//! matched.

use std::collections::HashMap;
use std::fmt;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::anime::AnimeId;

/// One anime in a sequence and the release numbers that are its episodes.
/// `first` is its episode 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceEntry {
    pub anime: AnimeId,
    pub first: u32,
    pub last: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberingSequence {
    /// The database id; 0 before it is first saved.
    pub id: i64,
    /// The anime the sequence was made from, usually the first season.
    pub root: AnimeId,
    pub entries: Vec<SequenceEntry>,
    pub saved: Timestamp,
}

/// Why a sequence cannot be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumberingError {
    NoEntries,
    RootMissing,
    Duplicate(AnimeId),
    /// The first number is above the last.
    Backwards(AnimeId),
    /// More numbers than the anime has episodes.
    TooLong {
        anime: AnimeId,
        numbers: u32,
        count: u32,
    },
    Overlap(AnimeId, AnimeId),
    /// The anime is already in another saved sequence.
    InOtherSequence(AnimeId),
}

impl NumberingError {
    /// The message for the user, with anime named by `title`.
    pub fn message(&self, title: impl Fn(AnimeId) -> String) -> String {
        match self {
            Self::NoEntries => "Include at least one anime.".into(),
            Self::RootMissing => {
                "The anime this numbering was made from must stay included.".into()
            }
            Self::Duplicate(id) => format!("{} is included twice.", title(*id)),
            Self::Backwards(id) => format!("{}: the first number is above the last.", title(*id)),
            Self::TooLong {
                anime,
                numbers,
                count,
            } => format!(
                "{}: {numbers} numbers, but it has only {count} episodes.",
                title(*anime)
            ),
            Self::Overlap(a, b) => format!("{} and {} overlap.", title(*a), title(*b)),
            Self::InOtherSequence(id) => format!(
                "{} already belongs to another remembered numbering. Forget that one first.",
                title(*id)
            ),
        }
    }
}

impl fmt::Display for NumberingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message(|id| id.to_string()))
    }
}

impl std::error::Error for NumberingError {}

/// Where a release number lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub anime: AnimeId,
    pub range: (u32, u32),
    pub entry: SequenceEntry,
}

impl NumberingSequence {
    /// The anime and episode numbers for release numbers `first`-`last`,
    /// when one entry holds all of them. A batch across two entries, or a
    /// number in a gap, is no match.
    pub fn resolve(&self, (first, last): (u32, u32)) -> Option<Resolved> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.first <= first && last <= e.last)?;
        Some(Resolved {
            anime: entry.anime,
            range: (first - entry.first + 1, last - entry.first + 1),
            entry: *entry,
        })
    }

    /// Checks the rules a sequence must follow before it is saved.
    /// `count` gives an anime's episode count when known; `others` are the
    /// other saved sequences.
    pub fn validate(
        &self,
        count: impl Fn(AnimeId) -> Option<u32>,
        others: &Numbering,
    ) -> Result<(), NumberingError> {
        if self.entries.is_empty() {
            return Err(NumberingError::NoEntries);
        }
        if !self.entries.iter().any(|e| e.anime == self.root) {
            return Err(NumberingError::RootMissing);
        }
        for (i, e) in self.entries.iter().enumerate() {
            if self.entries[..i].iter().any(|o| o.anime == e.anime) {
                return Err(NumberingError::Duplicate(e.anime));
            }
            if e.first > e.last {
                return Err(NumberingError::Backwards(e.anime));
            }
            let numbers = e.last - e.first + 1;
            if let Some(count) = count(e.anime).filter(|&c| c > 0)
                && numbers > count
            {
                return Err(NumberingError::TooLong {
                    anime: e.anime,
                    numbers,
                    count,
                });
            }
            if others.of(e.anime).is_some_and(|s| s.id != self.id) {
                return Err(NumberingError::InOtherSequence(e.anime));
            }
        }
        let mut sorted = self.entries.clone();
        sorted.sort_by_key(|e| e.first);
        for pair in sorted.windows(2) {
            if pair[1].first <= pair[0].last {
                return Err(NumberingError::Overlap(pair[0].anime, pair[1].anime));
            }
        }
        Ok(())
    }
}

/// Continuous numbering for anime in order: 1-12, 13-24, 25-36. Stops at the
/// first anime whose count is unknown, returning those before it and that
/// anime, since its last number would be a guess.
pub fn continuous(
    order: &[(AnimeId, Option<u32>)],
) -> Result<Vec<SequenceEntry>, (Vec<SequenceEntry>, AnimeId)> {
    let mut next = 1;
    let mut entries = Vec::new();
    for &(anime, count) in order {
        let Some(count) = count.filter(|&c| c > 0) else {
            return Err((entries, anime));
        };
        entries.push(SequenceEntry {
            anime,
            first: next,
            last: next + count - 1,
        });
        next += count;
    }
    Ok(entries)
}

/// Every saved sequence, looked up by any anime in it.
#[derive(Debug, Clone, Default)]
pub struct Numbering {
    sequences: Vec<NumberingSequence>,
    by_anime: HashMap<AnimeId, usize>,
}

impl Numbering {
    pub fn new(sequences: Vec<NumberingSequence>) -> Self {
        let mut numbering = Self::default();
        for sequence in sequences {
            numbering.insert(sequence);
        }
        numbering
    }

    pub fn sequences(&self) -> &[NumberingSequence] {
        &self.sequences
    }

    /// The sequence an anime belongs to.
    pub fn of(&self, id: AnimeId) -> Option<&NumberingSequence> {
        self.by_anime.get(&id).map(|&i| &self.sequences[i])
    }

    /// Adds or replaces (by id) a sequence.
    pub fn insert(&mut self, sequence: NumberingSequence) {
        self.sequences.retain(|s| s.id != sequence.id);
        self.sequences.push(sequence);
        self.reindex();
    }

    pub fn remove(&mut self, id: i64) {
        self.sequences.retain(|s| s.id != id);
        self.reindex();
    }

    fn reindex(&mut self) {
        self.by_anime.clear();
        for (i, s) in self.sequences.iter().enumerate() {
            for e in &s.entries {
                self.by_anime.insert(e.anime, i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_seasons() -> NumberingSequence {
        NumberingSequence {
            id: 1,
            root: AnimeId(1001),
            entries: continuous(&[
                (AnimeId(1001), Some(12)),
                (AnimeId(1002), Some(12)),
                (AnimeId(1003), Some(12)),
            ])
            .unwrap(),
            saved: Timestamp::UNIX_EPOCH,
        }
    }

    #[test]
    fn continuous_numbering_and_lookup() {
        let s = three_seasons();
        assert_eq!(s.entries[2].first, 25);
        assert_eq!(s.entries[2].last, 36);
        let r = s.resolve((30, 30)).unwrap();
        assert_eq!((r.anime, r.range), (AnimeId(1003), (6, 6)));
        assert_eq!(s.resolve((13, 14)).unwrap().range, (1, 2));
        // A batch across two seasons, and a number past the end.
        assert!(s.resolve((24, 25)).is_none());
        assert!(s.resolve((37, 37)).is_none());
    }

    #[test]
    fn continuous_stops_at_an_unknown_count() {
        let (done, at) = continuous(&[
            (AnimeId(1), Some(12)),
            (AnimeId(2), None),
            (AnimeId(3), Some(12)),
        ])
        .unwrap_err();
        assert_eq!(done.len(), 1);
        assert_eq!(at, AnimeId(2));
    }

    #[test]
    fn gaps_are_allowed_and_mean_no_match() {
        let mut s = three_seasons();
        s.entries[1] = SequenceEntry {
            anime: AnimeId(1002),
            first: 14,
            last: 24,
        };
        assert!(s.validate(|_| Some(12), &Numbering::default()).is_ok());
        assert!(s.resolve((13, 13)).is_none());
    }

    #[test]
    fn validation() {
        let none = Numbering::default();
        let count = |_| Some(12);
        let mut s = three_seasons();
        assert!(s.validate(count, &none).is_ok());

        s.entries[1].last = 25;
        // With the count known, the range is too long before it overlaps.
        assert_eq!(
            s.validate(count, &none),
            Err(NumberingError::TooLong {
                anime: AnimeId(1002),
                numbers: 13,
                count: 12
            })
        );
        assert_eq!(
            s.validate(|_| None, &none),
            Err(NumberingError::Overlap(AnimeId(1002), AnimeId(1003)))
        );

        let mut s = three_seasons();
        s.entries[0].first = 20;
        assert_eq!(
            s.validate(count, &none),
            Err(NumberingError::Backwards(AnimeId(1001)))
        );

        let mut s = three_seasons();
        s.entries.remove(0);
        assert_eq!(s.validate(count, &none), Err(NumberingError::RootMissing));

        // Unknown counts do not limit a range.
        let mut s = three_seasons();
        s.entries[2].last = 99;
        assert!(s.validate(|_| None, &none).is_ok());
    }

    #[test]
    fn an_anime_belongs_to_one_sequence() {
        let saved = Numbering::new(vec![three_seasons()]);
        let mut other = three_seasons();
        other.id = 2;
        assert_eq!(
            other.validate(|_| Some(12), &saved),
            Err(NumberingError::InOtherSequence(AnimeId(1001)))
        );
        // Editing the saved one itself is fine.
        assert!(three_seasons().validate(|_| Some(12), &saved).is_ok());
        assert_eq!(saved.of(AnimeId(1002)).map(|s| s.id), Some(1));
        let mut saved = saved;
        saved.remove(1);
        assert!(saved.of(AnimeId(1002)).is_none());
    }
}
