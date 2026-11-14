//! How the service links one anime to another: sequels, prequels, side
//! stories. Used to suggest episode numbering across seasons.

use serde::{Deserialize, Serialize};

use crate::anime::AnimeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationKind {
    Prequel,
    Sequel,
    SideStory,
    Alternative,
    SpinOff,
    Summary,
    Parent,
    Other,
}

impl RelationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prequel => "Prequel",
            Self::Sequel => "Sequel",
            Self::SideStory => "Side story",
            Self::Alternative => "Alternative version",
            Self::SpinOff => "Spin-off",
            Self::Summary => "Summary",
            Self::Parent => "Parent story",
            Self::Other => "Related",
        }
    }
}

/// One link from an anime to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    pub kind: RelationKind,
    pub anime: AnimeId,
}
