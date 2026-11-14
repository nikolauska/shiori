//! The app's state and the steps that move data between the crates. The
//! binary's command-line mode and the UI both drive these.

pub mod backup;
pub mod detection;
pub mod export;
pub mod home;
pub mod import;
pub mod list_view;
pub mod mal_xml;
pub mod posters;
pub mod refresh;
pub mod single_instance;
pub mod startup;
pub mod state;
pub mod sync;
pub mod tracking;
pub mod upload;
