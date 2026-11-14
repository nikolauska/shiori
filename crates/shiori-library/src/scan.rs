use std::collections::{BTreeMap, VecDeque};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use shiori_core::AnimeId;
use shiori_core::episode::VIDEO_EXTENSIONS;

use crate::Recognizer;

/// How deep below a library folder the walk goes. Real libraries are a few
/// levels deep; the limit stops a symbolic link that points back up from
/// walking forever.
const MAX_DEPTH: usize = 16;

/// How deep below a library folder an anime folder is looked for. Taiga
/// looks at the folders in the library folder only; one more level allows
/// a split such as `Anime/2024/Show`.
const MAX_ANIME_FOLDER_DEPTH: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Folder(Vec<Node>),
    File(FileInfo),
}

/// A file or folder in a library folder, as the Library page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub modified: Option<SystemTime>,
    pub kind: NodeKind,
}

impl Node {
    pub fn children(&self) -> &[Node] {
        match &self.kind {
            NodeKind::Folder(children) => children,
            NodeKind::File(_) => &[],
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self.kind, NodeKind::Folder(_))
    }
}

/// Reads a library folder into a tree. Folders come before files, each in
/// name order ignoring case. A folder that cannot be read shows empty
/// rather than failing the whole scan.
pub fn scan_folder(root: &Path) -> Node {
    scan_node(root, 0)
}

fn scan_node(path: &Path, depth: usize) -> Node {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        // A drive or filesystem root has no name of its own.
        .unwrap_or_else(|| path.display().to_string());
    let metadata = std::fs::metadata(path).ok();
    let modified = metadata.as_ref().and_then(|m| m.modified().ok());
    if metadata.as_ref().is_some_and(|m| m.is_file()) {
        let size = metadata.map_or(0, |m| m.len());
        return Node {
            path: path.to_owned(),
            name,
            modified,
            kind: NodeKind::File(FileInfo { size, modified }),
        };
    }
    let mut children: Vec<Node> = if depth < MAX_DEPTH {
        entries(path)
            .into_iter()
            .map(|(child, _)| scan_node(&child, depth + 1))
            .collect()
    } else {
        Vec::new()
    };
    children.sort_by_cached_key(|n| (!n.is_folder(), n.name.to_lowercase()));
    Node {
        path: path.to_owned(),
        name,
        modified,
        kind: NodeKind::Folder(children),
    }
}

/// The entries of a folder in name order, with whether each is a folder.
/// Hidden entries (a leading dot) are left out: on Linux they are caches
/// and the like, never episodes. Symbolic links are followed, so a library
/// assembled from links still works.
fn entries(folder: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(read) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut entries: Vec<(PathBuf, bool)> = read
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| {
            let path = e.path();
            let is_dir = std::fs::metadata(&path).is_ok_and(|m| m.is_dir());
            (path, is_dir)
        })
        .collect();
    entries.sort_by_cached_key(|(p, is_dir)| {
        (
            !is_dir,
            p.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
        )
    });
    entries
}

/// Calls `visit` with each video file under `roots`, in tree order, until
/// it breaks. Checking the extension first spares parsing every subtitle
/// and image in the library.
fn walk_videos(roots: &[PathBuf], visit: &mut impl FnMut(&Path) -> ControlFlow<()>) {
    fn walk(
        folder: &Path,
        depth: usize,
        visit: &mut impl FnMut(&Path) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        for (path, is_dir) in entries(folder) {
            if is_dir {
                if depth < MAX_DEPTH {
                    walk(&path, depth + 1, visit)?;
                }
            } else if is_video_name(&path) {
                visit(&path)?;
            }
        }
        ControlFlow::Continue(())
    }
    for root in roots {
        if walk(root, 0, visit).is_break() {
            return;
        }
    }
}

pub(crate) fn is_video_name(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        let ext = ext.to_string_lossy();
        VIDEO_EXTENSIONS
            .iter()
            .any(|v| v.eq_ignore_ascii_case(&ext))
    })
}

/// Every video file under `roots`.
pub fn video_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_videos(roots, &mut |path| {
        files.push(path.to_owned());
        ControlFlow::Continue(())
    });
    files
}

/// The first video file under `roots` that is recognized as episode
/// `number` of anime `id`. A file holding several episodes counts for each
/// of them. Roots are searched in order, so the anime's own folder can go
/// first.
pub fn find_episode(
    recognizer: &Recognizer,
    roots: &[PathBuf],
    id: AnimeId,
    number: u32,
) -> Option<PathBuf> {
    let mut found = None;
    walk_videos(roots, &mut |path| {
        let matches = recognizer.file(path).is_some_and(|episode| {
            episode.anime_id == Some(id)
                && episode
                    .episode_number_range()
                    // A file without a number is the only episode of a
                    // single-episode anime: identify lets it through only
                    // then.
                    .map_or(number == 1, |(first, last)| {
                        (first..=last).contains(&number)
                    })
        });
        if matches {
            found = Some(path.to_owned());
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    found
}

/// Every episode of anime `id` found under `roots`, with the first file
/// found for each number. For Random, which picks among what is there.
pub fn find_episodes(
    recognizer: &Recognizer,
    roots: &[PathBuf],
    id: AnimeId,
) -> BTreeMap<u32, PathBuf> {
    let mut found = BTreeMap::new();
    walk_videos(roots, &mut |path| {
        if let Some(episode) = recognizer.file(path)
            && episode.anime_id == Some(id)
        {
            let (first, last) = episode.episode_number_range().unwrap_or((1, 1));
            for number in first..=last {
                found.entry(number).or_insert_with(|| path.to_owned());
            }
        }
        ControlFlow::Continue(())
    });
    found
}

/// The folder in a library folder whose name is recognized as anime `id`.
/// Shallower folders are tried first, so `Show` wins over `Show/Show OVA`.
pub fn find_anime_folder(recognizer: &Recognizer, id: AnimeId) -> Option<PathBuf> {
    let mut queue: VecDeque<(PathBuf, usize)> = recognizer
        .library_folders
        .iter()
        .map(|root| (root.clone(), 0))
        .collect();
    while let Some((folder, depth)) = queue.pop_front() {
        for (path, is_dir) in entries(&folder) {
            if !is_dir {
                continue;
            }
            if recognizer.folder(&path) == Some(id) {
                return Some(path);
            }
            if depth + 1 < MAX_ANIME_FOLDER_DEPTH {
                queue.push_back((path, depth + 1));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use shiori_core::recognition::{Relations, TitleIndex};
    use shiori_core::{Anime, ServiceId};

    use super::*;

    fn anime(id: u32, title: &str, episodes: u32) -> Anime {
        let mut anime = Anime::new(AnimeId(id));
        anime.titles.romaji = title.into();
        anime.episode_count = Some(episodes);
        anime
    }

    fn recognizer(root: &Path) -> Recognizer {
        let anime: std::collections::BTreeMap<_, _> = [
            anime(1, "Sousou no Frieren", 28),
            anime(2, "Bocchi the Rock!", 12),
            anime(3, "Kimi no Na wa.", 1),
        ]
        .into_iter()
        .map(|a| (a.id, a))
        .collect();
        Recognizer {
            index: TitleIndex::build(anime.values(), |_| None),
            anime,
            relations: Relations::parse("", ServiceId::AniList),
            numbering: Default::default(),
            library_folders: vec![root.to_owned()],
        }
    }

    fn touch(root: &Path, path: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    fn library() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(
            root,
            "Sousou no Frieren/[Group] Sousou no Frieren - 01 [1080p].mkv",
        );
        touch(
            root,
            "Sousou no Frieren/[Group] Sousou no Frieren - 01 [1080p].ass",
        );
        touch(
            root,
            "Sousou no Frieren/[Group] Sousou no Frieren - 02-03 [1080p].mkv",
        );
        touch(root, "Bocchi the Rock!/05.mp4");
        touch(root, "Kimi no Na wa. (2016) [BD 1080p].mkv");
        touch(root, ".cache/[Group] Sousou no Frieren - 04.mkv");
        dir
    }

    #[test]
    fn finds_episode_files() {
        let dir = library();
        let root = dir.path();
        let r = recognizer(root);
        let roots = [root.to_owned()];
        let find = |id, n| {
            find_episode(&r, &roots, AnimeId(id), n)
                // Forward slashes, so the expected paths hold on Windows too.
                .map(|p| {
                    p.strip_prefix(root)
                        .unwrap()
                        .display()
                        .to_string()
                        .replace('\\', "/")
                })
        };
        assert_eq!(
            find(1, 1).as_deref(),
            Some("Sousou no Frieren/[Group] Sousou no Frieren - 01 [1080p].mkv")
        );
        // A file with two episodes counts for both.
        assert_eq!(find(1, 2), find(1, 3));
        assert!(find(1, 2).unwrap().contains("02-03"));
        // Hidden folders are skipped.
        assert_eq!(find(1, 4), None);
        // The title comes from the folder when the file name has none.
        assert_eq!(find(2, 5).as_deref(), Some("Bocchi the Rock!/05.mp4"));
        // A film has no episode number.
        assert!(find(3, 1).is_some());
        assert_eq!(find(2, 6), None);
        let all: Vec<u32> = find_episodes(&r, &roots, AnimeId(1)).into_keys().collect();
        assert_eq!(all, [1, 2, 3]);
    }

    #[test]
    fn finds_anime_folders() {
        let dir = library();
        let root = dir.path();
        touch(root, "2024/Bocchi the Rock!/01.mkv");
        let r = recognizer(root);
        assert_eq!(
            find_anime_folder(&r, AnimeId(1)),
            Some(root.join("Sousou no Frieren"))
        );
        // The shallower folder wins.
        assert_eq!(
            find_anime_folder(&r, AnimeId(2)),
            Some(root.join("Bocchi the Rock!"))
        );
        fs::remove_dir_all(root.join("Bocchi the Rock!")).unwrap();
        assert_eq!(
            find_anime_folder(&r, AnimeId(2)),
            Some(root.join("2024/Bocchi the Rock!"))
        );
        assert_eq!(find_anime_folder(&r, AnimeId(3)), None);
    }

    #[test]
    fn scans_a_tree() {
        let dir = library();
        let tree = scan_folder(dir.path());
        let names: Vec<&str> = tree.children().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Bocchi the Rock!",
                "Sousou no Frieren",
                "Kimi no Na wa. (2016) [BD 1080p].mkv"
            ]
        );
        assert_eq!(tree.children()[1].children().len(), 3);
        assert_eq!(video_files(&[dir.path().to_owned()]).len(), 4);
    }
}
