//! Where the list lives: on this computer only, or in an account on the
//! list service. The first-start question and moving the list later.

use std::rc::Rc;

use gpui::{App, Context, Entity, ParentElement as _, Styled as _, Window, div};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{WindowExt as _, h_flex};
use shiori_app::state::AppEvent;
use shiori_core::entry::is_in_list;
use shiori_store::ListLocation;

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
        // Everything below was about anime in the list that was just closed.
        self.selected = None;
        self.tracker.suggestion = None;
        if let Some(media) = self.tracker.media.clone() {
            self.media_found(Some(media), cx);
        }
        self.recognize_library_again(cx);
        self.emit(vec![AppEvent::ListReplaced, AppEvent::SettingsChanged], cx);
        let service = self.service_name();
        match to {
            ListLocation::Local => self.set_status("Your list is now kept on this computer.", cx),
            ListLocation::Account => {
                self.set_status(format!("Using your {service} list."), cx);
                if self.state().settings.sync_enabled {
                    self.synchronize(cx);
                }
            }
        }
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
    let (current, has_local, service) = {
        let model = model.read(cx);
        (
            model.state().settings.list,
            model.startup.has_local_list(),
            model.service_name(),
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
        ListLocation::Account => choose(
            "Use your AniList account?",
            "The list on this computer is kept, so you can switch back.".into(),
            vec![(
                "Use AniList",
                Rc::new(move |_, cx| {
                    model.update(cx, |model, cx| {
                        model.move_list(ListLocation::Account, false, cx)
                    })
                }),
            )],
            window,
            cx,
        ),
    }
}
