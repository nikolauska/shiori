//! The library in the UI: playing episodes, opening anime folders, and the
//! scanned tree the Library page shows. The walking and recognizing runs on
//! the background executor with a `Recognizer` snapshot, so a large or slow
//! disk never stalls the window.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui::{AppContext as _, Context, Task};
use shiori_core::recognition::{normalize, title_to_remember};
use shiori_core::{AnimeId, Episode, ListStatus};
use shiori_library::{
    Node, Recognizer, can_start_rewatch, find_anime_folder, find_episode, find_episodes,
    last_episode, next_episode, random_episode_limit, scan_folder, video_files,
};

use super::format;
use super::model::Model;

/// How many files are recognized between updates of the Library page, so
/// that its columns fill in steadily without a redraw per file.
const RECOGNIZE_BATCH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayChoice {
    Next,
    Last,
    Random,
    Episode(u32),
}

#[derive(Default)]
pub struct Library {
    /// One tree per library folder, in the settings' order.
    pub roots: Vec<Node>,
    /// Video files recognized so far. Kept across scans so a rescan only
    /// parses new files; the page shows nothing for a file not in here yet.
    pub recognized: HashMap<PathBuf, Episode>,
    /// A scan or its recognition is running.
    pub scanning: bool,
    /// Whether the folders were scanned since the app started; the page
    /// scans on first view, not at start-up, to leave sleeping disks alone.
    pub scanned: bool,
    /// Grows with every change, so the page rebuilds its rows only then
    /// and not on every model notification.
    pub generation: u64,
    scan: Option<Task<()>>,
}

impl Model {
    fn recognizer(&self) -> Recognizer {
        let state = self.state();
        Recognizer {
            index: state.title_index.clone(),
            anime: state.anime.clone(),
            relations: self.tracker.relations.clone(),
            numbering: state.numbering.clone(),
            library_folders: state.settings.library_folders.clone(),
        }
    }

    /// Recognizes again the library files titled `title`, after the user
    /// linked that title to an anime. Only those can change, and they are
    /// few, so this runs at once instead of rescanning.
    pub(super) fn recognize_library_title(&mut self, title: &str, cx: &mut Context<Self>) {
        let key = normalize(title);
        let paths: Vec<PathBuf> = self
            .library
            .recognized
            .iter()
            .filter(|(_, e)| title_to_remember(e).is_some_and(|t| normalize(&t) == key))
            .map(|(path, _)| path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        let recognizer = self.recognizer();
        for path in paths {
            if let Some(episode) = recognizer.file(&path) {
                self.library.recognized.insert(path, episode);
            }
        }
        cx.notify();
    }

    /// Recognizes again the library files a numbering change can affect:
    /// those not recognized, and those recognized through a numbering.
    pub(super) fn recognize_library_again(&mut self, cx: &mut Context<Self>) {
        let numbering = &self.state().numbering;
        let paths: Vec<PathBuf> = self
            .library
            .recognized
            .iter()
            .filter(|(_, e)| e.anime_id.is_none_or(|id| numbering.of(id).is_some()))
            .map(|(path, _)| path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        let recognizer = self.recognizer();
        for path in paths {
            if let Some(episode) = recognizer.file(&path) {
                self.library.recognized.insert(path, episode);
            }
        }
        cx.notify();
    }

    /// The anime's own folder from its settings, when set, then the library
    /// folders: the folder the user picked is the likeliest place.
    fn episode_roots(&self, id: AnimeId) -> Vec<PathBuf> {
        let state = self.state();
        let own = state
            .anime_settings
            .get(&id)
            .map(|s| s.folder.trim())
            .filter(|f| !f.is_empty())
            .map(PathBuf::from);
        own.into_iter()
            .chain(state.settings.library_folders.iter().cloned())
            .collect()
    }

    pub fn play(&mut self, id: AnimeId, choice: PlayChoice, cx: &mut Context<Self>) {
        let state = self.state();
        let entry = state.entries.get(&id);
        let anime = state.anime.get(&id);
        match choice {
            PlayChoice::Next => self.play_episode(id, next_episode(entry, anime), cx),
            PlayChoice::Last => match last_episode(entry) {
                Some(number) => self.play_episode(id, number, cx),
                None => self.set_error(
                    format!("No episode of {} is watched yet.", state.title(id)),
                    cx,
                ),
            },
            PlayChoice::Episode(number) => self.play_episode(id, number, cx),
            PlayChoice::Random => match random_episode_limit(entry, anime) {
                Some(limit) => self.play_random_episode(id, limit, cx),
                None => self.set_error(
                    format!("No episode of {} to pick from.", state.title(id)),
                    cx,
                ),
            },
        }
    }

    /// Ctrl+N: the next episode of the anime in the inspector.
    pub fn play_selected_next(&mut self, cx: &mut Context<Self>) {
        match self.selected {
            Some(id) => self.play(id, PlayChoice::Next, cx),
            None => self.set_status("Select an anime to play its next episode.", cx),
        }
    }

    /// Plays episode `number` of anime `id` with the default program.
    pub fn play_episode(&mut self, id: AnimeId, number: u32, cx: &mut Context<Self>) {
        let roots = self.episode_roots(id);
        let title = self.state().title(id);
        if roots.is_empty() {
            self.no_folders(cx);
            return;
        }
        let recognizer = self.recognizer();
        self.set_status(format!("Looking for episode #{number} ({title})..."), cx);
        let search =
            cx.background_spawn(async move { find_episode(&recognizer, &roots, id, number) });
        cx.spawn(async move |this, cx| {
            let found = search.await;
            this.update(cx, |this, cx| match found {
                Some(path) => this.open_file(&path, cx),
                None => this.set_error(format!("Could not find episode #{number} ({title})."), cx),
            })
        })
        .detach();
    }

    /// A random episode up to `limit` among those in the library, so that
    /// the pick is something that can actually be played.
    fn play_random_episode(&mut self, id: AnimeId, limit: u32, cx: &mut Context<Self>) {
        let roots = self.episode_roots(id);
        let title = self.state().title(id);
        if roots.is_empty() {
            self.no_folders(cx);
            return;
        }
        let recognizer = self.recognizer();
        let search = cx.background_spawn(async move {
            let files: Vec<PathBuf> = find_episodes(&recognizer, &roots, id)
                .into_iter()
                .filter(|&(number, _)| number <= limit)
                .map(|(_, path)| path)
                .collect();
            (!files.is_empty()).then(|| files[fastrand::usize(..files.len())].clone())
        });
        cx.spawn(async move |this, cx| {
            let found = search.await;
            this.update(cx, |this, cx| match found {
                Some(path) => this.open_file(&path, cx),
                None => this.set_error(
                    format!("Could not find any episode up to #{limit} ({title})."),
                    cx,
                ),
            })
        })
        .detach();
    }

    /// "Play random anime": a Watching anime whose next episode is in the
    /// library, picked at random.
    pub fn play_random_anime(&mut self, cx: &mut Context<Self>) {
        let state = self.state();
        let watching: HashMap<AnimeId, u32> = state
            .entries
            .values()
            .filter(|e| e.status == ListStatus::Watching && !e.pending_delete)
            .map(|e| {
                (
                    e.anime_id,
                    next_episode(Some(e), state.anime.get(&e.anime_id)),
                )
            })
            .collect();
        if state.settings.library_folders.is_empty() {
            self.no_folders(cx);
            return;
        }
        let mut roots: Vec<PathBuf> = watching
            .keys()
            .flat_map(|&id| self.episode_roots(id))
            .collect();
        roots.sort();
        roots.dedup();
        let recognizer = self.recognizer();
        self.set_status("Looking for a next episode to play...", cx);
        let search = cx.background_spawn(async move {
            let mut next: HashMap<AnimeId, PathBuf> = HashMap::new();
            for path in video_files(&roots) {
                let Some(episode) = recognizer.file(&path) else {
                    continue;
                };
                let (Some(id), Some((first, last))) =
                    (episode.anime_id, episode.episode_number_range())
                else {
                    continue;
                };
                if let Some(&number) = watching.get(&id)
                    && (first..=last).contains(&number)
                {
                    next.entry(id).or_insert(path);
                }
            }
            let mut files: Vec<PathBuf> = next.into_values().collect();
            // HashMap order is random but not uniformly so; sorting first
            // leaves the pick to fastrand alone.
            files.sort();
            (!files.is_empty()).then(|| files[fastrand::usize(..files.len())].clone())
        });
        cx.spawn(async move |this, cx| {
            let found = search.await;
            this.update(cx, |this, cx| match found {
                Some(path) => this.open_file(&path, cx),
                None => this.set_error(
                    "No anime in Watching has its next episode in the library.",
                    cx,
                ),
            })
        })
        .detach();
    }

    /// "Start new rewatch": rewatching on, back to episode 0, Watching, then
    /// episode 1.
    pub fn start_rewatch(&mut self, id: AnimeId, cx: &mut Context<Self>) {
        let state = self.state();
        let today = format::today_in_japan();
        let entry = state.entries.get(&id);
        if !can_start_rewatch(entry, state.anime.get(&id), today) {
            return;
        }
        let Some(mut entry) = entry.cloned() else {
            return;
        };
        entry.rewatching = true;
        entry.watched_episodes = 0;
        entry.status = ListStatus::Watching;
        self.save_entry(entry, cx);
        self.play_episode(id, 1, cx);
    }

    /// Opens the anime's folder: the one in its settings, or a folder in the
    /// library recognized as it.
    pub fn open_anime_folder(&mut self, id: AnimeId, cx: &mut Context<Self>) {
        let title = self.state().title(id);
        let own = self
            .state()
            .anime_settings
            .get(&id)
            .map(|s| PathBuf::from(s.folder.trim()))
            .filter(|f| !f.as_os_str().is_empty() && f.is_dir());
        if let Some(folder) = own {
            cx.open_with_system(&folder);
            return;
        }
        let recognizer = self.recognizer();
        let search = cx.background_spawn(async move { find_anime_folder(&recognizer, id) });
        cx.spawn(async move |this, cx| {
            let found = search.await;
            this.update(cx, |this, cx| match found {
                Some(folder) => cx.open_with_system(&folder),
                None => this.set_error(format!("Could not find folder for {title}."), cx),
            })
        })
        .detach();
    }

    /// Opens a file with the default program. Playback is then detected
    /// like any other, so the usual update rules apply.
    pub fn open_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        cx.open_with_system(path);
        self.set_status(format!("Opened {name}."), cx);
    }

    fn no_folders(&mut self, cx: &mut Context<Self>) {
        self.set_error("No library folders yet. Add one on the Library page.", cx);
    }

    pub fn add_library_folders(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let mut settings = self.state().settings.clone();
        for path in paths {
            if !settings.library_folders.contains(&path) {
                settings.library_folders.push(path);
            }
        }
        self.apply_settings(settings, None, cx);
    }

    /// Scans on the Library page's first view.
    pub fn scan_library_once(&mut self, cx: &mut Context<Self>) {
        if !self.library.scanned {
            self.scan_library(cx);
        }
    }

    /// Reads the library folders again, then recognizes the video files
    /// that are new since the last scan, a batch at a time.
    pub fn scan_library(&mut self, cx: &mut Context<Self>) {
        let folders = self.state().settings.library_folders.clone();
        let recognizer = self.recognizer();
        self.library.scanned = true;
        self.library.scanning = true;
        self.library.generation += 1;
        cx.notify();
        let task = cx.spawn(async move |this, cx| {
            let roots = cx
                .background_spawn(async move {
                    folders.iter().map(|f| scan_folder(f)).collect::<Vec<_>>()
                })
                .await;
            let Ok(pending) = this.update(cx, |this, cx| {
                let library = &mut this.library;
                let mut files = Vec::new();
                for root in &roots {
                    collect_files(root, &mut files);
                }
                // Forget files that are gone, and recognize only new ones.
                let present: HashSet<&PathBuf> = files.iter().collect();
                library.recognized.retain(|path, _| present.contains(path));
                files.retain(|path| !library.recognized.contains_key(path));
                library.roots = roots;
                library.generation += 1;
                cx.notify();
                files
            }) else {
                return;
            };
            for batch in pending.chunks(RECOGNIZE_BATCH) {
                let batch = batch.to_vec();
                let recognizer = recognizer.clone();
                let results = cx
                    .background_spawn(async move {
                        batch
                            .into_iter()
                            .filter_map(|path| recognizer.file(&path).map(|e| (path, e)))
                            .collect::<Vec<_>>()
                    })
                    .await;
                let updated = this.update(cx, |this, cx| {
                    this.library.recognized.extend(results);
                    this.library.generation += 1;
                    cx.notify();
                });
                if updated.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.library.scanning = false;
                this.library.generation += 1;
                cx.notify();
            });
        });
        // Replacing the task drops a scan still running, so two scans never
        // race to fill the tree.
        self.library.scan = Some(task);
    }

    /// Renames a file or folder in place; `name` is the new last part.
    pub fn rename_path(&mut self, path: &Path, name: &str, cx: &mut Context<Self>) -> bool {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
            self.set_error("Enter a name without slashes.", cx);
            return false;
        }
        let target = path.with_file_name(name);
        if target == path {
            return true;
        }
        // rename() replaces an existing file on Unix without a word.
        if target.exists() {
            self.set_error(format!("{name} already exists."), cx);
            return false;
        }
        match std::fs::rename(path, &target) {
            Ok(()) => {
                // The new name is recognized afresh by the rescan; a folder's
                // old file paths drop out there too.
                self.library.recognized.remove(path);
                self.set_status(format!("Renamed to {name}."), cx);
                self.scan_library(cx);
                true
            }
            Err(error) => {
                self.set_error(format!("Could not rename: {error}"), cx);
                false
            }
        }
    }

    /// Moves a file or folder to the trash.
    pub fn delete_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let delete = cx.background_spawn(async move { trash::delete(&path) });
        cx.spawn(async move |this, cx| {
            let result = delete.await;
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.set_status(format!("Moved {name} to the trash."), cx);
                    this.scan_library(cx);
                }
                Err(error) => this.set_error(format!("Could not delete {name}: {error}"), cx),
            })
        })
        .detach();
    }
}

fn collect_files(node: &Node, files: &mut Vec<PathBuf>) {
    if node.is_folder() {
        for child in node.children() {
            collect_files(child, files);
        }
    } else {
        files.push(node.path.clone());
    }
}
