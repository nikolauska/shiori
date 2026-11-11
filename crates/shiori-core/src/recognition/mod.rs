//! Turning a file name or window title into an anime id and episode number.

mod normalize;
mod parse;

pub use normalize::normalize;
pub use parse::{ParseOptions, parse, parse_path};
