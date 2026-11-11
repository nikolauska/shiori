//! `settings.toml`: the user's settings.
//!
//! Key names follow Taiga v2 so a later import is a straight copy. Missing or
//! invalid keys take their default, and keys Shiori does not know are kept
//! when the file is written back.

use std::path::{Path, PathBuf};

use shiori_core::{ServiceId, TitleLanguage, UpdateTrigger};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value};

use crate::{Result, atomic, service_key};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProxyKind {
    #[default]
    Http,
    Socks5,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxySettings {
    pub kind: ProxyKind,
    /// Empty means no proxy.
    pub host: String,
    pub port: Option<u16>,
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub color_scheme: ColorScheme,
    pub service: ServiceId,
    pub sync_enabled: bool,
    pub library_folders: Vec<PathBuf>,
    pub title_language: TitleLanguage,
    pub detection_enabled: bool,
    /// Milliseconds between detection polls.
    pub detection_interval_ms: u32,
    /// Seconds before an update is applied, between 10 and 3600.
    pub update_delay_s: u32,
    pub update_trigger: UpdateTrigger,
    /// Ignore files outside library folders.
    pub library_only: bool,
    pub pause_when_unfocused: bool,
    /// Player names to ignore, compared without regard to case.
    pub disabled_players: Vec<String>,
    pub streaming_enabled: bool,
    pub proxy: ProxySettings,
}

pub const UPDATE_DELAY_RANGE: std::ops::RangeInclusive<u32> = 10..=3600;

impl Default for Settings {
    fn default() -> Self {
        Self {
            color_scheme: ColorScheme::System,
            service: ServiceId::AniList,
            sync_enabled: true,
            library_folders: Vec::new(),
            title_language: TitleLanguage::Romaji,
            detection_enabled: true,
            detection_interval_ms: 3000,
            update_delay_s: 120,
            update_trigger: UpdateTrigger::AfterDelay,
            library_only: false,
            pause_when_unfocused: false,
            disabled_players: Vec::new(),
            streaming_enabled: false,
            proxy: ProxySettings::default(),
        }
    }
}

/// The settings file, holding the parsed document so unknown keys survive a
/// rewrite.
#[derive(Debug)]
pub struct SettingsFile {
    path: PathBuf,
    document: DocumentMut,
}

impl SettingsFile {
    /// Reads the file. A missing file gives the defaults; a file that is not
    /// valid TOML is logged and also gives the defaults, and is replaced on
    /// the next save.
    pub fn load(path: &Path) -> Result<(Self, Settings)> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        let document = text.parse::<DocumentMut>().unwrap_or_else(|error| {
            tracing::warn!("Invalid settings file {}: {error}", path.display());
            DocumentMut::new()
        });
        let settings = read(&document);
        Ok((
            Self {
                path: path.to_owned(),
                document,
            },
            settings,
        ))
    }

    /// Writes the settings, keeping unknown keys and comments.
    pub fn save(&mut self, settings: &Settings) -> Result<()> {
        write(&mut self.document, settings);
        atomic::write(&self.path, self.document.to_string().as_bytes())?;
        Ok(())
    }
}

fn read(doc: &DocumentMut) -> Settings {
    let d = Settings::default();
    let item = |path: &str| get(doc.as_item(), path);
    let bool_at = |path: &str, default: bool| item(path).and_then(Item::as_bool).unwrap_or(default);
    let str_at = |path: &str| item(path).and_then(Item::as_str);
    let int_at = |path: &str| item(path).and_then(Item::as_integer);
    let list_at = |path: &str| -> Vec<String> {
        item(path)
            .and_then(Item::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };

    Settings {
        color_scheme: match str_at("app.colorScheme") {
            Some("light") => ColorScheme::Light,
            Some("dark") => ColorScheme::Dark,
            _ => d.color_scheme,
        },
        service: match str_at("sync.service") {
            Some("myanimelist") => ServiceId::MyAnimeList,
            Some("kitsu") => ServiceId::Kitsu,
            _ => d.service,
        },
        sync_enabled: bool_at("sync.enabled", d.sync_enabled),
        library_folders: list_at("library.folders")
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        title_language: match str_at("library.titleLanguage") {
            Some("english") => TitleLanguage::English,
            Some("native") => TitleLanguage::Native,
            _ => d.title_language,
        },
        detection_enabled: bool_at("track.detection.enabled", d.detection_enabled),
        detection_interval_ms: int_at("track.detection.interval")
            .and_then(|v| u32::try_from(v).ok())
            .filter(|&v| v > 0)
            .unwrap_or(d.detection_interval_ms),
        update_delay_s: int_at("track.update.delay")
            .map(|v| {
                v.clamp(
                    *UPDATE_DELAY_RANGE.start() as i64,
                    *UPDATE_DELAY_RANGE.end() as i64,
                ) as u32
            })
            .unwrap_or(d.update_delay_s),
        update_trigger: match str_at("track.update.trigger") {
            Some("onPlayerClose") => UpdateTrigger::OnPlayerClose,
            _ => d.update_trigger,
        },
        library_only: bool_at("track.update.libraryOnly", d.library_only),
        pause_when_unfocused: bool_at("track.update.pauseWhenUnfocused", d.pause_when_unfocused),
        disabled_players: list_at("recognition.mediaPlayers.disabled"),
        streaming_enabled: bool_at("recognition.streaming.enabled", d.streaming_enabled),
        proxy: ProxySettings {
            kind: match str_at("network.proxy.type") {
                Some("socks5") => ProxyKind::Socks5,
                _ => ProxyKind::Http,
            },
            host: str_at("network.proxy.host").unwrap_or_default().to_owned(),
            port: int_at("network.proxy.port").and_then(|v| u16::try_from(v).ok()),
            username: str_at("network.proxy.username")
                .unwrap_or_default()
                .to_owned(),
        },
    }
}

fn write(doc: &mut DocumentMut, s: &Settings) {
    let mut set = |path: &str, value: Option<Value>| set(doc.as_table_mut(), path, value);
    set("meta.version", Some(env!("CARGO_PKG_VERSION").into()));
    set(
        "app.colorScheme",
        Some(
            match s.color_scheme {
                ColorScheme::System => "system",
                ColorScheme::Light => "light",
                ColorScheme::Dark => "dark",
            }
            .into(),
        ),
    );
    set("sync.service", Some(service_key(s.service).into()));
    set("sync.enabled", Some(s.sync_enabled.into()));
    set(
        "library.folders",
        Some(list(
            s.library_folders
                .iter()
                .map(|p| p.to_string_lossy().into_owned()),
        )),
    );
    set(
        "library.titleLanguage",
        Some(
            match s.title_language {
                TitleLanguage::Romaji => "romaji",
                TitleLanguage::English => "english",
                TitleLanguage::Native => "native",
            }
            .into(),
        ),
    );
    set("track.detection.enabled", Some(s.detection_enabled.into()));
    set(
        "track.detection.interval",
        Some(i64::from(s.detection_interval_ms).into()),
    );
    set(
        "track.update.delay",
        Some(i64::from(s.update_delay_s).into()),
    );
    set(
        "track.update.trigger",
        Some(
            match s.update_trigger {
                UpdateTrigger::AfterDelay => "afterDelay",
                UpdateTrigger::OnPlayerClose => "onPlayerClose",
            }
            .into(),
        ),
    );
    set("track.update.libraryOnly", Some(s.library_only.into()));
    set(
        "track.update.pauseWhenUnfocused",
        Some(s.pause_when_unfocused.into()),
    );
    set(
        "recognition.mediaPlayers.disabled",
        Some(list(s.disabled_players.iter().cloned())),
    );
    set(
        "recognition.streaming.enabled",
        Some(s.streaming_enabled.into()),
    );
    set(
        "network.proxy.type",
        Some(
            match s.proxy.kind {
                ProxyKind::Http => "http",
                ProxyKind::Socks5 => "socks5",
            }
            .into(),
        ),
    );
    set("network.proxy.host", Some(s.proxy.host.as_str().into()));
    set(
        "network.proxy.port",
        s.proxy.port.map(|p| i64::from(p).into()),
    );
    set(
        "network.proxy.username",
        Some(s.proxy.username.as_str().into()),
    );
}

fn list(items: impl Iterator<Item = String>) -> Value {
    let mut array = Array::new();
    for item in items {
        array.push(item);
    }
    Value::Array(array)
}

/// Looks up a dotted key path such as `track.update.delay`.
fn get<'a>(root: &'a Item, path: &str) -> Option<&'a Item> {
    path.split('.').try_fold(root, |item, key| item.get(key))
}

/// Sets a dotted key path, creating tables on the way. `None` removes it.
fn set(root: &mut Table, path: &str, value: Option<Value>) {
    let mut parts: Vec<&str> = path.split('.').collect();
    let Some(last) = parts.pop() else {
        return;
    };
    let mut table: &mut dyn TableLike = root;
    for part in parts {
        // Inline tables written by hand count as tables too. Anything else in
        // the way is a setting of the wrong type and is replaced.
        if !table.get(part).is_some_and(Item::is_table_like) {
            let mut t = Table::new();
            // Written as `[a.b]` headers only, without an empty `[a]` above.
            t.set_implicit(true);
            table.insert(part, Item::Table(t));
        }
        table = table
            .get_mut(part)
            .and_then(Item::as_table_like_mut)
            .expect("table was just ensured");
    }
    match value {
        Some(value) => {
            // Keep the comments around a key when updating it.
            if let Some(existing) = table.get_mut(last).and_then(Item::as_value_mut) {
                let decor = existing.decor().clone();
                *existing = value;
                *existing.decor_mut() = decor;
            } else {
                table.insert(last, Item::Value(value));
            }
        }
        None => {
            table.remove(last);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_key_changed() -> Settings {
        Settings {
            color_scheme: ColorScheme::Dark,
            service: ServiceId::Kitsu,
            sync_enabled: false,
            library_folders: vec!["/media/anime".into(), "D:\\Anime".into()],
            title_language: TitleLanguage::Native,
            detection_enabled: false,
            detection_interval_ms: 5000,
            update_delay_s: 300,
            update_trigger: UpdateTrigger::OnPlayerClose,
            library_only: true,
            pause_when_unfocused: true,
            disabled_players: vec!["VLC".into()],
            streaming_enabled: true,
            proxy: ProxySettings {
                kind: ProxyKind::Socks5,
                host: "proxy.local".into(),
                port: Some(1080),
                username: "me".into(),
            },
        }
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let (_, settings) = SettingsFile::load(&dir.path().join("settings.toml")).unwrap();
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn every_key_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let (mut file, _) = SettingsFile::load(&path).unwrap();
        file.save(&every_key_changed()).unwrap();
        let (_, loaded) = SettingsFile::load(&path).unwrap();
        assert_eq!(loaded, every_key_changed());

        let (mut file, _) = SettingsFile::load(&path).unwrap();
        file.save(&Settings::default()).unwrap();
        let (_, loaded) = SettingsFile::load(&path).unwrap();
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn written_with_taiga_key_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let (mut file, _) = SettingsFile::load(&path).unwrap();
        file.save(&every_key_changed()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let doc: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(
            doc["track"]["update"]["pauseWhenUnfocused"].as_bool(),
            Some(true)
        );
        assert_eq!(
            doc["recognition"]["mediaPlayers"]["disabled"][0].as_str(),
            Some("VLC")
        );
        assert_eq!(doc["sync"]["service"].as_str(), Some("kitsu"));
        assert!(doc["meta"]["version"].is_str());
    }

    #[test]
    fn unknown_keys_and_comments_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            "# my settings\n[track.update]\n# seconds\ndelay = 60\nfuture = \"x\"\n\n[plugin]\nkey = 1\n",
        )
        .unwrap();
        let (mut file, settings) = SettingsFile::load(&path).unwrap();
        assert_eq!(settings.update_delay_s, 60);
        file.save(&settings).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my settings"), "{text}");
        assert!(text.contains("# seconds"), "{text}");
        assert!(text.contains("future = \"x\""), "{text}");
        assert!(text.contains("[plugin]"), "{text}");
    }

    #[test]
    fn invalid_values_take_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            "[track.update]\ndelay = 5\ntrigger = \"sometimes\"\n[sync]\nenabled = \"yes\"\n[network.proxy]\nport = 99999\n",
        )
        .unwrap();
        let (_, s) = SettingsFile::load(&path).unwrap();
        assert_eq!(s.update_delay_s, 10, "kept within the range");
        assert_eq!(s.update_trigger, UpdateTrigger::AfterDelay);
        assert!(s.sync_enabled);
        assert_eq!(s.proxy.port, None);

        std::fs::write(&path, "[track.update]\ndelay = 99999\n").unwrap();
        assert_eq!(SettingsFile::load(&path).unwrap().1.update_delay_s, 3600);
    }

    #[test]
    fn broken_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "this is [ not toml").unwrap();
        let (_, s) = SettingsFile::load(&path).unwrap();
        assert_eq!(s, Settings::default());
    }
}
