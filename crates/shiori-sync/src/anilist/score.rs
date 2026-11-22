//! AniList score formats. Scores are always fetched and sent as 0 to 100
//! (`POINT_100`), so the format only changes what the user sees and picks.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ScoreFormat {
    Point100,
    Point10Decimal,
    #[default]
    Point10,
    Point5,
    Point3,
}

impl ScoreFormat {
    /// The name AniList uses. Unknown names fall back to `POINT_10`, which is
    /// AniList's own default.
    pub fn from_name(name: &str) -> Self {
        match name {
            "POINT_100" => ScoreFormat::Point100,
            "POINT_10_DECIMAL" => ScoreFormat::Point10Decimal,
            "POINT_10" => ScoreFormat::Point10,
            "POINT_5" => ScoreFormat::Point5,
            "POINT_3" => ScoreFormat::Point3,
            _ => ScoreFormat::default(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ScoreFormat::Point100 => "POINT_100",
            ScoreFormat::Point10Decimal => "POINT_10_DECIMAL",
            ScoreFormat::Point10 => "POINT_10",
            ScoreFormat::Point5 => "POINT_5",
            ScoreFormat::Point3 => "POINT_3",
        }
    }
}

pub use crate::score::Rating;

fn rating(value: u8, label: impl Into<String>) -> Rating {
    Rating {
        value,
        label: label.into(),
    }
}

/// The choices offered in the score menu, lowest first.
pub fn rating_list(format: ScoreFormat) -> Vec<Rating> {
    match format {
        ScoreFormat::Point100 => std::iter::once(rating(0, "0"))
            .chain((10..=100).step_by(5).map(|v| rating(v, v.to_string())))
            .collect(),
        ScoreFormat::Point10Decimal => std::iter::once(rating(0, "0"))
            .chain(
                (10..100)
                    .step_by(5)
                    .map(|v| rating(v, format!("{:.1}", f32::from(v) / 10.0))),
            )
            .chain(std::iter::once(rating(100, "10")))
            .collect(),
        ScoreFormat::Point10 => (0..=100)
            .step_by(10)
            .map(|v| rating(v, (v / 10).to_string()))
            .collect(),
        ScoreFormat::Point5 => [0, 10, 30, 50, 70, 90]
            .into_iter()
            .map(|v| rating(v, stars(v)))
            .collect(),
        ScoreFormat::Point3 => [0, 35, 60, 85]
            .into_iter()
            .map(|v| rating(v, smiley(v)))
            .collect(),
    }
}

/// How a stored score is shown.
pub fn format_rating(format: ScoreFormat, value: u8) -> String {
    match format {
        ScoreFormat::Point100 => value.to_string(),
        ScoreFormat::Point10Decimal => format!("{:.1}", f32::from(value) / 10.0),
        ScoreFormat::Point10 => (value / 10).to_string(),
        ScoreFormat::Point5 => stars(value),
        ScoreFormat::Point3 => smiley(value).to_owned(),
    }
}

fn stars(value: u8) -> String {
    let filled = match value {
        0 => 0,
        1..30 => 1,
        30..50 => 2,
        50..70 => 3,
        70..90 => 4,
        _ => 5,
    };
    "★".repeat(filled) + &"☆".repeat(5 - filled)
}

fn smiley(value: u8) -> &'static str {
    match value {
        0 => "No Score",
        1..36 => ":(",
        36..61 => ":|",
        _ => ":)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(format: ScoreFormat) -> Vec<(u8, String)> {
        rating_list(format)
            .into_iter()
            .map(|r| (r.value, r.label))
            .collect()
    }

    #[test]
    fn names() {
        for format in [
            ScoreFormat::Point100,
            ScoreFormat::Point10Decimal,
            ScoreFormat::Point10,
            ScoreFormat::Point5,
            ScoreFormat::Point3,
        ] {
            assert_eq!(ScoreFormat::from_name(format.name()), format);
        }
        assert_eq!(ScoreFormat::from_name("POINT_7"), ScoreFormat::Point10);
        assert_eq!(ScoreFormat::from_name(""), ScoreFormat::Point10);
    }

    #[test]
    fn menus() {
        let point100 = labels(ScoreFormat::Point100);
        assert_eq!(point100.len(), 20);
        assert_eq!(point100[0], (0, "0".into()));
        assert_eq!(point100[1], (10, "10".into()));
        assert_eq!(point100[2], (15, "15".into()));
        assert_eq!(point100[19], (100, "100".into()));

        let decimal = labels(ScoreFormat::Point10Decimal);
        assert_eq!(decimal.len(), 20);
        assert_eq!(decimal[1], (10, "1.0".into()));
        assert_eq!(decimal[2], (15, "1.5".into()));
        assert_eq!(decimal[18], (95, "9.5".into()));
        assert_eq!(decimal[19], (100, "10".into()));

        let point10 = labels(ScoreFormat::Point10);
        assert_eq!(point10.len(), 11);
        assert_eq!(point10[10], (100, "10".into()));

        assert_eq!(
            labels(ScoreFormat::Point5),
            [
                (0, "☆☆☆☆☆".into()),
                (10, "★☆☆☆☆".into()),
                (30, "★★☆☆☆".into()),
                (50, "★★★☆☆".into()),
                (70, "★★★★☆".into()),
                (90, "★★★★★".into()),
            ]
        );
        assert_eq!(
            labels(ScoreFormat::Point3),
            [
                (0, "No Score".into()),
                (35, ":(".into()),
                (60, ":|".into()),
                (85, ":)".into()),
            ]
        );
    }

    #[test]
    fn formatting() {
        assert_eq!(format_rating(ScoreFormat::Point100, 85), "85");
        assert_eq!(format_rating(ScoreFormat::Point10Decimal, 85), "8.5");
        assert_eq!(format_rating(ScoreFormat::Point10, 85), "8");
        assert_eq!(format_rating(ScoreFormat::Point5, 0), "☆☆☆☆☆");
        assert_eq!(format_rating(ScoreFormat::Point5, 29), "★☆☆☆☆");
        assert_eq!(format_rating(ScoreFormat::Point5, 50), "★★★☆☆");
        assert_eq!(format_rating(ScoreFormat::Point5, 100), "★★★★★");
        assert_eq!(format_rating(ScoreFormat::Point3, 0), "No Score");
        assert_eq!(format_rating(ScoreFormat::Point3, 35), ":(");
        assert_eq!(format_rating(ScoreFormat::Point3, 36), ":|");
        assert_eq!(format_rating(ScoreFormat::Point3, 61), ":)");
    }
}
