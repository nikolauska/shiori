//! The anime list page: the user's list as a table, filtered by status and
//! title.

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::PopupMenu;
use gpui_component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
use shiori_core::{AnimeId, ListStatus};

use shiori_app::list_view::{ListColumn, Sort, SortKey, list_rows};

use crate::ui::model::Model;
use crate::ui::{actions, format};

pub struct ListDelegate {
    model: Entity<Model>,
    pub rows: Vec<AnimeId>,
    status: Option<ListStatus>,
    filter: String,
    sort: Sort,
}

impl ListDelegate {
    fn rebuild(&mut self, cx: &App) {
        let state = self.model.read(cx).state();
        self.rows = list_rows(state, self.status, &self.filter, self.sort);
    }
}

impl TableDelegate for ListDelegate {
    fn columns_count(&self, _: &App) -> usize {
        ListColumn::ALL.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        let column = ListColumn::ALL[col_ix];
        let width = match column {
            ListColumn::Title => 320.,
            ListColumn::Notes => 200.,
            ListColumn::LastUpdated => 140.,
            ListColumn::Season => 110.,
            ListColumn::Started | ListColumn::Completed => 100.,
            _ => 80.,
        };
        Column::new(column.key(), column.name())
            .width(gpui::px(width))
            .sortable()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let column = ListColumn::ALL[col_ix];
        match sort {
            ColumnSort::Default => self.sort = Sort::default(),
            ColumnSort::Ascending | ColumnSort::Descending => self.sort.by(SortKey {
                column,
                descending: sort == ColumnSort::Descending,
            }),
        }
        self.rebuild(cx);
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let state = self.model.read(cx).state();
        let text = self
            .rows
            .get(row_ix)
            .and_then(|id| state.anime.get(id))
            .map(|anime| {
                format::cell(
                    state,
                    ListColumn::ALL[col_ix],
                    anime,
                    state.entries.get(&anime.id),
                )
            })
            .unwrap_or_default();
        div().truncate().child(text)
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        match self.rows.get(row_ix) {
            Some(&id) => actions::context_menu(&self.model, id, menu, window, cx),
            None => menu,
        }
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, cx: &App) -> Option<SharedString> {
        let state = self.model.read(cx).state();
        let anime = state.anime.get(self.rows.get(row_ix)?)?;
        let text = format::cell(
            state,
            ListColumn::ALL[col_ix],
            anime,
            state.entries.get(&anime.id),
        );
        Some(text.into())
    }
}

pub struct ListPage {
    model: Entity<Model>,
    table: Entity<TableState<ListDelegate>>,
    filter: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl ListPage {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delegate = ListDelegate {
            model: model.clone(),
            rows: Vec::new(),
            status: Some(ListStatus::Watching),
            filter: String::new(),
            sort: Sort::default(),
        };
        let table = cx.new(|cx| TableState::new(delegate, window, cx).col_movable(false));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter by title"));
        let subscriptions = vec![
            // Any change to the data may change which rows are shown.
            cx.observe(&model, |this, _, cx| this.refresh(cx)),
            cx.subscribe(&filter, |this, input, event, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    this.table.update(cx, |table, cx| {
                        table.delegate_mut().filter = text;
                        table.delegate_mut().rebuild(cx);
                        cx.notify();
                    });
                }
            }),
        ];
        let mut page = Self {
            model,
            table,
            filter,
            _subscriptions: subscriptions,
        };
        page.refresh(cx);
        page
    }

    pub fn set_status(&mut self, status: Option<ListStatus>, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().status = status;
            table.delegate_mut().rebuild(cx);
            cx.notify();
        });
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().rebuild(cx);
            cx.notify();
        });
    }
}

impl Render for ListPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let delegate = self.table.read(cx).delegate();
        let heading = delegate.status.map_or("Anime list", format::status_label);
        let count = delegate.rows.len();
        let _ = &self.model;
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
                    .child(div().w_64().child(Input::new(&self.filter))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(DataTable::new(&self.table).stripe(true)),
            )
    }
}
