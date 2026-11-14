//! Turning a file name or window title into an anime id and episode number.

mod identify;
mod index;
mod link;
mod normalize;
mod numbering;
mod parse;
mod relations;
mod suggest;

pub use identify::{MatchReason, NumberMiss, identify, identify_with_reason, number_miss};
pub use index::{TitleIndex, TitleKind};
pub use link::{close_matches, is_generic_title, link_title, linked_to, title_to_remember};
pub use normalize::{normalize, normalize_words};
pub use numbering::{
    Numbering, NumberingError, NumberingSequence, Resolved, SequenceEntry, continuous,
};
pub use parse::{ParseOptions, parse, parse_path};
pub use relations::{Redirection, Relations};
pub use suggest::{Chain, ChainStep, Proposal, ReviewReason, propose, series_chain};
