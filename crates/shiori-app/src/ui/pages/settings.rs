//! Settings. Starts with the account, so that signing in works from the
//! window; the other sections follow.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
use shiori_sync::anilist;

use crate::ui::model::Model;

pub struct SettingsPage {
    model: Entity<Model>,
    token: Entity<InputState>,
}

impl SettingsPage {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste the token from AniList")
                .masked(true)
        });
        Self { model, token }
    }

    fn sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.token.read(cx).value().trim().to_owned();
        if token.is_empty() {
            return;
        }
        self.token
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.model.update(cx, |model, cx| model.sign_in(token, cx));
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.model.read(cx).state();
        let account = &state.accounts.anilist;
        let who = if account.username.is_empty() {
            "Not signed in.".to_owned()
        } else if state.signed_in {
            format!("Signed in as {}.", account.username)
        } else {
            format!("Account: {}.", account.username)
        };
        v_flex()
            .p_4()
            .gap_3()
            .max_w(gpui::px(640.))
            .child(div().text_xl().child("Settings"))
            .child(div().text_lg().child("AniList account"))
            .child(div().text_color(cx.theme().muted_foreground).child(who))
            .child(div().text_sm().child(
                "Click Authorize, approve Shiori on AniList, then paste the token it shows here.",
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("authorize")
                            .label("Authorize")
                            .on_click(|_, _, cx| cx.open_url(&anilist::authorize_url())),
                    )
                    .child(div().flex_1().child(Input::new(&self.token)))
                    .child(
                        Button::new("sign-in")
                            .primary()
                            .label("Sign in")
                            .on_click(cx.listener(|this, _, window, cx| this.sign_in(window, cx))),
                    ),
            )
    }
}
