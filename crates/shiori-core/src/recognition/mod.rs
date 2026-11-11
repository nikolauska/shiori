//! Turning a file name or window title into an anime id and episode number.

mod parse;

pub use parse::{ParseOptions, parse, parse_path};
