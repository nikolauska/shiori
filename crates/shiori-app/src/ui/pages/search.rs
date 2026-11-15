//! The search page: searches the service, with filters for year, season,
//! type and airing status. Season and year together browse a whole season,
//! fetching every result page; other searches fetch more on request.

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, prelude::*,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::{ActiveTheme as _, IconName, h_flex, v_flex};
use shiori_core::{AiringStatus, AnimeId, AnimeType, SearchParams, Season, SeasonName};
use shiori_store::SearchFilters;
use shiori_sync::PageToken;

use crate::ui::anime_view::{self, AnimeView, Place};
use crate::ui::format;
use crate::ui::model::Model;
use crate::ui::widgets::Choice;

type FilterSelect<T> = SelectState<Vec<Choice<Option<T>>>>;

fn choices<T: Copy>(any: &'static str, items: &[(T, &'static str)]) -> Vec<Choice<Option<T>>> {
    std::iter::once(Choice::new(any, None))
        .chain(
            items
                .iter()
                .map(|&(value, label)| Choice::new(label, Some(value))),
        )
        .collect()
}

fn season_choices() -> Vec<Choice<Option<SeasonName>>> {
    choices(
        "Any season",
        &[
            (SeasonName::Winter, "Winter"),
            (SeasonName::Spring, "Spring"),
            (SeasonName::Summer, "Summer"),
            (SeasonName::Fall, "Fall"),
        ],
    )
}

fn type_choices() -> Vec<Choice<Option<AnimeType>>> {
    let types = [
        AnimeType::Tv,
        AnimeType::Ova,
        AnimeType::Movie,
        AnimeType::Special,
        AnimeType::Ona,
        AnimeType::Music,
    ];
    let items: Vec<_> = types.iter().map(|&t| (t, format::type_label(t))).collect();
    choices("Any type", &items)
}

fn status_choices() -> Vec<Choice<Option<AiringStatus>>> {
    choices(
        "Any status",
        &[
            (AiringStatus::Airing, "Currently airing"),
            (AiringStatus::FinishedAiring, "Finished airing"),
            (AiringStatus::NotYetAired, "Not yet aired"),
        ],
    )
}

/// A filter dropdown with `value` chosen.
fn filter_select<T: Copy + PartialEq + 'static>(
    items: Vec<Choice<Option<T>>>,
    value: Option<T>,
    window: &mut Window,
    cx: &mut Context<SearchPage>,
) -> Entity<FilterSelect<T>> {
    let ix = items
        .iter()
        .position(|c| c.value == value)
        .map(gpui_component::IndexPath::new);
    cx.new(|cx| SelectState::new(items, ix, window, cx))
}

pub struct SearchPage {
    model: Entity<Model>,
    view: Entity<AnimeView>,
    text: Entity<InputState>,
    year: Entity<InputState>,
    season: Entity<FilterSelect<SeasonName>>,
    kind: Entity<FilterSelect<AnimeType>>,
    status: Entity<FilterSelect<AiringStatus>>,
    /// Results so far, in the service's order.
    results: Vec<AnimeId>,
    params: SearchParams,
    next: Option<PageToken>,
    searching: bool,
    /// Bumped by every new search, so that pages of an older one that
    /// arrive late are dropped.
    generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl SearchPage {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filters = model.read(cx).session.search_filters.clone();
        let view = cx.new(|cx| AnimeView::new(model.clone(), Place::Search, window, cx));
        let text = cx.new(|cx| InputState::new(window, cx).placeholder("Title"));
        let year = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Year")
                .default_value(filters.year.map(|y| y.to_string()).unwrap_or_default())
        });
        let season = filter_select(season_choices(), filters.season, window, cx);
        let kind = filter_select(type_choices(), filters.kind, window, cx);
        let status = filter_select(status_choices(), filters.status, window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&text, window, |this, _, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.run(window, cx);
                }
            }),
            cx.subscribe_in(&year, window, |this, _, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.run(window, cx);
                }
            }),
            cx.subscribe(&season, |this, _, _: &SelectEvent<_>, cx| this.remember(cx)),
            cx.subscribe(&kind, |this, _, _: &SelectEvent<_>, cx| this.remember(cx)),
            cx.subscribe(&status, |this, _, _: &SelectEvent<_>, cx| this.remember(cx)),
        ];
        Self {
            model,
            view,
            text,
            year,
            season,
            kind,
            status,
            results: Vec::new(),
            params: SearchParams::default(),
            next: None,
            searching: false,
            generation: 0,
            _subscriptions: subscriptions,
        }
    }

    /// Search from the toolbar box.
    pub fn search(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.text
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.run(window, cx);
    }

    fn filters(&self, cx: &App) -> SearchFilters {
        let year = self.year.read(cx).value().trim().parse::<u16>().ok();
        SearchFilters {
            year,
            season: self.season.read(cx).selected_value().copied().flatten(),
            kind: self.kind.read(cx).selected_value().copied().flatten(),
            status: self.status.read(cx).selected_value().copied().flatten(),
        }
    }

    fn remember(&mut self, cx: &mut Context<Self>) {
        let filters = self.filters(cx);
        self.model.update(cx, |model, cx| {
            if model.session.search_filters != filters {
                model.session.search_filters = filters;
                model.session_changed(cx);
            }
        });
    }

    fn run(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.remember(cx);
        let filters = self.filters(cx);
        self.params = SearchParams {
            text: self.text.read(cx).value().trim().to_owned(),
            year: filters.year,
            season: filters.season,
            kind: filters.kind,
            status: filters.status,
            ..SearchParams::default()
        };
        self.generation += 1;
        self.results.clear();
        self.next = None;
        self.show(cx);
        self.fetch(None, cx);
    }

    /// Jumps to the season before or after the one in the filters, or to
    /// the current season when none is set.
    fn browse(&mut self, step: i32, window: &mut Window, cx: &mut Context<Self>) {
        let filters = self.filters(cx);
        let season = match (filters.season, filters.year) {
            (Some(name), Some(year)) => {
                let season = Season { name, year };
                match step {
                    s if s < 0 => season.previous(),
                    s if s > 0 => season.next(),
                    _ => season,
                }
            }
            _ => Season::from_date(format::today_in_japan().into()),
        };
        self.year.update(cx, |input, cx| {
            input.set_value(season.year.to_string(), window, cx)
        });
        self.season.update(cx, |select, cx| {
            select.set_selected_value(&Some(season.name), window, cx)
        });
        self.text
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.run(window, cx);
    }

    fn fetch(&mut self, page: Option<PageToken>, cx: &mut Context<Self>) {
        self.searching = true;
        let generation = self.generation;
        let params = self.params.clone();
        let this = cx.entity().downgrade();
        self.model.update(cx, |model, cx| {
            model.search(params, page, cx, move |result, cx| {
                let _ = this.update(cx, |this, cx| {
                    if this.generation != generation {
                        return;
                    }
                    this.searching = false;
                    let Some((ids, next)) = result else {
                        cx.notify();
                        return;
                    };
                    for id in ids {
                        // Pages can overlap when the service's order shifts
                        // between requests.
                        if !this.results.contains(&id) {
                            this.results.push(id);
                        }
                    }
                    this.next = next;
                    this.show(cx);
                    if this.params.is_season_browse()
                        && let Some(next) = this.next.take()
                    {
                        this.fetch(Some(next), cx);
                    }
                });
            });
        });
        cx.notify();
    }

    fn show(&mut self, cx: &mut Context<Self>) {
        let rows = self.results.clone();
        self.view.update(cx, |view, cx| view.set_rows(rows, cx));
        cx.notify();
    }
}

impl Render for SearchPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.results.len();
        let more = self.next.is_some() && !self.params.is_season_browse();
        v_flex()
            .size_full()
            .p_3()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child(div().text_lg().child("Search"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{count} anime")),
                    )
                    .child(div().flex_1())
                    .child(anime_view::sort_menu(&self.view, cx))
                    .child(anime_view::mode_switcher(&self.view, cx)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(div().w_64().child(Input::new(&self.text)))
                    .child(div().w_20().child(Input::new(&self.year)))
                    .child(div().w_32().child(Select::new(&self.season)))
                    .child(div().w_32().child(Select::new(&self.kind)))
                    .child(div().w_40().child(Select::new(&self.status)))
                    .child(
                        Button::new("search")
                            .primary()
                            .label("Search")
                            .loading(self.searching)
                            .on_click(cx.listener(|this, _, window, cx| this.run(window, cx))),
                    )
                    // One group, so a narrow window wraps the season
                    // buttons together instead of leaving one alone.
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("previous-season")
                                    .icon(IconName::ChevronLeft)
                                    .ghost()
                                    .tooltip("Previous season")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse(-1, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("this-season")
                                    .ghost()
                                    .label("This season")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse(0, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("next-season")
                                    .icon(IconName::ChevronRight)
                                    .ghost()
                                    .tooltip("Next season")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse(1, window, cx)
                                    })),
                            ),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .when(more, |page| {
                page.child(
                    h_flex().justify_center().child(
                        Button::new("more")
                            .label("Load more")
                            .loading(self.searching)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(next) = this.next.take() {
                                    this.fetch(Some(next), cx);
                                }
                            })),
                    ),
                )
            })
    }
}
