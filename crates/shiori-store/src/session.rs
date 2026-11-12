//! `session.toml`: UI state that is not a setting. Losing it loses nothing
//! important, so any problem reading it gives the defaults.

use std::path::Path;

use serde::{Deserialize, Serialize};
use shiori_core::{AiringStatus, AnimeType, SeasonName};

use crate::Result;
use crate::accounts::{load_toml, save_toml};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowGeometry {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub maximized: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    /// Rich rows with posters, the default since the redesign (I22).
    /// Files from before it say `list` for the only list view there was,
    /// which now opens as rows, so the redesign reaches existing users.
    #[default]
    #[serde(alias = "list")]
    Rows,
    /// The detailed table.
    Table,
    Cards,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ListView {
    pub sort_column: String,
    pub sort_descending: bool,
    pub second_sort_column: String,
    pub second_sort_descending: bool,
    pub view_mode: ViewMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchFilters {
    pub year: Option<u16>,
    pub season: Option<SeasonName>,
    #[serde(rename = "type")]
    pub kind: Option<AnimeType>,
    pub status: Option<AiringStatus>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Session {
    pub main_window: WindowGeometry,
    /// Width of the inspector panel in pixels.
    pub inspector_width: Option<f32>,
    pub inspector_hidden: bool,
    pub anime_list: ListView,
    pub search_list: ListView,
    pub search_filters: SearchFilters,
}

impl Session {
    pub fn load(path: &Path) -> Result<Self> {
        load_toml(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save_toml(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.toml");
        assert_eq!(Session::load(&path).unwrap(), Session::default());
        let session = Session {
            main_window: WindowGeometry {
                x: Some(-10),
                y: Some(20),
                width: Some(1280),
                height: Some(720),
                maximized: true,
            },
            inspector_width: Some(320.0),
            inspector_hidden: true,
            anime_list: ListView {
                sort_column: "title".into(),
                sort_descending: true,
                second_sort_column: "score".into(),
                second_sort_descending: false,
                view_mode: ViewMode::Cards,
            },
            search_list: ListView::default(),
            search_filters: SearchFilters {
                year: Some(2026),
                season: Some(SeasonName::Fall),
                kind: Some(AnimeType::Tv),
                status: Some(AiringStatus::Airing),
            },
        };
        session.save(&path).unwrap();
        assert_eq!(Session::load(&path).unwrap(), session);
    }

    #[test]
    fn old_details_window_keys_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.toml");
        std::fs::write(
            &path,
            "detailsSplitter = 0.3\n[detailsWindow]\nwidth = 900\n[animeList]\nviewMode = \"list\"\n",
        )
        .unwrap();
        let session = Session::load(&path).unwrap();
        assert_eq!(session.anime_list.view_mode, ViewMode::Rows);
        assert_eq!(session.inspector_width, None);
        // A file from before the redesign has no view mode for search.
        assert_eq!(session.search_list.view_mode, ViewMode::Rows);
    }

    #[test]
    fn broken_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.toml");
        std::fs::write(&path, "mainWindow = 5").unwrap();
        assert_eq!(Session::load(&path).unwrap(), Session::default());
    }
}
