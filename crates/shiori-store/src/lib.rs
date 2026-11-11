//! Everything Shiori keeps on disk: the SQLite database of each service, the
//! settings files and the secrets. No SQL leaves this crate.

mod database;
mod folders;

pub use database::{Database, Loaded};
pub use folders::{Folders, service_key};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
