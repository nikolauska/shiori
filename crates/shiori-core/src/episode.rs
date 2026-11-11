//! A parsed file name or title, plus the identification result.

use serde::{Deserialize, Serialize};

use crate::anime::AnimeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ElementKind {
    AudioTerm,
    Device,
    Episode,
    EpisodeTitle,
    FileChecksum,
    FileExtension,
    Language,
    Other,
    Part,
    ReleaseGroup,
    ReleaseInformation,
    ReleaseVersion,
    Season,
    Source,
    Subtitles,
    Title,
    Type,
    VideoResolution,
    VideoTerm,
    Volume,
    Year,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Element {
    pub kind: ElementKind,
    pub value: String,
}

impl Element {
    pub fn new(kind: ElementKind, value: impl Into<String>) -> Self {
        Self {
            kind,
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub anime_id: Option<AnimeId>,
    pub elements: Vec<Element>,
}

impl Episode {
    pub fn new(elements: Vec<Element>) -> Self {
        Self {
            anime_id: None,
            elements,
        }
    }

    /// The first element of a kind.
    pub fn get(&self, kind: ElementKind) -> Option<&str> {
        self.elements
            .iter()
            .find(|e| e.kind == kind)
            .map(|e| e.value.as_str())
    }

    pub fn has(&self, kind: ElementKind) -> bool {
        self.elements.iter().any(|e| e.kind == kind)
    }

    pub fn all(&self, kind: ElementKind) -> impl Iterator<Item = &str> {
        self.elements
            .iter()
            .filter(move |e| e.kind == kind)
            .map(|e| e.value.as_str())
    }

    pub fn title(&self) -> Option<&str> {
        self.get(ElementKind::Title)
    }

    /// The parser only produces a file extension for video containers.
    pub fn is_video_file(&self) -> bool {
        self.has(ElementKind::FileExtension)
    }

    /// First and last episode number. A single episode gives the same number
    /// twice.
    pub fn episode_number_range(&self) -> Option<(u32, u32)> {
        let mut numbers = self.all(ElementKind::Episode).map(leading_number);
        let first = numbers.next()?;
        let last = numbers.last().unwrap_or(first);
        Some((first, last))
    }

    /// Replaces every episode element with `first`, and `last` when it is
    /// higher.
    pub fn set_episode_number_range(&mut self, (first, last): (u32, u32)) {
        self.elements.retain(|e| e.kind != ElementKind::Episode);
        self.elements
            .push(Element::new(ElementKind::Episode, first.to_string()));
        if last > first {
            self.elements
                .push(Element::new(ElementKind::Episode, last.to_string()));
        }
    }
}

/// Leading digits as a number: `7.5` is 7 and `4a` is 4. Taiga v2 reads these
/// as 0, which loses the episode; truncating keeps it usable.
fn leading_number(text: &str) -> u32 {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text[..end].parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(values: &[&str]) -> Episode {
        Episode::new(
            values
                .iter()
                .map(|v| Element::new(ElementKind::Episode, *v))
                .collect(),
        )
    }

    #[test]
    fn number_range() {
        assert_eq!(ep(&[]).episode_number_range(), None);
        assert_eq!(ep(&["01"]).episode_number_range(), Some((1, 1)));
        assert_eq!(ep(&["01", "13"]).episode_number_range(), Some((1, 13)));
        assert_eq!(ep(&["7.5"]).episode_number_range(), Some((7, 7)));
        assert_eq!(ep(&["4a"]).episode_number_range(), Some((4, 4)));
        assert_eq!(ep(&["x"]).episode_number_range(), Some((0, 0)));
    }

    #[test]
    fn set_number_range() {
        let mut e = ep(&["14"]);
        e.elements.push(Element::new(ElementKind::Title, "A"));
        e.set_episode_number_range((2, 2));
        assert_eq!(e.all(ElementKind::Episode).collect::<Vec<_>>(), ["2"]);
        e.set_episode_number_range((1, 3));
        assert_eq!(e.all(ElementKind::Episode).collect::<Vec<_>>(), ["1", "3"]);
        assert_eq!(e.title(), Some("A"));
    }
}
