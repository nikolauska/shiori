//! Small pieces shared by several views. The visual ones (tiles, chips,
//! cards, empty states) exist so that numbers and choices are shown, not
//! written out in sentences, and look the same wherever they appear.

use gpui::{
    AnyElement, App, Div, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, Stateful, Styled, div, prelude::FluentBuilder,
};
use gpui_component::searchable_list::SearchableListItem;
use gpui_component::{ActiveTheme as _, Icon, IconNamed, h_flex, v_flex};

/// A dropdown item that shows a label and stands for a value.
#[derive(Debug, Clone)]
pub struct Choice<T> {
    pub label: SharedString,
    pub value: T,
}

impl<T> Choice<T> {
    pub fn new(label: impl Into<SharedString>, value: T) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

impl<T: Clone + PartialEq> SearchableListItem for Choice<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &T {
        &self.value
    }
}

/// A small heading over a group, in capitals so it reads as a label and not
/// as content.
pub fn section_label(text: impl Into<SharedString>, cx: &App) -> Div {
    let text: SharedString = text.into();
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(SharedString::from(text.to_uppercase()))
}

/// A number with what it counts under it: episodes, minutes, anime added.
/// `value` is any element so it can carry an icon or a color.
pub fn stat_tile(value: impl IntoElement, label: impl Into<SharedString>, cx: &App) -> Div {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(value),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(label.into()),
        )
}

/// A rounded label: a genre, a tag, a player. `on` draws it in the accent.
pub fn chip(text: impl Into<SharedString>, on: bool, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .gap_1()
        .px_2()
        .py_0p5()
        .rounded_full()
        .text_xs()
        .map(|chip| {
            if on {
                chip.bg(super::style::accent_soft(cx))
                    .text_color(super::style::accent(cx))
            } else {
                chip.bg(cx.theme().muted)
            }
        })
        .child(text.into())
}

/// An icon on a rounded square. `strong` fills it with the primary color,
/// for the suggested or chosen option.
pub fn icon_tile(icon: impl IconNamed, strong: bool, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .size_9()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .map(|tile| {
            if strong {
                tile.bg(theme.primary).text_color(theme.primary_foreground)
            } else {
                tile.bg(theme.muted).text_color(theme.muted_foreground)
            }
        })
        .child(Icon::new(icon).size_4())
}

/// The same tile tinted with a status color, such as green for a fresh
/// backup or amber for one that is due.
pub fn tinted_icon_tile(icon: impl IconNamed, color: Hsla) -> Div {
    div()
        .size_9()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(color.opacity(0.15))
        .text_color(color)
        .child(Icon::new(icon).size_4())
}

/// A clickable option: an icon tile, a title and one line under it. The
/// chosen one gets a strong border and a filled icon, so the current choice
/// is seen without reading.
pub fn option_card(
    id: impl Into<ElementId>,
    icon: impl IconNamed,
    title: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    chosen: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let muted = theme.muted;
    h_flex()
        .id(id)
        .flex_1()
        .min_w_0()
        .gap_3()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(if chosen { theme.primary } else { theme.border })
        .cursor_pointer()
        .hover(move |card| card.bg(muted))
        .child(icon_tile(icon, chosen, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(hint.into()),
                ),
        )
}

/// A square action tile: icon on top, a word, and what it works on. For a
/// few actions side by side, where buttons in a row would read as a sentence.
pub fn action_tile(
    id: impl Into<ElementId>,
    icon: impl IconNamed,
    title: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let muted = theme.muted;
    v_flex()
        .id(id)
        .flex_1()
        .min_w_0()
        .gap_1()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .cursor_pointer()
        .hover(move |tile| tile.bg(muted))
        .child(icon_tile(icon, false, cx))
        .child(
            div()
                .pt_1()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(hint.into()),
        )
}

/// What an empty page shows: an icon, a few words, one line under them and
/// the action that fills the page, if there is one.
pub fn empty_state(
    icon: impl IconNamed,
    title: impl Into<SharedString>,
    line: impl Into<SharedString>,
    action: Option<AnyElement>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .child(icon_tile(icon, false, cx).size_12())
        .child(
            div()
                .pt_1()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(line.into()),
        )
        .children(action.map(|action| div().pt_2().child(action)))
}

/// A thin bar filled to `fraction` with the accent gradient.
pub fn progress_bar(fraction: f32, cx: &App) -> Div {
    let fraction = fraction.clamp(0.0, 1.0);
    div()
        .h(gpui::px(5.))
        .w_full()
        .rounded_full()
        .bg(cx.theme().muted)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(gpui::relative(fraction))
                .rounded_full()
                .bg(super::style::gradient()),
        )
}
