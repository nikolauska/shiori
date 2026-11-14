//! Moving a list in and out: MyAnimeList XML files, which MyAnimeList,
//! AniList and Taiga all read or write, and public AniList lists.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{AnyWindowHandle, App, Context};
use gpui_component::WindowExt as _;
use jiff::Timestamp;
use shiori_app::import::{self, Summary};
use shiori_app::mal_xml;
use shiori_core::entry::is_in_list;
use shiori_core::{Anime, AnimeId};
use shiori_sync::anilist::AniList;
use shiori_sync::{Account, Service as _};

use super::model::Model;

/// What `fetch_all` calls with every anime found.
type Found = Box<dyn FnOnce(&mut Model, Result<Vec<Anime>, String>, &mut Context<Model>)>;

/// Called with an import's outcome, to show it.
pub type Finished = Box<dyn FnOnce(Summary, &mut App)>;

#[derive(Debug, Clone, Copy)]
enum Lookup {
    AniList,
    MyAnimeList,
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// "A, B, C and 4 more", for telling which anime something happened to.
fn some_titles(titles: &[String]) -> String {
    const SHOWN: usize = 5;
    let mut text = titles
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if titles.len() > SHOWN {
        text.push_str(&format!(" and {} more", titles.len() - SHOWN));
    }
    text
}

impl Model {
    /// Looks up anime by AniList or MyAnimeList ids, 50 to a request and
    /// one request at a time, which keeps a large list within AniList's
    /// rate limit.
    fn fetch_all(
        &mut self,
        lookup: Lookup,
        mut ids: Vec<u32>,
        mut found: Vec<Anime>,
        then: Found,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            then(self, Ok(found), cx);
            return;
        }
        let rest = ids.split_off(ids.len().min(AniList::IDS_PER_REQUEST));
        let service = self.service().clone();
        self.request(
            async move {
                match lookup {
                    Lookup::AniList => {
                        let ids: Vec<AnimeId> = ids.into_iter().map(AnimeId).collect();
                        service.fetch_anime_by_ids(&ids).await
                    }
                    Lookup::MyAnimeList => service.fetch_anime_by_mal_ids(&ids).await,
                }
            },
            cx,
            move |this, result, cx| match result {
                Ok(anime) => {
                    found.extend(anime);
                    this.fetch_all(lookup, rest, found, then, cx);
                }
                Err(error) => then(this, Err(error.to_string()), cx),
            },
        );
    }

    /// Writes the list as a MyAnimeList XML file. Anime stored before
    /// Shiori kept MyAnimeList ids are looked up first.
    pub fn export_mal_xml(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let state = self.state();
        let missing: Vec<u32> = state
            .entries
            .values()
            .filter(|e| is_in_list(Some(e)))
            .filter(|e| {
                state
                    .anime
                    .get(&e.anime_id)
                    .is_none_or(|a| a.mal_id.is_none())
            })
            .map(|e| e.anime_id.0)
            .collect();
        if !missing.is_empty() {
            self.set_status("Looking up MyAnimeList ids...", cx);
        }
        let write =
            move |this: &mut Model, result: Result<Vec<Anime>, String>, cx: &mut Context<Model>| {
                match result {
                    Ok(anime) => {
                        let result = this.startup.state.store_anime(anime);
                        this.apply(result, cx);
                    }
                    Err(error) => {
                        this.set_error(format!("Could not look up MyAnimeList ids: {error}"), cx);
                        return;
                    }
                }
                let (xml, left_out) = mal_xml::write(this.state());
                if let Err(error) = std::fs::write(&path, xml) {
                    tracing::error!("Could not export to {}: {error}", path.display());
                    this.set_error(format!("Could not export to {}.", path.display()), cx);
                    return;
                }
                let mut text = format!("Exported your list to {}.", path.display());
                if !left_out.is_empty() {
                    let titles: Vec<String> =
                        left_out.iter().map(|&id| this.state().title(id)).collect();
                    text.push_str(&format!(
                        " Left out, with no MyAnimeList id: {}.",
                        some_titles(&titles)
                    ));
                }
                this.set_status(text, cx);
            };
        self.fetch_all(Lookup::AniList, missing, Vec::new(), Box::new(write), cx);
    }

    /// Adds the anime in a MyAnimeList XML file to a local list.
    pub fn import_mal_xml(&mut self, path: PathBuf, finished: Finished, cx: &mut Context<Self>) {
        if !self.state().is_local() {
            return;
        }
        let name = file_name(&path);
        let entries = match std::fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| mal_xml::parse(&bytes))
        {
            Ok(entries) => entries,
            Err(error) => {
                self.set_error(format!("Could not read {name}: {error:#}"), cx);
                return;
            }
        };
        if entries.is_empty() {
            self.set_status(format!("There are no anime in {name}."), cx);
            return;
        }
        self.set_status(format!("Importing {} anime...", entries.len()), cx);
        let mut ids: Vec<u32> = entries.iter().map(|e| e.mal_id).collect();
        ids.sort_unstable();
        ids.dedup();
        let add =
            move |this: &mut Model, result: Result<Vec<Anime>, String>, cx: &mut Context<Model>| {
                let anime = match result {
                    Ok(anime) => anime,
                    Err(error) => {
                        this.set_error(format!("Could not import {name}: {error}"), cx);
                        return;
                    }
                };
                let by_mal: HashMap<u32, AnimeId> = anime
                    .iter()
                    .filter_map(|a| Some((a.mal_id?, a.id)))
                    .collect();
                let now = Timestamp::now();
                let mut summary = Summary::default();
                let mut list = Vec::new();
                for entry in &entries {
                    match by_mal.get(&entry.mal_id) {
                        Some(&id) => list.push(entry.to_entry(id, now)),
                        None => summary.not_found.push(entry.title.clone()),
                    }
                }
                let result =
                    import::add_missing(&mut this.startup.state, anime, list, now, &mut summary);
                this.apply(result, cx);
                this.set_status(format!("Imported {} anime from {name}.", summary.added), cx);
                finished(summary, cx);
            };
        self.fetch_all(Lookup::MyAnimeList, ids, Vec::new(), Box::new(add), cx);
    }

    /// Adds the anime in someone's public AniList list to a local list. No
    /// sign-in is needed, so nothing is ever sent to that account.
    pub fn copy_public_list(
        &mut self,
        username: String,
        finished: Finished,
        cx: &mut Context<Self>,
    ) {
        if !self.state().is_local() || username.is_empty() {
            return;
        }
        self.set_status(format!("Reading {username}'s list..."), cx);
        let service = self.service().clone();
        let account = Account {
            username: username.clone(),
            token: None,
        };
        self.request(
            async move {
                let progress = |_: u64, _: u64| {};
                service.fetch_list(&account, &progress).await
            },
            cx,
            move |this, result, cx| match result {
                Ok(list) => {
                    let mut summary = Summary::default();
                    let result = import::add_missing(
                        &mut this.startup.state,
                        list.anime,
                        list.entries,
                        Timestamp::now(),
                        &mut summary,
                    );
                    this.apply(result, cx);
                    this.set_status(
                        format!("Copied {} anime from {username}'s list.", summary.added),
                        cx,
                    );
                    finished(summary, cx);
                }
                Err(error) => {
                    this.set_error(format!("Could not read {username}'s list: {error}"), cx)
                }
            },
        );
    }
}

/// Shows an import's outcome in a dialog on `window`.
pub fn show_summary(window: AnyWindowHandle) -> Finished {
    Box::new(move |summary, cx| {
        let mut text = match summary.added {
            1 => "Added 1 anime.".to_owned(),
            n => format!("Added {n} anime."),
        };
        if summary.already > 0 {
            text.push_str(&format!(
                " {} were already in your list and were left as they were.",
                summary.already
            ));
        }
        if !summary.not_found.is_empty() {
            text.push_str(&format!(
                "\n\nNot found on AniList: {}.",
                some_titles(&summary.not_found)
            ));
        }
        let _ = window.update(cx, move |_, window, cx| {
            window.open_alert_dialog(cx, move |alert, _, _| {
                alert.title("Import finished").description(text.clone())
            });
        });
    })
}
