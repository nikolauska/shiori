//! Where the list lives: on this computer only, or in an account on the
//! list service. The first-start question and moving the list later.

use std::rc::Rc;

use gpui::{App, Context, Entity, ParentElement as _, Styled as _, Window, div};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{WindowExt as _, h_flex};
use std::path::PathBuf;

use jiff::Timestamp;
use shiori_app::refresh::anime_to_refresh;
use shiori_app::state::AppEvent;
use shiori_app::{backup, upload};
use shiori_core::AnimeId;
use shiori_core::entry::is_in_list;
use shiori_store::ListLocation;
use shiori_sync::anilist::AniList;

use super::format;
use super::model::Model;
use super::pages;

impl Model {
    /// There is an account to synchronize with, signed in or not.
    pub fn has_account(&self) -> bool {
        !self.state().accounts.anilist.username.is_empty() || self.account().token.is_some()
    }

    /// Opens the list kept at `to` in place of the current one.
    pub fn move_list(&mut self, to: ListLocation, copy: bool, cx: &mut Context<Self>) {
        // A queue answer arriving after the move would land in the other
        // list's queue.
        if self.syncing {
            self.set_error("Wait for synchronization to finish.", cx);
            return;
        }
        if let Err(error) = self.startup.move_list(to, copy) {
            tracing::error!("Could not open the list: {error:#}");
            self.set_error(format!("Could not open the list: {error:#}"), cx);
            return;
        }
        self.list_replaced(cx);
        let service = self.service_name();
        match to {
            ListLocation::Local => {
                self.set_status("Your list is now kept on this computer.", cx);
                self.local_upkeep(cx);
            }
            ListLocation::Account => {
                self.set_status(format!("Using your {service} list."), cx);
                if self.state().settings.sync_enabled {
                    self.synchronize(cx);
                }
            }
        }
    }
}

impl Model {
    /// Everything that was about anime in the list just closed or replaced.
    fn list_replaced(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.tracker.suggestion = None;
        if let Some(media) = self.tracker.media.clone() {
            self.media_found(Some(media), cx);
        }
        self.recognize_library_again(cx);
        self.emit(vec![AppEvent::ListReplaced, AppEvent::SettingsChanged], cx);
    }

    /// What a local list needs now and then, in place of synchronizing:
    /// fresh anime data, and a weekly backup.
    pub fn local_upkeep(&mut self, cx: &mut Context<Self>) {
        if !self.state().is_local() {
            return;
        }
        self.refresh_anime_data(false, cx);
        if !self.state().settings.backup_weekly {
            return;
        }
        let dir = self.startup.folders.backups();
        let today = jiff::Zoned::now().date();
        match backup::back_up_weekly(self.state(), &dir, today) {
            Ok(Some(path)) => {
                tracing::info!("Backed up the list to {}", path.display());
                self.last_backup = Some(today);
                cx.notify();
            }
            Ok(None) => {}
            // Unlike a failed refresh this matters: the backup is the only
            // other copy of the list.
            Err(error) => {
                tracing::error!("Could not back up the list: {error:#}");
                self.set_error(format!("Could not back up your list: {error:#}"), cx);
            }
        }
    }

    /// Uploads the list kept on this computer to the account. The merge
    /// waits for the account's list to be fetched, so it is made against
    /// what the account has now.
    pub fn upload_local_list(&mut self, cx: &mut Context<Self>) {
        if self.state().is_local() || !self.startup.has_local_list() {
            return;
        }
        self.pending_upload = true;
        self.set_status("Uploading the list from this computer...", cx);
        // A sync already running picks the upload up at its fetch.
        if !self.syncing {
            self.synchronize(cx);
        }
    }

    /// Merges the local list into the freshly fetched account list. True
    /// when something was queued to send.
    pub(super) fn upload_now(&mut self, cx: &mut Context<Self>) -> bool {
        let loaded = match self.startup.load_local_list() {
            Ok(loaded) => loaded,
            Err(error) => {
                tracing::error!("Could not read the local list: {error:#}");
                self.set_error(
                    format!("Could not read the list on this computer: {error:#}"),
                    cx,
                );
                return false;
            }
        };
        let result = upload::merge(
            &mut self.startup.state,
            &loaded.anime,
            loaded.entries.into_values(),
            Timestamp::now(),
        );
        match result {
            Ok((merged, events)) => {
                self.emit(events, cx);
                let service = self.service_name();
                self.set_status(
                    format!(
                        "Uploading to {service}: {} new, {} updated.",
                        merged.added, merged.updated
                    ),
                    cx,
                );
                merged.added + merged.updated > 0
            }
            Err(error) => {
                self.apply(Err(error), cx);
                false
            }
        }
    }

    /// Saves a backup where the user chose.
    pub fn back_up_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match self.startup.state.back_up(&path) {
            Ok(()) => self.set_status(format!("Backed up your list to {}.", path.display()), cx),
            Err(error) => {
                tracing::error!("Could not back up to {}: {error:#}", path.display());
                self.set_error(format!("Could not back up your list: {error:#}"), cx);
            }
        }
    }

    /// Replaces the local list with a backup. The list as it was is saved
    /// first, so restoring the wrong file can be undone.
    pub fn restore(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.state().is_local() {
            return;
        }
        let saved = backup::before_restore(&self.startup.folders.backups());
        if let Err(error) = self.state().back_up(&saved) {
            tracing::error!("Could not save the list before restoring: {error:#}");
            self.set_error(format!("Could not save your list first: {error:#}"), cx);
            return;
        }
        match self.startup.state.restore(&path) {
            Ok(()) => {
                self.list_replaced(cx);
                self.set_status(format!("Restored your list from {}.", path.display()), cx);
            }
            Err(error) => {
                tracing::error!("Could not restore {}: {error:#}", path.display());
                self.set_error(format!("Could not restore: {error:#}"), cx);
            }
        }
    }

    /// Fetches the list's followed anime again, 50 to a request. `asked`
    /// is the user's "Refresh anime data": every followed anime is fetched
    /// and the outcome is shown. Otherwise only stale ones are, quietly,
    /// since a missing connection is no news to the user.
    pub fn refresh_anime_data(&mut self, asked: bool, cx: &mut Context<Self>) {
        if self.refreshing {
            return;
        }
        let ids = anime_to_refresh(
            self.state(),
            Timestamp::now(),
            format::today_in_japan(),
            asked,
        );
        if ids.is_empty() {
            if asked {
                self.set_status("Anime data is up to date.", cx);
            }
            return;
        }
        if asked {
            self.set_status(format!("Refreshing {} anime...", ids.len()), cx);
        }
        self.refreshing = true;
        self.refresh_batch(ids, 0, asked, cx);
    }

    /// One request at a time, so a large list stays within AniList's rate
    /// limit.
    fn refresh_batch(
        &mut self,
        mut ids: Vec<AnimeId>,
        done: usize,
        asked: bool,
        cx: &mut Context<Self>,
    ) {
        let rest = ids.split_off(ids.len().min(AniList::IDS_PER_REQUEST));
        let service = self.service().clone();
        self.request(
            async move { service.fetch_anime_by_ids(&ids).await },
            cx,
            move |this, result, cx| match result {
                Ok(anime) => {
                    let done = done + anime.len();
                    let result = this.startup.state.store_anime(anime);
                    this.apply(result, cx);
                    if rest.is_empty() {
                        this.refreshing = false;
                        if asked {
                            this.set_status(format!("Refreshed {done} anime."), cx);
                        }
                    } else {
                        this.refresh_batch(rest, done, asked, cx);
                    }
                }
                Err(error) => {
                    this.refreshing = false;
                    tracing::warn!("Could not refresh anime data: {error}");
                    if asked {
                        this.set_error(format!("Could not refresh anime data: {error}"), cx);
                    }
                }
            },
        );
    }
}

/// When the newest automatic backup was made, for Settings and the sidebar.
pub fn backed_up(last: Option<jiff::civil::Date>) -> String {
    let Some(last) = last else {
        return "Not backed up yet".to_owned();
    };
    let today = jiff::Zoned::now().date();
    if last == today {
        "Backed up today".to_owned()
    } else if last.tomorrow().ok() == Some(today) {
        "Backed up yesterday".to_owned()
    } else {
        format!("Backed up {}", last.strftime("%-d %b %Y"))
    }
}

/// Nothing is chosen yet and there is no account that would answer for the
/// user, so the first start asks.
pub fn should_ask(model: &Model) -> bool {
    model.state().settings.list.is_none() && !model.has_account()
}

type Run = Rc<dyn Fn(&mut Window, &mut App)>;

/// A dialog with a button per choice and Cancel. Each choice closes it.
fn choose(
    title: &'static str,
    text: String,
    choices: Vec<(&'static str, Run)>,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, _, _| {
        let last = choices.len().saturating_sub(1);
        let buttons = choices.iter().enumerate().map(|(ix, (label, run))| {
            let run = run.clone();
            Button::new(("choice", ix))
                .label(*label)
                .when_primary(ix == last)
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    run(window, cx);
                })
        });
        dialog.title(title).child(div().child(text.clone())).footer(
            h_flex()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("cancel")
                        .label("Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .children(buttons),
        )
    });
}

trait WhenPrimary {
    fn when_primary(self, primary: bool) -> Self;
}

impl WhenPrimary for Button {
    fn when_primary(self, primary: bool) -> Self {
        if primary { self.primary() } else { self }
    }
}

/// The first-start question.
pub fn ask_first(model: &Entity<Model>, window: &mut Window, cx: &mut App) {
    let service = model.read(cx).service_name();
    let account: Run = {
        let model = model.clone();
        Rc::new(move |window, cx| {
            model.update(cx, |model, cx| {
                model.move_list(ListLocation::Account, false, cx)
            });
            pages::settings::open(&model, window, cx);
        })
    };
    let local: Run = {
        let model = model.clone();
        Rc::new(move |window, cx| to_local(&model, window, cx))
    };
    choose(
        "Where should your list live?",
        format!("Anime info comes from {service} either way. You can change this in Settings."),
        vec![
            ("In my AniList account", account),
            ("On this computer", local),
        ],
        window,
        cx,
    );
}

/// Moves to this computer, asking first whether a new local list starts
/// with a copy of the current one.
fn to_local(model: &Entity<Model>, window: &mut Window, cx: &mut App) {
    let (has_local, count, service) = {
        let model = model.read(cx);
        let count = model
            .state()
            .entries
            .values()
            .filter(|e| is_in_list(Some(e)))
            .count();
        (model.startup.has_local_list(), count, model.service_name())
    };
    let start = move |copy: bool, model: &Entity<Model>| -> Run {
        let model = model.clone();
        Rc::new(move |_, cx| {
            model.update(cx, |model, cx| {
                model.move_list(ListLocation::Local, copy, cx)
            })
        })
    };
    if has_local || count == 0 {
        start(false, model)(window, cx);
        return;
    }
    choose(
        "Start your local list",
        format!("Copy the {count} anime from your {service} list, or start empty."),
        vec![
            ("Start empty", start(false, model)),
            ("Copy my list", start(true, model)),
        ],
        window,
        cx,
    );
}

/// Moves the list from Settings, after asking.
pub fn change(model: &Entity<Model>, to: ListLocation, window: &mut Window, cx: &mut App) {
    let (current, has_local, service, count, signed_in) = {
        let model = model.read(cx);
        let count = model
            .state()
            .entries
            .values()
            .filter(|e| is_in_list(Some(e)))
            .count();
        (
            model.state().settings.list,
            model.startup.has_local_list(),
            model.service_name(),
            count,
            model.account().token.is_some(),
        )
    };
    if current == Some(to) {
        return;
    }
    let model = model.clone();
    match to {
        // Nothing was chosen yet, so the list already is the account's;
        // choosing only records it.
        ListLocation::Account if current.is_none() => model.update(cx, |model, cx| {
            model.move_list(ListLocation::Account, false, cx)
        }),
        ListLocation::Local if !has_local => to_local(&model, window, cx),
        ListLocation::Local => choose(
            "Use the list on this computer?",
            format!("Your {service} list stays as it is."),
            vec![(
                "Use this computer",
                Rc::new(move |window, cx| to_local(&model, window, cx)),
            )],
            window,
            cx,
        ),
        ListLocation::Account => {
            let use_account = |upload: bool| -> Run {
                let model = model.clone();
                Rc::new(move |_, cx| {
                    model.update(cx, |model, cx| {
                        model.move_list(ListLocation::Account, false, cx);
                        if upload {
                            model.upload_local_list(cx);
                        }
                    })
                })
            };
            let (text, choices) = if count == 0 {
                (
                    "The list on this computer is kept, so you can switch back.".to_owned(),
                    vec![("Use AniList", use_account(false))],
                )
            } else if signed_in {
                (
                    format!(
                        "Upload the {count} anime on this computer to {service}? Nothing on \
                         {service} is removed, and the list on this computer is kept."
                    ),
                    vec![
                        ("Don't upload", use_account(false)),
                        ("Upload my list", use_account(true)),
                    ],
                )
            } else {
                (
                    format!(
                        "To upload the {count} anime on this computer, sign in to {service} \
                         first; Settings then offers to upload them. The list on this \
                         computer is kept."
                    ),
                    vec![("Use AniList", use_account(false))],
                )
            };
            choose("Use your AniList account?", text, choices, window, cx);
        }
    }
}

/// Uploads the list on this computer from Settings, after asking.
pub fn ask_upload(model: &Entity<Model>, window: &mut Window, cx: &mut App) {
    let (count, service) = {
        let model = model.read(cx);
        let count = model.startup.load_local_list().map_or(0, |loaded| {
            loaded
                .entries
                .values()
                .filter(|e| is_in_list(Some(e)))
                .count()
        });
        (count, model.service_name())
    };
    if count == 0 {
        model.update(cx, |model, cx| {
            model.set_status("The list on this computer is empty.", cx)
        });
        return;
    }
    let model = model.clone();
    choose(
        "Upload the list from this computer?",
        format!(
            "{count} anime are added to {service}, or updated where yours are further along. \
             Nothing on {service} is removed."
        ),
        vec![(
            "Upload",
            Rc::new(move |_, cx| model.update(cx, |model, cx| model.upload_local_list(cx))),
        )],
        window,
        cx,
    );
}
