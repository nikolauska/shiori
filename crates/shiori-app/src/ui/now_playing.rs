//! What is playing: the now playing part of the bottom bar, and the
//! inspector's Now playing tab with why Shiori thinks it is that anime
//! (ui.md, "Now playing bar" and "Inspector").

use gpui::{
    AnyElement, App, Entity, Hsla, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use shiori_core::entry::is_in_list;
use shiori_core::recognition::{MatchReason, TitleKind};
use shiori_core::{AnimeId, ElementKind, Phase, Reason};

use shiori_app::tracking::{Media, Source, update_settings};

use super::model::Model;
use super::{link_picker, poster, style};

/// The anime the playing media is identified as, if any.
pub fn playing_anime(model: &Model) -> Option<AnimeId> {
    model.tracker.media.as_ref()?.episode.anime_id
}

/// What the update session is doing, in words, with the buttons that go
/// with it. Shared by the bar and the inspector tab so they never disagree.
struct Update {
    detail: String,
    /// The user should look at it: a confirmation is waiting.
    attention: bool,
    /// How much of the countdown has passed, 0 to 1.
    progress: Option<f32>,
    /// Label and whether it accepts (true) or cancels.
    buttons: &'static [(&'static str, bool)],
}

fn update(model: &Model, media: &Media) -> Update {
    let state = model.tracker.state();
    let remaining = state.remaining.as_secs();
    let plain = |detail: &str| Update {
        detail: detail.to_owned(),
        attention: false,
        progress: None,
        buttons: &[],
    };
    match state.phase {
        Phase::Idle if media.episode.anime_id.is_none() => plain(""),
        Phase::Idle => plain(""),
        Phase::Denied => plain(match state.reason {
            Reason::OutsideLibrary => "Not updating: outside the library folders",
            Reason::InvalidEpisode => "Not updating: not a valid episode number",
            Reason::AlreadyWatched => "Already watched",
            Reason::None | Reason::SkipsAhead => "Not updating",
        }),
        // Shown over the countdown, which stands still until a pick is made.
        Phase::Countdown | Phase::WaitingForClose if model.tracker.is_held() => {
            plain("Waiting while you pick an anime")
        }
        Phase::Countdown => {
            let delay = update_settings(&model.state().settings).delay;
            Update {
                detail: format!(
                    "Updating in {}:{:02}{}",
                    remaining / 60,
                    remaining % 60,
                    if state.paused { " (paused)" } else { "" }
                ),
                attention: false,
                progress: (delay > 0)
                    .then(|| 1.0 - (remaining as f32 / delay as f32).clamp(0.0, 1.0)),
                buttons: &[("Update now", true), ("Cancel", false)],
            }
        }
        Phase::WaitingForClose => Update {
            detail: format!("Will update when {} is closed", media.player),
            attention: false,
            progress: None,
            buttons: &[("Update now", true), ("Cancel", false)],
        },
        Phase::Confirming => Update {
            detail: format!(
                "Episode {} is ahead of your progress ({})",
                state.episode, state.previous_episode
            ),
            attention: true,
            progress: None,
            buttons: &[("Update", true), ("Cancel", false)],
        },
        Phase::Committed => plain(&format!("Updated to episode {}", state.episode)),
        Phase::Cancelled => Update {
            detail: "Update cancelled".into(),
            attention: false,
            progress: None,
            buttons: &[("Update anyway", true)],
        },
    }
}

fn buttons(model: &Entity<Model>, update: &Update) -> impl Iterator<Item = Button> {
    update.buttons.iter().map(|&(label, accept)| {
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
    })
}

/// "7", or "7-8" for a release with several episodes.
fn episode_label(media: &Media) -> Option<String> {
    let (first, last) = media.episode.episode_number_range()?;
    Some(if last > first {
        format!("{first}-{last}")
    } else {
        last.to_string()
    })
}

/// The file name, or the window title for streams.
fn source_label(media: &Media) -> String {
    match &media.source {
        Source::File(path) => path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        ),
        Source::Title(title) => title.clone(),
    }
}

/// Watched count, or a note when the anime is not in the list (watching it
/// adds it, as Watching).
fn progress_label(model: &Model, id: AnimeId) -> String {
    match model
        .state()
        .entries
        .get(&id)
        .filter(|e| is_in_list(Some(e)))
    {
        Some(entry) => format!("watched {}", entry.watched_episodes),
        None => "not in your list".into(),
    }
}

fn countdown_bar(progress: f32, width: f32, cx: &App) -> impl IntoElement {
    div()
        .w(px(width))
        .h(px(3.))
        .rounded_full()
        .bg(cx.theme().muted)
        .child(
            div()
                .h_full()
                .w(px(width * progress))
                .rounded_full()
                .bg(style::gradient()),
        )
}

/// A poster-sized box: the anime's poster, or a "?" while not identified.
fn poster_box(model: &Entity<Model>, id: Option<AnimeId>, w: f32, cx: &mut App) -> AnyElement {
    let frame = div()
        .w(px(w))
        .h(px(w * 1.4))
        .flex_none()
        .rounded_sm()
        .overflow_hidden();
    match id {
        Some(id) => frame.child(poster::view(model, id, cx)).into_any_element(),
        None => frame
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().muted)
            .text_color(cx.theme().muted_foreground)
            .font_semibold()
            .child("?")
            .into_any_element(),
    }
}

/// The bar's content while media is detected; `None` when nothing is
/// playing, so the status text shows instead.
pub fn view(model: &Entity<Model>, cx: &mut App) -> Option<AnyElement> {
    let m = model.read(cx);
    let media = m.tracker.media.as_ref()?.clone();
    let id = media.episode.anime_id;
    let update = update(m, &media);
    let episode = episode_label(&media);
    let warning = cx.theme().warning;
    let muted = cx.theme().muted_foreground;

    let (title, meta): (SharedString, String) = match id {
        Some(id) => {
            let anime = m.state().anime.get(&id);
            let total = anime.and_then(|a| a.episode_count);
            let number = match (&episode, total) {
                (Some(n), Some(total)) => format!("Episode {n} of {total} · "),
                (Some(n), None) => format!("Episode {n} · "),
                (None, _) => String::new(),
            };
            (
                m.state().title(id).into(),
                format!(
                    "{number}{} · {} · {}",
                    progress_label(m, id),
                    media.player,
                    source_label(&media)
                ),
            )
        }
        None => {
            let parsed = media
                .episode
                .get(ElementKind::Title)
                .unwrap_or("Unknown title");
            let title = match &episode {
                Some(n) => format!("{parsed} · episode {n}"),
                None => parsed.to_owned(),
            };
            (
                title.into(),
                format!(
                    "Not recognized · {} · {}",
                    media.player,
                    source_label(&media)
                ),
            )
        }
    };

    let poster = poster_box(model, id, 28., cx);
    let what = h_flex()
        .id("now-playing")
        .flex_1()
        .min_w_0()
        .gap_2()
        .px_1()
        .rounded_md()
        .child(poster)
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_sm()
                        .font_medium()
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
                .child(
                    div()
                        .truncate()
                        .text_color(if id.is_some() { muted } else { warning })
                        .child(meta),
                ),
        )
        // Only an identified anime has a page to go to.
        .when_some(id, |el, _| {
            let model = model.clone();
            el.cursor_pointer()
                .hover(|el| el.bg(cx.theme().muted))
                .on_click(move |_, _, cx| model.update(cx, |model, cx| model.show_now_playing(cx)))
        });

    let status = v_flex()
        .flex_none()
        // Wide enough for the longest message, the not-recognized hint.
        .max_w(px(420.))
        .items_end()
        .gap_1()
        .child(
            div()
                .truncate()
                .text_color(if update.attention { warning } else { muted })
                .child(update.detail.clone()),
        )
        .when_some(update.progress, |el, progress| {
            el.child(countdown_bar(progress, 96., cx))
        });

    // Not recognized: the likely answers as one-click chips, then the
    // picker. Recognized: a way to say it is the wrong anime, plainer when
    // the update is denied, which is when a wrong match usually shows.
    let suggestions: Vec<(AnimeId, String)> = if id.is_none() {
        let m = model.read(cx);
        m.tracker
            .suggestions
            .iter()
            .take(2)
            .map(|&s| (s, m.state().title(s)))
            .collect()
    } else {
        Vec::new()
    };
    let denied = model.read(cx).tracker.state().phase == Phase::Denied;
    let chips = suggestions
        .into_iter()
        .enumerate()
        .map(|(ix, (anime, title))| {
            let model = model.clone();
            Button::new(("suggestion", ix))
                .xsmall()
                .outline()
                .label(title)
                .tooltip("Link and remember")
                .on_click(move |_, _, cx| {
                    model.update(cx, |model, cx| model.set_now_playing(anime, cx))
                })
        });
    let link = {
        let model = model.clone();
        let (label, tooltip) = match (id, denied) {
            (None, _) => ("Link anime…", "Tell Shiori which anime this is"),
            (Some(_), true) => ("Wrong anime?", "Link it to another anime"),
            (Some(_), false) => ("Change…", "Link it to another anime"),
        };
        Button::new("link-anime")
            .xsmall()
            .when(id.is_none(), |b| b.primary())
            .when(id.is_some(), |b| b.ghost())
            .label(label)
            .tooltip(tooltip)
            .on_click(move |_, window, cx| {
                link_picker::open(&model, link_picker::Target::NowPlaying, window, cx)
            })
    };

    Some(
        h_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(what)
            .child(status)
            .when(id.is_none(), |el| {
                el.child(div().flex_none().text_color(muted).child("Is it"))
            })
            .children(chips)
            .children(buttons(model, &update))
            .child(link)
            .into_any_element(),
    )
}

/// Why the media matched the anime, as a sentence.
fn matched_because(model: &Model, source: &Source, reason: &MatchReason) -> String {
    let from = match source {
        Source::File(_) => "file name",
        Source::Title(_) => "window title",
    };
    // "Title" or "Title" with season 3: the season counts only when it was
    // needed to find the anime.
    let quoted = |title: &str, season: Option<u32>| match season {
        Some(season) => format!("\"{title}\" with season {season}"),
        None => format!("\"{title}\""),
    };
    match reason {
        MatchReason::Title {
            title,
            season,
            kind,
        } => {
            let file = quoted(title, *season);
            match kind {
                TitleKind::MainTitle => format!("The {from}'s {file} matches its title."),
                TitleKind::UserSynonym => format!(
                    "The {from}'s {file} is one of your alternative titles for it (Settings tab)."
                ),
                TitleKind::TitleWithYear => {
                    format!("The {from}'s {file} matches its title with the start year.")
                }
                TitleKind::ServiceSynonym => format!(
                    "The {from}'s {file} matches another title {} lists for it.",
                    model.service_name()
                ),
            }
        }
        MatchReason::Redirect {
            title,
            season,
            from,
            from_range: (first, last),
            ..
        } => {
            let number = if last > first {
                format!("episodes {first}-{last}")
            } else {
                format!("episode {last}")
            };
            format!(
                "The {from}'s {} matches {}, which has no {number}. The anime relations file moves it to this anime.",
                quoted(title, *season),
                model.state().title(*from)
            )
        }
        MatchReason::Manual => "You picked it with Set as now playing.".into(),
    }
}

/// One small labeled line of the tab.
fn row(label: &'static str, value: String, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_2()
        .items_start()
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().text_sm().child(value))
}

/// One block per episode: watched ones faint, the playing one in the accent.
/// Long series get a plain bar; 60 blocks is about what stays readable.
fn episode_rail(watched: u32, playing: Option<u32>, total: u32, cx: &App) -> AnyElement {
    if total == 0 || total > 60 {
        let progress = if total == 0 {
            0.0
        } else {
            watched.min(total) as f32 / total as f32
        };
        return div()
            .h(px(6.))
            .w_full()
            .rounded_full()
            .bg(cx.theme().muted)
            .child(
                div()
                    .h_full()
                    .w(gpui::relative(progress))
                    .rounded_full()
                    .bg(style::gradient()),
            )
            .into_any_element();
    }
    // Stronger than the badge tint so watched blocks read as filled.
    let watched_color: Hsla = style::accent(cx).opacity(0.4);
    h_flex()
        .gap(px(2.))
        .children((1..=total).map(|n| {
            let block = div().flex_1().h(px(8.)).rounded(px(2.));
            if Some(n) == playing {
                block.bg(style::gradient())
            } else if n <= watched {
                block.bg(watched_color)
            } else {
                block.bg(cx.theme().muted)
            }
        }))
        .into_any_element()
}

/// The inspector's Now playing tab for `id`; empty when something else is
/// playing (the inspector hides the tab then).
pub fn tab(model: &Entity<Model>, id: AnimeId, cx: &App) -> AnyElement {
    let m = model.read(cx);
    let Some(media) = m
        .tracker
        .media
        .as_ref()
        .filter(|m| m.episode.anime_id == Some(id))
    else {
        return div().into_any_element();
    };
    let update = update(m, media);
    let anime = m.state().anime.get(&id);
    let total = anime.and_then(|a| a.episode_count);
    let entry = m.state().entries.get(&id).filter(|e| is_in_list(Some(e)));
    let watched = entry.map_or(0, |e| e.watched_episodes);
    let playing = media.episode.episode_number_range().map(|(_, last)| last);
    let heading = match (episode_label(media), total) {
        (Some(n), Some(total)) => format!("Episode {n} of {total}"),
        (Some(n), None) => format!("Episode {n}"),
        (None, _) => "No episode number".into(),
    };
    let muted = cx.theme().muted_foreground;
    let warning = cx.theme().warning;

    let mut details = vec![
        (
            match media.source {
                Source::File(_) => "File",
                Source::Title(_) => "Window title",
            },
            source_label(media),
        ),
        ("Player", media.player.clone()),
    ];
    if let Source::File(path) = &media.source
        && let Some(folder) = path.parent()
    {
        details.push(("Folder", folder.display().to_string()));
    }
    for (label, kind) in [
        ("Release group", ElementKind::ReleaseGroup),
        ("Resolution", ElementKind::VideoResolution),
    ] {
        if let Some(value) = media.episode.get(kind) {
            details.push((label, value.to_owned()));
        }
    }
    let because = media
        .matched
        .as_ref()
        .map(|r| matched_because(m, &media.source, r));

    v_flex()
        .gap_3()
        .p_4()
        .child(
            h_flex()
                .justify_between()
                .child(div().text_sm().font_semibold().child(heading))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(progress_label(m, id)),
                ),
        )
        .child(episode_rail(watched, playing, total.unwrap_or(0), cx))
        .child(
            v_flex()
                .gap_2()
                .when(!update.detail.is_empty(), |el| {
                    el.child(
                        div()
                            .text_sm()
                            .text_color(if update.attention { warning } else { muted })
                            .child(update.detail.clone()),
                    )
                })
                .when_some(update.progress, |el, progress| {
                    el.child(countdown_bar(progress, 160., cx))
                })
                .child(h_flex().gap_2().children(buttons(model, &update))),
        )
        .when_some(because, |el, because| {
            el.child(
                v_flex()
                    .gap_1()
                    .p_2()
                    .rounded_md()
                    .bg(cx.theme().muted)
                    .text_xs()
                    .child(div().text_color(muted).child("Matched because"))
                    .child(div().child(because))
                    .child(div().child({
                        let model = model.clone();
                        Button::new("wrong-anime")
                            .xsmall()
                            .link()
                            .label("Wrong anime? Link another…")
                            .on_click(move |_, window, cx| {
                                link_picker::open(
                                    &model,
                                    link_picker::Target::NowPlaying,
                                    window,
                                    cx,
                                )
                            })
                    })),
            )
        })
        .child(
            v_flex().gap_1().children(
                details
                    .into_iter()
                    .map(|(label, value)| row(label, value, cx)),
            ),
        )
        .into_any_element()
}
