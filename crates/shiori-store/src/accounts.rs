//! `accounts.toml`: who the user is on each service. No secrets; those are
//! in the keychain.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Result, atomic};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AniListAccount {
    pub username: String,
    pub rating_system: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MyAnimeListAccount {
    pub username: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct KitsuAccount {
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub user_id: String,
    pub rating_system: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Accounts {
    pub anilist: AniListAccount,
    pub myanimelist: MyAnimeListAccount,
    pub kitsu: KitsuAccount,
}

impl Accounts {
    /// A missing or unreadable file gives empty accounts. The user signs in
    /// again; nothing else is lost.
    pub fn load(path: &Path) -> Result<Self> {
        load_toml(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save_toml(path, self)
    }
}

pub(crate) fn load_toml<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return Err(e.into()),
    };
    Ok(toml::from_str(&text).unwrap_or_else(|error| {
        tracing::warn!("Invalid file {}: {error}", path.display());
        T::default()
    }))
}

pub(crate) fn save_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = toml::to_string_pretty(value).map_err(std::io::Error::other)?;
    atomic::write(path, text.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.toml");
        assert_eq!(Accounts::load(&path).unwrap(), Accounts::default());
        let accounts = Accounts {
            anilist: AniListAccount {
                username: "a".into(),
                rating_system: "POINT_100".into(),
            },
            myanimelist: MyAnimeListAccount {
                username: "m".into(),
            },
            kitsu: KitsuAccount {
                username: "k".into(),
                email: "k@example.com".into(),
                display_name: "K".into(),
                user_id: "123".into(),
                rating_system: "advanced".into(),
            },
        };
        accounts.save(&path).unwrap();
        assert_eq!(Accounts::load(&path).unwrap(), accounts);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("ratingSystem"), "{text}");
        assert!(text.contains("displayName"), "{text}");
    }

    #[test]
    fn partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.toml");
        std::fs::write(&path, "[anilist]\nusername = \"a\"\n").unwrap();
        let accounts = Accounts::load(&path).unwrap();
        assert_eq!(accounts.anilist.username, "a");
        assert_eq!(accounts.kitsu, KitsuAccount::default());
    }
}
