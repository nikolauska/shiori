//! The anime list component used by the anime list and the search page: a
//! table or a grid of cards over the same rows, sort and selection.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled, Subscription, Window, div, prelude::*, px, uniform_list,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_component::table::{Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
use shiori_core::AnimeId;
use shiori_store::{ListView, ViewMode};

use shiori_app::list_view::{ListColumn, Selection, Sort, SortKey, sort_rows};

use super::model::Model;
use super::{actions, details, format, poster};

gpui::actions!(anime_view, [OpenSelected]);

/// Which remembered settings in `session.toml` a view uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    AnimeList,
    Search,
}

fn session_view(model: &Model, place: Place) -> &ListView {
    match place {
        Place::AnimeList => &model.session.anime_list,
        Place::Search => &model.session.search_list,
    }
}

pub struct AnimeDelegate {
    model: Entity<Model>,
    place: Place,
    pub rows: Vec<AnimeId>,
    sort: Sort,
    pub selection: Selection,
    /// Rebuilt into the context menu; set by the owning view.
    invert: Option<Rc<actions::Handler>>,
}

impl AnimeDelegate {
    fn resort(&mut self, cx: &App) {
        sort_rows(self.model.read(cx).state(), &mut self.rows, self.sort);
    }

    fn menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut App,
    ) -> PopupMenu {
        let Some(&id) = self.rows.get(row_ix) else {
            return menu;
        };
        self.selection.select_for_menu(id);
        let ids = self.selection.in_order(&self.rows);
        actions::context_menu(&self.model, ids, self.invert.clone(), menu, window, cx)
    }
}

impl TableDelegate for AnimeDelegate {
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
        // Shows the remembered sort on the header after a restart.
        let sort = match self.sort.primary {
            Some(key) if key.column == column && key.descending => ColumnSort::Descending,
            Some(key) if key.column == column => ColumnSort::Ascending,
            _ => ColumnSort::Default,
        };
        Column::new(column.key(), column.name())
            .width(px(width))
            .sortable()
            .sort(sort)
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
        self.resort(cx);
        let (sort, place) = (self.sort, self.place);
        self.model.update(cx, |model, cx| {
            match place {
                Place::AnimeList => sort.to_session(&mut model.session.anime_list),
                Place::Search => sort.to_session(&mut model.session.search_list),
            }
            model.session_changed(cx);
        });
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<gpui::Div> {
        let selected = self
            .rows
            .get(row_ix)
            .is_some_and(|id| self.selection.contains(*id));
        div()
            .id(("row", row_ix))
            .when(selected, |row| row.bg(cx.theme().table_active))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |table, event: &gpui::MouseDownEvent, _, cx| {
                    let delegate = table.delegate_mut();
                    let toggle = event.modifiers.secondary(cx);
                    let rows = delegate.rows.clone();
                    delegate
                        .selection
                        .click(&rows, row_ix, toggle, event.modifiers.shift);
                    cx.notify();
                }),
            )
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

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        cx.notify();
        self.menu(row_ix, menu, window, cx)
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

const CARD_WIDTH: f32 = 150.;
const CARD_GAP: f32 = 12.;
const POSTER_HEIGHT: f32 = 212.;
const CARD_HEIGHT: f32 = POSTER_HEIGHT + 64.;

pub struct AnimeView {
    model: Entity<Model>,
    pub table: Entity<TableState<AnimeDelegate>>,
    mode: ViewMode,
    _subscriptions: Vec<Subscription>,
}

impl AnimeView {
    pub fn new(
        model: Entity<Model>,
        place: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let remembered = session_view(model.read(cx), place).clone();
        let delegate = AnimeDelegate {
            model: model.clone(),
            place,
            rows: Vec::new(),
            sort: Sort::from_session(&remembered),
            selection: Selection::default(),
            invert: None,
        };
        let table = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .col_movable(false)
                // The delegate keeps the multi-row selection itself.
                .select_on_modifier_click(false)
        });
        let invert: Rc<actions::Handler> = Rc::new({
            let table = table.downgrade();
            move |_, cx| {
                let _ = table.update(cx, |table, cx| {
                    let delegate = table.delegate_mut();
                    let rows = delegate.rows.clone();
                    delegate.selection.invert(&rows);
                    cx.notify();
                });
            }
        });
        table.update(cx, |table, _| table.delegate_mut().invert = Some(invert));
        let subscriptions = vec![
            cx.subscribe_in(
                &table,
                window,
                |this, table, event, window, cx| match event {
                    TableEvent::DoubleClickedRow(row) => {
                        if let Some(&id) = table.read(cx).delegate().rows.get(*row) {
                            details::open(&this.model, id, window, cx);
                        }
                    }
                    // Keyboard moves select a single row, as a plain click does.
                    TableEvent::SelectRow(row) => {
                        table.update(cx, |table, cx| {
                            let delegate = table.delegate_mut();
                            if let Some(&id) = delegate.rows.get(*row)
                                && !delegate.selection.contains(id)
                            {
                                let rows = delegate.rows.clone();
                                delegate.selection.click(&rows, *row, false, false);
                                cx.notify();
                            }
                        });
                    }
                    _ => {}
                },
            ),
            // Posters arriving and edits change what the cells show.
            cx.observe(&model, |_, _, cx| cx.notify()),
        ];
        Self {
            model,
            table,
            mode: remembered.view_mode,
            _subscriptions: subscriptions,
        }
    }

    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    pub fn rows(&self, cx: &App) -> usize {
        self.table.read(cx).delegate().rows.len()
    }

    /// Shows these anime, in the view's sort order.
    pub fn set_rows(&mut self, rows: Vec<AnimeId>, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.rows = rows;
            delegate.resort(cx);
            let rows = delegate.rows.clone();
            delegate.selection.retain(&rows);
            table.refresh(cx);
            cx.notify();
        });
        cx.notify();
    }

    pub fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        self.mode = match self.mode {
            ViewMode::List => ViewMode::Cards,
            ViewMode::Cards => ViewMode::List,
        };
        let (mode, place) = (self.mode, self.table.read(cx).delegate().place);
        self.model.update(cx, |model, cx| {
            match place {
                Place::AnimeList => model.session.anime_list.view_mode = mode,
                Place::Search => model.session.search_list.view_mode = mode,
            }
            model.session_changed(cx);
        });
        cx.notify();
    }

    fn open_selected(&mut self, _: &OpenSelected, window: &mut Window, cx: &mut Context<Self>) {
        let delegate = self.table.read(cx).delegate();
        if let Some(&id) = delegate.selection.in_order(&delegate.rows).first() {
            details::open(&self.model, id, window, cx);
        }
    }

    fn card_click(
        &mut self,
        ix: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.modifiers();
        let toggle = modifiers.secondary(cx);
        let id = self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            let rows = delegate.rows.clone();
            delegate.selection.click(&rows, ix, toggle, modifiers.shift);
            cx.notify();
            rows.get(ix).copied()
        });
        if event.click_count() == 2
            && event.standard_click()
            && let Some(id) = id
        {
            details::open(&self.model, id, window, cx);
        }
        cx.notify();
    }

    fn render_cards(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.rows(cx);
        // The page sits right of the sidebar; leave room for it and padding.
        let width = f32::from(window.viewport_size().width) - 300.;
        let per_row = ((width / (CARD_WIDTH + CARD_GAP)).floor() as usize).max(1);
        let lines = count.div_ceil(per_row);
        let view = cx.entity();
        uniform_list("cards", lines, move |range, _, cx| {
            range
                .map(|line| {
                    let start = line * per_row;
                    let end = (start + per_row).min(count);
                    h_flex().gap(px(CARD_GAP)).pb(px(CARD_GAP)).children(
                        (start..end).map(|ix| view.update(cx, |this, cx| this.card(ix, cx))),
                    )
                })
                .collect()
        })
        .size_full()
    }

    fn card(&mut self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let delegate = self.table.read(cx).delegate();
        let id = delegate.rows[ix];
        let selected = delegate.selection.contains(id);
        let state = self.model.read(cx).state();
        let title = state.title(id);
        let entry = state.entries.get(&id);
        let anime = state.anime.get(&id);
        let progress = entry
            .map(|e| format::progress_label(e, anime))
            .unwrap_or_default();
        let score = entry
            .map(|e| format::score_label(state, e.score))
            .unwrap_or_default();
        let line = [progress, score]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let poster = poster::view(&self.model, id, cx);
        let table = self.table.downgrade();
        let theme = cx.theme();
        v_flex()
            .id(("card", id.0 as usize))
            .w(px(CARD_WIDTH))
            .h(px(CARD_HEIGHT))
            .p_1()
            .gap_1()
            .rounded_md()
            .when(selected, |card| card.bg(theme.table_active))
            .hover(|card| card.bg(theme.table_hover))
            .child(div().w_full().h(px(POSTER_HEIGHT)).child(poster))
            .child(div().text_sm().line_clamp(2).child(title))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(line),
            )
            .on_click(
                cx.listener(move |this, event, window, cx| this.card_click(ix, event, window, cx)),
            )
            .context_menu(move |menu, window, cx| match table.upgrade() {
                Some(table) => table.update(cx, |table, cx| {
                    table.delegate_mut().menu(ix, menu, window, cx)
                }),
                None => menu,
            })
    }
}

impl Render for AnimeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.mode {
            ViewMode::List => DataTable::new(&self.table).stripe(true).into_any_element(),
            ViewMode::Cards => self.render_cards(window, cx).into_any_element(),
        };
        div()
            .size_full()
            .key_context("AnimeView")
            .on_action(cx.listener(Self::open_selected))
            .child(content)
    }
}
