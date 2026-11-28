//! Where the list lives: on this computer only, or in an account on the
//! list service. The first-start question and moving the list later.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, StyledExt as _, WindowExt as _, h_flex, v_flex,
};
use std::path::PathBuf;

use jiff::Timestamp;
use shiori_app::refresh::anime_to_refresh;
use shiori_app::state::AppEvent;
use shiori_app::{backup, upload};
use shiori_core::entry::is_in_list;
use shiori_core::{AnimeId, ServiceId};
use shiori_store::ListLocation;
use shiori_sync::anilist::AniList;

use super::format;
use super::icons::AppIcon;
use super::model::Model;
use super::pages;

impl Model {
    /// There is an account to synchronize with, signed in or not.
    pub fn has_account(&self) -> bool {
        let account = self.account();
        !account.username.is_empty() || account.token.is_some()
    }

    /// Makes `service`'s account list the current one, from this computer's
    /// list or from another service's account.
    pub fn use_account(&mut self, service: ServiceId, cx: &mut Context<Self>) -> bool {
        if self.syncing {
            self.set_error("Wait for synchronization to finish.", cx);
            return false;
        }
        if let Err(error) = self.startup.use_account(service) {
            tracing::error!("Could not open the list: {error:#}");
            self.set_error(format!("Could not open the list: {error:#}"), cx);
            return false;
        }
        if let Err(error) = self.rebuild_clients() {
            self.set_error(format!("Could not reach {}: {error:#}", service.name()), cx);
        }
        self.list_replaced(cx);
        self.set_status(format!("Using your {} list.", service.name()), cx);
        if self.state().settings.sync_enabled && self.account().token.is_some() {
            self.synchronize(cx);
        }
        true
    }

    /// Removes the active service's tokens. The username and the list stay;
    /// a public list can still be read.
    pub fn sign_out(&mut self, cx: &mut Context<Self>) {
        if self.syncing {
            self.set_error("Wait for synchronization to finish.", cx);
            return;
        }
        if let Err(error) = self.startup.forget_tokens() {
            self.set_error(format!("Could not remove the token: {error:#}"), cx);
            return;
        }
        if let Some(service) = self.service().as_myanimelist() {
            service.forget_tokens();
        }
        self.startup.state.signed_in = false;
        let name = self.service_name();
        self.emit(vec![AppEvent::AuthenticationChanged(false)], cx);
        self.set_status(format!("Signed out of {name}."), cx);
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
        let today = jiff::ZonedDateTime::now().date();
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
        // A MyAnimeList account uses other ids than the local list.
        let (anime, entries, skipped) = if self.state().service == ServiceId::MyAnimeList {
            upload::to_mal_ids(&loaded.anime, loaded.entries.into_values())
        } else {
            (
                loaded.anime,
                loaded.entries.into_values().collect(),
                Vec::new(),
            )
        };
        if !skipped.is_empty() {
            tracing::info!("Not uploaded, no MyAnimeList id: {}", skipped.join(", "));
        }
        let result = upload::merge(&mut self.startup.state, &anime, entries, Timestamp::now());
        match result {
            Ok((merged, events)) => {
                self.emit(events, cx);
                let service = self.service_name();
                let left_out = match skipped.len() {
                    0 => String::new(),
                    1 => format!(" Not on {service}: {}.", skipped[0]),
                    n => format!(" {n} anime are not on {service}."),
                };
                self.set_status(
                    format!(
                        "Uploading to {service}: {} new, {} updated.{left_out}",
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
            Ok(()) => self.saved("Backed up your list", &path, None, cx),
            Err(error) => {
                tracing::error!("Could not back up to {}: {error:#}", path.display());
                self.set_error(format!("Could not back up your list: {error:#}"), cx);
            }
        }
    }

    /// Tells the main window a file was written, by its name only; the
    /// notification opens its folder.
    pub(super) fn saved(
        &mut self,
        title: &str,
        path: &std::path::Path,
        detail: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let folder = path.parent().map(PathBuf::from).unwrap_or_default();
        let detail = detail.unwrap_or_else(|| file_name(path));
        self.set_status(format!("{title}: {}.", file_name(path)), cx);
        cx.emit(super::model::UiRequest::Saved {
            title: title.to_owned().into(),
            detail: detail.into(),
            folder,
        });
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
                self.set_status(format!("Restored your list from {}.", file_name(&path)), cx);
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
        let service = self.anilist().clone();
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

impl Model {
    /// For a MyAnimeList list: copies next-episode times from AniList, 50
    /// anime to a request, one request at a time. Quiet on failure; the
    /// schedule is then only as fresh as the last time this worked.
    pub(super) fn borrow_airing_times(&mut self, cx: &mut Context<Self>) {
        if self.state().catalog_service() != ServiceId::MyAnimeList {
            return;
        }
        let ids = shiori_app::airing::ids_to_look_up(self.state());
        self.airing_batch(ids, cx);
    }

    fn airing_batch(&mut self, mut ids: Vec<u32>, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        let rest = ids.split_off(ids.len().min(AniList::IDS_PER_REQUEST));
        let service = self.anilist().clone();
        self.request(
            async move { service.fetch_anime_by_mal_ids(&ids).await },
            cx,
            move |this, result, cx| match result {
                Ok(found) => {
                    let updated =
                        shiori_app::airing::with_airing_times(&this.state().anime, &found);
                    if !updated.is_empty() {
                        let result = this.startup.state.store_anime(updated);
                        this.apply(result, cx);
                    }
                    this.airing_batch(rest, cx);
                }
                Err(error) => tracing::warn!("Could not get airing times from AniList: {error}"),
            },
        );
    }
}

/// A path as its file name, which is what people recognize.
pub fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// When the newest automatic backup was made, for Settings and the sidebar.
pub fn backed_up(last: Option<jiff::civil::Date>) -> String {
    let Some(last) = last else {
        return "Not backed up yet".to_owned();
    };
    let today = jiff::ZonedDateTime::now().date();
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

/// A number worth seeing before choosing, such as how many anime a list has.
struct Stat {
    icon: AppIcon,
    value: String,
    label: String,
}

/// One answer: an icon, a few words, and what choosing it does.
struct Card {
    icon: AppIcon,
    title: String,
    hint: String,
    /// The suggested answer, drawn out.
    primary: bool,
    run: Run,
}

fn card(icon: AppIcon, title: &str, hint: impl Into<String>, primary: bool, run: Run) -> Card {
    Card {
        icon,
        title: title.to_owned(),
        hint: hint.into(),
        primary,
        run,
    }
}

fn stat(icon: AppIcon, value: impl Into<String>, label: impl Into<String>) -> Stat {
    Stat {
        icon,
        value: value.into(),
        label: label.into(),
    }
}

/// A question answered by clicking a card. Stats show what is at stake;
/// two of them are joined by an arrow, as "from → to".
fn ask(title: &'static str, stats: Vec<Stat>, cards: Vec<Card>, window: &mut Window, cx: &mut App) {
    let stats = Rc::new(stats);
    let cards = Rc::new(cards);
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme();
        let (border, muted, muted_fg, primary) = (
            theme.border,
            theme.muted,
            theme.muted_foreground,
            theme.primary,
        );
        let tiles = stats.iter().enumerate().flat_map(|(ix, s)| {
            let tile = v_flex()
                .flex_1()
                .items_center()
                .gap_1()
                .p_3()
                .rounded_lg()
                .bg(muted)
                .child(Icon::new(s.icon).size_5().text_color(muted_fg))
                .child(div().text_2xl().font_bold().child(s.value.clone()))
                .child(div().text_xs().text_color(muted_fg).child(s.label.clone()))
                .into_any_element();
            let arrow = (ix > 0).then(|| {
                Icon::new(AppIcon::ArrowRight)
                    .size_5()
                    .text_color(muted_fg)
                    .into_any_element()
            });
            arrow.into_iter().chain([tile])
        });
        let choices = cards.iter().enumerate().map(|(ix, c)| {
            let run = c.run.clone();
            h_flex()
                .id(("card", ix))
                .gap_3()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(if c.primary { primary } else { border })
                .cursor_pointer()
                .hover(|card| card.bg(muted))
                .child(
                    div()
                        .size_10()
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .map(|tile| {
                            if c.primary {
                                tile.bg(primary).text_color(theme.primary_foreground)
                            } else {
                                tile.bg(muted).text_color(muted_fg)
                            }
                        })
                        .child(Icon::new(c.icon).size_5()),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().font_semibold().child(c.title.clone()))
                        .child(div().text_sm().text_color(muted_fg).child(c.hint.clone())),
                )
                .child(
                    Icon::new(IconName::ChevronRight)
                        .size_4()
                        .text_color(muted_fg),
                )
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    run(window, cx);
                })
        });
        dialog
            .title(title)
            .width(px(440.))
            .child(
                v_flex()
                    .gap_4()
                    .when(!stats.is_empty(), |body| {
                        body.child(h_flex().gap_3().items_center().children(tiles))
                    })
                    .child(v_flex().gap_2().children(choices)),
            )
            .footer(
                h_flex().justify_end().child(
                    Button::new("cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}

/// The first-start question.
pub fn ask_first(model: &Entity<Model>, window: &mut Window, cx: &mut App) {
    let account: Run = {
        let model = model.clone();
        Rc::new(move |window, cx| {
            model.update(cx, |model, cx| {
                model.use_account(ServiceId::AniList, cx);
            });
            pages::settings::open(&model, window, cx);
        })
    };
    // Straight to signing in (decision I39); cancelling it leaves the user
    // in Settings with MyAnimeList chosen.
    let myanimelist: Run = {
        let model = model.clone();
        Rc::new(move |window, cx| {
            let switched = model.update(cx, |model, cx| {
                model.use_account(ServiceId::MyAnimeList, cx)
            });
            if switched {
                pages::settings::open(&model, window, cx);
                super::mal_sign_in::open(&model, window, cx);
            }
        })
    };
    let local: Run = {
        let model = model.clone();
        Rc::new(move |window, cx| to_local(&model, window, cx))
    };
    ask(
        "Where should your list live?",
        Vec::new(),
        vec![
            card(
                AppIcon::Laptop,
                "On this computer",
                "No account needed",
                true,
                local,
            ),
            card(
                AppIcon::Cloud,
                "In my AniList account",
                "Synced with AniList",
                false,
                account,
            ),
            card(
                AppIcon::Cloud,
                "In my MyAnimeList account",
                "Synced with MyAnimeList",
                false,
                myanimelist,
            ),
        ],
        window,
        cx,
    );
}

fn list_count(model: &Model) -> usize {
    model
        .state()
        .entries
        .values()
        .filter(|e| is_in_list(Some(e)))
        .count()
}

/// Moves to this computer, asking first whether a new local list starts
/// with a copy of the current one.
fn to_local(model: &Entity<Model>, window: &mut Window, cx: &mut App) {
    let (has_local, count, service) = {
        let model = model.read(cx);
        (
            model.startup.has_local_list(),
            list_count(model),
            model.service_name(),
        )
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
    ask(
        "Start your local list",
        vec![stat(
            AppIcon::Cloud,
            count.to_string(),
            format!("on {service}"),
        )],
        vec![
            card(
                AppIcon::Copy,
                "Copy my list",
                format!("Your {service} list as it is now"),
                true,
                start(true, model),
            ),
            card(
                AppIcon::FilePlus,
                "Start empty",
                "Add anime as you go",
                false,
                start(false, model),
            ),
        ],
        window,
        cx,
    );
}

/// Where Settings can put the list: this computer, or one service's account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Local,
    Account(ServiceId),
}

/// Where the list is now, as Settings shows it. Before the user has chosen,
/// an existing account means the list is already the account's.
pub fn current_place(model: &Model) -> Place {
    let state = model.state();
    match state.settings.list {
        Some(ListLocation::Local) => Place::Local,
        Some(ListLocation::Account) => Place::Account(state.service),
        None if model.has_account() => Place::Account(state.service),
        None => Place::Local,
    }
}

/// Moves the list from Settings, after asking.
pub fn change(model: &Entity<Model>, to: Place, window: &mut Window, cx: &mut App) {
    let (current, chosen, has_local, active, count) = {
        let model = model.read(cx);
        (
            current_place(model),
            model.state().settings.list,
            model.startup.has_local_list(),
            model.state().service,
            list_count(model),
        )
    };
    if current == to {
        return;
    }
    let model = model.clone();
    let service = match to {
        Place::Local if !has_local => return to_local(&model, window, cx),
        Place::Local => {
            let name = active.name();
            return ask(
                "Use the list on this computer?",
                Vec::new(),
                vec![card(
                    AppIcon::Laptop,
                    "Use this computer",
                    format!("Your {name} list stays as it is"),
                    true,
                    Rc::new(move |window, cx| to_local(&model, window, cx)),
                )],
                window,
                cx,
            );
        }
        Place::Account(service) => service,
    };
    // Nothing was chosen yet, so the list already is the account's;
    // choosing only records it.
    if chosen.is_none() && service == active {
        model.update(cx, |model, cx| {
            model.move_list(ListLocation::Account, false, cx)
        });
        return;
    }
    let name = service.name();
    let signed_in = model.read(cx).startup.has_token(service);
    // Switches, then signs in or uploads as asked. Signing in to
    // MyAnimeList is its own dialog; AniList's token field is in Settings.
    let use_account = |upload: bool| -> Run {
        let model = model.clone();
        Rc::new(move |window, cx| {
            let switched = model.update(cx, |model, cx| {
                let switched = model.use_account(service, cx);
                if switched && upload {
                    model.upload_local_list(cx);
                }
                switched
            });
            if switched && !signed_in && service == ServiceId::MyAnimeList {
                super::mal_sign_in::open(&model, window, cx);
            }
        })
    };
    if let Place::Account(from) = current {
        // From one account to another: nothing is copied, each service has
        // its own list (decision I40).
        let from_count = count;
        return ask(
            if signed_in {
                "Switch to your other account?"
            } else {
                "Switch service?"
            },
            vec![
                stat(AppIcon::Cloud, from.name(), format!("{from_count} anime")),
                stat(
                    AppIcon::Cloud,
                    name,
                    if signed_in {
                        "signed in"
                    } else {
                        "not signed in"
                    },
                ),
            ],
            vec![card(
                if signed_in {
                    AppIcon::Cloud
                } else {
                    AppIcon::LogIn
                },
                &format!("Use {name}"),
                format!(
                    "Your {} list stays as it is. Library links, numbering and history are \
                     kept per service.",
                    from.name()
                ),
                true,
                use_account(false),
            )],
            window,
            cx,
        );
    }
    // From this computer. A MyAnimeList account needs MyAnimeList ids
    // (decision I38); anime without one stay here, and the hint says so.
    let (uploadable, left_out) = if service == ServiceId::MyAnimeList {
        let state = model.read(cx).state();
        let (_, entries, skipped) =
            shiori_app::upload::to_mal_ids(&state.anime, state.entries.values().cloned());
        (entries.len(), skipped.len())
    } else {
        (count, 0)
    };
    let upload_hint = match left_out {
        0 => format!("Adds what {name} lacks. Removes nothing."),
        1 => format!("Adds what {name} lacks. 1 anime is not on {name} and stays here."),
        n => format!("Adds what {name} lacks. {n} anime are not on {name} and stay here."),
    };
    let here = stat(AppIcon::Laptop, count.to_string(), "on this computer");
    let there = stat(
        AppIcon::Cloud,
        uploadable.to_string(),
        format!("can go to {name}"),
    );
    let (stats, cards) = if count == 0 {
        (
            Vec::new(),
            vec![card(
                AppIcon::Cloud,
                &format!("Use {name}"),
                "You can switch back any time",
                true,
                use_account(false),
            )],
        )
    } else if signed_in {
        (
            vec![here, there],
            vec![
                card(
                    AppIcon::CloudUpload,
                    "Upload my list",
                    upload_hint,
                    true,
                    use_account(true),
                ),
                card(
                    AppIcon::Cloud,
                    &format!("Just use {name}"),
                    "Keep the list on this computer apart",
                    false,
                    use_account(false),
                ),
            ],
        )
    } else {
        (
            vec![here, there],
            vec![card(
                AppIcon::LogIn,
                &format!("Use {name}"),
                "Sign in, then upload from Settings",
                true,
                use_account(false),
            )],
        )
    };
    ask(
        if name == "AniList" {
            "Use your AniList account?"
        } else {
            "Use your MyAnimeList account?"
        },
        stats,
        cards,
        window,
        cx,
    );
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
    ask(
        "Upload the list from this computer?",
        vec![
            stat(AppIcon::Laptop, count.to_string(), "on this computer"),
            stat(AppIcon::Cloud, service, "your account"),
        ],
        vec![card(
            AppIcon::CloudUpload,
            "Upload",
            format!("Adds what {service} lacks. Removes nothing."),
            true,
            Rc::new(move |_, cx| model.update(cx, |model, cx| model.upload_local_list(cx))),
        )],
        window,
        cx,
    );
}
