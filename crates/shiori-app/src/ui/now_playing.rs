//! What is playing: the now playing part of the bottom bar, and the
//! inspector's Now playing tab with why Shiori thinks it is that anime
//! (ui.md, "Now playing bar" and "Inspector").

use gpui::{
    AnyElement, App, Entity, Hsla, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex};
use shiori_core::entry::is_in_list;
use shiori_core::recognition::{
    MatchReason, NumberingSequence, Proposal, ReviewReason, SequenceEntry, TitleKind,
};
use shiori_core::{AnimeId, ElementKind, Phase, Reason};

use shiori_app::tracking::{Media, Source, update_settings};

use super::model::Model;
use super::numbering::SuggestionState;
use super::{link_picker, numbering_editor, poster, style};

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
            Reason::OutsideLibrary => "Outside library folders",
            Reason::InvalidEpisode => "No valid episode number",
            Reason::AlreadyWatched => "Already watched",
            Reason::None | Reason::SkipsAhead => "Not updating",
        }),
        // Shown over the countdown, which stands still until a pick is made.
        Phase::Countdown | Phase::WaitingForClose if model.tracker.is_held() => plain("Paused"),
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
            detail: "Updates when closed".into(),
            attention: false,
            progress: None,
            buttons: &[("Update now", true), ("Cancel", false)],
        },
        Phase::Confirming => Update {
            detail: format!("Skips ahead of episode {}", state.previous_episode),
            attention: true,
            progress: None,
            buttons: &[("Update", true), ("Cancel", false)],
        },
        Phase::Committed => plain("Updated"),
        Phase::Cancelled => Update {
            detail: "Cancelled".into(),
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
            // The player and file are on the Now playing tab; the bar keeps
            // to what tells the user whether it is the right episode.
            (
                m.state().title(id).into(),
                format!("{number}{}", progress_label(m, id)),
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
            // With a numbering suggestion the reason it is not recognized
            // is known, and says more than "Not recognized".
            let why = match &m.tracker.suggestion {
                Some(suggestion) => {
                    let state = m.state();
                    let count = state
                        .anime
                        .get(&suggestion.miss.anime)
                        .and_then(|a| a.episode_count)
                        .unwrap_or_default();
                    format!(
                        "{} has {count} episodes",
                        state.title(suggestion.miss.anime)
                    )
                }
                None => "Not recognized".to_owned(),
            };
            (title.into(), why)
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
            .take(1)
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
    // A recognized anime offers this only when the update is denied, which
    // is when a wrong match shows; otherwise it is on the Now playing tab.
    let link = (id.is_none() || denied).then(|| {
        let model = model.clone();
        Button::new("link-anime")
            .xsmall()
            .when(id.is_none(), |b| b.primary())
            .when(id.is_some(), |b| b.ghost())
            .label(if id.is_none() {
                "Link anime…"
            } else {
                "Wrong anime?"
            })
            .on_click(move |_, window, cx| {
                link_picker::open(&model, link_picker::Target::NowPlaying, window, cx)
            })
    });

    // The title matched but the number is above the anime's count: the
    // numbering suggestion is the one problem shown, instead of chips.
    if id.is_none()
        && let Some(numbering) = numbering_controls(model, cx)
    {
        return Some(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_3()
                .child(what)
                .child(numbering)
                .into_any_element(),
        );
    }

    Some(
        h_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(what)
            .child(status)
            .children(chips)
            .children(buttons(model, &update))
            .children(link)
            .into_any_element(),
    )
}

/// The bar's part for an episode numbering suggestion: the question and its
/// answers (ui.md, "Episode numbering").
fn numbering_controls(model: &Entity<Model>, cx: &App) -> Option<AnyElement> {
    let m = model.read(cx);
    let suggestion = m.tracker.suggestion.clone()?;
    let state = m.state();
    let miss = &suggestion.miss;
    let number = miss.range.1;
    let count = state
        .anime
        .get(&miss.anime)
        .and_then(|a| a.episode_count)
        .unwrap_or_default();
    let warning = cx.theme().warning;
    let muted = cx.theme().muted_foreground;
    // What the editor opens with when there is no suggestion to show.
    let fallback = state
        .numbering
        .of(miss.anime)
        .cloned()
        .unwrap_or(NumberingSequence {
            id: 0,
            root: miss.anime,
            entries: vec![SequenceEntry {
                anime: miss.anime,
                first: 1,
                last: count.max(1),
            }],
            saved: jiff::Timestamp::UNIX_EPOCH,
        });
    let (question, draft, usable) = match &suggestion.state {
        SuggestionState::Loading => ("Checking seasons…".to_owned(), None, None),
        SuggestionState::Failed => ("Couldn't check seasons".to_owned(), Some(fallback), None),
        SuggestionState::Ready(Proposal::Use { sequence, resolved }) => (
            format!(
                "{}, episode {}?",
                state.title(resolved.anime),
                resolved.range.0
            ),
            Some(sequence.clone()),
            Some(sequence.clone()),
        ),
        SuggestionState::Ready(Proposal::Review { draft, reason }) => (
            match reason {
                ReviewReason::Branch => "Seasons branch".to_owned(),
                ReviewReason::UnknownCount(_) => "Episode count unknown".to_owned(),
                ReviewReason::NoFit => "No season fits".to_owned(),
                ReviewReason::Ambiguous => "Unclear which season".to_owned(),
            },
            Some(draft.clone()),
            None,
        ),
    };
    let preview = (miss.anime, number);
    let review = draft.map(|draft| {
        let model = model.clone();
        Button::new("review-numbering")
            .xsmall()
            .when(usable.is_none(), |b| b.primary())
            .when(usable.is_some(), |b| b.ghost())
            .label("Review…")
            .on_click(move |_, window, cx| {
                numbering_editor::open(&model, draft.clone(), Some(preview), true, window, cx)
            })
    });
    // "Use once" is in the editor: two ways to say yes in a bar this small
    // is one too many.
    let answers = usable.map(|sequence| {
        let model = model.clone();
        Button::new("use-remember")
            .xsmall()
            .primary()
            .label("Yes")
            .tooltip("Use and remember this numbering")
            .on_click(move |_, _, cx| {
                model.update(cx, |model, cx| {
                    if let Err(error) = model.save_numbering(sequence.clone(), cx) {
                        model.set_error(format!("Could not save the numbering: {error}"), cx);
                    }
                })
            })
    });
    let dismiss = {
        let model = model.clone();
        Button::new("dismiss-numbering")
            .xsmall()
            .ghost()
            .icon(IconName::Close)
            .tooltip("Dismiss")
            .on_click(move |_, _, cx| model.update(cx, |model, cx| model.dismiss_suggestion(cx)))
    };
    let attention = !matches!(suggestion.state, SuggestionState::Loading);
    Some(
        h_flex()
            .flex_none()
            .max_w(px(640.))
            .gap_2()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(if attention { warning } else { muted })
                    .child(question),
            )
            .children(answers)
            .children(review)
            .child(dismiss)
            .into_any_element(),
    )
}

/// How the media matched the anime, in a few words.
fn matched_by(model: &Model, reason: &MatchReason) -> String {
    let quoted = |title: &str, season: Option<u32>| match season {
        Some(season) => format!("\"{title}\" S{season}"),
        None => format!("\"{title}\""),
    };
    let state = model.state();
    match reason {
        MatchReason::Title {
            title,
            season,
            kind,
        } => {
            let title = quoted(title, *season);
            match kind {
                TitleKind::MainTitle => format!("Title {title}"),
                TitleKind::UserSynonym => format!("Your title {title}"),
                TitleKind::TitleWithYear => format!("Title with year {title}"),
                TitleKind::ServiceSynonym => format!("{} title {title}", model.service_name()),
            }
        }
        MatchReason::Redirect {
            from,
            from_range: (_, last),
            ..
        } => format!("Relations file ({} #{last})", state.title(*from)),
        MatchReason::Numbering {
            from,
            from_range: (_, last),
            ..
        } => format!("Your numbering ({} #{last})", state.title(*from)),
        MatchReason::NumberingOnce {
            from,
            from_range: (_, last),
        } => format!("Suggested numbering, once ({} #{last})", state.title(*from)),
        MatchReason::Manual => "Picked by you".into(),
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

    let mut details = Vec::new();
    if let Some(reason) = &media.matched {
        details.push(("Matched by", matched_by(m, reason)));
    }
    details.push((
        match media.source {
            Source::File(_) => "File",
            Source::Title(_) => "Window",
        },
        source_label(media),
    ));
    details.push(("Player", media.player.clone()));

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
        .child(
            v_flex().gap_1().children(
                details
                    .into_iter()
                    .map(|(label, value)| row(label, value, cx)),
            ),
        )
        .child(h_flex().child({
            let model = model.clone();
            Button::new("wrong-anime")
                .xsmall()
                .ghost()
                .label("Wrong anime?")
                .on_click(move |_, window, cx| {
                    link_picker::open(&model, link_picker::Target::NowPlaying, window, cx)
                })
        }))
        .into_any_element()
}
