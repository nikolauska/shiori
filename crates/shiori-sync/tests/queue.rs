//! The queue scenarios from the sync spec, against a fake service.

use std::collections::BTreeMap;
use std::sync::Mutex;

use jiff::Timestamp;
use shiori_core::{
    Anime, AnimeId, Fields, ListEntry, ListEntryId, ListStatus, SearchParams, ServiceId,
};
use shiori_sync::queue::{Reconcile, Request, Step};
use shiori_sync::sync::should_auto_process;
use shiori_sync::{
    Account, AccountInfo, FetchedList, PageToken, SavedEntry, SearchPage, Service, ServiceError,
    SyncQueue,
};

/// Keeps entries like a service would and records every write it receives.
#[derive(Default)]
struct FakeService {
    entries: Mutex<BTreeMap<AnimeId, ListEntry>>,
    log: Mutex<Vec<String>>,
    fail_next: Mutex<Option<ServiceError>>,
    next_id: Mutex<i64>,
}

impl FakeService {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn has(&self, id: AnimeId) -> bool {
        self.entries.lock().unwrap().contains_key(&id)
    }

    fn save(
        &self,
        what: &str,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("{what} {} {:?}", entry.anime_id, dirty));
        if let Some(error) = self.fail_next.lock().unwrap().take() {
            return Err(error);
        }
        let mut entries = self.entries.lock().unwrap();
        let stored = entries.entry(entry.anime_id).or_insert_with(|| {
            let mut next = self.next_id.lock().unwrap();
            *next += 1;
            let mut fresh = ListEntry::new(entry.anime_id);
            fresh.id = Some(ListEntryId(*next));
            fresh
        });
        if dirty.contains(Fields::EPISODE) {
            stored.watched_episodes = entry.watched_episodes;
        }
        if dirty.contains(Fields::SCORE) {
            stored.score = entry.score;
        }
        if dirty.contains(Fields::STATUS) {
            stored.status = entry.status;
        }
        if dirty.contains(Fields::NOTES) {
            stored.notes = entry.notes.clone();
        }
        Ok(Some(SavedEntry {
            entry: stored.clone(),
            anime: None,
        }))
    }
}

impl Service for FakeService {
    fn id(&self) -> ServiceId {
        ServiceId::AniList
    }

    async fn authenticate(&self, _: &Account) -> Result<AccountInfo, ServiceError> {
        Ok(AccountInfo::default())
    }

    async fn fetch_anime(&self, _: AnimeId) -> Result<Anime, ServiceError> {
        Err(ServiceError::NotFound)
    }

    async fn fetch_list(
        &self,
        _: &Account,
        _: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<FetchedList, ServiceError> {
        Ok(FetchedList {
            anime: Vec::new(),
            entries: self.entries.lock().unwrap().values().cloned().collect(),
        })
    }

    async fn search(
        &self,
        _: &SearchParams,
        _: Option<PageToken>,
    ) -> Result<SearchPage, ServiceError> {
        Ok(SearchPage::default())
    }

    async fn add_entry(
        &self,
        _: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        self.save("add", entry, dirty)
    }

    async fn update_entry(
        &self,
        _: &Account,
        entry: &ListEntry,
        dirty: Fields,
    ) -> Result<Option<SavedEntry>, ServiceError> {
        self.save("update", entry, dirty)
    }

    async fn delete_entry(&self, _: &Account, entry: &ListEntry) -> Result<(), ServiceError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("delete {}", entry.anime_id));
        // Like AniList: deleting what is not there succeeds.
        self.entries.lock().unwrap().remove(&entry.anime_id);
        Ok(())
    }
}

/// The parts of the app that matter here: local entries and the queue.
struct App {
    entries: BTreeMap<AnimeId, ListEntry>,
    queue: SyncQueue,
    service: FakeService,
    account: Account,
    clock: i64,
}

impl App {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            queue: SyncQueue::default(),
            service: FakeService::default(),
            account: Account::default(),
            clock: 0,
        }
    }

    fn now(&mut self) -> Timestamp {
        self.clock += 1;
        Timestamp::from_second(self.clock).unwrap()
    }

    fn edit(&mut self, id: AnimeId, change: impl FnOnce(&mut ListEntry)) {
        let before = self.entries.get(&id).cloned();
        let mut after = before.clone().unwrap_or_else(|| ListEntry::new(id));
        after.pending_delete = false;
        change(&mut after);
        let dirty = shiori_core::entry::changed_fields(before.as_ref(), &after);
        self.entries.insert(id, after);
        let now = self.now();
        self.queue.push(id, dirty, now);
    }

    fn remove(&mut self, id: AnimeId) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.pending_delete = true;
        }
        let now = self.now();
        self.queue.push_delete(id, now);
    }

    /// Takes the next request without finishing it.
    fn start(&mut self) -> Option<Request> {
        match self.queue.next(&self.entries, true).step {
            Step::Send(request) => Some(request),
            _ => None,
        }
    }

    async fn send(&self, request: &Request) -> Result<Option<ListEntry>, String> {
        let result = match request {
            Request::Add { entry, dirty } => {
                self.service.add_entry(&self.account, entry, *dirty).await
            }
            Request::Update { entry, dirty } => {
                self.service
                    .update_entry(&self.account, entry, *dirty)
                    .await
            }
            Request::Delete { entry } => self
                .service
                .delete_entry(&self.account, entry)
                .await
                .map(|()| None),
        };
        result
            .map(|saved| saved.map(|s| s.entry))
            .map_err(|e| e.to_string())
    }

    fn finish(&mut self, id: AnimeId, answer: Result<Option<ListEntry>, String>) -> bool {
        let finished = self.queue.finish(answer, self.entries.get(&id));
        match finished.reconcile {
            Reconcile::Nothing => {}
            Reconcile::DeleteLocal => {
                self.entries.remove(&id);
            }
            Reconcile::ClearServiceId => {
                self.entries.get_mut(&id).unwrap().id = None;
            }
            Reconcile::Store(entry) => {
                self.entries.insert(id, entry);
            }
            Reconcile::CopyServiceId(service_id) => {
                self.entries.get_mut(&id).unwrap().id = Some(service_id);
            }
        }
        finished.succeeded
    }

    /// Sends until the queue is empty or a request fails.
    async fn process(&mut self) {
        while let Some(request) = self.start() {
            let answer = self.send(&request).await;
            if !self.finish(request.anime_id(), answer) {
                break;
            }
        }
    }
}

const A: AnimeId = AnimeId(1);
const B: AnimeId = AnimeId(2);

#[tokio::test]
async fn two_edits_are_sent_as_one_request() {
    let mut app = App::new();
    app.edit(A, |e| {
        e.status = ListStatus::Watching;
        e.watched_episodes = 1;
    });
    app.edit(A, |e| e.score = 80);
    app.process().await;
    assert_eq!(
        app.service.log(),
        ["add 1 Fields(EPISODE | SCORE | STATUS)"]
    );
    assert!(app.queue.is_empty());
    // The service's id came back and is stored locally.
    assert!(app.entries[&A].id.is_some());
}

#[tokio::test]
async fn failed_request_keeps_the_item() {
    let mut app = App::new();
    app.edit(A, |e| e.status = ListStatus::Watching);
    *app.service.fail_next.lock().unwrap() = Some(ServiceError::Network("timed out".into()));
    app.process().await;
    let item = app.queue.get(A).unwrap();
    assert_eq!(item.retry_count, 1);
    assert_eq!(item.last_error.as_deref(), Some("timed out"));
    assert!(!app.service.has(A));

    app.process().await;
    assert!(app.queue.is_empty());
    assert!(app.service.has(A));
}

#[tokio::test]
async fn edit_during_request_is_sent_afterwards() {
    let mut app = App::new();
    app.edit(A, |e| {
        e.status = ListStatus::Watching;
        e.watched_episodes = 1;
    });
    let request = app.start().unwrap();
    app.edit(A, |e| e.watched_episodes = 2);
    let answer = app.send(&request).await;
    assert!(app.finish(A, answer));
    // The answer is older than the local edit, so the local value stays,
    // with the id from the answer.
    assert_eq!(app.entries[&A].watched_episodes, 2);
    assert!(app.entries[&A].id.is_some());
    assert!(app.queue.contains(A));

    app.process().await;
    assert_eq!(
        app.service.log(),
        [
            "add 1 Fields(EPISODE | STATUS)",
            "update 1 Fields(EPISODE | STATUS)",
        ]
    );
    assert_eq!(app.service.entries.lock().unwrap()[&A].watched_episodes, 2);
    assert!(app.queue.is_empty());
}

#[tokio::test]
async fn re_add_during_delete_ends_on_the_service() {
    let mut app = App::new();
    app.edit(A, |e| e.status = ListStatus::Watching);
    app.process().await;
    assert!(app.service.has(A));

    app.remove(A);
    let request = app.start().unwrap();
    assert!(matches!(request, Request::Delete { .. }));
    app.edit(A, |e| e.status = ListStatus::Completed);
    let answer = app.send(&request).await;
    assert!(app.finish(A, answer));
    assert_eq!(app.entries[&A].id, None);

    app.process().await;
    assert!(app.service.has(A));
    assert_eq!(
        app.service.entries.lock().unwrap()[&A].status,
        ListStatus::Completed
    );
    assert!(app.queue.is_empty());
    assert!(app.service.log().last().unwrap().starts_with("add 1"));
}

#[tokio::test]
async fn deleting_an_entry_the_service_lacks_succeeds() {
    let mut app = App::new();
    app.entries.insert(A, {
        let mut e = ListEntry::new(A);
        e.id = Some(ListEntryId(99));
        e.status = ListStatus::Watching;
        e
    });
    app.remove(A);
    app.process().await;
    assert!(app.queue.is_empty());
    assert!(!app.entries.contains_key(&A));
}

#[tokio::test]
async fn item_without_entry_is_dropped() {
    let mut app = App::new();
    app.edit(A, |e| e.status = ListStatus::Watching);
    app.edit(B, |e| e.status = ListStatus::Watching);
    app.entries.remove(&A);
    let next = app.queue.next(&app.entries, true);
    assert_eq!(next.dropped, [A]);
    assert!(matches!(next.step, Step::Send(ref r) if r.anime_id() == B));
}

#[test]
fn not_signed_in_asks_for_authentication() {
    let mut app = App::new();
    app.edit(A, |e| e.status = ListStatus::Watching);
    let next = app.queue.next(&app.entries, false);
    assert_eq!(next.step, Step::Authenticate);
    // Nothing was marked in flight, so a later run can send it.
    assert_eq!(app.queue.in_flight(), None);
}

#[test]
fn only_one_request_at_a_time() {
    let mut app = App::new();
    app.edit(A, |e| e.status = ListStatus::Watching);
    app.edit(B, |e| e.status = ListStatus::Watching);
    assert!(app.start().is_some());
    assert_eq!(app.start(), None);
}

#[test]
fn nothing_is_sent_when_sync_is_disabled() {
    // The timers check this before calling process.
    assert!(!should_auto_process(false, true, true));
}
