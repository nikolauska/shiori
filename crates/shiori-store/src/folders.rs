//! Where Shiori keeps its files.

use std::path::PathBuf;

use shiori_core::ServiceId;

const APP: &str = "shiori";

/// The three per-user folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    /// `settings.toml`, `accounts.toml`, and the secrets fallback file.
    pub settings: PathBuf,
    /// Databases, `session.toml`, user override files, logs.
    pub data: PathBuf,
    /// Poster images.
    pub cache: PathBuf,
}

impl Folders {
    /// The standard per-user folders for this system, or `None` when the home
    /// folder cannot be found.
    pub fn standard() -> Option<Self> {
        let base = directories::BaseDirs::new()?;
        Some(Self::from_base(
            base.config_dir().into(),
            base.data_dir().into(),
            base.cache_dir().into(),
        ))
    }

    #[cfg(windows)]
    fn from_base(config: PathBuf, _data: PathBuf, cache: PathBuf) -> Self {
        // Settings and data both live in the roaming folder, cache in the
        // local one, as Windows apps usually do.
        let settings = config.join(APP);
        Self {
            data: settings.join("data"),
            settings,
            cache: cache.join(APP).join("cache"),
        }
    }

    #[cfg(not(windows))]
    fn from_base(config: PathBuf, data: PathBuf, cache: PathBuf) -> Self {
        Self {
            settings: config.join(APP),
            data: data.join(APP),
            cache: cache.join(APP),
        }
    }

    /// All folders under one root, for tests and a later portable mode.
    pub fn under(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            settings: root.join("settings"),
            data: root.join("data"),
            cache: root.join("cache"),
        }
    }

    pub fn create_all(&self) -> std::io::Result<()> {
        for dir in [&self.settings, &self.data, &self.cache] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn settings_file(&self) -> PathBuf {
        self.settings.join("settings.toml")
    }

    pub fn accounts_file(&self) -> PathBuf {
        self.settings.join("accounts.toml")
    }

    pub fn secrets_file(&self) -> PathBuf {
        self.settings.join("secrets.toml")
    }

    pub fn session_file(&self) -> PathBuf {
        self.data.join("session.toml")
    }

    pub fn database_file(&self, service: ServiceId) -> PathBuf {
        self.data.join(format!("{}.sqlite", service_key(service)))
    }

    /// The list kept on this computer only. It is apart from the services'
    /// copies so that signing in later never mixes the two.
    pub fn local_database_file(&self) -> PathBuf {
        self.data.join("local.sqlite")
    }

    /// Automatic backups of the local list.
    pub fn backups(&self) -> PathBuf {
        self.data.join("backups")
    }

    pub fn logs(&self) -> PathBuf {
        self.data.join("logs")
    }

    pub fn players_override(&self) -> PathBuf {
        self.data.join("players.anisthesia")
    }

    pub fn relations_override(&self) -> PathBuf {
        self.data.join("anime-relations.txt")
    }

    /// `<cache>/<service>/media/<anime id>.<ext>`
    pub fn poster_folder(&self, service: ServiceId) -> PathBuf {
        self.cache.join(service_key(service)).join("media")
    }
}

/// Lower-case service name used in file names and settings values.
pub fn service_key(service: ServiceId) -> &'static str {
    match service {
        ServiceId::AniList => "anilist",
        ServiceId::MyAnimeList => "myanimelist",
        ServiceId::Kitsu => "kitsu",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        let f = Folders::under("/x");
        assert_eq!(
            f.database_file(ServiceId::MyAnimeList),
            PathBuf::from("/x/data/myanimelist.sqlite")
        );
        assert_eq!(
            f.local_database_file(),
            PathBuf::from("/x/data/local.sqlite")
        );
        assert_eq!(
            f.poster_folder(ServiceId::AniList),
            PathBuf::from("/x/cache/anilist/media")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_layout() {
        let f = Folders::from_base(
            "/h/.config".into(),
            "/h/.local/share".into(),
            "/h/.cache".into(),
        );
        assert_eq!(f.settings, PathBuf::from("/h/.config/shiori"));
        assert_eq!(f.data, PathBuf::from("/h/.local/share/shiori"));
        assert_eq!(f.cache, PathBuf::from("/h/.cache/shiori"));
    }
}
