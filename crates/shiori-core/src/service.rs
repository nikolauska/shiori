//! The list services Shiori can synchronize with.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceId {
    AniList,
    MyAnimeList,
    Kitsu,
}

impl ServiceId {
    pub const ALL: [ServiceId; 3] = [ServiceId::AniList, ServiceId::MyAnimeList, ServiceId::Kitsu];

    pub fn name(self) -> &'static str {
        match self {
            ServiceId::AniList => "AniList",
            ServiceId::MyAnimeList => "MyAnimeList",
            ServiceId::Kitsu => "Kitsu",
        }
    }
}
