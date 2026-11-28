//! Home: anime with new episodes to watch, and what airs this week.

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Window, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use shiori_core::AnimeId;

use shiori_app::home::{airing_schedule, continue_watching};

use crate::ui::model::Model;
use crate::ui::{poster, style};

pub struct HomePage {
    model: Entity<Model>,
    _subscriptions: Vec<Subscription>,
}

impl HomePage {
    pub fn new(model: Entity<Model>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&model, |_, _, cx| cx.notify())];
        Self {
            model,
            _subscriptions: subscriptions,
        }
    }

    /// Clicking an anime shows it in the Watching list with the inspector.
    fn open(&self, id: AnimeId, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::inspector::open(&self.model, id, window, cx);
    }

    fn render_continue(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let items = continue_watching(self.model.read(cx).state());
        let muted = cx.theme().muted_foreground;
        let cards = items.into_iter().map(|(id, new)| {
            let state = self.model.read(cx).state();
            let title: SharedString = state.title(id).into();
            let next = state.entries.get(&id).map_or(1, |e| e.watched_episodes + 1);
            let poster = poster::view(&self.model, id, cx);
            v_flex()
                .id(("continue", id.0 as usize))
                .w(px(120.))
                .flex_none()
                .gap_1()
                .p_1()
                .rounded_md()
                .cursor_pointer()
                .hover(|card| card.bg(cx.theme().table_hover))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(170.))
                        .rounded_md()
                        .overflow_hidden()
                        .child(poster)
                        .child(
                            div()
                                .absolute()
                                .top_1()
                                .left_1()
                                .px_2()
                                .rounded_full()
                                .text_xs()
                                .font_semibold()
                                .bg(style::badge_on_image(cx))
                                .text_color(style::accent(cx))
                                .child(format!("{new} new")),
                        ),
                )
                .child(div().text_sm().line_clamp(2).child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("Next: episode {next}")),
                )
                .on_click(cx.listener(move |this, _, window, cx| this.open(id, window, cx)))
        });
        let mut cards = cards.peekable();
        let empty = cards.peek().is_none();
        v_flex()
            .gap_2()
            .child(div().font_semibold().child("Continue watching"))
            .when(empty, |section| {
                section.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("You are up to date with everything you are watching."),
                )
            })
            .child(
                h_flex()
                    .id("continue-strip")
                    .gap_3()
                    .overflow_x_scroll()
                    .children(cards),
            )
    }

    fn render_schedule(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tz = TimeZone::system();
        let days = airing_schedule(self.model.read(cx).state(), Timestamp::now(), &tz, 7);
        let muted = cx.theme().muted_foreground;
        let day_sections = days.into_iter().enumerate().map(|(i, day)| {
            let name = match i {
                0 => "Today".to_owned(),
                1 => "Tomorrow".to_owned(),
                _ => day.date.strftime("%A").to_string(),
            };
            let date = day.date.strftime("%-d %b").to_string();
            let empty = day.airings.is_empty();
            let rows = day.airings.into_iter().map(|airing| {
                let state = self.model.read(cx).state();
                let id = airing.anime_id;
                let title: SharedString = state.title(id).into();
                let total = state
                    .anime
                    .get(&id)
                    .and_then(|a| a.episode_count)
                    .map_or(String::new(), |n| format!(" of {n}"));
                let behind = shiori_app::list_view::new_episodes(
                    state.anime.get(&id),
                    state.entries.get(&id),
                );
                let time = airing
                    .time
                    .to_zoned_datetime(tz.clone())
                    .strftime("%H:%M")
                    .to_string();
                let poster = poster::view(&self.model, id, cx);
                h_flex()
                    .id(("airing", id.0 as usize))
                    .gap_3()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|row| row.bg(cx.theme().table_hover))
                    .child(
                        div()
                            .w(px(44.))
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(time),
                    )
                    .child(
                        div()
                            .w(px(26.))
                            .h(px(36.))
                            .flex_none()
                            .rounded_sm()
                            .overflow_hidden()
                            .child(poster),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().truncate().child(title))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("Episode {}{total}", airing.episode)),
                            ),
                    )
                    .when(behind > 0, |row| {
                        row.child(
                            div()
                                .px_2()
                                .rounded_full()
                                .text_xs()
                                .font_semibold()
                                .bg(style::accent_soft(cx))
                                .text_color(style::accent(cx))
                                .child(format!("{behind} to catch up")),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| this.open(id, window, cx)))
            });
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_baseline()
                        .child(div().text_sm().font_semibold().child(name))
                        .child(div().text_xs().text_color(muted).child(date)),
                )
                .children(rows)
                .when(empty, |day| {
                    day.child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(muted)
                            .child("Nothing airing"),
                    )
                })
        });
        v_flex()
            .gap_3()
            .max_w(px(640.))
            .child(div().font_semibold().child("Airing this week"))
            .children(day_sections)
    }
}

impl Render for HomePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let today = jiff::ZonedDateTime::now()
            .strftime("%A, %-d %B")
            .to_string();
        v_flex()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .px_4()
            .pt_3()
            .pb_6()
            .gap_5()
            .child(
                h_flex()
                    .gap_2()
                    .items_baseline()
                    .child(div().text_lg().font_semibold().child("Home"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(today),
                    ),
            )
            .child(self.render_continue(cx))
            .child(self.render_schedule(cx))
    }
}
