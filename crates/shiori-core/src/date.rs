//! Dates where parts may be unknown, and anime seasons.

use std::cmp::Ordering;
use std::fmt;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// A date where any part may be unknown, such as "2019" or "April 2019".
/// A part that is 0 is unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FuzzyDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl FuzzyDate {
    pub const fn new(year: u16, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    /// Reads `YYYY-MM-DD`, where unknown parts are zeros. Anything shorter,
    /// including the empty string, is an unknown date, as in Taiga.
    pub fn parse(text: &str) -> Self {
        let part = |range: std::ops::Range<usize>| -> u32 {
            text.get(range)
                .map(|s| {
                    // Leading digits only, like `std::from_chars`.
                    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
                    digits.parse().unwrap_or(0)
                })
                .unwrap_or(0)
        };
        if text.len() < 10 {
            return Self::default();
        }
        Self {
            year: u16::try_from(part(0..4)).unwrap_or(0),
            month: u8::try_from(part(5..7)).unwrap_or(0),
            day: u8::try_from(part(8..10)).unwrap_or(0),
        }
    }

    /// Text for storage: `YYYY-MM-DD`, or an empty string when nothing is known.
    pub fn to_storage(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            self.to_string()
        }
    }

    /// All three parts are unknown.
    pub fn is_empty(&self) -> bool {
        self.year == 0 && self.month == 0 && self.day == 0
    }

    /// At least the year is known.
    pub fn is_set(&self) -> bool {
        self.year != 0
    }

    /// Every part is known.
    pub fn is_complete(&self) -> bool {
        self.year != 0 && self.month != 0 && self.day != 0
    }

    /// The calendar date, when every part is known and forms a real date.
    pub fn to_date(&self) -> Option<Date> {
        if !self.is_complete() {
            return None;
        }
        Date::new(
            i16::try_from(self.year).ok()?,
            i8::try_from(self.month).ok()?,
            i8::try_from(self.day).ok()?,
        )
        .ok()
    }

    /// Days from `other` to `self`. Only defined when both dates are complete.
    pub fn days_since(&self, other: &FuzzyDate) -> Option<i32> {
        let a = self.to_date()?;
        let b = other.to_date()?;
        Some(b.until(a).ok()?.get_days())
    }
}

impl From<Date> for FuzzyDate {
    fn from(date: Date) -> Self {
        Self {
            year: u16::try_from(date.year()).unwrap_or(0),
            month: u8::try_from(date.month()).unwrap_or(0),
            day: u8::try_from(date.day()).unwrap_or(0),
        }
    }
}

impl fmt::Display for FuzzyDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Ord for FuzzyDate {
    fn cmp(&self, other: &Self) -> Ordering {
        // An unknown part sorts after any known one, so a date known only to
        // the year is not treated as earlier than every day in that year.
        fn part(a: u16, b: u16) -> Ordering {
            match (a, b) {
                _ if a == b => Ordering::Equal,
                (0, _) => Ordering::Greater,
                (_, 0) => Ordering::Less,
                _ => a.cmp(&b),
            }
        }
        part(self.year, other.year)
            .then_with(|| part(self.month.into(), other.month.into()))
            .then_with(|| part(self.day.into(), other.day.into()))
    }
}

impl PartialOrd for FuzzyDate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SeasonName {
    #[default]
    Unknown,
    Winter,
    Spring,
    Summer,
    Fall,
}

impl SeasonName {
    pub fn from_month(month: u8) -> Self {
        match month {
            12 | 1 | 2 => Self::Winter,
            3..=5 => Self::Spring,
            6..=8 => Self::Summer,
            9..=11 => Self::Fall,
            _ => Self::Unknown,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Winter => Self::Spring,
            Self::Spring => Self::Summer,
            Self::Summer => Self::Fall,
            Self::Fall => Self::Winter,
            Self::Unknown => Self::Unknown,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Winter => Self::Fall,
            Self::Spring => Self::Winter,
            Self::Summer => Self::Spring,
            Self::Fall => Self::Summer,
            Self::Unknown => Self::Unknown,
        }
    }

    /// First and last month. Winter starts in December of the previous year.
    fn months(self) -> Option<(i8, i8)> {
        match self {
            Self::Winter => Some((12, 2)),
            Self::Spring => Some((3, 5)),
            Self::Summer => Some((6, 8)),
            Self::Fall => Some((9, 11)),
            Self::Unknown => None,
        }
    }
}

/// An anime season such as "Winter 2019". A year of 0 is unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Season {
    pub name: SeasonName,
    pub year: u16,
}

impl Season {
    pub const fn new(name: SeasonName, year: u16) -> Self {
        Self { name, year }
    }

    /// The season a date falls in. December belongs to the next year's winter.
    pub fn from_date(date: FuzzyDate) -> Self {
        let name = SeasonName::from_month(date.month);
        let mut year = date.year;
        if year != 0 && date.month == 12 {
            year += 1;
        }
        Self { name, year }
    }

    /// Both the name and the year are known.
    pub fn is_valid(&self) -> bool {
        self.name != SeasonName::Unknown && self.year != 0
    }

    pub fn next(&self) -> Self {
        let name = self.name.next();
        let year = if name == SeasonName::Winter {
            self.year + 1
        } else {
            self.year
        };
        Self { name, year }
    }

    pub fn previous(&self) -> Self {
        let name = self.name.previous();
        let year = if name == SeasonName::Fall {
            self.year.saturating_sub(1)
        } else {
            self.year
        };
        Self { name, year }
    }

    /// First and last day of the season, when the season is valid.
    pub fn date_range(&self) -> Option<(Date, Date)> {
        if !self.is_valid() {
            return None;
        }
        let (first_month, last_month) = self.name.months()?;
        let year = i16::try_from(self.year).ok()?;
        let first_year = if self.name == SeasonName::Winter {
            year - 1
        } else {
            year
        };
        let first = Date::new(first_year, first_month, 1).ok()?;
        let last = Date::new(year, last_month, 1).ok()?.last_of_month();
        Some((first, last))
    }
}

impl Ord for Season {
    fn cmp(&self, other: &Self) -> Ordering {
        // Unknown years and names sort last: they are treated as "in the future".
        if self.year != other.year {
            return match (self.year, other.year) {
                (0, _) => Ordering::Greater,
                (_, 0) => Ordering::Less,
                (a, b) => a.cmp(&b),
            };
        }
        if self.name != other.name {
            return match (self.name, other.name) {
                (SeasonName::Unknown, _) => Ordering::Greater,
                (_, SeasonName::Unknown) => Ordering::Less,
                (a, b) => (a as u8).cmp(&(b as u8)),
            };
        }
        Ordering::Equal
    }
}

impl PartialOrd for Season {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn fuzzy_date_text() {
        assert_eq!(FuzzyDate::parse("2019-04-00"), FuzzyDate::new(2019, 4, 0));
        assert_eq!(FuzzyDate::parse(""), FuzzyDate::default());
        assert_eq!(FuzzyDate::parse("2019"), FuzzyDate::default());
        assert_eq!(FuzzyDate::new(2019, 4, 0).to_string(), "2019-04-00");
        assert_eq!(FuzzyDate::default().to_string(), "0000-00-00");
        assert_eq!(FuzzyDate::default().to_storage(), "");
        assert_eq!(FuzzyDate::new(2019, 4, 7).to_storage(), "2019-04-07");
    }

    #[test]
    fn fuzzy_date_flags() {
        assert!(FuzzyDate::default().is_empty());
        assert!(!FuzzyDate::default().is_set());
        assert!(FuzzyDate::new(2019, 0, 0).is_set());
        assert!(!FuzzyDate::new(2019, 4, 0).is_complete());
        assert!(FuzzyDate::new(2019, 4, 1).is_complete());
    }

    #[test]
    fn fuzzy_date_ordering() {
        let d = FuzzyDate::new;
        assert!(d(2019, 1, 1) < d(2019, 1, 2));
        assert!(d(2018, 12, 31) < d(2019, 1, 1));
        assert!(d(2019, 4, 0) > d(2019, 4, 30));
        assert!(d(0, 0, 0) > d(2100, 1, 1));
        assert_eq!(d(2019, 4, 0), d(2019, 4, 0));
    }

    #[test]
    fn fuzzy_date_days() {
        let a = FuzzyDate::new(2019, 3, 1);
        let b = FuzzyDate::new(2019, 2, 1);
        assert_eq!(a.days_since(&b), Some(28));
        assert_eq!(b.days_since(&a), Some(-28));
        assert_eq!(FuzzyDate::new(2019, 3, 0).days_since(&b), None);
    }

    #[test]
    fn season_from_date() {
        let s = |m, y| Season::from_date(FuzzyDate::new(y, m, 1));
        assert_eq!(s(12, 2018), Season::new(SeasonName::Winter, 2019));
        assert_eq!(s(1, 2019), Season::new(SeasonName::Winter, 2019));
        assert_eq!(s(3, 2019), Season::new(SeasonName::Spring, 2019));
        assert_eq!(s(8, 2019), Season::new(SeasonName::Summer, 2019));
        assert_eq!(s(11, 2019), Season::new(SeasonName::Fall, 2019));
        assert!(!Season::from_date(FuzzyDate::new(2019, 0, 0)).is_valid());
    }

    #[test]
    fn season_next_previous() {
        let fall = Season::new(SeasonName::Fall, 2018);
        let winter = Season::new(SeasonName::Winter, 2019);
        assert_eq!(fall.next(), winter);
        assert_eq!(winter.previous(), fall);
        assert_eq!(
            Season::new(SeasonName::Spring, 2019).next(),
            Season::new(SeasonName::Summer, 2019)
        );
    }

    #[test]
    fn season_range() {
        assert_eq!(
            Season::new(SeasonName::Winter, 2019).date_range(),
            Some((date(2018, 12, 1), date(2019, 2, 28)))
        );
        assert_eq!(
            Season::new(SeasonName::Winter, 2020).date_range(),
            Some((date(2019, 12, 1), date(2020, 2, 29)))
        );
        assert_eq!(
            Season::new(SeasonName::Fall, 2019).date_range(),
            Some((date(2019, 9, 1), date(2019, 11, 30)))
        );
        assert_eq!(Season::new(SeasonName::Unknown, 2019).date_range(), None);
    }

    #[test]
    fn season_ordering() {
        let s = Season::new;
        use SeasonName::*;
        assert!(s(Fall, 2018) < s(Winter, 2019));
        assert!(s(Winter, 2019) < s(Spring, 2019));
        assert!(s(Fall, 2100) < s(Winter, 0));
        assert!(s(Fall, 2019) < s(Unknown, 2019));
    }
}
