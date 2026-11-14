//! The inspector: the selected anime beside the list, with its list entry,
//! details and per-anime settings. It replaced the separate details window
//! (decision I22).
//!
//! Edits save as they are made: a choice or a +/- click at once, a text box
//! on Enter or when it loses focus. The queue merges edits made in a row, so
//! saving often costs no extra requests.

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement,
    PathPromptOptions, Render, SharedString, Styled, Subscription, Window, div, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};
use jiff::Timestamp;
use shiori_core::anime::{MAX_EPISODE_COUNT, is_stale};
use shiori_core::entry::is_in_list;
use shiori_core::recognition::normalize;
use shiori_core::{AnimeId, AnimeSettings, FuzzyDate, ListEntry, ListStatus};
use shiori_sync::anilist::score::rating_list;

use shiori_app::list_view::{airing_label, new_episodes};
use shiori_app::state::AppEvent;

use super::library::PlayChoice;
use super::model::Model;
use super::widgets::Choice;
use super::{format, now_playing, numbering_editor, poster, style};

/// Shows an anime: selects it, and the main window brings up the inspector
/// or the full page. Old data is fetched again on the way.
pub fn open(model: &Entity<Model>, id: AnimeId, _: &mut Window, cx: &mut App) {
    if !model.read(cx).state().anime.contains_key(&id) {
        return;
    }
    model.update(cx, |model, cx| model.open_anime(id, cx));
    fetch_if_stale(model, id, cx);
}

/// Showing an anime whose data is old fetches it again.
pub fn fetch_if_stale(model: &Entity<Model>, id: AnimeId, cx: &mut App) {
    let stale = model
        .read(cx)
        .state()
        .anime
        .get(&id)
        .is_some_and(|anime| is_stale(anime, Timestamp::now(), format::today_in_japan()));
    if stale {
        model.update(cx, |model, cx| model.refresh_anime(id, cx));
    }
}

type StatusSelect = SelectState<Vec<Choice<ListStatus>>>;
type ScoreSelect = SelectState<Vec<Choice<u8>>>;

fn status_choices() -> Vec<Choice<ListStatus>> {
    ListStatus::SHOWN
        .iter()
        .map(|&status| Choice::new(format::status_label(status), status))
        .collect()
}

/// Index of the Now playing tab, after List, Details and Settings.
const NOW_PLAYING_TAB: usize = 3;

pub struct Inspector {
    model: Entity<Model>,
    /// The anime shown; follows the model's selection.
    id: Option<AnimeId>,
    tab: usize,
    // List tab
    status: Entity<StatusSelect>,
    watched: Entity<InputState>,
    score: Entity<ScoreSelect>,
    started: Entity<InputState>,
    completed: Entity<InputState>,
    rewatched: Entity<InputState>,
    notes: Entity<TextareaState>,
    // Settings tab
    synonyms: Entity<TextareaState>,
    folder: Entity<InputState>,
    display_title: Entity<InputState>,
    /// Loading values into the boxes must not save them back.
    loading: bool,
    /// The anime the text boxes were filled from. A box loses focus when
    /// another row is clicked, after the selection already moved on, so a
    /// save must go to the anime the text belongs to, not the shown one.
    entry_for: Option<AnimeId>,
    settings_for: Option<AnimeId>,
    _subscriptions: Vec<Subscription>,
}

impl Inspector {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let ratings: Vec<Choice<u8>> = rating_list(format::score_format(model.read(cx).state()))
            .into_iter()
            .map(|r| {
                Choice::new(
                    if r.value == 0 {
                        "–".to_owned()
                    } else {
                        r.label
                    },
                    r.value,
                )
            })
            .collect();
        let status = cx.new(|cx| SelectState::new(status_choices(), None, window, cx));
        let score = cx.new(|cx| SelectState::new(ratings, None, window, cx));
        let watched = text("0", window, cx);
        let started = text("YYYY-MM-DD", window, cx);
        let completed = text("YYYY-MM-DD", window, cx);
        let rewatched = text("0", window, cx);
        let notes = cx.new(|cx| TextareaState::new(window, cx).placeholder("Add a note…"));
        let synonyms = cx.new(|cx| TextareaState::new(window, cx));
        let folder = text("Folder of this anime's files", window, cx);
        let display_title = text("Title to show instead of the service's", window, cx);

        let mut subscriptions = vec![
            cx.observe_in(&model, window, |this, model, window, cx| {
                let selected = model.read(cx).selected;
                if selected != this.id {
                    this.id = selected;
                    this.reload(window, cx);
                }
                cx.notify();
            }),
            cx.subscribe_in(&model, window, Self::on_event),
            cx.subscribe_in(
                &status,
                window,
                |this, _, event: &SelectEvent<_>, window, cx| {
                    if let SelectEvent::Confirm(Some(status)) = event {
                        let status = *status;
                        // Choosing a status of an anime not in the list adds it.
                        this.save_or_add(window, cx, |e| e.status = status);
                    }
                },
            ),
            cx.subscribe_in(
                &score,
                window,
                |this, _, event: &SelectEvent<_>, window, cx| {
                    if let SelectEvent::Confirm(Some(score)) = event {
                        let score = *score;
                        this.save(window, cx, |e| e.score = score);
                    }
                },
            ),
        ];
        // Text boxes save when the user is done with them.
        for input in [&watched, &started, &completed, &rewatched] {
            subscriptions.push(
                cx.subscribe_in(input, window, |this, _, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.save_fields(window, cx);
                    }
                }),
            );
        }
        subscriptions.push(
            cx.subscribe_in(&notes, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Blur) {
                    this.save_fields(window, cx);
                }
            }),
        );
        for input in [&folder, &display_title] {
            subscriptions.push(
                cx.subscribe_in(input, window, |this, _, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.save_settings(window, cx);
                    }
                }),
            );
        }
        subscriptions.push(
            cx.subscribe_in(&synonyms, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Blur) {
                    this.save_settings(window, cx);
                }
            }),
        );

        let id = model.read(cx).selected;
        let mut inspector = Self {
            model,
            id,
            tab: 0,
            status,
            watched,
            score,
            started,
            completed,
            rewatched,
            notes,
            synonyms,
            folder,
            display_title,
            loading: false,
            entry_for: None,
            settings_for: None,
            _subscriptions: subscriptions,
        };
        inspector.reload(window, cx);
        inspector
    }

    fn on_event(
        &mut self,
        _: &Entity<Model>,
        event: &AppEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(shown) = self.id else {
            return;
        };
        match event {
            // A sync or an edit made elsewhere changed what is shown.
            AppEvent::EntryUpdated(id) | AppEvent::EntryDeleted(id) if *id == shown => {
                self.load_entry(window, cx)
            }
            AppEvent::AnimeSettingsUpdated(id) if *id == shown => self.load_settings(window, cx),
            AppEvent::AnimeDeleted { id, .. } if *id == shown => self.id = None,
            _ => {}
        }
        cx.notify();
    }

    /// Switches to the Now playing tab; it shows only while the selected
    /// anime is the one playing.
    pub fn show_now_playing(&mut self, cx: &mut Context<Self>) {
        self.tab = NOW_PLAYING_TAB;
        cx.notify();
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load_entry(window, cx);
        self.load_settings(window, cx);
    }

    fn entry(&self, cx: &App) -> Option<ListEntry> {
        self.entry_of(self.id?, cx)
    }

    fn entry_of(&self, id: AnimeId, cx: &App) -> Option<ListEntry> {
        self.model
            .read(cx)
            .state()
            .entries
            .get(&id)
            .filter(|e| is_in_list(Some(e)))
            .cloned()
    }

    fn load_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.id else {
            return;
        };
        self.loading = true;
        self.entry_for = Some(id);
        let entry = self.entry(cx);
        let in_list = entry.is_some();
        let entry = entry.unwrap_or_else(|| ListEntry::new(id));
        let set = |input: &Entity<InputState>, value: String, window: &mut Window, cx: &mut App| {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        };
        set(
            &self.watched,
            entry.watched_episodes.to_string(),
            window,
            cx,
        );
        set(
            &self.rewatched,
            entry.rewatched_times.to_string(),
            window,
            cx,
        );
        set(
            &self.started,
            format::date_label(entry.date_started),
            window,
            cx,
        );
        set(
            &self.completed,
            format::date_label(entry.date_completed),
            window,
            cx,
        );
        self.notes.update(cx, |input, cx| {
            input.set_value(entry.notes.clone(), window, cx)
        });
        // Something has to be chosen to add an anime; planning to watch it
        // is the usual first step.
        let status = if in_list {
            entry.status
        } else {
            ListStatus::PlanToWatch
        };
        self.status.update(cx, |select, cx| {
            select.set_selected_value(&status, window, cx)
        });
        self.score.update(cx, |select, cx| {
            select.set_selected_value(&entry.score, window, cx)
        });
        self.loading = false;
    }

    fn load_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.id else {
            return;
        };
        self.loading = true;
        self.settings_for = Some(id);
        let settings = self
            .model
            .read(cx)
            .state()
            .anime_settings
            .get(&id)
            .cloned()
            .unwrap_or_default();
        self.display_title.update(cx, |input, cx| {
            input.set_value(settings.display_title.clone(), window, cx)
        });
        self.folder.update(cx, |input, cx| {
            input.set_value(settings.folder.clone(), window, cx)
        });
        self.synonyms.update(cx, |input, cx| {
            input.set_value(settings.synonyms.join("\n"), window, cx)
        });
        self.loading = false;
    }

    /// Changes the entry of an anime in the list. Nothing happens for one
    /// that is not: it is added with the status choice and its button.
    fn save(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut ListEntry),
    ) {
        if self.loading {
            return;
        }
        let Some(mut entry) = self.entry(cx) else {
            return;
        };
        change(&mut entry);
        self.model
            .update(cx, |model, cx| model.save_entry(entry, cx));
    }

    fn save_or_add(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut ListEntry),
    ) {
        if self.entry(cx).is_some() {
            self.save(window, cx, change);
        }
    }

    fn add_to_list(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.id else {
            return;
        };
        let status = self
            .status
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(ListStatus::PlanToWatch);
        let mut entry = ListEntry::new(id);
        entry.status = status;
        self.model
            .update(cx, |model, cx| model.save_entry(entry, cx));
    }

    fn step(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let max = self.episode_limit(cx);
        self.save(window, cx, |e| {
            e.watched_episodes = e.watched_episodes.saturating_add_signed(delta).min(max);
        });
    }

    fn episode_limit(&self, cx: &App) -> u32 {
        self.id
            .and_then(|id| self.model.read(cx).state().anime.get(&id))
            .and_then(|a| a.episode_count)
            .unwrap_or(MAX_EPISODE_COUNT)
    }

    /// Reads every text box of the List tab and saves what changed. A box
    /// that does not parse is put back, with a note saying why.
    fn save_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.entry_for.filter(|_| !self.loading) else {
            return;
        };
        let Some(entry) = self.entry_of(id, cx) else {
            return;
        };
        let max = self
            .model
            .read(cx)
            .state()
            .anime
            .get(&id)
            .and_then(|a| a.episode_count)
            .unwrap_or(MAX_EPISODE_COUNT);
        let value = |input: &Entity<InputState>, cx: &App| input.read(cx).value().trim().to_owned();
        let mut changed = entry.clone();
        let mut problem = None;
        match value(&self.watched, cx).parse::<u32>() {
            Ok(n) if n <= max => changed.watched_episodes = n,
            _ => problem = Some(format!("Enter a number of episodes from 0 to {max}.")),
        }
        match value(&self.rewatched, cx).parse::<u32>() {
            Ok(n) => changed.rewatched_times = n,
            Err(_) => problem = Some("Enter the number of times rewatched.".to_owned()),
        }
        for (date, input) in [
            (&mut changed.date_started, &self.started),
            (&mut changed.date_completed, &self.completed),
        ] {
            let text = value(input, cx);
            let parsed = FuzzyDate::parse(&text);
            // An empty box is "no date"; anything else has to be readable.
            if !text.is_empty() && parsed.is_empty() {
                problem = Some(format!("\"{text}\" is not a date (YYYY-MM-DD)."));
            } else {
                *date = parsed;
            }
        }
        changed.notes = self.notes.read(cx).value().trim().to_owned();
        if let Some(problem) = problem {
            window.push_notification(problem, cx);
            if self.id == Some(id) {
                self.load_entry(window, cx);
            }
            return;
        }
        if changed != entry {
            self.model
                .update(cx, |model, cx| model.save_entry(changed, cx));
        }
    }

    fn save_settings(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let (Some(id), false) = (self.settings_for, self.loading) else {
            return;
        };
        let synonyms = self
            .synonyms
            .read(cx)
            .value()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect::<Vec<String>>();
        // A linked title the user deleted from the box is no longer linked.
        let mut linked = self
            .model
            .read(cx)
            .state()
            .anime_settings
            .get(&id)
            .map(|s| s.linked.clone())
            .unwrap_or_default();
        linked.retain(|l| {
            let key = normalize(&l.title);
            synonyms.iter().any(|s: &String| normalize(s) == key)
        });
        let settings = AnimeSettings {
            id,
            display_title: self.display_title.read(cx).value().trim().to_owned(),
            folder: self.folder.read(cx).value().trim().to_owned(),
            synonyms,
            linked,
        };
        // Saving unchanged settings is a no-op in AppState.
        self.model
            .update(cx, |model, cx| model.save_anime_settings(settings, cx));
    }

    fn browse_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.first()
            {
                let path = path.display().to_string();
                let _ = this.update_in(cx, |this, window, cx| {
                    this.folder
                        .update(cx, |input, cx| input.set_value(path, window, cx));
                    this.save_settings(window, cx);
                });
            }
        })
        .detach();
    }

    fn render_header(&self, id: AnimeId, cx: &mut Context<Self>) -> AnyElement {
        let state = self.model.read(cx).state();
        let Some(anime) = state.anime.get(&id) else {
            return div().into_any_element();
        };
        let title: SharedString = state.title(id).into();
        let meta = [
            format::type_label(anime.kind).to_owned(),
            format::season_label(anime.date_started),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        let genres = anime
            .genres
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ");
        let badge = badge(state, id, cx);
        let muted = cx.theme().muted_foreground;
        h_flex()
            .gap_3()
            .p_4()
            .items_start()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .w(px(84.))
                    .h(px(120.))
                    .flex_none()
                    .rounded_md()
                    .overflow_hidden()
                    .child(poster::view(&self.model, id, cx)),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(div().font_semibold().child(title))
                    .child(div().text_xs().text_color(muted).child(meta))
                    .child(div().text_xs().text_color(muted).child(genres))
                    // In a row of its own so the badge keeps its width.
                    .child(h_flex().pt_1().children(badge)),
            )
            .into_any_element()
    }

    fn row(label: &'static str, control: impl IntoElement, cx: &App) -> impl IntoElement {
        h_flex()
            .gap_2()
            .px_4()
            .py_1()
            .child(
                div()
                    .w(px(96.))
                    .flex_none()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(div().flex_1().min_w_0().child(control))
    }

    fn render_list(&self, id: AnimeId, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.entry(cx) else {
            return v_flex()
                .gap_2()
                .p_4()
                .child(div().text_sm().child("Not in your list."))
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().w_40().child(Select::new(&self.status).small()))
                        .child(
                            Button::new("add")
                                .primary()
                                .small()
                                .label("Add to list")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add_to_list(window, cx)),
                                ),
                        ),
                )
                .into_any_element();
        };
        let total = self
            .model
            .read(cx)
            .state()
            .anime
            .get(&id)
            .and_then(|a| a.episode_count)
            .map_or("?".to_owned(), |n| n.to_string());
        let stepper = h_flex()
            .gap_1()
            .child(
                Button::new("minus")
                    .small()
                    .label("−")
                    .on_click(cx.listener(|this, _, window, cx| this.step(-1, window, cx))),
            )
            .child(div().w(px(56.)).child(Input::new(&self.watched).small()))
            .child(div().text_sm().child(format!("/ {total}")))
            .child(
                Button::new("plus")
                    .small()
                    .label("+")
                    .on_click(cx.listener(|this, _, window, cx| this.step(1, window, cx))),
            );
        let rewatching = h_flex()
            .gap_2()
            .child(
                Checkbox::new("rewatching")
                    .checked(entry.rewatching)
                    .on_click(cx.listener(|this, checked: &bool, window, cx| {
                        let checked = *checked;
                        this.save(window, cx, |e| e.rewatching = checked);
                    })),
            )
            .child(div().w(px(56.)).child(Input::new(&self.rewatched).small()))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("times before"),
            );
        v_flex()
            .py_2()
            .child(Self::row("Status", Select::new(&self.status).small(), cx))
            .child(Self::row("Episodes", stepper, cx))
            .child(Self::row(
                "Score",
                div().w_32().child(Select::new(&self.score).small()),
                cx,
            ))
            .child(Self::row(
                "Started",
                div().w_32().child(Input::new(&self.started).small()),
                cx,
            ))
            .child(Self::row(
                "Completed",
                div().w_32().child(Input::new(&self.completed).small()),
                cx,
            ))
            .child(Self::row("Rewatching", rewatching, cx))
            .child(Self::row(
                "Notes",
                div().h(px(64.)).child(Textarea::new(&self.notes)),
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .px_4()
                    .pt_3()
                    .child(
                        Button::new("anilist")
                            .small()
                            .label("Open on AniList")
                            .on_click(move |_, _, cx| {
                                cx.open_url(&format!("https://anilist.co/anime/{id}"))
                            }),
                    )
                    .child({
                        let model = self.model.clone();
                        Button::new("play-next")
                            .small()
                            .label("Play next")
                            .on_click(move |_, _, cx| {
                                model.update(cx, |model, cx| model.play(id, PlayChoice::Next, cx))
                            })
                    }),
            )
            .child(
                div()
                    .px_4()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Changes save as you make them and sync in the background."),
            )
            .into_any_element()
    }

    fn render_details(&self, id: AnimeId, cx: &mut Context<Self>) -> AnyElement {
        let state = self.model.read(cx).state();
        let Some(anime) = state.anime.get(&id) else {
            return div().into_any_element();
        };
        let titles = &anime.titles;
        let shown = state.title(id);
        let alternatives: Vec<&str> = [
            titles.romaji.as_str(),
            titles.english.as_str(),
            titles.japanese.as_str(),
        ]
        .into_iter()
        .chain(titles.synonyms.iter().map(String::as_str))
        .filter(|t| !t.is_empty() && *t != shown)
        .collect();
        let average = if anime.score > 0.0 {
            format!("{:.2}", anime.score)
        } else {
            String::new()
        };
        let rows = [
            ("Alternative titles", alternatives.join(", ")),
            ("Type", format::type_label(anime.kind).to_owned()),
            (
                "Episodes",
                anime
                    .episode_count
                    .map_or("Unknown".to_owned(), |n| n.to_string()),
            ),
            (
                "Duration",
                anime
                    .episode_length
                    .map_or(String::new(), |m| format!("{m} min")),
            ),
            (
                "Status",
                format::airing_label(anime, format::today_in_japan()).to_owned(),
            ),
            ("Season", format::season_label(anime.date_started)),
            ("Score", average),
            ("Genres", anime.genres.join(", ")),
            (
                "Tags",
                anime
                    .tags
                    .iter()
                    .take(12)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            ("Studios", anime.studios.join(", ")),
            ("Producers", anime.producers.join(", ")),
        ];
        let muted = cx.theme().muted_foreground;
        v_flex()
            .py_2()
            .children(
                rows.into_iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(label, value)| Self::row(label, div().text_sm().child(value), cx)),
            )
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .text_xs()
                    .text_color(muted)
                    .child("Synopsis"),
            )
            .child(
                div()
                    .px_4()
                    .py_1()
                    .text_sm()
                    .child(format::plain_text(&anime.synopsis)),
            )
            .into_any_element()
    }

    /// Which alternative titles came from linking a file, so the user can
    /// tell them from ones typed in and remove one to undo the link.
    fn linked_titles(&self, cx: &App) -> Option<impl IntoElement> {
        let id = self.id?;
        let settings = self.model.read(cx).state().anime_settings.get(&id)?;
        if settings.linked.is_empty() {
            return None;
        }
        let list = settings
            .linked
            .iter()
            .map(|l| format!("\"{}\" ({})", l.title, format::date_label(l.date)))
            .collect::<Vec<_>>()
            .join(", ");
        Some(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "Linked from a file: {list}. Delete a line above to undo its link."
                )),
        )
    }

    /// Whether the anime is in a remembered episode numbering, and the way
    /// to edit or make one.
    fn numbering_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let id = self.id?;
        let state = self.model.read(cx).state();
        let text = match state.numbering.of(id) {
            Some(s) => format!(
                "In a remembered episode numbering of {} anime, starting with {}.",
                s.entries.len(),
                state.title(s.root)
            ),
            None => "For releases that number episodes across seasons.".to_owned(),
        };
        let model = self.model.clone();
        Some(
            v_flex()
                .gap_1()
                .pt_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text),
                )
                .child(
                    h_flex().child(
                        Button::new("numbering")
                            .small()
                            .label("Episode numbering...")
                            .on_click(move |_, window, cx| {
                                numbering_editor::open_for_anime(&model, id, None, window, cx)
                            }),
                    ),
                ),
        )
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .p_4()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Alternative titles, one per line. Used to recognize file names."),
            )
            .child(div().h(px(80.)).child(Textarea::new(&self.synonyms)))
            .children(self.linked_titles(cx))
            .child(div().text_xs().text_color(muted).child("Folder"))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.folder).small()))
                    .child(Button::new("browse").small().label("Browse...").on_click(
                        cx.listener(|this, _, window, cx| this.browse_folder(window, cx)),
                    )),
            )
            .child(div().text_xs().text_color(muted).child("Display title"))
            .child(Input::new(&self.display_title).small())
            .children(self.numbering_row(cx))
            .into_any_element()
    }
}

/// The `<n> new` or `Ep <n> <when>` badge of an anime, if it has one.
pub fn badge(state: &shiori_app::state::AppState, id: AnimeId, cx: &App) -> Option<AnyElement> {
    let anime = state.anime.get(&id)?;
    let new = new_episodes(Some(anime), state.entries.get(&id));
    let (text, strong) = if new > 0 {
        (format!("{new} new"), true)
    } else {
        let time = anime.next_episode_time?;
        let when = airing_label(time, Timestamp::now(), &jiff::tz::TimeZone::system());
        (format!("Ep {} {when}", anime.last_aired_episode + 1), false)
    };
    Some(
        div()
            .flex_none()
            .px_2()
            .rounded_full()
            .text_xs()
            .font_semibold()
            .when(strong, |badge| {
                badge.bg(style::gradient()).text_color(gpui::white())
            })
            .when(!strong, |badge| {
                badge
                    .bg(style::accent_soft(cx))
                    .text_color(style::accent(cx))
            })
            .child(text)
            .into_any_element(),
    )
}

impl Render for Inspector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(id) = self
            .id
            .filter(|id| self.model.read(cx).state().anime.contains_key(id))
        else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Select an anime to see it here.")
                .into_any_element();
        };
        // The Now playing tab exists only while this anime is playing; when
        // playback ends or moves on, fall back to the List tab.
        let playing = now_playing::playing_anime(self.model.read(cx)) == Some(id);
        if self.tab == NOW_PLAYING_TAB && !playing {
            self.tab = 0;
        }
        let tab = match self.tab {
            0 => self.render_list(id, cx),
            1 => self.render_details(id, cx),
            2 => self.render_settings(cx),
            _ => now_playing::tab(&self.model, id, cx),
        };
        v_flex()
            .id("inspector")
            .size_full()
            .overflow_y_scroll()
            .child(self.render_header(id, cx))
            .child(
                TabBar::new("inspector-tabs")
                    .underline()
                    .small()
                    .px_3()
                    .selected_index(self.tab)
                    .child(Tab::new().label("List"))
                    .child(Tab::new().label("Details"))
                    .child(Tab::new().label("Settings"))
                    .when(playing, |tabs| tabs.child(Tab::new().label("Now playing")))
                    .on_click(cx.listener(|this, ix: &usize, _, cx| {
                        this.tab = *ix;
                        cx.notify();
                    })),
            )
            .child(tab)
            .into_any_element()
    }
}
