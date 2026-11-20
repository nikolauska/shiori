//! Running Synchronize against a service.
//!
//! The command-line mode awaits these directly. The UI will run the service
//! calls on the network thread instead and apply the answers on the main
//! thread, using the same `AppState` methods.

use anyhow::Result;
use shiori_sync::queue::{Request, Step};
use shiori_sync::sync::{SyncAction, SyncState, sync_action};
use shiori_sync::{Account, Service};

use crate::state::{AppEvent, AppState};

/// Reports progress to whoever runs the sync: the status bar later, the
/// terminal now.
pub trait Report {
    fn status(&self, text: &str);
    fn events(&self, state: &AppState, events: &[AppEvent]);
}

pub async fn synchronize<S: Service>(
    state: &mut AppState,
    service: &S,
    account: &Account,
    report: &impl Report,
) -> Result<()> {
    let name = service.id().name();
    // Bounded so that a service that keeps answering oddly cannot spin this
    // forever; a normal run needs at most authenticate, queue, fetch.
    for _ in 0..4 {
        let action = sync_action(SyncState {
            signed_in: state.signed_in,
            has_token: account.token.is_some(),
            has_username: !account.username.is_empty(),
            queue_empty: state.queue.is_empty(),
        });
        match action {
            SyncAction::NotSetUp => {
                report.status(&format!("No {name} account is set up."));
                return Ok(());
            }
            SyncAction::Authenticate => {
                report.status(&format!("Authenticating with {name}..."));
                match service.authenticate(account).await {
                    Ok(info) => {
                        let events = state.apply_sign_in(&info);
                        report.events(state, &events);
                        report.status(&format!("Logged in as {}.", info.username));
                    }
                    Err(error) => {
                        let events = state.apply_service_error(&error);
                        report.events(state, &events);
                        return Ok(());
                    }
                }
            }
            SyncAction::ProcessQueue => {
                if !process_queue(state, service, account, report).await? {
                    return Ok(());
                }
                // Everything was sent; fetch the list next so that what the
                // service changed elsewhere shows up too.
            }
            SyncAction::FetchList => {
                report.status(&format!("Synchronizing with {name}..."));
                let progress = |done: u64, total: u64| {
                    if total > 0 {
                        tracing::debug!("{done} of {total} bytes");
                    }
                };
                match service.fetch_list(account, &progress).await {
                    Ok(list) => {
                        let events = state.apply_list(list)?;
                        report.events(state, &events);
                    }
                    Err(error) => {
                        let events = state.apply_service_error(&error);
                        report.events(state, &events);
                    }
                }
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Sends queue items until the queue is empty. Returns false when it stopped
/// early: a request failed or sign-in is needed.
pub async fn process_queue<S: Service>(
    state: &mut AppState,
    service: &S,
    account: &Account,
    report: &impl Report,
) -> Result<bool> {
    loop {
        let (step, events) = state.queue_next()?;
        report.events(state, &events);
        let request = match step {
            Step::Idle => return Ok(state.queue.is_empty()),
            Step::Authenticate => return Ok(false),
            Step::Send(request) => request,
        };
        let title = state.title(request.anime_id());
        let answer = match &request {
            Request::Add { entry, dirty } => {
                report.status(&format!("Adding to list... ({title})"));
                service.add_entry(account, entry, *dirty).await
            }
            Request::Update { entry, dirty } => {
                report.status(&format!("Updating list entry... ({title})"));
                service.update_entry(account, entry, *dirty).await
            }
            Request::Delete { entry } => {
                report.status(&format!("Deleting list entry... ({title})"));
                service.delete_entry(account, entry).await.map(|()| None)
            }
        };
        let failed = answer.is_err();
        let events = state.queue_finished(answer)?;
        report.events(state, &events);
        if failed {
            return Ok(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use jiff::Timestamp;
    use shiori_core::{
        Anime, AnimeId, Fields, ListEntry, ListEntryId, ListStatus, SearchParams, ServiceId,
    };
    use shiori_store::{Accounts, Database, Settings};
    use shiori_sync::{AccountInfo, FetchedList, PageToken, SavedEntry, SearchPage, ServiceError};

    use super::*;

    #[derive(Default)]
    struct Fake {
        list: Mutex<BTreeMap<AnimeId, ListEntry>>,
        calls: Mutex<Vec<String>>,
        reject_token: bool,
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        fn call(&self, what: impl Into<String>) {
            self.calls.lock().unwrap().push(what.into());
        }
    }

    impl Service for Fake {
        fn id(&self) -> ServiceId {
            ServiceId::AniList
        }
        async fn authenticate(&self, _: &Account) -> Result<AccountInfo, ServiceError> {
            self.call("authenticate");
            if self.reject_token {
                return Err(ServiceError::Unauthorized(
                    "Access token has expired.".into(),
                ));
            }
            Ok(AccountInfo {
                user_id: "1".into(),
                username: "someone".into(),
                rating_system: "POINT_10".into(),
            })
        }
        async fn fetch_anime(&self, _: AnimeId) -> Result<Anime, ServiceError> {
            Err(ServiceError::NotFound)
        }
        async fn fetch_list(
            &self,
            _: &Account,
            _: &(dyn Fn(u64, u64) + Send + Sync),
        ) -> Result<FetchedList, ServiceError> {
            self.call("fetch_list");
            let entries: Vec<_> = self.list.lock().unwrap().values().cloned().collect();
            let anime = entries
                .iter()
                .map(|e| {
                    let mut a = Anime::new(e.anime_id);
                    a.titles.romaji = format!("Anime {}", e.anime_id);
                    a
                })
                .collect();
            Ok(FetchedList { anime, entries })
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
            a: &Account,
            entry: &ListEntry,
            dirty: Fields,
        ) -> Result<Option<SavedEntry>, ServiceError> {
            self.update_entry(a, entry, dirty).await
        }
        async fn update_entry(
            &self,
            _: &Account,
            entry: &ListEntry,
            _: Fields,
        ) -> Result<Option<SavedEntry>, ServiceError> {
            self.call(format!("save {}", entry.anime_id));
            let mut saved = entry.clone();
            saved
                .id
                .get_or_insert(ListEntryId(1000 + i64::from(entry.anime_id.0)));
            self.list
                .lock()
                .unwrap()
                .insert(entry.anime_id, saved.clone());
            Ok(Some(SavedEntry {
                entry: saved,
                anime: None,
            }))
        }
        async fn delete_entry(&self, _: &Account, entry: &ListEntry) -> Result<(), ServiceError> {
            self.call(format!("delete {}", entry.anime_id));
            self.list.lock().unwrap().remove(&entry.anime_id);
            Ok(())
        }
    }

    struct Quiet;
    impl Report for Quiet {
        fn status(&self, _: &str) {}
        fn events(&self, _: &AppState, _: &[AppEvent]) {}
    }

    fn state() -> AppState {
        AppState::new(
            Database::open_in_memory().unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap()
    }

    fn account() -> Account {
        Account {
            username: "someone".into(),
            token: Some("tok".into()),
        }
    }

    fn entry(id: u32, episodes: u32) -> ListEntry {
        let mut e = ListEntry::new(AnimeId(id));
        e.id = Some(ListEntryId(1000 + i64::from(id)));
        e.status = ListStatus::Watching;
        e.watched_episodes = episodes;
        e
    }

    fn now() -> Timestamp {
        "2026-11-11T12:00:00Z".parse().unwrap()
    }

    #[tokio::test]
    async fn first_sync_signs_in_and_fetches() {
        let service = Fake::default();
        service.list.lock().unwrap().insert(AnimeId(1), entry(1, 3));
        let mut state = state();
        synchronize(&mut state, &service, &account(), &Quiet)
            .await
            .unwrap();
        assert_eq!(service.calls(), ["authenticate", "fetch_list"]);
        assert!(state.signed_in);
        assert_eq!(state.accounts.anilist.username, "someone");
        assert_eq!(state.entries[&AnimeId(1)].watched_episodes, 3);
        assert_eq!(state.title(AnimeId(1)), "Anime 1");
        // Written through to the database.
        assert_eq!(state.stored().entries.len(), 1);
    }

    #[tokio::test]
    async fn edit_is_sent_before_the_list_is_fetched() {
        let service = Fake::default();
        service.list.lock().unwrap().insert(AnimeId(1), entry(1, 3));
        let mut state = state();
        synchronize(&mut state, &service, &account(), &Quiet)
            .await
            .unwrap();

        let events = state.save_entry(entry(1, 4), now()).unwrap();
        assert!(events.contains(&AppEvent::QueueChanged));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AppEvent::QueuedWhileSignedOut(_)))
        );
        synchronize(&mut state, &service, &account(), &Quiet)
            .await
            .unwrap();
        assert_eq!(
            service.calls(),
            ["authenticate", "fetch_list", "save 1", "fetch_list"]
        );
        assert_eq!(
            service.list.lock().unwrap()[&AnimeId(1)].watched_episodes,
            4
        );
        assert_eq!(state.entries[&AnimeId(1)].watched_episodes, 4);
        assert!(state.queue.is_empty());
        assert!(state.stored().queue.is_empty());
    }

    #[tokio::test]
    async fn queued_edit_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("anilist.sqlite");
        {
            let mut state = AppState::new(
                Database::open(&path).unwrap(),
                Settings::default(),
                Accounts::default(),
            )
            .unwrap();
            state.store_anime(vec![Anime::new(AnimeId(7))]).unwrap();
            let events = state.save_entry(entry(7, 1), now()).unwrap();
            assert!(events.contains(&AppEvent::QueuedWhileSignedOut(AnimeId(7))));
        }
        let mut state = AppState::new(
            Database::open(&path).unwrap(),
            Settings::default(),
            Accounts::default(),
        )
        .unwrap();
        assert!(state.queue.contains(AnimeId(7)));
        let service = Fake::default();
        synchronize(&mut state, &service, &account(), &Quiet)
            .await
            .unwrap();
        assert_eq!(service.calls(), ["authenticate", "save 7", "fetch_list"]);
        // Windows refuses to delete a file that is still open, so the
        // database has to be closed before the folder is removed.
        drop(state);
        dir.close().unwrap();
    }

    #[tokio::test]
    async fn removed_entry_is_deleted_after_confirmation() {
        let service = Fake::default();
        service.list.lock().unwrap().insert(AnimeId(1), entry(1, 3));
        let mut state = state();
        synchronize(&mut state, &service, &account(), &Quiet)
            .await
            .unwrap();

        state.remove_entry(AnimeId(1), now()).unwrap();
        assert!(state.entries[&AnimeId(1)].pending_delete);
        synchronize(&mut state, &service, &account(), &Quiet)
            .await
            .unwrap();
        assert!(!state.entries.contains_key(&AnimeId(1)));
        assert!(service.list.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn fetch_prunes_but_keeps_queued_entries() {
        let service = Fake::default();
        let mut state = state();
        state.signed_in = true;
        let earlier = FetchedList {
            anime: vec![Anime::new(AnimeId(5)), Anime::new(AnimeId(6))],
            entries: vec![entry(5, 1)],
        };
        state.apply_list(earlier).unwrap();

        // A local addition the service has not seen yet.
        let mut new = ListEntry::new(AnimeId(6));
        new.status = ListStatus::PlanToWatch;
        state.save_entry(new, now()).unwrap();

        // The service no longer has 5, and has never had 6.
        let list = service.fetch_list(&account(), &|_, _| {}).await.unwrap();
        let events = state.apply_list(list).unwrap();
        assert!(events.contains(&AppEvent::EntryDeleted(AnimeId(5))));
        assert!(!state.entries.contains_key(&AnimeId(5)));
        assert!(state.entries.contains_key(&AnimeId(6)));
        assert!(!state.stored().entries.contains_key(&AnimeId(5)));
    }

    #[tokio::test]
    async fn rejected_token_stops_the_sync() {
        let service = Fake {
            reject_token: true,
            ..Fake::default()
        };
        let mut state = state();
        synchronize(&mut state, &service, &account(), &Quiet)
            .await
            .unwrap();
        assert_eq!(service.calls(), ["authenticate"]);
        assert!(!state.signed_in);
    }

    #[tokio::test]
    async fn public_list_without_token() {
        let service = Fake::default();
        service.list.lock().unwrap().insert(AnimeId(1), entry(1, 3));
        let mut state = state();
        let account = Account {
            username: "someone".into(),
            token: None,
        };
        synchronize(&mut state, &service, &account, &Quiet)
            .await
            .unwrap();
        assert_eq!(service.calls(), ["fetch_list"]);
        assert!(!state.signed_in);
    }

    #[test]
    fn entry_needs_a_stored_anime() {
        let mut state = state();
        assert!(state.save_entry(entry(9, 1), now()).is_err());
        assert!(state.entries.is_empty());
    }

    #[test]
    fn invalidating_keeps_queued_anime() {
        let mut state = state();
        state
            .store_anime(vec![Anime::new(AnimeId(1)), Anime::new(AnimeId(2))])
            .unwrap();
        state.save_entry(entry(1, 1), now()).unwrap();
        assert!(state.invalidate_anime(AnimeId(1)).unwrap().is_empty());
        let events = state.invalidate_anime(AnimeId(2)).unwrap();
        assert!(matches!(
            events.last(),
            Some(AppEvent::AnimeDeleted { id: AnimeId(2), .. })
        ));
        assert!(!state.anime.contains_key(&AnimeId(2)));
    }
}
