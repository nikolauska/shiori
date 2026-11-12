//! The anime details window: the poster on the left, and tabs with the
//! anime's details, the user's list entry and the per-anime settings.
//!
//! There is one such window. Opening details for another anime reuses it,
//! as Taiga does, so the user never has to close a pile of them.

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, Global, IntoElement, ParentElement,
    PathPromptOptions, Render, SharedString, Styled, Subscription, Window, WindowBounds,
    WindowHandle, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::resizable::{ResizableState, h_resizable, resizable_panel};
use gpui_component::select::{Select, SelectState};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme as _, Root, Theme, WindowExt as _, h_flex, v_flex};
use jiff::Timestamp;
use shiori_core::anime::{MAX_EPISODE_COUNT, is_stale};
use shiori_core::entry::is_in_list;
use shiori_core::{AnimeId, AnimeSettings, FuzzyDate, ListEntry, ListStatus};
use shiori_sync::anilist::score::rating_list;

use shiori_app::state::AppEvent;

use super::format;
use super::model::Model;
use super::poster;
use super::widgets::Choice;

/// The open details window, if any.
#[derive(Default)]
struct DetailsWindow(Option<(WindowHandle<Root>, Entity<DetailsView>)>);

impl Global for DetailsWindow {}

/// Shows the details of an anime, in the open details window if there is one.
pub fn open(model: &Entity<Model>, id: AnimeId, _: &mut Window, cx: &mut App) {
    if !model.read(cx).state().anime.contains_key(&id) {
        return;
    }
    let open = cx.try_global::<DetailsWindow>().and_then(|w| w.0.clone());
    if let Some((handle, view)) = open {
        let shown = handle.update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.show(id, window, cx));
            window.activate_window();
        });
        if shown.is_ok() {
            fetch_if_stale(model, id, cx);
            return;
        }
    }

    let session = model.read(cx).session.details_window;
    let window_size = size(
        px(session.width.unwrap_or(900) as f32),
        px(session.height.unwrap_or(640) as f32),
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            window_size,
            cx,
        ))),
        ..WindowOptions::default()
    };
    let model_for_window = model.clone();
    let mut view = None;
    let opened = cx.open_window(options, |window, cx| {
        Theme::sync_system_appearance(Some(window), cx);
        let details = cx.new(|cx| DetailsView::new(model_for_window, id, window, cx));
        view = Some(details.clone());
        cx.new(|cx| Root::new(details, window, cx))
    });
    match (opened, view) {
        (Ok(handle), Some(view)) => {
            cx.set_global(DetailsWindow(Some((handle, view))));
            fetch_if_stale(model, id, cx);
        }
        (Err(error), _) => tracing::error!("Could not open the details window: {error:#}"),
        _ => {}
    }
}

/// Opening the details of an anime whose data is old fetches it again.
fn fetch_if_stale(model: &Entity<Model>, id: AnimeId, cx: &mut App) {
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

pub struct DetailsView {
    model: Entity<Model>,
    id: AnimeId,
    tab: usize,
    splitter: Entity<ResizableState>,
    // List tab
    watched: Entity<InputState>,
    rewatching: bool,
    rewatched: Entity<InputState>,
    status: Entity<StatusSelect>,
    score: Entity<ScoreSelect>,
    started: Entity<InputState>,
    completed: Entity<InputState>,
    notes: Entity<TextareaState>,
    // Settings tab
    synonyms: Entity<TextareaState>,
    folder: Entity<InputState>,
    display_title: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

fn status_choices() -> Vec<Choice<ListStatus>> {
    ListStatus::SHOWN
        .iter()
        .map(|&status| Choice::new(format::status_label(status), status))
        .collect()
}

impl DetailsView {
    fn new(model: Entity<Model>, id: AnimeId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let ratings: Vec<Choice<u8>> = rating_list(format::score_format(model.read(cx).state()))
            .into_iter()
            .map(|r| {
                Choice::new(
                    if r.value == 0 {
                        "-".to_owned()
                    } else {
                        r.label
                    },
                    r.value,
                )
            })
            .collect();
        let status = cx.new(|cx| SelectState::new(status_choices(), None, window, cx));
        let score = cx.new(|cx| SelectState::new(ratings, None, window, cx));
        let subscriptions = vec![cx.subscribe_in(&model, window, Self::on_event)];
        let mut view = Self {
            id,
            tab: 0,
            splitter: cx.new(|_| ResizableState::default()),
            watched: text("0", window, cx),
            rewatching: false,
            rewatched: text("0", window, cx),
            status,
            score,
            started: text("YYYY-MM-DD", window, cx),
            completed: text("YYYY-MM-DD", window, cx),
            notes: cx.new(|cx| TextareaState::new(window, cx)),
            synonyms: cx.new(|cx| TextareaState::new(window, cx)),
            folder: text("Folder of this anime's files", window, cx),
            display_title: text("Title to show instead of the service's", window, cx),
            model,
            _subscriptions: subscriptions,
        };
        view.load_entry(window, cx);
        view.load_settings(window, cx);
        view
    }

    fn show(&mut self, id: AnimeId, window: &mut Window, cx: &mut Context<Self>) {
        if self.id != id {
            self.id = id;
            self.load_entry(window, cx);
            self.load_settings(window, cx);
        }
        cx.notify();
    }

    fn on_event(
        &mut self,
        _: &Entity<Model>,
        event: &AppEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            // A sync or an edit made elsewhere changed what this tab shows.
            AppEvent::EntryUpdated(id) | AppEvent::EntryDeleted(id) if *id == self.id => {
                self.load_entry(window, cx)
            }
            AppEvent::AnimeSettingsUpdated(id) if *id == self.id => self.load_settings(window, cx),
            AppEvent::AnimeDeleted { id, .. } if *id == self.id => window.remove_window(),
            _ => {}
        }
        cx.notify();
    }

    fn entry(&self, cx: &App) -> Option<ListEntry> {
        let state = self.model.read(cx).state();
        state
            .entries
            .get(&self.id)
            .filter(|e| is_in_list(Some(e)))
            .cloned()
    }

    fn load_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entry = self.entry(cx);
        let in_list = entry.is_some();
        let entry = entry.unwrap_or_else(|| ListEntry::new(self.id));
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
        self.rewatching = entry.rewatching;
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
    }

    fn load_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self
            .model
            .read(cx)
            .state()
            .anime_settings
            .get(&self.id)
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
    }

    fn save_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let episode_count = self
            .model
            .read(cx)
            .state()
            .anime
            .get(&self.id)
            .and_then(|a| a.episode_count);
        let max = episode_count.unwrap_or(MAX_EPISODE_COUNT);
        let value = |input: &Entity<InputState>, cx: &App| input.read(cx).value().trim().to_owned();

        let Some(watched) = value(&self.watched, cx)
            .parse::<u32>()
            .ok()
            .filter(|n| *n <= max)
        else {
            window.push_notification(format!("Enter a number of episodes from 0 to {max}."), cx);
            return;
        };
        let Ok(rewatched) = value(&self.rewatched, cx).parse::<u32>() else {
            window.push_notification("Enter the number of times rewatched.", cx);
            return;
        };
        let mut dates = [FuzzyDate::default(); 2];
        for (date, input) in dates.iter_mut().zip([&self.started, &self.completed]) {
            let text = value(input, cx);
            *date = FuzzyDate::parse(&text);
            // An empty box is "no date"; anything else has to be readable.
            if !text.is_empty() && date.is_empty() {
                window.push_notification(format!("\"{text}\" is not a date (YYYY-MM-DD)."), cx);
                return;
            }
        }

        let mut entry = self.entry(cx).unwrap_or_else(|| ListEntry::new(self.id));
        entry.watched_episodes = watched;
        entry.rewatching = self.rewatching;
        entry.rewatched_times = rewatched;
        if let Some(&status) = self.status.read(cx).selected_value() {
            entry.status = status;
        }
        if let Some(&score) = self.score.read(cx).selected_value() {
            entry.score = score;
        }
        [entry.date_started, entry.date_completed] = dates;
        entry.notes = self.notes.read(cx).value().trim().to_owned();
        self.model
            .update(cx, |model, cx| model.save_entry(entry, cx));
    }

    fn save_settings(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let synonyms = self
            .synonyms
            .read(cx)
            .value()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        let settings = AnimeSettings {
            id: self.id,
            display_title: self.display_title.read(cx).value().trim().to_owned(),
            folder: self.folder.read(cx).value().trim().to_owned(),
            synonyms,
        };
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
        let folder = self.folder.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.first()
            {
                let path = path.display().to_string();
                let _ = folder.update_in(cx, |input, window, cx| input.set_value(path, window, cx));
            }
        })
        .detach();
    }

    fn render_details(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.model.read(cx).state();
        let Some(anime) = state.anime.get(&self.id) else {
            return div().into_any_element();
        };
        let titles = &anime.titles;
        let alternatives: Vec<&str> = [titles.english.as_str(), titles.japanese.as_str()]
            .into_iter()
            .chain(titles.synonyms.iter().map(String::as_str))
            .filter(|t| !t.is_empty() && *t != state.title(self.id))
            .collect();
        let episodes = anime
            .episode_count
            .map_or("Unknown".to_owned(), |n| n.to_string());
        let duration = anime
            .episode_length
            .map_or(String::new(), |m| format!("{m} min"));
        let average = if anime.score > 0.0 {
            format!("{:.2}", anime.score)
        } else {
            String::new()
        };
        let rows = [
            ("Alternative titles", alternatives.join(", ")),
            ("Type", format::type_label(anime.kind).to_owned()),
            ("Episodes", episodes),
            ("Duration", duration),
            (
                "Status",
                format::airing_label(anime, format::today_in_japan()).to_owned(),
            ),
            ("Season", format::season_label(anime.date_started)),
            ("Score", average),
            ("Genres", anime.genres.join(", ")),
            ("Tags", anime.tags.join(", ")),
            ("Studios", anime.studios.join(", ")),
            ("Producers", anime.producers.join(", ")),
        ];
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_1()
            .children(
                rows.into_iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(label, value)| {
                        h_flex()
                            .gap_3()
                            .items_start()
                            .child(div().w_32().flex_none().text_color(muted).child(label))
                            .child(div().flex_1().child(value))
                    }),
            )
            .child(div().pt_3().text_color(muted).child("Synopsis"))
            .child(div().child(format::plain_text(&anime.synopsis)))
            .into_any_element()
    }

    fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let in_list = self.entry(cx).is_some();
        let max = self
            .model
            .read(cx)
            .state()
            .anime
            .get(&self.id)
            .and_then(|a| a.episode_count)
            .map_or(String::new(), |n| format!("of {n}"));
        let muted = cx.theme().muted_foreground;
        let row = |label: &'static str, control: AnyElement| {
            h_flex()
                .gap_3()
                .child(div().w_32().flex_none().text_color(muted).child(label))
                .child(div().flex_1().child(control))
        };
        let save_label = if in_list { "Save" } else { "Add to list" };
        let rewatching = self.rewatching;
        v_flex()
            .gap_2()
            .child(row(
                "Episodes watched",
                h_flex()
                    .gap_2()
                    .child(div().w_24().child(Input::new(&self.watched)))
                    .child(max)
                    .into_any_element(),
            ))
            .child(row(
                "Rewatching",
                Checkbox::new("rewatching")
                    .checked(rewatching)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.rewatching = *checked;
                        cx.notify();
                    }))
                    .into_any_element(),
            ))
            .child(row(
                "Times rewatched",
                div()
                    .w_24()
                    .child(Input::new(&self.rewatched))
                    .into_any_element(),
            ))
            .child(row(
                "Status",
                div()
                    .w_48()
                    .child(Select::new(&self.status))
                    .into_any_element(),
            ))
            .child(row(
                "Score",
                div()
                    .w_48()
                    .child(Select::new(&self.score))
                    .into_any_element(),
            ))
            .child(row(
                "Date started",
                div()
                    .w_48()
                    .child(Input::new(&self.started))
                    .into_any_element(),
            ))
            .child(row(
                "Date completed",
                div()
                    .w_48()
                    .child(Input::new(&self.completed))
                    .into_any_element(),
            ))
            .child(row(
                "Notes",
                div()
                    .h_24()
                    .child(Textarea::new(&self.notes))
                    .into_any_element(),
            ))
            .child(
                h_flex().justify_end().child(
                    Button::new("save-entry")
                        .primary()
                        .label(save_label)
                        .on_click(cx.listener(|this, _, window, cx| this.save_entry(window, cx))),
                ),
            )
            .into_any_element()
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_color(muted)
                    .child("Alternative titles, one per line. Used to recognize file names."),
            )
            .child(div().h_24().child(Textarea::new(&self.synonyms)))
            .child(div().text_color(muted).child("Folder"))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.folder)))
                    .child(Button::new("browse").label("Browse...").on_click(
                        cx.listener(|this, _, window, cx| this.browse_folder(window, cx)),
                    )),
            )
            .child(div().text_color(muted).child("Display title"))
            .child(Input::new(&self.display_title))
            .child(
                h_flex().justify_end().child(
                    Button::new("save-settings")
                        .primary()
                        .label("Save")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.save_settings(window, cx)),
                        ),
                ),
            )
            .into_any_element()
    }
}

impl Render for DetailsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title: SharedString = self.model.read(cx).state().title(self.id).into();
        window.set_window_title(&title);
        let splitter = self.model.read(cx).session.details_splitter;
        let left = px(splitter.map_or(240., |w| w.clamp(120., 600.)));
        let tab = match self.tab {
            0 => self.render_details(cx),
            1 => self.render_list(cx),
            _ => self.render_settings(cx),
        };
        let model = self.model.clone();
        let content = h_resizable("details-split")
            .with_state(&self.splitter)
            .on_resize(move |state, _, cx| {
                let width = state.read(cx).sizes().first().map(|w| f32::from(*w));
                model.update(cx, |model, cx| model.set_details_splitter(width, cx));
            })
            .child(
                resizable_panel()
                    .size(left)
                    .size_range(px(120.)..px(600.))
                    .child(
                        div()
                            .p_3()
                            .size_full()
                            .child(poster::view(&self.model, self.id, cx)),
                    ),
            )
            .child(
                resizable_panel().child(
                    v_flex()
                        .id("details-tabs")
                        .size_full()
                        .p_3()
                        .gap_3()
                        .overflow_y_scroll()
                        .child(div().text_xl().child(title))
                        .child(
                            TabBar::new("tabs")
                                .underline()
                                .selected_index(self.tab)
                                .child(Tab::new().label("Details"))
                                .child(Tab::new().label("List"))
                                .child(Tab::new().label("Settings"))
                                .on_click(cx.listener(|this, ix: &usize, _, cx| {
                                    this.tab = *ix;
                                    cx.notify();
                                })),
                        )
                        .child(tab),
                ),
            );
        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(content)
    }
}
