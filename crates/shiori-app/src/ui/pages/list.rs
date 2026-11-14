//! The anime list page: the user's list, filtered by status, title and the
//! quick filter chips.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div, prelude::*,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use shiori_core::entry::is_in_list;
use shiori_core::{AnimeId, ListStatus};

use shiori_app::list_view::{Chip, list_rows};
use shiori_app::state::AppEvent;

use crate::ui::ToggleInspector;
use crate::ui::anime_view::{self, AnimeView, Place};
use crate::ui::format;
use crate::ui::icons::AppIcon;
use crate::ui::model::Model;

pub struct ListPage {
    model: Entity<Model>,
    view: Entity<AnimeView>,
    filter: Entity<InputState>,
    status: Option<ListStatus>,
    chip: Chip,
    /// How many anime each chip would show, in `Chip::ALL` order.
    chip_counts: [usize; 4],
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
                        | AppEvent::ListReplaced
                        | AppEvent::SettingsChanged
                ) {
                    this.update_rows(cx);
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
            chip: Chip::All,
            chip_counts: [0; 4],
            _subscriptions: subscriptions,
        };
        page.refresh(cx);
        page
    }

    pub fn set_status(&mut self, status: Option<ListStatus>, cx: &mut Context<Self>) {
        if self.status != status {
            self.status = status;
            // A chip picked for one status rarely means anything for another.
            self.chip = Chip::All;
            self.refresh(cx);
        }
    }

    fn set_chip(&mut self, chip: Chip, cx: &mut Context<Self>) {
        self.chip = chip;
        self.refresh(cx);
    }

    /// The rows for the current status, filter and chip, and the chip
    /// counts.
    fn compute_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnimeId> {
        let filter = self.filter.read(cx).value().to_string();
        let state = self.model.read(cx).state();
        let today = format::today_in_japan();
        let rows = list_rows(state, self.status, &filter, Default::default());
        let count = |chip: Chip| {
            rows.iter()
                .filter(|&&id| chip.matches(state, id, today))
                .count()
        };
        self.chip_counts = Chip::ALL.map(count);
        rows.into_iter()
            .filter(|&id| self.chip.matches(state, id, today))
            .collect()
    }

    /// Rebuilds and sorts the rows: for a new status, filter, chip or sort.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let rows = self.compute_rows(cx);
        self.view.update(cx, |view, cx| view.set_rows(rows, cx));
        cx.notify();
    }

    /// Follows a change to the data without moving rows around. A row that
    /// jumped away right after "+1" would put another anime's "+1" under
    /// the mouse, so rows keep their place and stay while they are still in
    /// this status; only anime new to the list are added, at the end.
    fn update_rows(&mut self, cx: &mut Context<Self>) {
        let fresh = self.compute_rows(cx);
        let state = self.model.read(cx).state();
        let status = self.status;
        let still_here = |id: &AnimeId| {
            state
                .entries
                .get(id)
                .is_some_and(|e| is_in_list(Some(e)) && status.is_none_or(|s| e.status == s))
        };
        let shown = self.view.read(cx).row_ids(cx);
        let mut rows: Vec<AnimeId> = shown.iter().copied().filter(still_here).collect();
        rows.extend(fresh.into_iter().filter(|id| !shown.contains(id)));
        self.view.update(cx, |view, cx| view.replace_rows(rows, cx));
        cx.notify();
    }
}

impl Render for ListPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let heading = self.status.map_or("Anime list", format::status_label);
        let count = self.view.read(cx).rows(cx);
        let chips = Chip::ALL.iter().zip(self.chip_counts).map(|(&chip, n)| {
            let label = match chip {
                Chip::All => chip.label().to_owned(),
                _ => format!("{} · {n}", chip.label()),
            };
            Button::new(chip.label())
                .small()
                .rounded_full()
                .label(label)
                .when(chip == self.chip, |b| b.primary())
                .when(chip != self.chip, |b| b.outline())
                // An empty chip is still shown so the row does not jump
                // around, but there is nothing to pick.
                .disabled(n == 0 && chip != Chip::All)
                .on_click(cx.listener(move |this, _, _, cx| this.set_chip(chip, cx)))
        });
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .gap_2()
                    .px_4()
                    .pt_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(heading),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(count.to_string()),
                    )
                    .child(div().flex_1())
                    .child(div().w_48().child(Input::new(&self.filter).small()))
                    .child(anime_view::sort_menu(&self.view, cx))
                    .child(anime_view::mode_switcher(&self.view, cx))
                    .child(
                        Button::new("toggle-inspector")
                            .small()
                            .ghost()
                            .icon(Icon::new(AppIcon::PanelRight))
                            .tooltip("Show or hide the inspector")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(ToggleInspector), cx)
                            }),
                    ),
            )
            .child(h_flex().gap_2().px_4().py_2().flex_wrap().children(chips))
            .child(div().flex_1().min_h_0().child(self.view.clone()))
    }
}
