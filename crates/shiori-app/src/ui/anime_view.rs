//! The anime list component used by the anime list and the search page:
//! rich rows, a table or a grid of cards over the same rows, sort and
//! selection. The first selected anime is shown in the inspector.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, prelude::*, px, relative,
    uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::table::{Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use shiori_core::AnimeId;
use shiori_core::entry::is_in_list;
use shiori_store::{ListView, ViewMode};

use shiori_app::list_view::{ListColumn, Selection, Sort, SortKey, new_episodes, sort_rows};
use shiori_app::state::AppState;

use super::icons::AppIcon;
use super::model::Model;
use super::{actions, format, inspector, poster, style};

gpui::actions!(anime_view, [OpenSelected, Increment, Decrement]);

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

    /// Hands the first selected anime to the inspector.
    fn show_selected(&self, cx: &mut App) {
        let first = self.selection.in_order(&self.rows).first().copied();
        self.model.update(cx, |model, cx| match first {
            Some(id) => model.select(id, cx),
            None => model.deselect(cx),
        });
    }

    fn remember_sort(&self, cx: &mut App) {
        let (sort, place) = (self.sort, self.place);
        self.model.update(cx, |model, cx| {
            match place {
                Place::AnimeList => sort.to_session(&mut model.session.anime_list),
                Place::Search => sort.to_session(&mut model.session.search_list),
            }
            model.session_changed(cx);
        });
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
        self.remember_sort(cx);
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
                    delegate.show_selected(cx);
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
const CARD_HEIGHT: f32 = POSTER_HEIGHT + 68.;
const ROW_HEIGHT: f32 = 64.;

/// Progress as a bar: watched in the accent gradient, aired but unwatched
/// in a faint accent after it.
fn progress_bar(state: &AppState, id: AnimeId, cx: &App) -> impl IntoElement + use<> {
    let entry = state.entries.get(&id);
    let anime = state.anime.get(&id);
    let watched = entry.map_or(0, |e| e.watched_episodes);
    let new = new_episodes(anime, entry);
    let total = anime
        .and_then(|a| a.episode_count)
        .unwrap_or(watched + new)
        .max(1) as f32;
    let done = (watched as f32 / total).min(1.);
    let aired = ((watched + new) as f32 / total).min(1.);
    div()
        .relative()
        .w_full()
        .h(px(5.))
        .rounded_full()
        .overflow_hidden()
        .bg(cx.theme().muted)
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(done))
                .w(relative(aired - done))
                .bg(style::accent_soft(cx)),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(relative(done))
                .rounded_full()
                .bg(style::gradient()),
        )
}

pub struct AnimeView {
    model: Entity<Model>,
    pub table: Entity<TableState<AnimeDelegate>>,
    mode: ViewMode,
    focus: FocusHandle,
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
            sort: match Sort::from_session(&remembered) {
                // Without a remembered sort the list leads with what can be
                // watched now; titles settle the rest.
                Sort { primary: None, .. } if place == Place::AnimeList => Sort {
                    primary: Some(SortKey {
                        column: ListColumn::NewEpisodes,
                        descending: true,
                    }),
                    secondary: None,
                },
                sort => sort,
            },
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
                    delegate.show_selected(cx);
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
                            inspector::open(&this.model, id, window, cx);
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
                                delegate.show_selected(cx);
                                cx.notify();
                            }
                        });
                    }
                    _ => {}
                },
            ),
            // Posters arriving and edits change what the rows show. An anime
            // opened from elsewhere (Home, History) becomes the selection,
            // so the list and the inspector agree.
            cx.observe(&model, |this, model, cx| {
                if let Some(id) = model.read(cx).selected {
                    this.table.update(cx, |table, cx| {
                        let delegate = table.delegate_mut();
                        if delegate.rows.contains(&id) && !delegate.selection.contains(id) {
                            delegate.selection.select_for_menu(id);
                            cx.notify();
                        }
                    });
                }
                cx.notify();
            }),
        ];
        Self {
            model,
            table,
            mode: remembered.view_mode,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    pub fn rows(&self, cx: &App) -> usize {
        self.table.read(cx).delegate().rows.len()
    }

    pub fn row_ids(&self, cx: &App) -> Vec<AnimeId> {
        self.table.read(cx).delegate().rows.clone()
    }

    /// Shows these anime in exactly this order, without sorting.
    pub fn replace_rows(&mut self, rows: Vec<AnimeId>, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.selection.retain(&rows);
            delegate.rows = rows;
            table.refresh(cx);
            cx.notify();
        });
        cx.notify();
    }

    pub fn sort(&self, cx: &App) -> Sort {
        self.table.read(cx).delegate().sort
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

    /// Sorts by one column from the Sort menu, the previous sort breaking
    /// ties. Picking the current sort again reverses it.
    pub fn sort_by(&mut self, column: ListColumn, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            let descending = match delegate.sort.primary {
                Some(key) if key.column == column => !key.descending,
                _ => column.descending_by_default(),
            };
            delegate.sort.by(SortKey { column, descending });
            delegate.resort(cx);
            delegate.remember_sort(cx);
            table.refresh(cx);
            cx.notify();
        });
        cx.notify();
    }

    pub fn set_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.mode = mode;
        let place = self.table.read(cx).delegate().place;
        self.model.update(cx, |model, cx| {
            match place {
                Place::AnimeList => model.session.anime_list.view_mode = mode,
                Place::Search => model.session.search_list.view_mode = mode,
            }
            model.session_changed(cx);
        });
        cx.notify();
    }

    fn selected(&self, cx: &App) -> Vec<AnimeId> {
        let delegate = self.table.read(cx).delegate();
        delegate.selection.in_order(&delegate.rows)
    }

    fn open_selected(&mut self, _: &OpenSelected, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(&id) = self.selected(cx).first() {
            inspector::open(&self.model, id, window, cx);
        }
    }

    fn increment(&mut self, _: &Increment, _: &mut Window, cx: &mut Context<Self>) {
        let ids = self.selected(cx);
        self.model.update(cx, |model, cx| {
            ids.iter().for_each(|&id| model.step_episode(id, 1, cx))
        });
    }

    fn decrement(&mut self, _: &Decrement, _: &mut Window, cx: &mut Context<Self>) {
        let ids = self.selected(cx);
        self.model.update(cx, |model, cx| {
            ids.iter().for_each(|&id| model.step_episode(id, -1, cx))
        });
    }

    /// A click on a row or card: selects, and opens on a double click with
    /// the left button.
    fn item_click(
        &mut self,
        ix: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let modifiers = event.modifiers();
        let toggle = modifiers.secondary(cx);
        let id = self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            let rows = delegate.rows.clone();
            delegate.selection.click(&rows, ix, toggle, modifiers.shift);
            delegate.show_selected(cx);
            cx.notify();
            rows.get(ix).copied()
        });
        if event.click_count() == 2
            && event.standard_click()
            && let Some(id) = id
        {
            inspector::open(&self.model, id, window, cx);
        }
        cx.notify();
    }

    fn context_menu_for(
        &self,
        ix: usize,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let table = self.table.downgrade();
        move |menu, window, cx| match table.upgrade() {
            Some(table) => table.update(cx, |table, cx| {
                let menu = table.delegate_mut().menu(ix, menu, window, cx);
                table.delegate().show_selected(cx);
                menu
            }),
            None => menu,
        }
    }

    fn render_rows(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.rows(cx);
        let view = cx.entity();
        uniform_list("rows", count, move |range, _, cx| {
            range
                .map(|ix| view.update(cx, |this, cx| this.row(ix, cx)))
                .collect()
        })
        .size_full()
        .px_2()
    }

    fn row(&mut self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let delegate = self.table.read(cx).delegate();
        let id = delegate.rows[ix];
        let selected = delegate.selection.contains(id);
        let state = self.model.read(cx).state();
        let title = state.title(id);
        let anime = state.anime.get(&id);
        let entry = state.entries.get(&id);
        let in_list = is_in_list(entry);
        let meta = anime
            .map(|a| {
                [
                    format::type_label(a.kind).to_owned(),
                    format::season_label(a.date_started),
                    a.studios.first().cloned().unwrap_or_default(),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
            })
            .unwrap_or_default();
        let total = anime
            .and_then(|a| a.episode_count)
            .map_or("?".to_owned(), |n| n.to_string());
        let progress = match entry.filter(|_| in_list) {
            Some(e) => format!("{} / {total}", e.watched_episodes),
            None => format!("{total} episodes"),
        };
        let length = anime
            .and_then(|a| a.episode_length)
            .map_or(String::new(), |m| format!("{m} min"));
        let score = entry
            .filter(|e| e.score > 0)
            .map_or("–".to_owned(), |e| format::score_label(state, e.score));
        let average = anime
            .filter(|a| a.score > 0.0)
            .map_or(String::new(), |a| format!(" · {:.1}", a.score));
        let badge = inspector::badge(state, id, cx);
        let bar = progress_bar(state, id, cx);
        let poster = poster::view(&self.model, id, cx);
        let theme = cx.theme();
        let (muted, hover, active) = (
            theme.muted_foreground,
            theme.table_hover,
            theme.table_active,
        );
        let accent = style::accent(cx);
        let model = self.model.clone();
        h_flex()
            .id(("anime-row", id.0 as usize))
            .group("anime-row")
            .w_full()
            .h(px(ROW_HEIGHT))
            .gap_3()
            .px_2()
            .rounded_md()
            .when(selected, |row| {
                row.bg(active).border_l_2().border_color(accent)
            })
            .hover(|row| row.bg(hover))
            .child(
                div()
                    .w(px(38.))
                    .h(px(54.))
                    .flex_none()
                    .rounded_sm()
                    .overflow_hidden()
                    .child(poster),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap_1()
                    .child(div().font_semibold().truncate().child(title))
                    .child(
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .children(badge)
                            .child(div().truncate().child(meta)),
                    ),
            )
            .child(
                v_flex()
                    .w(px(170.))
                    .flex_none()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .text_xs()
                            .child(progress)
                            .child(div().text_color(muted).child(length)),
                    )
                    .child(bar),
            )
            .child(
                div()
                    .w(px(64.))
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .text_center()
                    .child(format!("{score}{average}")),
            )
            .child(div().w(px(36.)).flex_none().when(in_list, |slot| {
                slot.child(
                    Button::new(("plus-one", id.0 as usize))
                        .small()
                        .label("+1")
                        .tooltip("Watched the next episode (+)")
                        // Shown on hover and on the selected row only,
                        // so a long list does not look like a wall of
                        // buttons.
                        .when(!selected, |button| {
                            button.invisible().group_hover("anime-row", |s| s.visible())
                        })
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            model.update(cx, |model, cx| model.step_episode(id, 1, cx));
                        }),
                )
            }))
            .on_click(
                cx.listener(move |this, event, window, cx| this.item_click(ix, event, window, cx)),
            )
            .context_menu(self.context_menu_for(ix))
    }

    fn render_cards(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.rows(cx);
        // The page sits between the sidebar and the inspector; leave room
        // for both and the padding.
        let session = &self.model.read(cx).session;
        let inspector = if session.inspector_hidden {
            0.
        } else {
            session.inspector_width.unwrap_or(320.)
        };
        let width = f32::from(window.viewport_size().width) - 220. - inspector - 40.;
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
        .px_3()
        .pt_2()
    }

    fn card(&mut self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let delegate = self.table.read(cx).delegate();
        let id = delegate.rows[ix];
        let selected = delegate.selection.contains(id);
        let state = self.model.read(cx).state();
        let title = state.title(id);
        let entry = state.entries.get(&id);
        let anime = state.anime.get(&id);
        let new = new_episodes(anime, entry);
        let progress = entry
            .filter(|e| is_in_list(Some(e)))
            .map(|e| format::progress_label(e, anime))
            .unwrap_or_default();
        let bar = progress_bar(state, id, cx);
        let poster = poster::view(&self.model, id, cx);
        let theme = cx.theme();
        let accent = style::accent(cx);
        v_flex()
            .id(("card", id.0 as usize))
            .w(px(CARD_WIDTH))
            .h(px(CARD_HEIGHT))
            .p_1()
            .gap_1()
            .rounded_md()
            .when(selected, |card| {
                card.bg(theme.table_active).border_2().border_color(accent)
            })
            .hover(|card| card.bg(theme.table_hover))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(POSTER_HEIGHT))
                    .rounded_md()
                    .overflow_hidden()
                    .child(poster)
                    .when(new > 0, |cover| {
                        cover.child(
                            div()
                                .absolute()
                                .top_1()
                                .left_1()
                                .px_2()
                                .rounded_full()
                                .text_xs()
                                .font_semibold()
                                .bg(style::gradient())
                                .text_color(gpui::white())
                                .child(format!("{new} new")),
                        )
                    })
                    .when(!progress.is_empty(), |cover| {
                        cover.child(div().absolute().left_0().right_0().bottom_0().child(bar))
                    }),
            )
            .child(div().text_sm().line_clamp(2).child(title))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(progress),
            )
            .on_click(
                cx.listener(move |this, event, window, cx| this.item_click(ix, event, window, cx)),
            )
            .context_menu(self.context_menu_for(ix))
    }
}

/// The Sort menu button for a view's page header.
pub fn sort_menu(view: &Entity<AnimeView>, cx: &App) -> impl IntoElement + use<> {
    let sort = view.read(cx).sort(cx);
    let label = sort.primary.map_or("Sort", |key| key.column.name());
    let view = view.downgrade();
    Button::new("sort")
        .small()
        .ghost()
        .label(format!("Sort: {label}"))
        .dropdown_menu(move |mut menu, _, _| {
            for column in ListColumn::SORTS {
                let view = view.clone();
                menu = menu.item(
                    PopupMenuItem::new(column.name())
                        .checked(sort.primary.is_some_and(|k| k.column == column))
                        .on_click(move |_, _, cx| {
                            let _ = view.update(cx, |view, cx| view.sort_by(column, cx));
                        }),
                );
            }
            menu
        })
}

/// The Rows / Table / Cards switcher for a view's page header.
pub fn mode_switcher(view: &Entity<AnimeView>, cx: &App) -> impl IntoElement + use<> {
    let current = view.read(cx).mode();
    let modes = [
        (ViewMode::Rows, Icon::new(AppIcon::LayoutList), "Rows"),
        (ViewMode::Table, Icon::new(AppIcon::Table), "Table"),
        (
            ViewMode::Cards,
            Icon::new(IconName::LayoutDashboard),
            "Cards",
        ),
    ];
    h_flex().children(modes.into_iter().map(|(mode, icon, label)| {
        let view = view.downgrade();
        Button::new(label)
            .small()
            .icon(icon)
            .tooltip(label)
            .when(mode == current, |b| b.primary())
            .when(mode != current, |b| b.ghost())
            .on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.set_mode(mode, cx));
            })
    }))
}

impl Focusable for AnimeView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AnimeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.mode {
            ViewMode::Rows => self.render_rows(cx).into_any_element(),
            ViewMode::Table => DataTable::new(&self.table).stripe(true).into_any_element(),
            ViewMode::Cards => self.render_cards(window, cx).into_any_element(),
        };
        div()
            .size_full()
            .track_focus(&self.focus)
            .key_context("AnimeView")
            .on_action(cx.listener(Self::open_selected))
            .on_action(cx.listener(Self::increment))
            .on_action(cx.listener(Self::decrement))
            .child(content)
    }
}
