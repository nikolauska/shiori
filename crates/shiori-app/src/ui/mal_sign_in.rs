//! Signing in to MyAnimeList: the browser opens, a listener on the loopback
//! address catches the code, and pasting the address is the fallback
//! (decision I35, mockup 2 of the MyAnimeList UI).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Window, div, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::spinner::Spinner;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use shiori_sync::myanimelist::auth::{self, AuthError, Authorization, Listener};
use shiori_sync::{Account, Service as _};

use super::model::Model;

/// How long the listener waits before the dialog says it gave up.
const WAIT: Duration = Duration::from_secs(10 * 60);

/// Opens the sign-in dialog and the browser.
pub fn open(model: &Entity<Model>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| SignIn::new(model.clone(), window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title("Sign in to MyAnimeList")
            .w(px(440.))
            .child(view.clone())
    });
}

enum Phase {
    Waiting,
    Exchanging,
    Done(String),
    Failed(String),
}

pub struct SignIn {
    model: Entity<Model>,
    authorization: Authorization,
    /// Stops the listener when the dialog goes away.
    cancel: Arc<AtomicBool>,
    phase: Phase,
    /// The listener could not start; the paste field is the only way.
    no_listener: Option<String>,
    show_paste: bool,
    paste: Entity<InputState>,
}

impl Drop for SignIn {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl SignIn {
    fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let authorization = Authorization::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let paste = cx.new(|cx| {
            InputState::new(window, cx).placeholder("http://127.0.0.1:47391/callback?code=…")
        });
        let mut this = Self {
            model,
            authorization,
            cancel,
            phase: Phase::Waiting,
            no_listener: None,
            show_paste: false,
            paste,
        };
        this.start(cx);
        this
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        cx.open_url(&self.authorization.url);
        match Listener::bind() {
            Ok(listener) => self.listen(listener, cx),
            Err(error) => {
                tracing::warn!("{error}");
                self.no_listener = Some(error.to_string());
                self.show_paste = true;
            }
        }
    }

    fn listen(&mut self, listener: Listener, cx: &mut Context<Self>) {
        let state = self.authorization.state.clone();
        let cancel = self.cancel.clone();
        let task = cx.background_spawn(async move { listener.wait(&state, WAIT, &cancel) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(code) => this.exchange(code, cx),
                // The dialog is closing; nobody to tell.
                Err(AuthError::Cancelled) => {}
                Err(error) => this.fail(error.to_string(), cx),
            });
        })
        .detach();
    }

    fn submit_paste(&mut self, cx: &mut Context<Self>) {
        let text = self.paste.read(cx).value().to_string();
        match auth::code_from_input(&text, &self.authorization.state) {
            Ok(code) => {
                // The browser may still come back to the listener; one code
                // is enough.
                self.cancel.store(true, Ordering::Relaxed);
                self.exchange(code, cx);
            }
            Err(error) => self.fail(error.to_string(), cx),
        }
    }

    fn exchange(&mut self, code: String, cx: &mut Context<Self>) {
        if matches!(self.phase, Phase::Exchanging | Phase::Done(_)) {
            return;
        }
        self.phase = Phase::Exchanging;
        cx.notify();
        let verifier = self.authorization.verifier.clone();
        let view = cx.entity().downgrade();
        self.model.update(cx, |model, cx| {
            model.finish_mal_sign_in(code, verifier, cx, move |result, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.phase = match result {
                        Ok(username) => Phase::Done(username),
                        Err(error) => Phase::Failed(error),
                    };
                    cx.notify();
                });
            });
        });
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.phase = Phase::Failed(message);
        self.show_paste = true;
        cx.notify();
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        // A fresh attempt: the old state and verifier must not be reused.
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.authorization = Authorization::new();
        self.phase = Phase::Waiting;
        self.no_listener = None;
        self.show_paste = false;
        self.start(cx);
        cx.notify();
    }
}

impl Render for SignIn {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, success, danger) = (
            theme.muted_foreground,
            theme.border,
            theme.success,
            theme.danger,
        );
        let (icon, title, detail): (gpui::AnyElement, SharedString, SharedString) = match &self
            .phase
        {
            Phase::Waiting if self.no_listener.is_some() => (
                Icon::new(IconName::TriangleAlert)
                    .text_color(theme.warning)
                    .into_any_element(),
                "Shiori couldn't listen for MyAnimeList's answer".into(),
                "Approve Shiori in the browser, then paste the address it ends up on below.".into(),
            ),
            Phase::Waiting => (
                Spinner::new().into_any_element(),
                "Waiting for MyAnimeList…".into(),
                "Approve Shiori in the browser tab that just opened.".into(),
            ),
            Phase::Exchanging => (
                Spinner::new().into_any_element(),
                "Signing in…".into(),
                "MyAnimeList answered; checking the account.".into(),
            ),
            Phase::Done(name) => (
                Icon::new(IconName::CircleCheck)
                    .text_color(success)
                    .into_any_element(),
                format!("Logged in as {name}.").into(),
                "Your MyAnimeList list is being synchronized.".into(),
            ),
            Phase::Failed(message) => (
                Icon::new(IconName::CircleX)
                    .text_color(danger)
                    .into_any_element(),
                "Could not sign in".into(),
                message.clone().into(),
            ),
        };
        let header = h_flex()
            .gap_3()
            .items_center()
            .child(div().flex_none().child(icon))
            .child(
                v_flex()
                    .min_w_0()
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
                    .child(div().text_sm().text_color(muted).child(detail)),
            );
        let done = matches!(self.phase, Phase::Done(_));
        let busy = matches!(self.phase, Phase::Exchanging);
        let actions = h_flex()
            .gap_2()
            .when(!done && !busy, |row| {
                let url = self.authorization.url.clone();
                row.child(
                    Button::new("open-again")
                        .small()
                        .icon(IconName::ExternalLink)
                        .label("Open the page again")
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
            })
            .when(matches!(self.phase, Phase::Failed(_)), |row| {
                row.child(
                    Button::new("retry")
                        .small()
                        .label("Start over")
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                )
            });
        let fallback = (!done && !busy).then(|| {
            let toggle = Button::new("didnt-work")
                .small()
                .ghost()
                .icon(if self.show_paste {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .label("Didn't work?")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_paste = !this.show_paste;
                    cx.notify();
                }));
            v_flex()
                .gap_2()
                .pt_2()
                .border_t_1()
                .border_color(border)
                .child(h_flex().child(toggle))
                .when(self.show_paste, |section| {
                    section
                        .child(div().text_sm().text_color(muted).child(
                            "If the browser says it can't connect after you approve, copy the \
                             whole address from its address bar and paste it here.",
                        ))
                        .child(Input::new(&self.paste).small())
                        .child(
                            h_flex().justify_end().child(
                                Button::new("paste-sign-in")
                                    .small()
                                    .primary()
                                    .label("Sign in")
                                    .on_click(cx.listener(|this, _, _, cx| this.submit_paste(cx))),
                            ),
                        )
                })
        });
        let _ = window;
        v_flex()
            .gap_4()
            .child(header)
            .child(actions)
            .children(fallback)
            .child(
                h_flex().justify_end().child(
                    Button::new("close")
                        .when(done, |b| b.primary())
                        .when(!done, |b| b.ghost())
                        .label(if done { "Done" } else { "Cancel" })
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    }
}

impl Model {
    /// Trades the code for tokens, stores them and signs in. `done` gets the
    /// username, or the message to show.
    pub fn finish_mal_sign_in(
        &mut self,
        code: String,
        verifier: String,
        cx: &mut Context<Self>,
        done: impl FnOnce(Result<String, String>, &mut App) + 'static,
    ) {
        let Some(service) = self.service().as_myanimelist().cloned() else {
            cx.defer(|cx| done(Err("MyAnimeList is not the active service.".into()), cx));
            return;
        };
        self.set_status("Authenticating with MyAnimeList...", cx);
        self.request(
            async move {
                let tokens = service.exchange_code(&code, &verifier).await?;
                let account = Account {
                    username: String::new(),
                    token: Some(tokens.access.clone()),
                    refresh_token: Some(tokens.refresh.clone()),
                };
                let info = service.authenticate(&account).await?;
                Ok((tokens, info))
            },
            cx,
            move |this, result: Result<_, shiori_sync::ServiceError>, cx| match result {
                Ok((tokens, info)) => {
                    if let Err(error) = this.startup.store_tokens(&tokens) {
                        let message = format!("Could not store the token: {error:#}");
                        this.set_error(message.clone(), cx);
                        cx.defer(move |cx| done(Err(message), cx));
                        return;
                    }
                    let events = this.startup.state.apply_sign_in(&info);
                    if let Err(error) = this.startup.save_accounts() {
                        tracing::error!("Could not save accounts: {error:#}");
                    }
                    this.set_status(format!("Logged in as {}.", info.username), cx);
                    this.emit(events, cx);
                    this.synchronize(cx);
                    let name = info.username;
                    cx.defer(move |cx| done(Ok(name), cx));
                }
                Err(error) => {
                    let message = error.to_string();
                    this.service_error(error, cx);
                    cx.defer(move |cx| done(Err(message), cx));
                }
            },
        );
    }
}
