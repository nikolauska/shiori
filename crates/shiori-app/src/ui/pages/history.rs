//! The history page: watched episodes, newest first, and the sync queue.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::table::{Column, DataTable, TableDelegate, TableEvent, TableState};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
use shiori_core::{AnimeId, Fields, HistoryItem, QueueItem};

use crate::ui::icons::AppIcon;
use crate::ui::model::Model;
use crate::ui::{actions, inspector, widgets};

fn time_label(time: jiff::Timestamp) -> String {
    time.to_zoned_datetime(jiff::tz::TimeZone::system())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

pub struct HistoryDelegate {
    model: Entity<Model>,
    rows: Vec<HistoryItem>,
}

impl HistoryDelegate {
    fn rebuild(&mut self, cx: &App) {
        let mut rows = self.model.read(cx).state().history.clone();
        // Ids grow with every insert, so they order items that share a time.
        rows.sort_by_key(|item| std::cmp::Reverse((item.time, item.id)));
        self.rows = rows;
    }
}

impl TableDelegate for HistoryDelegate {
    fn columns_count(&self, _: &App) -> usize {
        3
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        match col_ix {
            0 => Column::new("title", "Title").width(gpui::px(360.)),
            1 => Column::new("details", "Details").width(gpui::px(140.)),
            _ => Column::new("time", "Last modified").width(gpui::px(160.)),
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .truncate()
            .child(self.cell_label(row_ix, col_ix, cx).unwrap_or_default())
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        widgets::empty_state(
            AppIcon::Clock,
            "Nothing watched yet",
            "Episodes you watch show up here.",
            None,
            cx,
        )
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, cx: &App) -> Option<SharedString> {
        let item = self.rows.get(row_ix)?;
        let text = match col_ix {
            0 => self.model.read(cx).state().title(item.anime_id),
            1 => format!("Episode: {}", item.episode),
            _ => time_label(item.time),
        };
        Some(text.into())
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let Some(item) = self.rows.get(row_ix).cloned() else {
            return menu;
        };
        let model = self.model.clone();
        let m = model.clone();
        menu.item(
            PopupMenuItem::new("Details")
                .on_click(move |_, window, cx| inspector::open(&m, item.anime_id, window, cx)),
        )
        .item(
            PopupMenuItem::new("Remove...").on_click(move |_, window, cx| {
                let model = model.clone();
                actions::confirm(
                    "Remove this from history?",
                    String::new(),
                    "Remove",
                    actions::Risk::Danger,
                    window,
                    cx,
                    move |cx| model.update(cx, |model, cx| model.remove_history(item.id, cx)),
                );
            }),
        )
    }
}

/// What a queue item will send, in words.
fn queue_label(item: &QueueItem) -> String {
    if item.is_delete() {
        return "Delete".to_owned();
    }
    let names = [
        (Fields::EPISODE, "episode"),
        (Fields::SCORE, "score"),
        (Fields::STATUS, "status"),
        (Fields::REWATCHING, "rewatching"),
        (Fields::REWATCHED_TIMES, "times rewatched"),
        (Fields::DATE_STARTED, "date started"),
        (Fields::DATE_COMPLETED, "date completed"),
        (Fields::NOTES, "notes"),
    ];
    let changed: Vec<&str> = names
        .iter()
        .filter(|(field, _)| item.dirty.contains(*field))
        .map(|(_, name)| *name)
        .collect();
    format!("Update {}", changed.join(", "))
}

pub struct QueueDelegate {
    model: Entity<Model>,
    rows: Vec<QueueItem>,
}

impl QueueDelegate {
    fn rebuild(&mut self, cx: &App) {
        let state = self.model.read(cx).state();
        self.rows = state.queue.items().into_iter().cloned().collect();
    }
}

impl TableDelegate for QueueDelegate {
    fn columns_count(&self, _: &App) -> usize {
        4
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        match col_ix {
            0 => Column::new("title", "Anime").width(gpui::px(300.)),
            1 => Column::new("what", "Changes").width(gpui::px(220.)),
            2 => Column::new("retries", "Retries").width(gpui::px(70.)),
            _ => Column::new("error", "Last error").width(gpui::px(300.)),
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .truncate()
            .child(self.cell_label(row_ix, col_ix, cx).unwrap_or_default())
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        widgets::empty_state(
            AppIcon::CircleCheck,
            "Nothing waiting",
            "Every change has been synchronized.",
            None,
            cx,
        )
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, cx: &App) -> Option<SharedString> {
        let item = self.rows.get(row_ix)?;
        let text = match col_ix {
            0 => self.model.read(cx).state().title(item.anime_id),
            1 => queue_label(item),
            2 => item.retry_count.to_string(),
            _ => item.last_error.clone().unwrap_or_default(),
        };
        Some(text.into())
    }
}

pub struct HistoryPage {
    model: Entity<Model>,
    history: Entity<TableState<HistoryDelegate>>,
    queue: Entity<TableState<QueueDelegate>>,
    _subscriptions: Vec<Subscription>,
}

impl HistoryPage {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let history = cx.new(|cx| {
            let delegate = HistoryDelegate {
                model: model.clone(),
                rows: Vec::new(),
            };
            TableState::new(delegate, window, cx)
                .col_movable(false)
                .sortable(false)
        });
        let queue = cx.new(|cx| {
            let delegate = QueueDelegate {
                model: model.clone(),
                rows: Vec::new(),
            };
            TableState::new(delegate, window, cx)
                .col_movable(false)
                .sortable(false)
        });
        let subscriptions = vec![
            cx.observe(&model, |this, _, cx| this.refresh(cx)),
            cx.subscribe_in(&history, window, |this, table, event, window, cx| {
                if let TableEvent::DoubleClickedRow(row) = event
                    && let Some(id) = table.read(cx).delegate().rows.get(*row).map(|i| i.anime_id)
                {
                    inspector::open(&this.model, id, window, cx);
                }
            }),
            cx.subscribe_in(&queue, window, |this, table, event, window, cx| {
                if let TableEvent::DoubleClickedRow(row) = event {
                    let id: Option<AnimeId> =
                        table.read(cx).delegate().rows.get(*row).map(|i| i.anime_id);
                    if let Some(id) = id {
                        inspector::open(&this.model, id, window, cx);
                    }
                }
            }),
        ];
        let mut page = Self {
            model,
            history,
            queue,
            _subscriptions: subscriptions,
        };
        page.refresh(cx);
        page
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.history.update(cx, |table, cx| {
            table.delegate_mut().rebuild(cx);
            cx.notify();
        });
        self.queue.update(cx, |table, cx| {
            table.delegate_mut().rebuild(cx);
            cx.notify();
        });
    }

    fn clear_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        actions::confirm(
            "Clear your history?",
            "Every entry is removed for good.".to_owned(),
            "Clear history",
            actions::Risk::Danger,
            window,
            cx,
            move |cx| model.update(cx, |model, cx| model.clear_history(cx)),
        );
    }

    fn clear_queue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let service = model.read(cx).service_name();
        actions::confirm(
            "Clear the sync queue?",
            format!("Changes not yet sent to {service} are lost."),
            "Clear queue",
            actions::Risk::Danger,
            window,
            cx,
            move |cx| model.update(cx, |model, cx| model.clear_queue(cx)),
        );
    }
}

fn heading(
    title: &'static str,
    count: usize,
    button: Button,
    cx: &mut Context<HistoryPage>,
) -> impl IntoElement {
    h_flex()
        .gap_3()
        .child(div().text_lg().child(title))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(count.to_string()),
        )
        .child(div().flex_1())
        .child(button)
}

impl Render for HistoryPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let history_count = self.history.read(cx).delegate().rows.len();
        let queue_count = self.queue.read(cx).delegate().rows.len();
        // A local list never queues anything.
        let local = self.model.read(cx).state().is_local();
        let clear_history = Button::new("clear-history")
            .ghost()
            .label("Clear history...")
            .on_click(cx.listener(|this, _, window, cx| this.clear_history(window, cx)));
        let clear_queue = Button::new("clear-queue")
            .ghost()
            .label("Clear queue...")
            .on_click(cx.listener(|this, _, window, cx| this.clear_queue(window, cx)));
        v_flex()
            .size_full()
            .p_3()
            .gap_2()
            .child(heading("History", history_count, clear_history, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(DataTable::new(&self.history).stripe(true)),
            )
            .when(!local, |page| {
                page.child(heading("Sync queue", queue_count, clear_queue, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(DataTable::new(&self.queue).stripe(true)),
                    )
            })
    }
}
