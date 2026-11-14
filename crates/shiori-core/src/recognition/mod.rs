//! Turning a file name or window title into an anime id and episode number.

mod identify;
mod index;
mod normalize;
mod parse;
mod relations;

pub use identify::{MatchReason, identify, identify_with_reason};
pub use index::{TitleIndex, TitleKind};
pub use normalize::normalize;
pub use parse::{ParseOptions, parse, parse_path};
pub use relations::{Redirection, Relations};
