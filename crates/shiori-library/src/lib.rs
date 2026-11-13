//! Library folders: walking them, recognizing the files in them, finding
//! the file for an episode and the folder of an anime, and which episode
//! the Play choices mean. See docs/domain/library.md and the library parts
//! of docs/ui.md.
//!
//! Nothing here touches `AppState`: the app hands over a [`Recognizer`]
//! snapshot so that the walking and parsing can run off the UI thread.

mod play;
mod recognizer;
mod scan;

pub use play::{can_start_rewatch, last_episode, next_episode, random_episode_limit};
pub use recognizer::Recognizer;
pub use scan::{
    FileInfo, Node, NodeKind, find_anime_folder, find_episode, find_episodes, scan_folder,
    video_files,
};
