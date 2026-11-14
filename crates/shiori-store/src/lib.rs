//! Everything Shiori keeps on disk: the SQLite database of each service, the
//! settings files and the secrets. No SQL leaves this crate.

mod accounts;
mod atomic;
mod database;
mod folders;
mod secrets;
mod session;
mod settings;

pub use accounts::{Accounts, AniListAccount, KitsuAccount, MyAnimeListAccount};
pub use database::{Database, Loaded};
pub use folders::{Folders, service_key};
pub use secrets::{SecretKey, Secrets};
pub use session::{ListView, SearchFilters, Session, ViewMode, WindowGeometry};
pub use settings::{
    ColorScheme, ListLocation, ProxyKind, ProxySettings, Settings, SettingsFile, UPDATE_DELAY_RANGE,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("keychain error: {0}")]
    Secrets(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
