//! The Library page's "By anime" view: video files grouped under the anime
//! they were recognized as, with the ones that matched nothing first. Most
//! people look at the library to ask "what do I have of this anime?" or
//! "what did Shiori not recognize?", and a tree of release file names
//! answers neither at a glance.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use shiori_core::{AnimeId, Episode};

/// The files recognized as one anime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub anime: AnimeId,
    pub title: String,
    /// Sorted by episode, then by name.
    pub files: Vec<GroupFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFile {
    pub path: PathBuf,
    pub name: String,
    pub episodes: Option<(u32, u32)>,
}

impl Group {
    /// The episodes on disk, as "1-3, 5".
    pub fn episodes_label(&self) -> String {
        let mut numbers: Vec<u32> = self
            .files
            .iter()
            .filter_map(|f| f.episodes)
            .flat_map(|(first, last)| first..=last.max(first))
            .collect();
        numbers.sort_unstable();
        numbers.dedup();
        ranges(&numbers)
    }
}

/// Runs of consecutive numbers, as "1-3, 5, 7-8".
pub fn ranges(numbers: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut iter = numbers.iter().copied().peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while iter.peek() == Some(&(end + 1)) {
            end = iter.next().unwrap_or(end);
        }
        parts.push(if end > start {
            format!("{start}-{end}")
        } else {
            start.to_string()
        });
    }
    parts.join(", ")
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Groups recognized files by anime, sorted by title, and lists the files
/// that matched nothing, sorted by name.
pub fn group(
    recognized: &HashMap<PathBuf, Episode>,
    title: impl Fn(AnimeId) -> String,
) -> (Vec<Group>, Vec<PathBuf>) {
    let mut by_anime: BTreeMap<AnimeId, Vec<GroupFile>> = BTreeMap::new();
    let mut unmatched = Vec::new();
    for (path, episode) in recognized {
        match episode.anime_id {
            Some(id) => by_anime.entry(id).or_default().push(GroupFile {
                path: path.clone(),
                name: file_name(path),
                episodes: episode.episode_number_range(),
            }),
            None => unmatched.push(path.clone()),
        }
    }
    let mut groups: Vec<Group> = by_anime
        .into_iter()
        .map(|(anime, mut files)| {
            files.sort_by(|a, b| a.episodes.cmp(&b.episodes).then(a.name.cmp(&b.name)));
            Group {
                anime,
                title: title(anime),
                files,
            }
        })
        .collect();
    groups.sort_by_key(|g| g.title.to_lowercase());
    unmatched.sort_by_key(|p| file_name(p).to_lowercase());
    (groups, unmatched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_join_runs() {
        assert_eq!(ranges(&[]), "");
        assert_eq!(ranges(&[4]), "4");
        assert_eq!(ranges(&[1, 2, 3, 5, 7, 8]), "1-3, 5, 7-8");
    }

    #[test]
    fn groups_by_anime_and_keeps_unmatched_apart() {
        let mut recognized = HashMap::new();
        let episode = |id: Option<u32>| Episode {
            anime_id: id.map(AnimeId),
            elements: Vec::new(),
        };
        recognized.insert(PathBuf::from("/a/b - 02.mkv"), episode(Some(2)));
        recognized.insert(PathBuf::from("/a/a - 01.mkv"), episode(Some(1)));
        recognized.insert(PathBuf::from("/a/z.mkv"), episode(None));
        recognized.insert(PathBuf::from("/a/b - 01.mkv"), episode(Some(2)));
        let (groups, unmatched) = group(&recognized, |id| {
            if id == AnimeId(1) { "Zeta" } else { "alpha" }.to_owned()
        });
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].title, "alpha", "sorted by title, ignoring case");
        assert_eq!(groups[0].files.len(), 2);
        assert_eq!(unmatched, [PathBuf::from("/a/z.mkv")]);
    }
}
