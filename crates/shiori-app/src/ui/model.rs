//! The app model: `AppState` plus the network bridge, as one gpui entity.
//!
//! Views read from it and call its methods. Requests run on the tokio
//! runtime; their answers come back to the main thread and are applied to
//! `AppState` there, so the state is never shared across threads.

use std::future::Future;
use std::time::Duration;

use gpui::{Context, EventEmitter, SharedString, Task};
use jiff::Timestamp;
use shiori_core::{AnimeId, AnimeSettings, ListEntry, SearchParams};
use shiori_store::{SecretKey, Session, Settings};
use shiori_sync::anilist::AniList;
use shiori_sync::queue::{Request, Step};
use shiori_sync::sync::{SyncAction, SyncState, should_auto_process, sync_action};
use shiori_sync::{Account, PageToken, Service, ServiceError};

use shiori_app::posters;
use shiori_app::startup::Context as Startup;
use shiori_app::state::{AppEvent, AppState};

use super::format;
use super::library::Library;
use super::poster::Posters;
use super::tracker::{ConfirmHook, Tracker};

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
    /// For downloads that are not service requests, such as posters.
    http: reqwest::Client,
    pub status: Status,
    /// Synchronize is running; a second click waits for it.
    syncing: bool,
    pub posters: Posters,
    pub session: Session,
    /// A session write is scheduled.
    session_save: Option<Task<()>>,
    /// The anime shown in the inspector.
    pub selected: Option<AnimeId>,
    pub tracker: Tracker,
    pub library: Library,
    /// Called when an update needs the user's say; the UI decides how to
    /// ask (a notification, unless the main window is active).
    pub confirm_hook: Option<ConfirmHook>,
}

impl EventEmitter<AppEvent> for Model {}

/// Requests from views that the main window carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiRequest {
    OpenAnime(AnimeId),
}

impl EventEmitter<UiRequest> for Model {}

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
        let http = startup.network()?.client()?;
        // Losing the session loses nothing important; start from defaults.
        let session = Session::load(&startup.folders.session_file()).unwrap_or_else(|error| {
            tracing::warn!("Could not read the session: {error:#}");
            Session::default()
        });
        Ok(Self {
            startup,
            service,
            runtime,
            http,
            status: Status::default(),
            syncing: false,
            posters: Posters::default(),
            session,
            session_save: None,
            selected: None,
            library: Library::default(),
            tracker: Tracker::default(),
            confirm_hook: None,
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

    pub(super) fn set_error(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
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

    /// Marks one more or one less episode watched, within 0 and the episode
    /// count. Only for anime in the list.
    pub fn step_episode(&mut self, id: AnimeId, delta: i32, cx: &mut Context<Self>) {
        let state = self.state();
        let Some(mut entry) = state
            .entries
            .get(&id)
            .filter(|e| shiori_core::entry::is_in_list(Some(e)))
            .cloned()
        else {
            return;
        };
        let max = state
            .anime
            .get(&id)
            .and_then(|a| a.episode_count)
            .unwrap_or(shiori_core::anime::MAX_EPISODE_COUNT);
        let watched = entry.watched_episodes.saturating_add_signed(delta).min(max);
        if watched != entry.watched_episodes {
            entry.watched_episodes = watched;
            self.save_entry(entry, cx);
        }
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

    pub fn save_anime_settings(&mut self, settings: AnimeSettings, cx: &mut Context<Self>) {
        let result = self.startup.state.save_anime_settings(settings);
        self.apply(result, cx);
    }

    // ---- History and queue ------------------------------------------------

    pub fn remove_history(&mut self, id: i64, cx: &mut Context<Self>) {
        let result = self.startup.state.remove_history(id);
        self.apply(result, cx);
    }

    pub fn clear_history(&mut self, cx: &mut Context<Self>) {
        let result = self.startup.state.clear_history();
        self.apply(result, cx);
    }

    pub fn clear_queue(&mut self, cx: &mut Context<Self>) {
        // Synchronize may be between two requests, with none in flight; a
        // cleared queue would then end it halfway without a word.
        if self.syncing {
            self.set_error("Wait for synchronization to finish.", cx);
            return;
        }
        let result = self.startup.state.clear_queue();
        self.apply(result, cx);
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
                    this.load_poster(id, true, cx);
                }
                Err(ServiceError::NotFound) => {
                    let result = this.startup.state.invalidate_anime(id);
                    this.apply(result, cx);
                }
                Err(error) => this.service_error(error, cx),
            },
        );
    }

    // ---- Settings -----------------------------------------------------------

    /// Stores changed settings and applies them. `proxy_password` is `None`
    /// when the password was left as it was.
    pub fn apply_settings(
        &mut self,
        settings: Settings,
        proxy_password: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let before = self.state().settings.clone();
        if let Err(error) = self.startup.settings_file.save(&settings) {
            self.set_error(format!("Could not save settings: {error:#}"), cx);
            return;
        }
        if let Some(password) = &proxy_password {
            let result = if password.is_empty() {
                self.startup.secrets.delete(SecretKey::ProxyPassword)
            } else {
                self.startup.secrets.set(SecretKey::ProxyPassword, password)
            };
            if let Err(error) = result {
                self.set_error(format!("Could not store the proxy password: {error}"), cx);
            }
        }
        let proxy_changed = before.proxy != settings.proxy || proxy_password.is_some();
        self.startup.state.settings = settings;
        // The HTTP clients are built with the proxy, so a new proxy needs new
        // clients. Requests already running finish on the old ones.
        if proxy_changed {
            let clients = self
                .startup
                .service()
                .and_then(|service| Ok((service, self.startup.network()?.client()?)));
            match clients {
                Ok((service, http)) => {
                    self.service = service;
                    self.http = http;
                }
                Err(error) => self.set_error(format!("Proxy settings: {error:#}"), cx),
            }
        }
        let detection_changed = before.detection_enabled != self.state().settings.detection_enabled
            || before.detection_interval_ms != self.state().settings.detection_interval_ms;
        if detection_changed {
            self.detection_settings_changed(cx);
        }
        // A scan of the old folders would show the wrong tree.
        if before.library_folders != self.state().settings.library_folders && self.library.scanned {
            self.scan_library(cx);
        }
        self.emit(vec![AppEvent::SettingsChanged], cx);
    }

    /// Flips one on/off setting, such as "Enable synchronization".
    pub fn toggle_setting(&mut self, flip: fn(&mut Settings), cx: &mut Context<Self>) {
        let mut settings = self.state().settings.clone();
        flip(&mut settings);
        self.apply_settings(settings, None, cx);
    }

    /// Writes the list as Markdown and says how it went.
    pub fn export_markdown(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let text = shiori_app::export::markdown(self.state());
        match std::fs::write(&path, text) {
            Ok(()) => self.set_status(format!("Exported list to {}.", path.display()), cx),
            Err(error) => {
                tracing::error!("Could not export list to {}: {error}", path.display());
                self.set_error(format!("Could not export list to {}.", path.display()), cx);
            }
        }
    }

    // ---- Search -------------------------------------------------------------

    /// Runs one search page. Found anime are stored like any other anime, so
    /// that the views and the details window can show them. `done` gets the
    /// ids in the service's order and the next page, or `None` on failure.
    pub fn search(
        &mut self,
        params: SearchParams,
        page: Option<PageToken>,
        cx: &mut Context<Self>,
        done: impl FnOnce(Option<(Vec<AnimeId>, Option<PageToken>)>, &mut gpui::App) + 'static,
    ) {
        let service = self.service.clone();
        self.set_status("Searching...", cx);
        self.request(
            async move { service.search(&params, page).await },
            cx,
            move |this, result, cx| match result {
                Ok(page) => {
                    let ids: Vec<AnimeId> = page.anime.iter().map(|a| a.id).collect();
                    let result = this.startup.state.store_anime(page.anime);
                    this.apply(result, cx);
                    this.set_status(format!("Found {} anime.", ids.len()), cx);
                    done(Some((ids, page.next)), cx);
                }
                Err(error) => {
                    this.service_error(error, cx);
                    done(None, cx);
                }
            },
        );
    }

    // ---- Posters ------------------------------------------------------------

    /// Reads the cached poster, or downloads it when it is missing,
    /// outdated, or `force` is set.
    pub fn load_poster(&mut self, id: AnimeId, force: bool, cx: &mut Context<Self>) {
        let Some(anime) = self.state().anime.get(&id).cloned() else {
            return;
        };
        if !self.posters.pending.insert(id) {
            return;
        }
        let folder = self.startup.folders.poster_folder(self.state().service);
        let client = self.http.clone();
        let today = format::today_in_japan();
        let task = self.runtime.spawn(async move {
            let cached = posters::cached_file(&folder, id);
            let written = cached
                .as_ref()
                .and_then(|path| path.metadata().ok()?.modified().ok())
                .and_then(|time| jiff::Timestamp::try_from(time).ok());
            let fresh = written.is_some_and(|written| {
                !posters::is_outdated(&anime, written, jiff::Timestamp::now(), today)
            });
            if (fresh && !force) || anime.image_url.is_empty() {
                if let Some(path) = &cached {
                    return std::fs::read(path).map_err(|e| e.to_string());
                }
                return Err("no image address".to_owned());
            }
            let download = async {
                let response = client.get(&anime.image_url).send().await?;
                response.error_for_status()?.bytes().await
            };
            match download.await {
                Ok(bytes) => {
                    posters::store(&folder, id, &bytes).map_err(|e| e.to_string())?;
                    Ok(bytes.to_vec())
                }
                // An old poster is better than none.
                Err(error) => match &cached {
                    Some(path) => std::fs::read(path).map_err(|e| e.to_string()),
                    None => Err(error.to_string()),
                },
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.posters.pending.remove(&id);
                match result {
                    Ok(Ok(bytes)) => {
                        this.posters.remove(id);
                        this.posters.insert(id, bytes);
                    }
                    Ok(Err(error)) => {
                        tracing::debug!("No poster for {id}: {error}");
                        this.posters.failed.insert(id);
                    }
                    Err(error) => {
                        tracing::error!("Poster task failed: {error}");
                        this.posters.failed.insert(id);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---- Session ------------------------------------------------------------

    /// Writes the session soon, at most once per second however often it
    /// changes, since dragging a divider changes it on every frame.
    pub fn session_changed(&mut self, cx: &mut Context<Self>) {
        if self.session_save.is_some() {
            return;
        }
        self.session_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, _| {
                this.session_save = None;
                this.save_session();
            });
        }));
    }

    pub fn save_session(&mut self) {
        if let Err(error) = self.session.save(&self.startup.folders.session_file()) {
            tracing::error!("Could not save the session: {error:#}");
        }
    }

    pub fn set_inspector_width(&mut self, width: f32, cx: &mut Context<Self>) {
        if self.session.inspector_width != Some(width) {
            self.session.inspector_width = Some(width);
            self.session_changed(cx);
        }
    }

    pub fn set_inspector_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        if self.session.inspector_hidden != hidden {
            self.session.inspector_hidden = hidden;
            self.session_changed(cx);
            cx.notify();
        }
    }

    // ---- Selection ----------------------------------------------------------

    /// The anime the inspector shows.
    pub fn select(&mut self, id: AnimeId, cx: &mut Context<Self>) {
        if self.selected != Some(id) {
            self.selected = Some(id);
            cx.notify();
        }
    }

    pub fn deselect(&mut self, cx: &mut Context<Self>) {
        if self.selected.take().is_some() {
            cx.notify();
        }
    }

    /// Asks the main window to show an anime: in the inspector, or as a full
    /// page when the inspector is hidden or the window is narrow.
    pub fn open_anime(&mut self, id: AnimeId, cx: &mut Context<Self>) {
        self.select(id, cx);
        cx.emit(UiRequest::OpenAnime(id));
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
