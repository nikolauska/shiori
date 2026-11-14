//! The "Link anime" picker: tells Shiori which anime a file or the playing
//! media is, when recognition found nothing or the wrong one, and can
//! remember the answer as an alternative title (ui.md, "Link anime").

use std::path::PathBuf;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex,
};
use shiori_core::entry::is_in_list;
use shiori_core::recognition::{is_generic_title, linked_to, normalize, title_to_remember};
use shiori_core::{
    Action, AnimeId, ElementKind, Episode, ListStatus, Reason, SearchParams, decide_update,
};

use shiori_app::tracking::close_anime;

use super::model::Model;
use super::{format, poster, style};

/// What the picker links.
#[derive(Debug, Clone)]
pub enum Target {
    /// The media playing now.
    NowPlaying,
    /// A library file. Only remembering makes sense for it: there is no
    /// playback to link once.
    File(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Close,
    List,
    Online,
}

impl Group {
    fn label(self) -> &'static str {
        match self {
            Group::Close => "Close matches",
            Group::List => "In your list",
            Group::Online => "On AniList",
        }
    }
}

/// How a result would fare, in the words the badge and preview use.
struct Fit {
    badge: String,
    tone: Tone,
    preview: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Good,
    Neutral,
    Warn,
}

/// Opens the picker for the playing media or a library file.
pub fn open(model: &Entity<Model>, target: Target, window: &mut Window, cx: &mut App) {
    let episode = {
        let m = model.read(cx);
        match &target {
            Target::NowPlaying => m.tracker.media.as_ref().map(|media| media.parsed.clone()),
            Target::File(path) => m.library.recognized.get(path).cloned(),
        }
    };
    let Some(episode) = episode else {
        return;
    };
    let picker = cx.new(|cx| LinkPicker::new(model.clone(), target.clone(), episode, window, cx));
    if matches!(target, Target::NowPlaying) {
        let hold = picker.downgrade().into();
        model.update(cx, |model, cx| {
            model.tracker.hold = Some(hold);
            cx.notify();
        });
    }
    let focus = picker.read(cx).query.focus_handle(cx);
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.title("Link anime").w(px(640.)).child(picker.clone())
    });
    focus.focus(window, cx);
}

pub struct LinkPicker {
    model: Entity<Model>,
    target: Target,
    /// The episode as parsed when the picker opened. A pick applies to this
    /// media only, never to whatever plays by the time it is made.
    episode: Episode,
    query: Entity<InputState>,
    results: Vec<(Group, AnimeId)>,
    /// Found on AniList for `searched`.
    online: Vec<AnimeId>,
    searched: Option<String>,
    searching: bool,
    selected: Option<AnimeId>,
    remember: bool,
    /// Shown instead of linking, when the pick can no longer apply.
    problem: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl LinkPicker {
    fn new(
        model: Entity<Model>,
        target: Target,
        episode: Episode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = episode.title().unwrap_or_default().to_owned();
        let episode_for_remember = episode.clone();
        let query = cx.new(|cx| InputState::new(window, cx).default_value(title.clone()));
        let subscription =
            cx.subscribe_in(&query, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.refresh(cx),
                InputEvent::PressEnter { .. } => this.link(window, cx),
                _ => {}
            });
        let mut picker = Self {
            model,
            target,
            episode,
            query,
            results: Vec::new(),
            online: Vec::new(),
            searched: None,
            searching: false,
            selected: None,
            // A title like "S2" would link files of other anime too.
            remember: title_to_remember(&episode_for_remember)
                .is_some_and(|t| !is_generic_title(&t)),
            problem: None,
            _subscriptions: vec![subscription],
        };
        picker.refresh(cx);
        picker
    }

    /// The title a link remembers: the file's title, with its season.
    fn title(&self) -> String {
        title_to_remember(&self.episode).unwrap_or_default()
    }

    fn season(&self) -> Option<u32> {
        self.episode
            .get(ElementKind::Season)
            .and_then(|s| s.trim().parse().ok())
    }

    /// Works out the results for the text in the box.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let text = self.query.read(cx).value().trim().to_owned();
        let m = self.model.read(cx);
        let state = m.state();
        let mut results: Vec<(Group, AnimeId)> = Vec::new();
        let mut push = |group, id| {
            if !results.iter().any(|(_, seen)| *seen == id) {
                results.push((group, id));
            }
        };
        // The season from the file name still helps when the user edits the
        // title, so "Kusuriya" finds the third season of a "S3" file.
        for id in close_anime(state, &text, self.season(), 6) {
            push(Group::Close, id);
        }
        let words: Vec<String> = text
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let mut listed: Vec<(bool, String, AnimeId)> = state
            .entries
            .values()
            .filter(|e| is_in_list(Some(e)))
            .filter_map(|e| {
                let anime = state.anime.get(&e.anime_id)?;
                let t = &anime.titles;
                let all = format!("{} {} {}", state.title(e.anime_id), t.romaji, t.english)
                    .to_lowercase();
                words.iter().all(|w| all.contains(w.as_str())).then(|| {
                    (
                        e.status != ListStatus::Watching,
                        state.title(e.anime_id),
                        e.anime_id,
                    )
                })
            })
            .collect();
        // Watching first, as that is what is likely playing.
        listed.sort();
        for (_, _, id) in listed.into_iter().take(8) {
            push(Group::List, id);
        }
        if self.searched.as_deref() == Some(text.as_str()) {
            for &id in &self.online {
                push(Group::Online, id);
            }
        }
        self.results = results;
        if !self
            .selected
            .is_some_and(|s| self.results.iter().any(|(_, id)| *id == s))
        {
            // Preselect what Enter should link: the first result that would
            // actually update, else the first one.
            self.selected = self
                .results
                .iter()
                .map(|(_, id)| *id)
                .find(|id| self.fit(*id, cx).tone == Tone::Good)
                .or_else(|| self.results.first().map(|(_, id)| *id));
        }
        cx.notify();
    }

    fn search_online(&mut self, cx: &mut Context<Self>) {
        let text = self.query.read(cx).value().trim().to_owned();
        if text.is_empty() || self.searching {
            return;
        }
        self.searching = true;
        cx.notify();
        let picker: WeakEntity<Self> = cx.entity().downgrade();
        let params = SearchParams {
            text: text.clone(),
            ..SearchParams::default()
        };
        self.model.update(cx, |model, cx| {
            model.search(params, None, cx, move |found, cx| {
                let _ = picker.update(cx, |picker, cx| {
                    picker.searching = false;
                    if let Some((ids, _)) = found {
                        picker.online = ids;
                        picker.searched = Some(text);
                    }
                    picker.refresh(cx);
                });
            });
        });
    }

    /// What linking to `id` would do, from the same rules the tracker uses.
    fn fit(&self, id: AnimeId, cx: &App) -> Fit {
        let m = self.model.read(cx);
        let state = m.state();
        let name = state.title(id);
        let Some(anime) = state.anime.get(&id) else {
            return Fit {
                badge: String::new(),
                tone: Tone::Neutral,
                preview: String::new(),
            };
        };
        if let Target::File(_) = self.target {
            return Fit {
                badge: String::new(),
                tone: Tone::Neutral,
                preview: format!(
                    "Files titled \"{}\" will be recognized as {name}.",
                    self.title()
                ),
            };
        }
        let entry = state.entries.get(&id).filter(|e| is_in_list(Some(e)));
        let mut episode = self.episode.clone();
        episode.anime_id = Some(id);
        let number = episode.episode_number_range().map_or(1, |(_, last)| last);
        let decision = decide_update(&episode, anime, entry, false);
        let (badge, tone, preview) = match (decision.action, decision.reason) {
            (Action::Allow, _) if entry.is_none() => (
                "Adds to Watching".to_owned(),
                Tone::Good,
                format!(
                    "It is added to your list as Watching, at episode {number}, after the countdown."
                ),
            ),
            (Action::Allow, _) => (
                "Next episode".to_owned(),
                Tone::Good,
                format!("Your progress goes to episode {number} after the countdown."),
            ),
            (Action::Confirm, _) => (
                "Ahead of progress".to_owned(),
                Tone::Warn,
                format!(
                    "Episode {number} is ahead of your progress ({}), so Shiori asks first.",
                    entry.map_or(0, |e| e.watched_episodes)
                ),
            ),
            (_, Reason::AlreadyWatched) => (
                "Already watched".to_owned(),
                Tone::Neutral,
                "Already watched, so nothing will update.".to_owned(),
            ),
            (_, Reason::InvalidEpisode) => match anime.episode_count {
                Some(count) if number > count => (
                    format!("Episode {number} > {count}"),
                    Tone::Warn,
                    format!(
                        "Episode {number} is beyond its {count} episodes, so nothing will update."
                    ),
                ),
                _ => (
                    "No episode number".to_owned(),
                    Tone::Warn,
                    "The file has no episode number Shiori can use, so nothing will update."
                        .to_owned(),
                ),
            },
            _ => (String::new(), Tone::Neutral, String::new()),
        };
        Fit {
            badge,
            tone,
            preview: format!("{name}: {preview}"),
        }
    }

    fn link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        let title = self.title().to_owned();
        match self.target.clone() {
            Target::NowPlaying => {
                let still_playing = self
                    .model
                    .read(cx)
                    .tracker
                    .media
                    .as_ref()
                    .is_some_and(|m| m.parsed.elements == self.episode.elements);
                if !still_playing {
                    self.problem = Some(
                        "The playing video changed since this opened. Close it and pick again."
                            .into(),
                    );
                    cx.notify();
                    return;
                }
                let remember = self.remember && !title.is_empty();
                self.model
                    .update(cx, |model, cx| model.link_now_playing(id, remember, cx));
            }
            Target::File(_) => {
                if title.is_empty() {
                    return;
                }
                self.model
                    .update(cx, |model, cx| model.link_title(id, &title, cx));
            }
        }
        window.close_dialog(cx);
    }

    fn render_source(&self, cx: &App) -> impl IntoElement {
        let m = self.model.read(cx);
        let from = match &self.target {
            Target::NowPlaying => m
                .tracker
                .media
                .as_ref()
                .map(|media| media.player.clone())
                .unwrap_or_default(),
            Target::File(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        let chip = |text: String, strong: bool| {
            div()
                .px_1p5()
                .rounded_sm()
                .border_1()
                .border_color(if strong {
                    style::accent(cx)
                } else {
                    cx.theme().border
                })
                .text_color(if strong {
                    style::accent(cx)
                } else {
                    cx.theme().muted_foreground
                })
                .child(text)
        };
        let mut chips = Vec::new();
        if let Some(title) = self.episode.title() {
            chips.push(chip(title.to_owned(), true));
        }
        for (label, kind) in [
            ("season ", ElementKind::Season),
            ("episode ", ElementKind::Episode),
            ("", ElementKind::ReleaseGroup),
        ] {
            if let Some(value) = self.episode.get(kind) {
                chips.push(chip(format!("{label}{value}"), false));
            }
        }
        h_flex()
            .flex_wrap()
            .gap_1()
            .text_xs()
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{from} ·")),
            )
            .children(chips)
    }

    fn render_row(&self, ix: usize, id: AnimeId, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let state = m.state();
        let Some(anime) = state.anime.get(&id) else {
            return div().into_any_element();
        };
        let mut meta = vec![
            format::type_label(anime.kind).to_owned(),
            format::season_label(anime.date_started),
        ];
        if let Some(count) = anime.episode_count {
            meta.push(format!("{count} eps"));
        }
        match state.entries.get(&id).filter(|e| is_in_list(Some(e))) {
            Some(e) => meta.push(format!(
                "{} {}",
                format::status_label(e.status),
                format::progress_label(e, Some(anime))
            )),
            None => meta.push("not in your list".into()),
        }
        let meta = meta
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let title = state.title(id);
        let fit = self.fit(id, cx);
        let selected = self.selected == Some(id);
        let accent = style::accent(cx);
        let (badge_bg, badge_fg) = match fit.tone {
            Tone::Good => (style::accent_soft(cx), accent),
            Tone::Warn => (cx.theme().warning.opacity(0.15), cx.theme().warning),
            Tone::Neutral => (cx.theme().muted, cx.theme().muted_foreground),
        };
        let poster = div()
            .w(px(26.))
            .h(px(36.))
            .flex_none()
            .rounded_sm()
            .overflow_hidden()
            .child(poster::view(&self.model, id, cx));
        h_flex()
            .id(("link-result", ix))
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .when(selected, |el| {
                el.bg(cx.theme().muted).border_l_2().border_color(accent)
            })
            .hover(|el| el.bg(cx.theme().muted))
            .child(poster)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().text_sm().child(title))
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(meta),
                    ),
            )
            .when(!fit.badge.is_empty(), |el| {
                el.child(
                    div()
                        .flex_none()
                        .px_2()
                        .rounded_full()
                        .text_xs()
                        .font_medium()
                        .bg(badge_bg)
                        .text_color(badge_fg)
                        .child(fit.badge),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(id);
                this.problem = None;
                cx.notify();
            }))
            .into_any_element()
    }
}

impl Render for LinkPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let text = self.query.read(cx).value().trim().to_owned();
        let mut list: Vec<AnyElement> = Vec::new();
        let mut last_group = None;
        for (ix, (group, id)) in self.results.clone().into_iter().enumerate() {
            if last_group != Some(group) {
                last_group = Some(group);
                list.push(
                    div()
                        .px_2()
                        .pt_2()
                        .text_xs()
                        .font_semibold()
                        .text_color(muted)
                        .child(group.label().to_uppercase())
                        .into_any_element(),
                );
            }
            list.push(self.render_row(ix, id, cx));
        }
        if self.results.is_empty() {
            list.push(
                div()
                    .p_2()
                    .text_sm()
                    .text_color(muted)
                    .child("No anime in Shiori matches. Search AniList below.")
                    .into_any_element(),
            );
        }
        let searched_here = self.searched.as_deref() == Some(text.as_str());
        let search_label = if self.searching {
            "Searching AniList...".to_owned()
        } else if searched_here && self.online.is_empty() {
            format!("Nothing on AniList for \"{text}\"")
        } else {
            format!("Search AniList for \"{text}\"")
        };

        let selected = self.selected;
        let fit = selected.map(|id| self.fit(id, cx));
        let title = self.title().to_owned();
        let state = self.model.read(cx).state();
        let target_name = selected.map(|id| state.title(id)).unwrap_or_default();
        let current = linked_to(&state.anime_settings, &title).filter(|id| Some(*id) != selected);
        let remember_text = match current {
            Some(other) => format!(
                "Remember: move \"{title}\" from {} to {target_name}",
                state.title(other)
            ),
            None => format!("Remember: files titled \"{title}\" are {target_name}"),
        };
        let can_remember = !title.is_empty() && !normalize(&title).is_empty();
        let now_playing = matches!(self.target, Target::NowPlaying);
        let link_label = match (&self.target, self.remember) {
            (Target::File(_), _) | (Target::NowPlaying, true) => "Link and remember",
            (Target::NowPlaying, false) => "Link for this playback",
        };

        v_flex()
            .gap_2()
            .child(self.render_source(cx))
            .child(Input::new(&self.query).small().cleanable(true))
            .child(
                v_flex()
                    .id("link-results")
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .gap_0p5()
                    .children(list)
                    .child(
                        div().pt_1().child(
                            Button::new("search-online")
                                .ghost()
                                .small()
                                .label(search_label)
                                .disabled(self.searching || text.is_empty())
                                .on_click(cx.listener(|this, _, _, cx| this.search_online(cx))),
                        ),
                    ),
            )
            .child(div().h(px(1.)).bg(cx.theme().border))
            .when_some(fit, |el, fit| el.child(div().text_sm().child(fit.preview)))
            .when(now_playing && can_remember && selected.is_some(), |el| {
                el.child(
                    Checkbox::new("remember")
                        .checked(self.remember)
                        .label(remember_text)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.remember = *checked;
                            cx.notify();
                        })),
                )
            })
            .when_some(self.problem.clone(), |el, problem| {
                el.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(problem),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("link")
                            .primary()
                            .label(link_label)
                            .disabled(selected.is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.link(window, cx))),
                    ),
            )
    }
}
