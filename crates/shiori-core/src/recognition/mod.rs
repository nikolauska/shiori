//! Turning a file name or window title into an anime id and episode number.

mod identify;
mod index;
mod link;
mod normalize;
mod parse;
mod relations;

pub use identify::{MatchReason, identify, identify_with_reason};
pub use index::{TitleIndex, TitleKind};
pub use link::{close_matches, is_generic_title, link_title, linked_to, title_to_remember};
pub use normalize::{normalize, normalize_words};
pub use parse::{ParseOptions, parse, parse_path};
pub use relations::{Redirection, Relations};
