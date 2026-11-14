//! Episode numbering in the model: AniList relations, following a series
//! from season to season, the suggestion for the playing media, and saving
//! or forgetting a remembered numbering (ui.md, "Episode numbering").

use std::collections::HashMap;

use gpui::Context;
use shiori_core::recognition::{
    Chain, ChainStep, NumberMiss, NumberingSequence, Proposal, number_miss, propose, series_chain,
};
use shiori_core::{AnimeId, AnimeType, Element, Relation};

use super::model::Model;

/// AniList requests one suggestion may make. A long franchise is cut off
/// rather than flooding the service; the editor can load more.
const MAX_FETCHES: usize = 12;

/// What `follow_series` calls with the seasons it found.
type ChainDone = Box<dyn FnOnce(&mut Model, Result<Chain, String>, &mut Context<Model>)>;

/// The suggestion for media whose episode number is above its anime's count.
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// The media it is for, as parsed. A result for other media is dropped.
    pub key: Vec<Element>,
    pub miss: NumberMiss,
    pub state: SuggestionState,
}

#[derive(Debug, Clone)]
pub enum SuggestionState {
    Loading,
    Ready(Proposal),
    /// The relations could not be loaded; the reason is in the log.
    Failed,
}

/// Seasons are TV and web series; movies, OVAs and specials are numbered
/// apart, unless the user includes one on purpose.
pub fn is_series(model: &Model, id: AnimeId) -> bool {
    model
        .state()
        .anime
        .get(&id)
        .is_some_and(|a| matches!(a.kind, AnimeType::Tv | AnimeType::Ona))
}

impl Model {
    /// Loads the anime related to `id` and stores them, so their titles and
    /// counts are known.
    pub fn fetch_relations(
        &mut self,
        id: AnimeId,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<(), String>, &mut Context<Self>) + 'static,
    ) {
        let service = self.service().clone();
        self.request(
            async move { service.fetch_relations(id).await },
            cx,
            move |this, result, cx| match result {
                Ok(related) => {
                    let relations = related
                        .iter()
                        .map(|(kind, anime)| Relation {
                            kind: *kind,
                            anime: anime.id,
                        })
                        .collect();
                    let anime = related.into_iter().map(|(_, anime)| anime).collect();
                    let result = this.startup.state.store_anime(anime);
                    this.apply(result, cx);
                    this.relations.insert(id, relations);
                    then(this, Ok(()), cx);
                }
                Err(error) => then(this, Err(error.to_string()), cx),
            },
        );
    }

    /// The seasons `start` belongs to, fetching the relations still missing.
    pub fn follow_series(
        &mut self,
        start: AnimeId,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<Chain, String>, &mut Context<Self>) + 'static,
    ) {
        self.follow_series_step(start, 0, Box::new(then), cx);
    }

    fn follow_series_step(
        &mut self,
        start: AnimeId,
        fetched: usize,
        then: ChainDone,
        cx: &mut Context<Self>,
    ) {
        let step = series_chain(start, &self.relations, |id| is_series(self, id));
        match step {
            ChainStep::Done(chain) => then(self, Ok(chain), cx),
            ChainStep::NeedRelations(_) if fetched >= MAX_FETCHES => then(
                self,
                Err("The series has more seasons than Shiori follows at once.".into()),
                cx,
            ),
            ChainStep::NeedRelations(id) => {
                self.fetch_relations(id, cx, move |this, result, cx| match result {
                    Ok(()) => this.follow_series_step(start, fetched + 1, then, cx),
                    Err(error) => then(this, Err(error), cx),
                });
            }
        }
    }

    /// Starts or drops the suggestion for the playing media, after it was
    /// identified. Called on every poll; only new media starts a request.
    pub(super) fn update_suggestion(&mut self, cx: &mut Context<Self>) {
        let Some(media) = &self.tracker.media else {
            self.tracker.suggestion = None;
            return;
        };
        if media.episode.anime_id.is_some() {
            self.tracker.suggestion = None;
            return;
        }
        let key = media.parsed.elements.clone();
        if self
            .tracker
            .suggestion
            .as_ref()
            .is_some_and(|s| s.key == key)
            || self.tracker.dismissed.contains(&key)
        {
            return;
        }
        let state = self.state();
        let Some(miss) = number_miss(&media.parsed, &state.title_index, &state.anime) else {
            self.tracker.suggestion = None;
            return;
        };
        // An anime in a remembered numbering already had its say: a number
        // in its gaps is no match, and suggesting another numbering for it
        // would compete with the user's.
        if state.numbering.of(miss.anime).is_some() {
            self.tracker.suggestion = None;
            return;
        }
        self.tracker.suggestion = Some(Suggestion {
            key: key.clone(),
            miss: miss.clone(),
            state: SuggestionState::Loading,
        });
        cx.notify();
        self.follow_series(miss.anime, cx, move |this, chain, cx| {
            let Some(suggestion) = this.tracker.suggestion.as_mut().filter(|s| s.key == key) else {
                // The media changed meanwhile; this answer is for nothing.
                return;
            };
            suggestion.state = match chain {
                Ok(chain) => {
                    let anime = &this.startup.state.anime;
                    SuggestionState::Ready(propose(&miss, &chain, |id| {
                        anime.get(&id).and_then(|a| a.episode_count)
                    }))
                }
                Err(error) => {
                    tracing::warn!("Could not check related seasons: {error}");
                    SuggestionState::Failed
                }
            };
            cx.notify();
        });
    }

    /// Hides the suggestion for this media until it changes. Nothing is
    /// saved.
    pub fn dismiss_suggestion(&mut self, cx: &mut Context<Self>) {
        if let Some(suggestion) = self.tracker.suggestion.take() {
            self.tracker.dismissed.push(suggestion.key);
        }
        cx.notify();
    }

    /// Saves a numbering and recognizes again what it can change. Returns
    /// the error to show when it could not be saved.
    pub fn save_numbering(
        &mut self,
        sequence: NumberingSequence,
        cx: &mut Context<Self>,
    ) -> Result<NumberingSequence, String> {
        match self.startup.state.save_numbering(sequence) {
            Ok((saved, events)) => {
                self.emit(events, cx);
                let root = self.state().title(saved.root);
                self.set_status(format!("Remembered episode numbering for {root}."), cx);
                self.numbering_changed(cx);
                Ok(saved)
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn forget_numbering(&mut self, id: i64, cx: &mut Context<Self>) {
        let root = self
            .state()
            .numbering
            .sequences()
            .iter()
            .find(|s| s.id == id)
            .map(|s| self.state().title(s.root));
        let result = self.startup.state.forget_numbering(id);
        self.apply(result, cx);
        if let Some(root) = root {
            self.set_status(format!("Forgot the episode numbering for {root}."), cx);
        }
        self.numbering_changed(cx);
    }

    /// The playing media and the library's unrecognized and affected files
    /// may resolve differently now. Any pending countdown restarts for what
    /// the media is now.
    fn numbering_changed(&mut self, cx: &mut Context<Self>) {
        self.tracker.suggestion = None;
        if let Some(media) = self.tracker.media.clone() {
            self.media_found(Some(media), cx);
        }
        self.recognize_library_again(cx);
    }

    /// Uses a numbering for the playing media only: its episode is the one
    /// `sequence` gives, until the media changes.
    pub fn use_numbering_once(&mut self, sequence: &NumberingSequence, cx: &mut Context<Self>) {
        let Some(suggestion) = &self.tracker.suggestion else {
            return;
        };
        let Some(resolved) = sequence.resolve(suggestion.miss.range) else {
            return;
        };
        let from = suggestion.miss.anime;
        self.apply_numbering_once(from, resolved, cx);
    }

    /// The anime's relations, if fetched in this run.
    pub fn relations_of(&self, id: AnimeId) -> Option<&Vec<Relation>> {
        self.relations.get(&id)
    }

    /// Every anime related to any of `ids` that is known, with the first
    /// relation found, for the editor's list of candidates.
    pub fn related_to(&self, ids: &[AnimeId]) -> HashMap<AnimeId, Relation> {
        let mut found = HashMap::new();
        for id in ids {
            for r in self.relations.get(id).into_iter().flatten() {
                found.entry(r.anime).or_insert(*r);
            }
        }
        found
    }
}
