//! Episode redirects from the anime-relations file.
//!
//! Fansub groups often number a second season on from the first (episode 14
//! instead of season 2 episode 1). The rules map those numbers to the right
//! anime and episode.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::anime::AnimeId;
use crate::service::ServiceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rule {
    destination: AnimeId,
    source_range: (u32, u32),
    destination_range: (u32, u32),
}

/// A redirect target: the anime and the episode numbers there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Redirection {
    pub id: AnimeId,
    pub episode_range: (u32, u32),
}

/// The rules for one service, keyed by source anime, in file order.
#[derive(Debug, Clone, Default)]
pub struct Relations {
    rules: HashMap<AnimeId, Vec<Rule>>,
}

static RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?<src_ids>(?:\d+|[?~])(?:\|(?:\d+|[?~]))*):(?<src_start>\d+)(?:-(?<src_end>\d+|\?))? -> ",
        r"(?<dst_ids>(?:\d+|[?~])(?:\|(?:\d+|[?~]))*):(?<dst_start>\d+)(?:-(?<dst_end>\d+|\?))?(?<bang>!)?$",
    ))
    .expect("valid pattern")
});

/// Which id column belongs to a service in the file.
fn column(service: ServiceId) -> usize {
    match service {
        ServiceId::MyAnimeList => 0,
        ServiceId::Kitsu => 1,
        ServiceId::AniList => 2,
    }
}

impl Relations {
    /// Reads the rules for one service from the text of `anime-relations.txt`.
    /// Lines that do not parse are logged and skipped.
    pub fn parse(text: &str, service: ServiceId) -> Self {
        let column = column(service);
        let mut relations = Self::default();
        let mut in_rules = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with("::") {
                in_rules = line == "::rules";
                continue;
            }
            if !in_rules {
                continue;
            }
            let line = line.strip_prefix('-').map_or(line, str::trim);
            relations.parse_rule(line, column);
        }
        relations
    }

    fn parse_rule(&mut self, line: &str, column: usize) {
        let Some(m) = RULE.captures(line) else {
            tracing::warn!("Could not parse anime relations rule: {line}");
            return;
        };
        // `?` and `~` do not parse as numbers; what they mean depends on the side.
        let id = |group: &str| -> Option<AnimeId> {
            m[group]
                .split('|')
                .nth(column)
                .and_then(|id| id.parse().ok())
                .map(AnimeId)
        };
        let range = |start: &str, end: &str| -> (u32, u32) {
            let start: u32 = m[start].parse().unwrap_or(0);
            match m.name(end) {
                None => (start, start),
                // `?` is an open end.
                Some(end) => (start, end.as_str().parse().unwrap_or(u32::MAX)),
            }
        };
        let Some(source) = id("src_ids") else {
            return;
        };
        // `~` or a missing id means the same anime as the source.
        let destination = id("dst_ids").unwrap_or(source);
        let source_range = range("src_start", "src_end");
        let destination_range = range("dst_start", "dst_end");
        let rule = Rule {
            destination,
            source_range,
            destination_range,
        };
        self.rules.entry(source).or_default().push(rule);
        if m.name("bang").is_some() {
            // `!` also redirects the destination to itself, for releases
            // that are matched to the second season but numbered on from the
            // first.
            self.rules.entry(destination).or_default().push(rule);
        }
    }

    /// Where one episode of an anime is redirected to, if anywhere.
    pub fn find(&self, id: AnimeId, episode: u32) -> Option<Redirection> {
        self.rules.get(&id)?.iter().find_map(|rule| {
            let (start, end) = rule.source_range;
            if episode < start || episode > end {
                return None;
            }
            let (dst_start, dst_end) = rule.destination_range;
            let destination = if dst_start == dst_end {
                dst_start
            } else {
                dst_start.saturating_add(episode - start)
            };
            (destination <= dst_end).then_some(Redirection {
                id: rule.destination,
                episode_range: (destination, destination),
            })
        })
    }

    /// Where a range of episodes is redirected to. Both ends must lead to the
    /// same anime.
    pub fn find_redirection(&self, id: AnimeId, (first, last): (u32, u32)) -> Option<Redirection> {
        let start = self.find(id, first)?;
        if first == last {
            return Some(start);
        }
        let end = self.find(id, last)?;
        if start.id != end.id {
            return None;
        }
        Some(Redirection {
            id: start.id,
            episode_range: (start.episode_range.0, end.episode_range.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(lines: &str, service: ServiceId) -> Relations {
        Relations::parse(&format!("::meta\n- version: 1\n::rules\n{lines}"), service)
    }

    #[test]
    fn spec_example() {
        let r = rules(
            "- 41380|43367|116242:13-24 -> 44881|43883|127366:1-12!",
            ServiceId::AniList,
        );
        let redirect = |id, n| r.find(AnimeId(id), n).map(|x| (x.id.0, x.episode_range.0));
        assert_eq!(redirect(116242, 14), Some((127366, 2)));
        assert_eq!(redirect(116242, 12), None);
        assert_eq!(redirect(116242, 25), None);
        // The `!` rule: the second season numbered on from the first.
        assert_eq!(redirect(127366, 13), Some((127366, 1)));
    }

    #[test]
    fn columns_per_service() {
        let line = "- 41380|43367|116242:13-24 -> 44881|43883|127366:1-12";
        let r = rules(line, ServiceId::MyAnimeList);
        assert_eq!(r.find(AnimeId(41380), 13).unwrap().id, AnimeId(44881));
        let r = rules(line, ServiceId::Kitsu);
        assert_eq!(r.find(AnimeId(43367), 24).unwrap().episode_range, (12, 12));
    }

    #[test]
    fn unknown_and_same_ids() {
        let r = rules("- 1|?|3:2 -> ~|5|~:1", ServiceId::AniList);
        assert_eq!(
            r.find(AnimeId(3), 2),
            Some(Redirection {
                id: AnimeId(3),
                episode_range: (1, 1)
            })
        );
        // Kitsu's source is unknown, so the rule does not exist there.
        let r = rules("- 1|?|3:2 -> ~|5|~:1", ServiceId::Kitsu);
        assert!(r.rules.is_empty());
    }

    #[test]
    fn single_destination_and_open_end() {
        let r = rules(
            "- 1|1|1:13-? -> 2|2|2:1-?\n- 7|7|7:5-6 -> 8|8|8:1",
            ServiceId::AniList,
        );
        assert_eq!(r.find(AnimeId(1), 100).unwrap().episode_range, (88, 88));
        assert_eq!(r.find(AnimeId(7), 6).unwrap().episode_range, (1, 1));
    }

    #[test]
    fn range_lookup() {
        let r = rules(
            "- 1|1|1:13-24 -> 2|2|2:1-12\n- 1|1|1:25-36 -> 3|3|3:1-12",
            ServiceId::AniList,
        );
        assert_eq!(
            r.find_redirection(AnimeId(1), (13, 24)),
            Some(Redirection {
                id: AnimeId(2),
                episode_range: (1, 12)
            })
        );
        assert_eq!(r.find_redirection(AnimeId(1), (13, 30)), None);
        assert_eq!(r.find_redirection(AnimeId(1), (13, 40)), None);
        assert_eq!(r.find_redirection(AnimeId(1), (1, 13)), None);
    }

    #[test]
    fn ignores_other_sections_and_bad_lines() {
        let r = Relations::parse(
            "# comment\n::meta\n- 1|1|1:1 -> 2|2|2:1\n::rules\nnot a rule\n1|1|1:2 -> 2|2|2:5\n",
            ServiceId::AniList,
        );
        assert_eq!(r.find(AnimeId(1), 1), None);
        assert_eq!(r.find(AnimeId(1), 2).unwrap().episode_range, (5, 5));
    }

    #[test]
    fn bundled_file_parses() {
        let text = include_str!("../../../../data/anime-relations.txt");
        let rule_lines: Vec<_> = text
            .lines()
            .skip_while(|l| l.trim() != "::rules")
            .filter_map(|l| l.trim().strip_prefix("- "))
            .collect();
        assert!(rule_lines.len() > 500);
        for line in &rule_lines {
            assert!(RULE.is_match(line), "{line}");
        }
        let r = Relations::parse(text, ServiceId::AniList);
        assert_eq!(
            r.find(AnimeId(116242), 14).map(|x| (x.id, x.episode_range)),
            Some((AnimeId(127366), (2, 2)))
        );
    }
}
