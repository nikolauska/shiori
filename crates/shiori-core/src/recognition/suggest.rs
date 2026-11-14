//! Suggesting episode numbering from the service's relations, for an
//! episode whose number is above its anime's count: "Show - 30" with three
//! 12-episode seasons is likely the third season's episode 6. A suggestion
//! is a hypothesis; the user confirms it before anything is remembered.

use std::collections::HashMap;

use jiff::Timestamp;

use crate::anime::AnimeId;
use crate::recognition::{NumberMiss, NumberingSequence, Resolved, SequenceEntry, continuous};
use crate::relation::{Relation, RelationKind};

/// How far to follow sequels, so a cycle or a very long franchise cannot
/// make the walk endless.
const MAX_SEASONS: usize = 20;

/// The seasons of a series, first to last, as far as the relations say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub order: Vec<AnimeId>,
    /// Some anime had two prequels or two sequels that are series; the
    /// chain stops there, and which one belongs is the user's call.
    pub branch: bool,
}

/// The next step of following relations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainStep {
    Done(Chain),
    /// The relations of this anime are needed to go on.
    NeedRelations(AnimeId),
}

/// Follows prequels from `start` to the first season, then sequels to the
/// last. Only anime `is_series` accepts count as seasons: movies, specials
/// and side stories are left for the user to include on purpose.
pub fn series_chain(
    start: AnimeId,
    relations: &HashMap<AnimeId, Vec<Relation>>,
    is_series: impl Fn(AnimeId) -> bool,
) -> ChainStep {
    let next = |id: AnimeId, kind: RelationKind| -> Result<Vec<AnimeId>, AnimeId> {
        let found = relations.get(&id).ok_or(id)?;
        Ok(found
            .iter()
            .filter(|r| r.kind == kind && is_series(r.anime))
            .map(|r| r.anime)
            .collect())
    };
    let mut branch = false;
    let mut root = start;
    let mut seen = vec![start];
    while seen.len() <= MAX_SEASONS {
        let prequels = match next(root, RelationKind::Prequel) {
            Ok(p) => p,
            Err(id) => return ChainStep::NeedRelations(id),
        };
        match prequels.as_slice() {
            [] => break,
            [one] if !seen.contains(one) => {
                root = *one;
                seen.push(*one);
            }
            [_] => break,
            _ => {
                branch = true;
                break;
            }
        }
    }
    let mut order = vec![root];
    while order.len() < MAX_SEASONS {
        let last = *order.last().unwrap_or(&root);
        let sequels = match next(last, RelationKind::Sequel) {
            Ok(s) => s,
            Err(id) => return ChainStep::NeedRelations(id),
        };
        match sequels.as_slice() {
            [] => break,
            [one] if !order.contains(one) => order.push(*one),
            [_] => break,
            _ => {
                branch = true;
                break;
            }
        }
    }
    ChainStep::Done(Chain { order, branch })
}

/// Why a suggestion needs the user's review instead of a yes or no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewReason {
    /// The relations branch.
    Branch,
    /// This anime's episode count is unknown, so where its numbers end is.
    UnknownCount(AnimeId),
    /// No season holds the number, or a batch spans two.
    NoFit,
    /// Counting from the first season and counting from the matched one
    /// give different answers ("Season 2 - 18").
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposal {
    /// One clear answer: remember `sequence` (or use it once) and the
    /// episode is `resolved`.
    Use {
        sequence: NumberingSequence,
        resolved: Resolved,
    },
    /// Show `draft` in the editor.
    Review {
        draft: NumberingSequence,
        reason: ReviewReason,
    },
}

fn draft(root: AnimeId, entries: Vec<SequenceEntry>) -> NumberingSequence {
    NumberingSequence {
        id: 0,
        root,
        entries,
        saved: Timestamp::UNIX_EPOCH,
    }
}

/// Continuous numbering over `chain`, applied to the missed episode.
pub fn propose(
    miss: &NumberMiss,
    chain: &Chain,
    count: impl Fn(AnimeId) -> Option<u32>,
) -> Proposal {
    let root = chain.order.first().copied().unwrap_or(miss.anime);
    let order: Vec<_> = chain.order.iter().map(|&id| (id, count(id))).collect();
    let (entries, unknown) = match continuous(&order) {
        Ok(entries) => (entries, None),
        Err((entries, at)) => (entries, Some(at)),
    };
    let sequence = draft(root, entries);
    let review = |reason| Proposal::Review {
        draft: sequence.clone(),
        reason,
    };
    if chain.branch {
        return review(ReviewReason::Branch);
    }
    let Some(resolved) = sequence.resolve(miss.range) else {
        return review(match unknown {
            Some(at) => ReviewReason::UnknownCount(at),
            None => ReviewReason::NoFit,
        });
    };
    // A file named after a later season may count from that season instead
    // of the first: "Season 2 - 18" is then the third season's 6. Asking is
    // better than guessing.
    if miss.anime != root
        && let Some(start) = chain.order.iter().position(|&id| id == miss.anime)
        && let Ok(from_matched) = continuous(&order[start..])
    {
        let other = draft(miss.anime, from_matched).resolve(miss.range);
        if other.is_some_and(|o| o.anime != resolved.anime || o.range != resolved.range) {
            return review(ReviewReason::Ambiguous);
        }
    }
    Proposal::Use { sequence, resolved }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S1: AnimeId = AnimeId(1001);
    const S2: AnimeId = AnimeId(1002);
    const S3: AnimeId = AnimeId(1003);
    const OVA: AnimeId = AnimeId(1004);

    fn rel(kind: RelationKind, anime: AnimeId) -> Relation {
        Relation { kind, anime }
    }

    fn relations() -> HashMap<AnimeId, Vec<Relation>> {
        use RelationKind::*;
        HashMap::from([
            (S1, vec![rel(Sequel, S2), rel(SideStory, OVA)]),
            (S2, vec![rel(Prequel, S1), rel(Sequel, S3)]),
            (S3, vec![rel(Prequel, S2)]),
            (OVA, vec![rel(Parent, S1)]),
        ])
    }

    fn series(id: AnimeId) -> bool {
        id != OVA
    }

    fn chain(from: AnimeId, relations: &HashMap<AnimeId, Vec<Relation>>) -> Chain {
        match series_chain(from, relations, series) {
            ChainStep::Done(chain) => chain,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn follows_prequels_back_and_sequels_forward() {
        let all = relations();
        let expected = Chain {
            order: vec![S1, S2, S3],
            branch: false,
        };
        assert_eq!(chain(S2, &all), expected);
        assert_eq!(chain(S3, &all), expected);
    }

    #[test]
    fn asks_for_relations_it_does_not_have() {
        let mut some = relations();
        some.remove(&S3);
        assert_eq!(
            series_chain(S1, &some, series),
            ChainStep::NeedRelations(S3)
        );
    }

    #[test]
    fn two_sequels_are_a_branch() {
        let mut all = relations();
        all.get_mut(&S1)
            .unwrap()
            .push(rel(RelationKind::Sequel, AnimeId(2000)));
        all.insert(AnimeId(2000), vec![]);
        let found = chain(S1, &all);
        assert!(found.branch);
        assert_eq!(found.order, [S1]);
    }

    #[test]
    fn a_cycle_ends() {
        let mut all = relations();
        all.get_mut(&S3)
            .unwrap()
            .push(rel(RelationKind::Sequel, S1));
        assert_eq!(chain(S1, &all).order, [S1, S2, S3]);
    }

    fn miss(anime: AnimeId, n: u32) -> NumberMiss {
        NumberMiss {
            anime,
            range: (n, n),
        }
    }

    #[test]
    fn three_seasons_give_one_answer() {
        let c = chain(S1, &relations());
        match propose(&miss(S1, 30), &c, |_| Some(12)) {
            Proposal::Use { sequence, resolved } => {
                assert_eq!((resolved.anime, resolved.range), (S3, (6, 6)));
                assert_eq!(sequence.root, S1);
                assert_eq!(sequence.entries.len(), 3);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unknown_count_before_the_number_needs_review() {
        let c = chain(S1, &relations());
        let count = |id| (id != S2).then_some(12);
        assert_eq!(
            match propose(&miss(S1, 30), &c, count) {
                Proposal::Review { reason, .. } => reason,
                other => panic!("{other:?}"),
            },
            ReviewReason::UnknownCount(S2)
        );
        // A number that resolves before the unknown count is fine.
        assert!(matches!(
            propose(&miss(S1, 13), &c, |id| (id != S3).then_some(12)),
            Proposal::Use { .. }
        ));
    }

    #[test]
    fn counting_from_the_matched_season_is_ambiguous() {
        let c = chain(S2, &relations());
        // From the first season, 18 is the second season's 6; from the
        // second, it is the third's 6.
        assert!(matches!(
            propose(&miss(S2, 18), &c, |_| Some(12)),
            Proposal::Review {
                reason: ReviewReason::Ambiguous,
                ..
            }
        ));
        // 30 is past what counting from the second season reaches (24), so
        // only one reading is left.
        assert!(matches!(
            propose(&miss(S2, 30), &c, |_| Some(12)),
            Proposal::Use { .. }
        ));
    }

    #[test]
    fn a_number_past_every_season_needs_review() {
        let c = chain(S1, &relations());
        assert!(matches!(
            propose(&miss(S1, 40), &c, |_| Some(12)),
            Proposal::Review {
                reason: ReviewReason::NoFit,
                ..
            }
        ));
    }
}
