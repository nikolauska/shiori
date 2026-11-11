//! Tokens and passwords, kept in the operating system keychain.
//!
//! When no keychain is available (a Linux desktop without a running secret
//! service), they go to `secrets.toml` in the settings folder, readable only
//! by the user, and [`Secrets::is_fallback`] tells the UI to say so.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::accounts::load_toml;
use crate::{Error, Result, atomic};

/// Service name under which entries are stored in the keychain.
const KEYCHAIN_SERVICE: &str = "shiori";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SecretKey {
    AniListToken,
    MyAnimeListAccessToken,
    MyAnimeListRefreshToken,
    KitsuPassword,
    KitsuAccessToken,
    KitsuRefreshToken,
    ProxyPassword,
}

impl SecretKey {
    pub const ALL: [SecretKey; 7] = [
        SecretKey::AniListToken,
        SecretKey::MyAnimeListAccessToken,
        SecretKey::MyAnimeListRefreshToken,
        SecretKey::KitsuPassword,
        SecretKey::KitsuAccessToken,
        SecretKey::KitsuRefreshToken,
        SecretKey::ProxyPassword,
    ];

    /// The entry name in the keychain and the key in the fallback file.
    pub fn name(self) -> &'static str {
        match self {
            SecretKey::AniListToken => "anilist.token",
            SecretKey::MyAnimeListAccessToken => "myanimelist.accessToken",
            SecretKey::MyAnimeListRefreshToken => "myanimelist.refreshToken",
            SecretKey::KitsuPassword => "kitsu.password",
            SecretKey::KitsuAccessToken => "kitsu.accessToken",
            SecretKey::KitsuRefreshToken => "kitsu.refreshToken",
            SecretKey::ProxyPassword => "network.proxy.password",
        }
    }
}

#[derive(Debug)]
enum Backend {
    Keychain,
    File(PathBuf),
    Memory(Mutex<BTreeMap<String, String>>),
}

#[derive(Debug)]
pub struct Secrets {
    backend: Backend,
}

impl Secrets {
    /// Uses the keychain when it works, otherwise the fallback file.
    pub fn open(fallback_file: &Path) -> Self {
        match probe_keychain() {
            Ok(()) => Self {
                backend: Backend::Keychain,
            },
            Err(error) => {
                tracing::warn!(
                    "No keychain available ({error}); storing secrets in {}",
                    fallback_file.display()
                );
                Self::in_file(fallback_file)
            }
        }
    }

    pub fn in_file(path: &Path) -> Self {
        Self {
            backend: Backend::File(path.to_owned()),
        }
    }

    /// Secrets that are never written anywhere, for tests.
    pub fn in_memory() -> Self {
        Self {
            backend: Backend::Memory(Mutex::default()),
        }
    }

    /// True when secrets are in a file instead of the keychain.
    pub fn is_fallback(&self) -> bool {
        matches!(self.backend, Backend::File(_))
    }

    pub fn get(&self, key: SecretKey) -> Result<Option<String>> {
        match &self.backend {
            Backend::Keychain => match entry(key)?.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(secret_error(e)),
            },
            Backend::File(path) => Ok(read_file(path)?.remove(key.name())),
            Backend::Memory(map) => Ok(lock(map).get(key.name()).cloned()),
        }
    }

    /// Stores a secret. An empty value removes it.
    pub fn set(&self, key: SecretKey, value: &str) -> Result<()> {
        if value.is_empty() {
            return self.delete(key);
        }
        match &self.backend {
            Backend::Keychain => entry(key)?.set_password(value).map_err(secret_error),
            Backend::File(path) => {
                let mut map = read_file(path)?;
                map.insert(key.name().to_owned(), value.to_owned());
                write_file(path, &map)
            }
            Backend::Memory(map) => {
                lock(map).insert(key.name().to_owned(), value.to_owned());
                Ok(())
            }
        }
    }

    pub fn delete(&self, key: SecretKey) -> Result<()> {
        match &self.backend {
            Backend::Keychain => match entry(key)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(secret_error(e)),
            },
            Backend::File(path) => {
                let mut map = read_file(path)?;
                if map.remove(key.name()).is_some() {
                    write_file(path, &map)?;
                }
                Ok(())
            }
            Backend::Memory(map) => {
                lock(map).remove(key.name());
                Ok(())
            }
        }
    }
}

/// A keychain can exist but be unusable (for example no secret service is
/// running), so a real lookup is tried rather than trusting the store setup.
fn probe_keychain() -> keyring::Result<()> {
    let probe = keyring::Entry::new(KEYCHAIN_SERVICE, SecretKey::AniListToken.name())?;
    match probe.get_password() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e),
    }
}

fn entry(key: SecretKey) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, key.name()).map_err(secret_error)
}

fn secret_error(error: keyring::Error) -> Error {
    Error::Secrets(error.to_string())
}

fn lock(
    map: &Mutex<BTreeMap<String, String>>,
) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
    // A panic while holding the lock cannot leave the map half-changed.
    map.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_file(path: &Path) -> Result<BTreeMap<String, String>> {
    load_toml(path)
}

fn write_file(path: &Path, map: &BTreeMap<String, String>) -> Result<()> {
    let text = toml::to_string(map).map_err(std::io::Error::other)?;
    atomic::write_with(path, text.as_bytes(), restrict_to_owner)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_to_owner(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_to_owner(_file: &std::fs::File) -> std::io::Result<()> {
    // The settings folder is already per-user on Windows.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(secrets: &Secrets) {
        for key in SecretKey::ALL {
            assert_eq!(secrets.get(key).unwrap(), None);
        }
        for key in SecretKey::ALL {
            secrets
                .set(key, &format!("value of {}", key.name()))
                .unwrap();
        }
        for key in SecretKey::ALL {
            assert_eq!(
                secrets.get(key).unwrap(),
                Some(format!("value of {}", key.name()))
            );
        }
        secrets.set(SecretKey::KitsuPassword, "").unwrap();
        assert_eq!(secrets.get(SecretKey::KitsuPassword).unwrap(), None);
        secrets.delete(SecretKey::AniListToken).unwrap();
        secrets.delete(SecretKey::AniListToken).unwrap();
        assert_eq!(secrets.get(SecretKey::AniListToken).unwrap(), None);
        assert!(secrets.get(SecretKey::ProxyPassword).unwrap().is_some());
    }

    #[test]
    fn memory_backend() {
        exercise(&Secrets::in_memory());
    }

    #[test]
    fn file_backend() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        let secrets = Secrets::in_file(&path);
        assert!(secrets.is_fallback());
        exercise(&secrets);
        // A new instance reads what the first one wrote.
        let again = Secrets::in_file(&path);
        assert!(again.get(SecretKey::ProxyPassword).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        Secrets::in_file(&path)
            .set(SecretKey::AniListToken, "t")
            .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
