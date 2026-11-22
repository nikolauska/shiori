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
use gpui_component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::rating::Rating;
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::switch::Switch;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, StyledExt as _,
    WindowExt as _, h_flex, v_flex,
};
use jiff::Timestamp;
use shiori_core::anime::{MAX_EPISODE_COUNT, is_stale};
use shiori_core::entry::is_in_list;
use shiori_core::recognition::normalize;
use shiori_core::{AnimeId, AnimeSettings, FuzzyDate, ListEntry, ListStatus, ServiceId};
use std::time::Duration;

use shiori_sync::anilist::score::{ScoreFormat, rating_list};

use shiori_app::service::anime_page_url;

use shiori_app::list_view::{airing_label, new_episodes};
use shiori_app::state::AppEvent;

use super::icons::{self, AppIcon};
use super::library::PlayChoice;
use super::model::Model;
use super::widgets::Choice;
use super::{format, now_playing, numbering_editor, poster, style, widgets};

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

type ScoreSelect = SelectState<Vec<Choice<u8>>>;

/// The scores a row of stars stands for, lowest star first, when the score
/// format has few enough steps for stars. Finer formats keep the menu,
/// because whole stars would round away what the user picked.
fn star_scores(format: ScoreFormat) -> Option<Vec<u8>> {
    match format {
        ScoreFormat::Point10 | ScoreFormat::Point5 => Some(
            rating_list(format)
                .into_iter()
                .map(|r| r.value)
                .filter(|&v| v > 0)
                .collect(),
        ),
        _ => None,
    }
}

/// About three lines of the inspector: how much synopsis shows folded.
const SYNOPSIS_FOLDED: usize = 200;

/// How long "Saved" stays after an edit.
const SAVED_FOR: Duration = Duration::from_secs(2);

/// Index of the Now playing tab, after List, Details and Settings.
const NOW_PLAYING_TAB: usize = 3;

pub struct Inspector {
    model: Entity<Model>,
    /// The anime shown; follows the model's selection.
    id: Option<AnimeId>,
    tab: usize,
    // List tab
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
    /// Edits made since this many saves ago show "Saved" for a moment;
    /// the counter lets a later edit keep it up past an earlier timer.
    saves: usize,
    saved_shown: bool,
    /// The Details tab shows a few tags and three lines of synopsis until
    /// asked for the rest; another anime starts folded again.
    tags_open: bool,
    synopsis_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl Inspector {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let ratings: Vec<Choice<u8>> = format::ratings(model.read(cx).state())
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
            saves: 0,
            saved_shown: false,
            tags_open: false,
            synopsis_open: false,
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
            // The anime may not even be in the list that was opened.
            AppEvent::ListReplaced => self.id = None,
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
        self.tags_open = false;
        self.synopsis_open = false;
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
        self.show_saved(cx);
    }

    /// Choosing a status of an anime not in the list adds it with that
    /// status.
    fn set_status(&mut self, status: ListStatus, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.id else {
            return;
        };
        if self.entry(cx).is_some() {
            self.save(window, cx, |e| e.status = status);
            return;
        }
        let mut entry = ListEntry::new(id);
        entry.status = status;
        self.model
            .update(cx, |model, cx| model.save_entry(entry, cx));
        self.show_saved(cx);
    }

    /// Edits save at once, so instead of a standing note saying so, a short
    /// "Saved" confirms each one.
    fn show_saved(&mut self, cx: &mut Context<Self>) {
        self.saves += 1;
        self.saved_shown = true;
        let saves = self.saves;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVED_FOR).await;
            let _ = this.update(cx, |this, cx| {
                if this.saves == saves {
                    this.saved_shown = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
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
            self.show_saved(cx);
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
                    .w(px(72.))
                    .flex_none()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(div().flex_1().min_w_0().child(control))
    }

    /// The five statuses as icon buttons, so every choice is in sight. The
    /// chosen one also carries its name; the rest say it on hover.
    fn status_buttons(&self, current: Option<ListStatus>, cx: &mut Context<Self>) -> ButtonGroup {
        ButtonGroup::new("status")
            .outline()
            .small()
            .children(ListStatus::SHOWN.iter().map(|&status| {
                let chosen = current == Some(status);
                Button::new(format::status_label(status))
                    .icon(Icon::new(icons::status_icon(status)))
                    .when(chosen, |b| b.label(format::status_label(status)))
                    .when(!chosen, |b| b.tooltip(format::status_label(status)))
                    .selected(chosen)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, window, cx| {
                if let Some(&status) = clicked.first().and_then(|&ix| ListStatus::SHOWN.get(ix)) {
                    this.set_status(status, window, cx);
                }
            }))
    }

    fn score_control(&self, score: u8, cx: &mut Context<Self>) -> AnyElement {
        let state = self.model.read(cx).state();
        let format = format::score_format(state);
        // MyAnimeList users know its scores by name, "(8) Very Good".
        let named = state.catalog_service() == ServiceId::MyAnimeList;
        let named_label = format::score_label(state, score);
        let Some(steps) = star_scores(format) else {
            return div()
                .w_32()
                .child(Select::new(&self.score).small())
                .into_any_element();
        };
        // A stored score between two steps lights the stars up to the
        // lower one, as AniList shows it.
        let lit = steps.iter().filter(|&&v| v <= score).count();
        let max = steps.len();
        // Empty stars already say "no score".
        let label = if score == 0 {
            String::new()
        } else if named {
            named_label
        } else {
            format!("{lit}/{max}")
        };
        h_flex()
            .gap_2()
            .child(
                Rating::new("score")
                    .xsmall()
                    .max(max)
                    .value(lit)
                    .on_click(cx.listener(move |this, &stars: &usize, window, cx| {
                        // Rating reports one less than the star clicked when
                        // that star was already lit; recover the star, then
                        // treat a click on the top lit star as clearing and
                        // any other as "this many".
                        let clicked = if stars < lit { stars + 1 } else { stars };
                        let stars = if clicked == lit { 0 } else { clicked };
                        let value = stars.checked_sub(1).map_or(0, |ix| steps[ix]);
                        this.save(window, cx, |e| e.score = value);
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .into_any_element()
    }

    fn render_list(&self, id: AnimeId, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.entry(cx) else {
            return v_flex()
                .gap_2()
                .p_4()
                .child(widgets::section_label("Add to your list", cx))
                .child(self.status_buttons(None, cx))
                .into_any_element();
        };
        let total = self
            .model
            .read(cx)
            .state()
            .anime
            .get(&id)
            .and_then(|a| a.episode_count);
        let catalog = self.model.read(cx).state().catalog_service();
        let muted = cx.theme().muted_foreground;
        let stepper =
            v_flex()
                .gap_1p5()
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("minus").small().icon(IconName::Minus).on_click(
                                cx.listener(|this, _, window, cx| this.step(-1, window, cx)),
                            ),
                        )
                        .child(div().w(px(56.)).child(Input::new(&self.watched).small()))
                        .child(div().text_sm().text_color(muted).child(format!(
                            "/ {}",
                            total.map_or("?".to_owned(), |n| n.to_string())
                        )))
                        .child(
                            Button::new("plus").small().icon(IconName::Plus).on_click(
                                cx.listener(|this, _, window, cx| this.step(1, window, cx)),
                            ),
                        ),
                )
                .children(total.filter(|&n| n > 0).map(|n| {
                    div().w(px(176.)).child(widgets::progress_bar(
                        entry.watched_episodes as f32 / n as f32,
                        cx,
                    ))
                }));
        let dates = h_flex()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.started).small()),
            )
            .child(Icon::new(AppIcon::ArrowRight).xsmall().text_color(muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.completed).small()),
            );
        let rewatch = h_flex()
            .gap_3()
            .child(
                Switch::new("rewatching")
                    .checked(entry.rewatching)
                    .label("Now")
                    .tooltip("Rewatching it now")
                    .on_click(cx.listener(|this, checked: &bool, window, cx| {
                        let checked = *checked;
                        this.save(window, cx, |e| e.rewatching = checked);
                    })),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(div().w(px(44.)).child(Input::new(&self.rewatched).small()))
                    .child(div().text_xs().text_color(muted).child("before")),
            );
        let next = entry.watched_episodes + 1;
        let play_label = match total {
            Some(n) if next > n => "Play".to_owned(),
            _ => format!("Play ep {next}"),
        };
        let saved = self.saved_shown.then(|| {
            h_flex()
                .gap_1()
                .text_xs()
                .text_color(cx.theme().success)
                .child(Icon::new(IconName::Check).xsmall())
                .child("Saved")
        });
        v_flex()
            .py_2()
            .child(
                div()
                    .px_4()
                    .py_1()
                    .child(self.status_buttons(Some(entry.status), cx)),
            )
            .child(Self::row("Episodes", stepper, cx))
            .child(Self::row("Score", self.score_control(entry.score, cx), cx))
            .child(Self::row("Dates", dates, cx))
            .child(Self::row("Rewatching", rewatch, cx))
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
                    .child({
                        let model = self.model.clone();
                        Button::new("play-next")
                            .small()
                            .primary()
                            .icon(IconName::Play)
                            .label(play_label)
                            .on_click(move |_, _, cx| {
                                model.update(cx, |model, cx| model.play(id, PlayChoice::Next, cx))
                            })
                    })
                    .child(
                        Button::new("service-page")
                            .small()
                            .ghost()
                            .icon(IconName::ExternalLink)
                            .label(catalog.name())
                            .tooltip(format!("Open on {}", catalog.name()))
                            .on_click(move |_, _, cx| cx.open_url(&anime_page_url(catalog, id))),
                    )
                    .child(div().flex_1())
                    .children(saved),
            )
            .into_any_element()
    }

    fn render_details(&self, id: AnimeId, cx: &mut Context<Self>) -> AnyElement {
        let state = self.model.read(cx).state();
        let Some(anime) = state.anime.get(&id) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let muted = theme.muted_foreground;
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

        // The numbers people compare between anime go in tiles, so they
        // are read at a glance instead of down a label column.
        let episodes = anime
            .episode_count
            .map_or("?".to_owned(), |n| n.to_string());
        let length = anime
            .episode_length
            .map_or("?".to_owned(), |m| format!("{m} min"));
        let average = if anime.score > 0.0 {
            h_flex()
                .gap_1()
                .child(
                    Icon::new(IconName::StarFill)
                        .small()
                        .text_color(theme.yellow),
                )
                .child(format!("{:.1}", anime.score))
                .into_any_element()
        } else {
            div().text_color(muted).child("–").into_any_element()
        };
        let stats = h_flex()
            .gap_2()
            .child(widgets::stat_tile(episodes, "episodes", cx))
            .child(widgets::stat_tile(length, "each", cx))
            .child(widgets::stat_tile(average, "average", cx));

        let today = format::today_in_japan();
        let airing = format::airing_label(anime, today);
        let dot = match shiori_core::anime::airing_status(anime, today) {
            shiori_core::AiringStatus::Airing => theme.success,
            shiori_core::AiringStatus::NotYetAired => style::accent(cx),
            _ => muted,
        };
        let status = (!airing.is_empty()).then(|| {
            h_flex()
                .gap_2()
                .text_sm()
                .child(div().size_2().rounded_full().bg(dot))
                .child(airing)
        });

        let tag_limit = if self.tags_open { usize::MAX } else { 6 };
        let hidden_tags = anime.tags.len().saturating_sub(tag_limit);
        let tags = (!anime.tags.is_empty()).then(|| {
            v_flex()
                .gap_1p5()
                .child(widgets::section_label("Tags", cx))
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_1p5()
                        .children(
                            anime
                                .tags
                                .iter()
                                .take(tag_limit)
                                .map(|t| widgets::chip(t.clone(), false, cx)),
                        )
                        .when(hidden_tags > 0, |row| {
                            row.child(
                                widgets::chip(format!("+{hidden_tags}"), false, cx)
                                    .id("more-tags")
                                    .cursor_pointer()
                                    .text_color(muted)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.tags_open = true;
                                        cx.notify();
                                    })),
                            )
                        }),
                )
        });
        let genres = (!anime.genres.is_empty()).then(|| {
            v_flex()
                .gap_1p5()
                .child(widgets::section_label("Genres", cx))
                .child(
                    h_flex().flex_wrap().gap_1p5().children(
                        anime
                            .genres
                            .iter()
                            .map(|g| widgets::chip(g.clone(), true, cx)),
                    ),
                )
        });

        let synopsis = format::plain_text(&anime.synopsis);
        // A few lines are enough to tell what it is about; the rest is
        // there for whoever wants it.
        let short = format::opening(&synopsis, SYNOPSIS_FOLDED);
        let long = short.len() < synopsis.trim().len();
        let synopsis = if long && !self.synopsis_open {
            short
        } else {
            synopsis
        };
        let synopsis = (!synopsis.trim().is_empty()).then(|| {
            v_flex()
                .gap_1()
                .child(widgets::section_label("Synopsis", cx))
                .child(div().text_sm().child(synopsis))
                .when(long, |section| {
                    section.child(
                        div()
                            .id("synopsis-more")
                            .text_xs()
                            .cursor_pointer()
                            .text_color(style::accent(cx))
                            .child(if self.synopsis_open {
                                "Show less"
                            } else {
                                "Show more"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.synopsis_open = !this.synopsis_open;
                                cx.notify();
                            })),
                    )
                })
        });

        let made_by = anime.studios.first().map(|studio| {
            let others: Vec<&String> = anime
                .studios
                .iter()
                .skip(1)
                .chain(
                    anime
                        .producers
                        .iter()
                        .filter(|p| !anime.studios.contains(p)),
                )
                .collect();
            let others = match others.len() {
                0 => String::new(),
                1 | 2 => format!(
                    "with {}",
                    others
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                n => format!("with {}, {} +{}", others[0], others[1], n - 2),
            };
            v_flex()
                .gap_1()
                .child(widgets::section_label("Made by", cx))
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .text_sm()
                        .child(div().font_semibold().child(studio.clone()))
                        .child(div().text_xs().text_color(muted).child(others)),
                )
        });
        let also_known = (!alternatives.is_empty()).then(|| {
            v_flex()
                .gap_1()
                .child(widgets::section_label("Also known as", cx))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(alternatives.join(" · ")),
                )
        });

        v_flex()
            .gap_4()
            .p_4()
            .child(v_flex().gap_2().child(stats).children(status))
            .children(genres)
            .children(tags)
            .children(synopsis)
            .children(made_by)
            .children(also_known)
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
            .map(|l| l.title.clone())
            .collect::<Vec<_>>()
            .join(", ");
        Some(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("Linked: {list}")),
        )
    }

    /// Whether the anime is in a remembered episode numbering, and the way
    /// to edit or make one.
    fn numbering_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let id = self.id?;
        let state = self.model.read(cx).state();
        let text = state
            .numbering
            .of(id)
            .map(|s| format!("Numbered with {} anime", s.entries.len()));
        let model = self.model.clone();
        Some(
            v_flex()
                .gap_1()
                .pt_2()
                .children(text.map(|text| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text)
                }))
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
                    .child("Alternative titles, one per line"),
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
    // "New" and "airing" badges share the calm accent tint: small text on
    // the bright gradient was hard to read.
    let text = if new > 0 {
        format!("{new} new")
    } else {
        let time = anime.next_episode_time?;
        let when = airing_label(time, Timestamp::now(), &jiff::tz::TimeZone::system());
        format!("Ep {} {when}", anime.last_aired_episode + 1)
    };
    Some(
        div()
            .flex_none()
            .px_2()
            .rounded_full()
            .text_xs()
            .font_semibold()
            .bg(style::accent_soft(cx))
            .text_color(style::accent(cx))
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
