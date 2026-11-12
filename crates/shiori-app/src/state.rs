//! `AppState`, the single owner of the in-memory data.
//!
//! Every change goes through a method here. The method updates memory,
//! writes to the database, and returns what happened as `AppEvent`s for the
//! UI to react to.

use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use jiff::Timestamp;
use shiori_core::anime::preferred_title;
use shiori_core::entry::changed_fields;
use shiori_core::recognition::TitleIndex;
use shiori_core::{Anime, AnimeId, AnimeSettings, HistoryItem, ListEntry, ServiceId};
use shiori_store::{Accounts, Database, Settings};
use shiori_sync::queue::{Reconcile, Step};
use shiori_sync::sync::entries_to_prune;
use shiori_sync::{AccountInfo, FetchedList, SavedEntry, ServiceError, SyncQueue};

#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    AnimeUpdated(AnimeId),
    AnimeDeleted { id: AnimeId, title: String },
    EntryUpdated(AnimeId),
    EntryDeleted(AnimeId),
    QueueChanged,
    HistoryChanged,
    AnimeSettingsUpdated(AnimeId),
    QueueProcessing(AnimeId),
    QueuedWhileSignedOut(AnimeId),
    AuthenticationChanged(bool),
    ListFetched,
    ServiceError(String),
}

#[derive(Debug)]
pub struct AppState {
    pub service: ServiceId,
    db: Database,
    pub anime: BTreeMap<AnimeId, Anime>,
    pub entries: BTreeMap<AnimeId, ListEntry>,
    pub anime_settings: BTreeMap<AnimeId, AnimeSettings>,
    pub history: Vec<HistoryItem>,
    pub queue: SyncQueue,
    pub title_index: TitleIndex,
    pub settings: Settings,
    pub accounts: Accounts,
    /// Set by a successful sign-in in this run; never stored.
    pub signed_in: bool,
}

impl AppState {
    pub fn new(db: Database, settings: Settings, accounts: Accounts) -> Result<Self> {
        let loaded = db.load_all()?;
        let title_index =
            TitleIndex::build(loaded.anime.values(), |id| loaded.anime_settings.get(&id));
        Ok(Self {
            service: settings.service,
            db,
            anime: loaded.anime,
            entries: loaded.entries,
            anime_settings: loaded.anime_settings,
            history: loaded.history,
            queue: SyncQueue::new(loaded.queue),
            title_index,
            settings,
            accounts,
            signed_in: false,
        })
    }

    /// The title to show, or the id when the anime is not known yet.
    pub fn title(&self, id: AnimeId) -> String {
        match self.anime.get(&id) {
            Some(anime) => preferred_title(
                anime,
                self.anime_settings.get(&id),
                self.settings.title_language,
            )
            .to_owned(),
            None => format!("#{id}"),
        }
    }

    /// Stores an edited entry and queues the changed fields.
    pub fn save_entry(&mut self, mut entry: ListEntry, now: Timestamp) -> Result<Vec<AppEvent>> {
        let id = entry.anime_id;
        // Entries are shown and recognized through their anime, and the
        // database enforces it too, so the anime must be stored first.
        if !self.anime.contains_key(&id) {
            anyhow::bail!("anime {id} is not in the database");
        }
        let before = self.entries.get(&id);
        // Adding back an entry whose delete is still queued: the service may
        // already have deleted it, so every field has to be sent again.
        let dirty = match before {
            Some(old) if old.pending_delete => changed_fields(None, &entry),
            old => changed_fields(old, &entry),
        };
        let was_pending_delete = before.is_some_and(|e| e.pending_delete);
        entry.pending_delete = false;
        if dirty.is_empty() && !was_pending_delete && before == Some(&entry) {
            return Ok(Vec::new());
        }
        entry.last_updated = now;
        self.db.upsert_entries(std::slice::from_ref(&entry))?;
        self.entries.insert(id, entry);
        let mut events = vec![AppEvent::EntryUpdated(id)];
        if !dirty.is_empty() {
            let item = self.queue.push(id, dirty, now).clone();
            self.db.upsert_queue_item(&item)?;
            events.extend(self.queued_events(id));
        }
        Ok(events)
    }

    /// Marks an entry for deletion; it disappears locally once the service
    /// confirms.
    pub fn remove_entry(&mut self, id: AnimeId, now: Timestamp) -> Result<Vec<AppEvent>> {
        let Some(entry) = self.entries.get_mut(&id) else {
            return Ok(Vec::new());
        };
        entry.pending_delete = true;
        self.db.upsert_entries(std::slice::from_ref(entry))?;
        let item = self.queue.push_delete(id, now).clone();
        self.db.upsert_queue_item(&item)?;
        let mut events = vec![AppEvent::EntryUpdated(id)];
        events.extend(self.queued_events(id));
        Ok(events)
    }

    fn queued_events(&self, id: AnimeId) -> Vec<AppEvent> {
        let mut events = vec![AppEvent::QueueChanged];
        if !self.signed_in {
            events.push(AppEvent::QueuedWhileSignedOut(id));
        }
        events
    }

    pub fn store_anime(&mut self, items: Vec<Anime>) -> Result<Vec<AppEvent>> {
        self.db.upsert_anime(&items)?;
        let mut events = Vec::with_capacity(items.len());
        for anime in items {
            self.title_index
                .update(&anime, self.anime_settings.get(&anime.id));
            events.push(AppEvent::AnimeUpdated(anime.id));
            self.anime.insert(anime.id, anime);
        }
        Ok(events)
    }

    pub fn apply_sign_in(&mut self, info: &AccountInfo) -> Vec<AppEvent> {
        self.signed_in = true;
        match self.service {
            ServiceId::AniList => {
                self.accounts.anilist.username = info.username.clone();
                self.accounts.anilist.rating_system = info.rating_system.clone();
            }
            ServiceId::MyAnimeList => self.accounts.myanimelist.username = info.username.clone(),
            ServiceId::Kitsu => {
                self.accounts.kitsu.username = info.username.clone();
                self.accounts.kitsu.user_id = info.user_id.clone();
                self.accounts.kitsu.rating_system = info.rating_system.clone();
            }
        }
        vec![AppEvent::AuthenticationChanged(true)]
    }

    /// Any service error. A rejected token also signs the user out, so the
    /// next sync asks for a new one instead of failing the same way.
    pub fn apply_service_error(&mut self, error: &ServiceError) -> Vec<AppEvent> {
        tracing::error!("[{}] {error}", self.service.name());
        let mut events = vec![AppEvent::ServiceError(error.to_string())];
        if matches!(error, ServiceError::Unauthorized(_)) && self.signed_in {
            self.signed_in = false;
            events.push(AppEvent::AuthenticationChanged(false));
        }
        events
    }

    /// Stores a fetched list and prunes entries the service no longer has.
    pub fn apply_list(&mut self, list: FetchedList) -> Result<Vec<AppEvent>> {
        let mut events = self.store_anime(list.anime)?;
        let fetched: HashSet<AnimeId> = list.entries.iter().map(|e| e.anime_id).collect();
        // A queued entry has local edits the service has not seen yet;
        // replacing it with the fetched version would lose them.
        let entries: Vec<ListEntry> = list
            .entries
            .into_iter()
            .filter(|e| !self.queue.contains(e.anime_id))
            .collect();
        self.db.upsert_entries(&entries)?;
        for entry in entries {
            events.push(AppEvent::EntryUpdated(entry.anime_id));
            self.entries.insert(entry.anime_id, entry);
        }
        let pruned = entries_to_prune(self.entries.keys().copied(), &fetched, &self.queue);
        for id in pruned {
            self.db.delete_entry(id)?;
            self.entries.remove(&id);
            events.push(AppEvent::EntryDeleted(id));
        }
        events.push(AppEvent::ListFetched);
        Ok(events)
    }

    /// The service says the anime no longer exists.
    pub fn invalidate_anime(&mut self, id: AnimeId) -> Result<Vec<AppEvent>> {
        // A queued edit means the user still cares about it; keep it until
        // the queue has dealt with it.
        if self.queue.contains(id) || !self.anime.contains_key(&id) {
            return Ok(Vec::new());
        }
        let title = self.title(id);
        self.db.delete_anime(id)?;
        self.anime.remove(&id);
        self.title_index.remove(id);
        let mut events = Vec::new();
        if self.entries.remove(&id).is_some() {
            events.push(AppEvent::EntryDeleted(id));
        }
        tracing::info!("Anime removed from database: {title}");
        events.push(AppEvent::AnimeDeleted { id, title });
        Ok(events)
    }

    /// The next queue step; items whose entry is gone are dropped from the
    /// database as well.
    pub fn queue_next(&mut self) -> Result<(Step, Vec<AppEvent>)> {
        let next = self.queue.next(&self.entries, self.signed_in);
        let mut events = Vec::new();
        for id in &next.dropped {
            self.db.delete_queue_item(*id)?;
        }
        if !next.dropped.is_empty() {
            events.push(AppEvent::QueueChanged);
        }
        if let Step::Send(request) = &next.step {
            events.push(AppEvent::QueueProcessing(request.anime_id()));
        }
        Ok((next.step, events))
    }

    /// Applies the answer to the request in flight.
    pub fn queue_finished(
        &mut self,
        answer: Result<Option<SavedEntry>, ServiceError>,
    ) -> Result<Vec<AppEvent>> {
        let id = self.queue_id()?;
        let (answer, anime) = match answer {
            Ok(Some(saved)) => (Ok(Some(saved.entry)), saved.anime),
            Ok(None) => (Ok(None), None),
            Err(error) => (Err(error), None),
        };
        let mut out = self.store_anime(anime.into_iter().collect())?;
        let error = answer.as_ref().err().cloned();
        let finished = self
            .queue
            .finish(answer.map_err(|e| e.to_string()), self.entries.get(&id));
        match finished.reconcile {
            Reconcile::Nothing => {}
            Reconcile::DeleteLocal => {
                self.db.delete_entry(id)?;
                self.entries.remove(&id);
                out.push(AppEvent::EntryDeleted(id));
            }
            Reconcile::ClearServiceId => self.update_local(id, &mut out, |e| e.id = None)?,
            Reconcile::CopyServiceId(service_id) => {
                self.update_local(id, &mut out, |e| e.id = Some(service_id))?
            }
            Reconcile::Store(entry) => {
                self.db.upsert_entries(std::slice::from_ref(&entry))?;
                self.entries.insert(id, entry);
                out.push(AppEvent::EntryUpdated(id));
            }
        }
        match &finished.item {
            Some(item) => self.db.upsert_queue_item(item)?,
            None => self.db.delete_queue_item(id)?,
        }
        out.push(AppEvent::QueueChanged);
        if let Some(error) = error {
            out.extend(self.apply_service_error(&error));
        }
        Ok(out)
    }

    /// Removes one history item. The list entry is left as it is.
    pub fn remove_history(&mut self, id: i64) -> Result<Vec<AppEvent>> {
        self.db.delete_history(id)?;
        self.history.retain(|item| item.id != id);
        Ok(vec![AppEvent::HistoryChanged])
    }

    pub fn clear_history(&mut self) -> Result<Vec<AppEvent>> {
        self.db.clear_history()?;
        self.history.clear();
        Ok(vec![AppEvent::HistoryChanged])
    }

    /// Drops every unsent change. Local entries keep their edited values
    /// until the next list fetch replaces them with the service's.
    pub fn clear_queue(&mut self) -> Result<Vec<AppEvent>> {
        // The answer to a request in flight would find no item to finish.
        if self.queue.in_flight().is_some() {
            anyhow::bail!("the queue is being sent");
        }
        self.db.clear_queue()?;
        self.queue.clear();
        Ok(vec![AppEvent::QueueChanged])
    }

    /// Stores per-anime settings and updates the title index, since the
    /// user's synonyms are used for recognition.
    pub fn save_anime_settings(&mut self, settings: AnimeSettings) -> Result<Vec<AppEvent>> {
        let id = settings.id;
        if self.anime_settings.get(&id) == Some(&settings) {
            return Ok(Vec::new());
        }
        self.db.upsert_anime_settings(&settings)?;
        self.anime_settings.insert(id, settings);
        if let Some(anime) = self.anime.get(&id) {
            self.title_index.update(anime, self.anime_settings.get(&id));
        }
        Ok(vec![AppEvent::AnimeSettingsUpdated(id)])
    }

    /// What is in the database, to check that changes were written through.
    #[cfg(test)]
    pub fn stored(&self) -> shiori_store::Loaded {
        self.db.load_all().unwrap()
    }

    fn queue_id(&self) -> Result<AnimeId> {
        self.queue
            .in_flight()
            .ok_or_else(|| anyhow::anyhow!("no queue request in flight"))
    }

    fn update_local(
        &mut self,
        id: AnimeId,
        events: &mut Vec<AppEvent>,
        change: impl FnOnce(&mut ListEntry),
    ) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(&id) {
            change(entry);
            self.db.upsert_entries(std::slice::from_ref(entry))?;
            events.push(AppEvent::EntryUpdated(id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use shiori_store::{Accounts, Database, Settings};

    use super::*;

    fn state() -> AppState {
        AppState::new(
            Database::open_in_memory().unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap()
    }

    #[test]
    fn history_items_are_removed_and_cleared() {
        let mut state = state();
        state.store_anime(vec![Anime::new(AnimeId(1))]).unwrap();
        for episode in 1..=3 {
            let id = state
                .db
                .add_history(AnimeId(1), episode, Timestamp::UNIX_EPOCH)
                .unwrap();
            state.history.push(HistoryItem {
                id,
                anime_id: AnimeId(1),
                episode,
                time: Timestamp::UNIX_EPOCH,
            });
        }
        let second = state.history[1].id;
        state.remove_history(second).unwrap();
        let episodes: Vec<u32> = state.stored().history.iter().map(|h| h.episode).collect();
        assert_eq!(episodes, [1, 3]);
        assert_eq!(state.history.len(), 2);
        state.clear_history().unwrap();
        assert!(state.history.is_empty());
        assert!(state.stored().history.is_empty());
    }

    #[test]
    fn clearing_the_queue_keeps_the_entry() {
        let mut state = state();
        state.store_anime(vec![Anime::new(AnimeId(1))]).unwrap();
        let mut entry = ListEntry::new(AnimeId(1));
        entry.watched_episodes = 2;
        state.save_entry(entry, Timestamp::UNIX_EPOCH).unwrap();
        assert!(state.queue.contains(AnimeId(1)));
        state.clear_queue().unwrap();
        assert!(state.queue.is_empty());
        assert!(state.stored().queue.is_empty());
        assert_eq!(state.entries[&AnimeId(1)].watched_episodes, 2);
    }

    #[test]
    fn anime_settings_update_recognition() {
        let mut state = state();
        let mut anime = Anime::new(AnimeId(1));
        anime.titles.romaji = "Sousou no Frieren".into();
        state.store_anime(vec![anime]).unwrap();
        let settings = AnimeSettings {
            id: AnimeId(1),
            display_title: "Frieren".into(),
            synonyms: vec!["Frieren Beyond".into()],
            ..AnimeSettings::default()
        };
        let events = state.save_anime_settings(settings.clone()).unwrap();
        assert_eq!(events, [AppEvent::AnimeSettingsUpdated(AnimeId(1))]);
        assert_eq!(state.title(AnimeId(1)), "Frieren");
        assert_eq!(state.title_index.lookup("Frieren Beyond")[0].0, AnimeId(1));
        assert_eq!(state.stored().anime_settings[&AnimeId(1)], settings);
        // Saving the same settings again changes nothing.
        assert!(state.save_anime_settings(settings).unwrap().is_empty());
    }
}
