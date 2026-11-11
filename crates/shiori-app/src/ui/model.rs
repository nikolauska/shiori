//! The app model: `AppState` plus the network bridge, as one gpui entity.
//!
//! Views read from it and call its methods. Requests run on the tokio
//! runtime; their answers come back to the main thread and are applied to
//! `AppState` there, so the state is never shared across threads.

use std::future::Future;

use gpui::{Context, EventEmitter, SharedString};
use jiff::Timestamp;
use shiori_core::{AnimeId, ListEntry};
use shiori_store::SecretKey;
use shiori_sync::anilist::AniList;
use shiori_sync::queue::{Request, Step};
use shiori_sync::sync::{SyncAction, SyncState, should_auto_process, sync_action};
use shiori_sync::{Account, Service, ServiceError};

use shiori_app::startup::Context as Startup;
use shiori_app::state::{AppEvent, AppState};

/// What the status bar shows.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub text: SharedString,
    pub is_error: bool,
    /// A request is in flight.
    pub busy: bool,
}

pub struct Model {
    pub startup: Startup,
    service: AniList,
    runtime: tokio::runtime::Runtime,
    pub status: Status,
    /// Synchronize is running; a second click waits for it.
    syncing: bool,
}

impl EventEmitter<AppEvent> for Model {}

impl Model {
    pub fn new(startup: Startup) -> anyhow::Result<Self> {
        let service = startup.service()?;
        // One worker is enough: requests are few and mostly sequential, and
        // it keeps networking on its own thread as the design asks.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("shiori-network")
            .enable_all()
            .build()?;
        Ok(Self {
            startup,
            service,
            runtime,
            status: Status::default(),
            syncing: false,
        })
    }

    pub fn state(&self) -> &AppState {
        &self.startup.state
    }

    pub fn service_name(&self) -> &'static str {
        self.startup.state.service.name()
    }

    fn account(&self) -> Account {
        self.startup.account().unwrap_or_else(|error| {
            tracing::error!("Could not read the token: {error:#}");
            Account {
                username: self.startup.state.accounts.anilist.username.clone(),
                token: None,
            }
        })
    }

    pub fn set_status(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status.text = text.into();
        self.status.is_error = false;
        cx.notify();
    }

    fn set_error(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status.text = text.into();
        self.status.is_error = true;
        cx.notify();
    }

    /// Runs `request` on the network thread and hands its result to `then`
    /// on the main thread.
    fn request<T: Send + 'static>(
        &mut self,
        request: impl Future<Output = T> + Send + 'static,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let handle = self.runtime.spawn(request);
        self.status.busy = true;
        cx.spawn(async move |this, cx| {
            let result = handle.await;
            let _ = this.update(cx, |this, cx| {
                this.status.busy = false;
                match result {
                    Ok(result) => then(this, result, cx),
                    Err(error) => this.set_error(format!("Request failed: {error}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows what happened and passes the events on to the views.
    pub fn emit(&mut self, events: Vec<AppEvent>, cx: &mut Context<Self>) {
        for event in events {
            match &event {
                AppEvent::ServiceError(text) => self.set_error(text.clone(), cx),
                AppEvent::QueuedWhileSignedOut(id) => {
                    let text = format!("{} is queued for update.", self.state().title(*id));
                    self.set_status(text, cx);
                }
                AppEvent::AnimeDeleted { title, .. } => {
                    self.set_status(format!("Anime removed from database: {title}"), cx);
                }
                _ => {}
            }
            cx.emit(event);
        }
        cx.notify();
    }

    fn apply(&mut self, result: anyhow::Result<Vec<AppEvent>>, cx: &mut Context<Self>) {
        match result {
            Ok(events) => self.emit(events, cx),
            // A database write failed; the in-memory state may be ahead of
            // the disk, which the user should know about.
            Err(error) => {
                tracing::error!("{error:#}");
                self.set_error(format!("Could not save: {error:#}"), cx);
            }
        }
    }

    fn service_error(&mut self, error: ServiceError, cx: &mut Context<Self>) {
        let events = self.startup.state.apply_service_error(&error);
        self.emit(events, cx);
    }

    // ---- Edits ------------------------------------------------------------

    pub fn save_entry(&mut self, entry: ListEntry, cx: &mut Context<Self>) {
        let result = self.startup.state.save_entry(entry, Timestamp::now());
        self.apply(result, cx);
        self.queue_settled(cx);
    }

    pub fn remove_entry(&mut self, id: AnimeId, cx: &mut Context<Self>) {
        let result = self.startup.state.remove_entry(id, Timestamp::now());
        self.apply(result, cx);
        self.queue_settled(cx);
    }

    /// Edits are sent right away for now. The 10 second settle timer that
    /// collects edits made in a row comes with the tracking work.
    fn queue_settled(&mut self, cx: &mut Context<Self>) {
        let has_token = self.account().token.is_some();
        if should_auto_process(
            self.state().settings.sync_enabled,
            self.state().signed_in,
            has_token,
        ) {
            self.synchronize(cx);
        }
    }

    /// Fetches the anime again. "Not found" means the service removed it.
    pub fn refresh_anime(&mut self, id: AnimeId, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let title = self.state().title(id);
        self.set_status(format!("Refreshing {title}..."), cx);
        self.request(
            async move { service.fetch_anime(id).await },
            cx,
            move |this, result, cx| match result {
                Ok(anime) => {
                    let result = this.startup.state.store_anime(vec![anime]);
                    this.apply(result, cx);
                    this.set_status(format!("Refreshed {title}."), cx);
                }
                Err(ServiceError::NotFound) => {
                    let result = this.startup.state.invalidate_anime(id);
                    this.apply(result, cx);
                }
                Err(error) => this.service_error(error, cx),
            },
        );
    }

    // ---- Synchronize --------------------------------------------------------

    pub fn synchronize(&mut self, cx: &mut Context<Self>) {
        if self.syncing {
            return;
        }
        self.syncing = true;
        self.sync_step(cx);
    }

    fn sync_done(&mut self, cx: &mut Context<Self>) {
        self.syncing = false;
        if let Err(error) = self.startup.save_accounts() {
            tracing::error!("Could not save accounts: {error:#}");
        }
        cx.notify();
    }

    fn sync_step(&mut self, cx: &mut Context<Self>) {
        let account = self.account();
        let name = self.service_name();
        let action = sync_action(SyncState {
            signed_in: self.state().signed_in,
            has_token: account.token.is_some(),
            has_username: !account.username.is_empty(),
            queue_empty: self.state().queue.is_empty(),
        });
        let service = self.service.clone();
        match action {
            SyncAction::NotSetUp => {
                self.set_status(
                    format!("No {name} account is set up. Add one in Settings."),
                    cx,
                );
                self.sync_done(cx);
            }
            SyncAction::Authenticate => {
                self.set_status(format!("Authenticating with {name}..."), cx);
                self.request(
                    async move { service.authenticate(&account).await },
                    cx,
                    |this, result, cx| match result {
                        Ok(info) => {
                            let events = this.startup.state.apply_sign_in(&info);
                            this.set_status(format!("Logged in as {}.", info.username), cx);
                            this.emit(events, cx);
                            this.sync_step(cx);
                        }
                        Err(error) => {
                            this.service_error(error, cx);
                            this.sync_done(cx);
                        }
                    },
                );
            }
            SyncAction::ProcessQueue => self.process_queue(cx),
            SyncAction::FetchList => {
                self.set_status(format!("Synchronizing with {name}..."), cx);
                self.request(
                    async move { service.fetch_list(&account, &|_, _| {}).await },
                    cx,
                    |this, result, cx| {
                        match result {
                            Ok(list) => {
                                let result = this.startup.state.apply_list(list);
                                this.apply(result, cx);
                                if !this.status.is_error {
                                    this.set_status("List synchronized.", cx);
                                }
                            }
                            Err(error) => this.service_error(error, cx),
                        }
                        this.sync_done(cx);
                    },
                );
            }
        }
    }

    fn process_queue(&mut self, cx: &mut Context<Self>) {
        let (step, events) = match self.startup.state.queue_next() {
            Ok(next) => next,
            Err(error) => {
                self.apply(Err(error), cx);
                self.sync_done(cx);
                return;
            }
        };
        self.emit(events, cx);
        let request = match step {
            // Everything was sent: fetch the list so that changes made
            // elsewhere show up too.
            Step::Idle if self.state().queue.is_empty() => return self.sync_step(cx),
            Step::Idle | Step::Authenticate => return self.sync_done(cx),
            Step::Send(request) => request,
        };
        let title = self.state().title(request.anime_id());
        let account = self.account();
        let service = self.service.clone();
        let text = match &request {
            Request::Add { .. } => format!("Adding to list... ({title})"),
            Request::Update { .. } => format!("Updating list entry... ({title})"),
            Request::Delete { .. } => format!("Deleting list entry... ({title})"),
        };
        self.set_status(text, cx);
        self.request(
            async move {
                match request {
                    Request::Add { entry, dirty } => {
                        service.add_entry(&account, &entry, dirty).await
                    }
                    Request::Update { entry, dirty } => {
                        service.update_entry(&account, &entry, dirty).await
                    }
                    Request::Delete { entry } => {
                        service.delete_entry(&account, &entry).await.map(|()| None)
                    }
                }
            },
            cx,
            |this, answer, cx| {
                let failed = answer.is_err();
                let result = this.startup.state.queue_finished(answer);
                this.apply(result, cx);
                if failed {
                    this.sync_done(cx);
                } else {
                    this.process_queue(cx);
                }
            },
        );
    }

    // ---- Account --------------------------------------------------------------

    /// Checks a pasted token and stores it when AniList accepts it.
    pub fn sign_in(&mut self, token: String, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let account = Account {
            username: String::new(),
            token: Some(token.clone()),
        };
        self.set_status(
            format!("Authenticating with {}...", self.service_name()),
            cx,
        );
        self.request(
            async move { service.authenticate(&account).await },
            cx,
            move |this, result, cx| match result {
                Ok(info) => {
                    if let Err(error) = this.startup.secrets.set(SecretKey::AniListToken, &token) {
                        this.set_error(format!("Could not store the token: {error}"), cx);
                        return;
                    }
                    let events = this.startup.state.apply_sign_in(&info);
                    if let Err(error) = this.startup.save_accounts() {
                        tracing::error!("Could not save accounts: {error:#}");
                    }
                    this.set_status(format!("Logged in as {}.", info.username), cx);
                    this.emit(events, cx);
                    this.synchronize(cx);
                }
                Err(error) => this.service_error(error, cx),
            },
        );
    }
}
