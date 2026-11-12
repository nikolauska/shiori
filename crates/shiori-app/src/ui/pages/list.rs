//! The anime list page: the user's list, filtered by status and title.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, IconName, h_flex, v_flex};
use shiori_core::ListStatus;
use shiori_store::ViewMode;

use shiori_app::list_view::list_rows;
use shiori_app::state::AppEvent;

use crate::ui::anime_view::{AnimeView, Place};
use crate::ui::format;
use crate::ui::model::Model;

pub struct ListPage {
    model: Entity<Model>,
    view: Entity<AnimeView>,
    filter: Entity<InputState>,
    status: Option<ListStatus>,
    _subscriptions: Vec<Subscription>,
}

impl ListPage {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| AnimeView::new(model.clone(), Place::AnimeList, window, cx));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter by title"));
        let subscriptions = vec![
            cx.subscribe(&model, |this, _, event: &AppEvent, cx| {
                // Only changes to the data can change which rows are shown;
                // a poster arriving does not.
                if matches!(
                    event,
                    AppEvent::EntryUpdated(_)
                        | AppEvent::EntryDeleted(_)
                        | AppEvent::AnimeUpdated(_)
                        | AppEvent::AnimeDeleted { .. }
                        | AppEvent::AnimeSettingsUpdated(_)
                        | AppEvent::ListFetched
                ) {
                    this.refresh(cx);
                }
            }),
            cx.subscribe(&filter, |this, _, event, cx| {
                if let InputEvent::Change = event {
                    this.refresh(cx);
                }
            }),
        ];
        let mut page = Self {
            model,
            view,
            filter,
            status: Some(ListStatus::Watching),
            _subscriptions: subscriptions,
        };
        page.refresh(cx);
        page
    }

    pub fn set_status(&mut self, status: Option<ListStatus>, cx: &mut Context<Self>) {
        if self.status != status {
            self.status = status;
            self.refresh(cx);
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter.read(cx).value().to_string();
        let rows = list_rows(
            self.model.read(cx).state(),
            self.status,
            &filter,
            Default::default(),
        );
        self.view.update(cx, |view, cx| view.set_rows(rows, cx));
        cx.notify();
    }
}

impl Render for ListPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let heading = self.status.map_or("Anime list", format::status_label);
        let count = self.view.read(cx).rows(cx);
        let (icon, tooltip) = match self.view.read(cx).mode() {
            ViewMode::List => (IconName::LayoutDashboard, "Show as cards"),
            ViewMode::Cards => (IconName::Menu, "Show as a list"),
        };
        v_flex()
            .size_full()
            .p_3()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child(div().text_lg().child(heading))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{count} anime")),
                    )
                    .child(div().flex_1())
                    .child(div().w_64().child(Input::new(&self.filter)))
                    .child(
                        Button::new("view-mode")
                            .icon(icon)
                            .ghost()
                            .tooltip(tooltip)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.view.update(cx, |view, cx| view.toggle_mode(cx))
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.view.clone()))
    }
}
