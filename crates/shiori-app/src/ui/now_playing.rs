//! The now playing part of the bottom bar: what is playing and what will
//! happen to the list (ui.md, "Now playing bar").

use gpui::{App, Entity, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex};
use shiori_core::{ElementKind, Phase, Reason};

use super::model::Model;
use super::style;

/// `None` when nothing is playing, so the status text shows instead.
pub fn view(model: &Entity<Model>, cx: &App) -> Option<impl IntoElement> {
    let m = model.read(cx);
    let media = m.tracker.media.as_ref()?;
    let state = m.tracker.state();
    let episode = &media.episode;
    let number = episode
        .episode_number_range()
        .map(|(_, last)| format!(" - episode {last}"))
        .unwrap_or_default();
    let title: SharedString = match episode.anime_id {
        Some(id) => format!("{}{number}", m.state().title(id)).into(),
        None => format!(
            "{}{number}",
            episode.get(ElementKind::Title).unwrap_or("Unknown title")
        )
        .into(),
    };
    let remaining = state.remaining.as_secs();
    let (detail, buttons): (String, &[(&str, bool)]) = match state.phase {
        Phase::Idle if episode.anime_id.is_none() => (
            "Not recognized. Right-click the anime in your list and pick Set as now playing."
                .into(),
            &[],
        ),
        Phase::Idle => (String::new(), &[]),
        Phase::Denied => (
            match state.reason {
                Reason::OutsideLibrary => "Not updating: outside the library folders",
                Reason::InvalidEpisode => "Not updating: not a valid episode number",
                Reason::AlreadyWatched => "Already watched",
                Reason::None | Reason::SkipsAhead => "Not updating",
            }
            .into(),
            &[],
        ),
        Phase::Countdown => (
            format!(
                "Updating in {}:{:02}{}",
                remaining / 60,
                remaining % 60,
                if state.paused { " (paused)" } else { "" }
            ),
            &[("Update now", true), ("Cancel", false)],
        ),
        Phase::WaitingForClose => (
            "Will update when the player is closed".into(),
            &[("Update now", true), ("Cancel", false)],
        ),
        Phase::Confirming => (
            format!(
                "Episode {} is ahead of your progress ({})",
                state.episode, state.previous_episode
            ),
            &[("Update", true), ("Cancel", false)],
        ),
        Phase::Committed => (format!("Updated to episode {}", state.episode), &[]),
        Phase::Cancelled => ("Update cancelled".into(), &[("Update anyway", true)]),
    };
    let theme = cx.theme();
    let ring = if episode.anime_id.is_some() {
        style::gradient()
    } else {
        theme.muted_foreground.into()
    };
    let buttons = buttons.iter().map(|&(label, accept)| {
        let model = model.clone();
        Button::new(label)
            .xsmall()
            .label(label)
            .when(accept, |b| b.primary())
            .when(!accept, |b| b.ghost())
            .on_click(move |_, _, cx| {
                model.update(cx, |model, cx| {
                    if accept {
                        model.accept_update(cx)
                    } else {
                        model.cancel_update(cx)
                    }
                })
            })
    });
    Some(
        h_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .child(div().size(px(8.)).flex_none().rounded_full().bg(ring))
            .child(
                div()
                    .flex_none()
                    .max_w(px(360.))
                    .truncate()
                    .text_color(theme.foreground)
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(detail),
            )
            .children(buttons),
    )
}
