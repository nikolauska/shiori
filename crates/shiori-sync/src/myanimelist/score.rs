//! MyAnimeList scores: whole numbers 1 to 10, each with MyAnimeList's own
//! word for it. Stored as 0 to 100 like every other service.

use crate::score::Rating;

const LABELS: [&str; 11] = [
    "No Score",
    "Appalling",
    "Horrible",
    "Very Bad",
    "Bad",
    "Average",
    "Fine",
    "Good",
    "Very Good",
    "Great",
    "Masterpiece",
];

/// A score from MyAnimeList (0 to 10) as stored.
pub(crate) fn parse_list_score(value: u8) -> u8 {
    value.min(10) * 10
}

/// A stored score as MyAnimeList takes it. Rounds down, as Taiga does, so
/// that a score set on another service never shows higher here.
pub(crate) fn to_list_score(value: u8) -> u8 {
    value.min(100) / 10
}

/// The choices offered in the score menu, lowest first.
pub fn rating_list() -> Vec<Rating> {
    (0..=10u8)
        .map(|n| Rating {
            value: n * 10,
            label: label(n),
        })
        .collect()
}

/// How a stored score is shown: `(8) Very Good`.
pub fn format_rating(value: u8) -> String {
    label(to_list_score(value))
}

fn label(n: u8) -> String {
    format!("({n}) {}", LABELS[usize::from(n.min(10))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_has_every_label() {
        let list = rating_list();
        assert_eq!(list.len(), 11);
        assert_eq!(list[0].label, "(0) No Score");
        assert_eq!(list[7].value, 70);
        assert_eq!(list[7].label, "(7) Good");
        assert_eq!(list[10].label, "(10) Masterpiece");
    }

    #[test]
    fn conversion_both_ways() {
        assert_eq!(parse_list_score(8), 80);
        assert_eq!(parse_list_score(0), 0);
        assert_eq!(parse_list_score(200), 100);
        assert_eq!(to_list_score(80), 8);
        assert_eq!(to_list_score(85), 8);
        assert_eq!(to_list_score(100), 10);
        assert_eq!(format_rating(85), "(8) Very Good");
    }
}
