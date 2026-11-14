//! The episode numbering editor: which anime share one release numbering
//! and which numbers each one covers (ui.md, "Episode numbering").

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex,
};
use shiori_core::AnimeId;
use shiori_core::recognition::{NumberingSequence, SequenceEntry, continuous};

use super::model::Model;
use super::numbering::is_series;
use super::{actions, format};

/// Opens the editor with a draft: a suggestion, a saved numbering, or a new
/// one. `preview` is the anime and number to show the result for; with
/// `for_playing`, "Use once" applies the draft to the playing media.
pub fn open(
    model: &Entity<Model>,
    draft: NumberingSequence,
    preview: Option<(AnimeId, u32)>,
    for_playing: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let title = model.read(cx).state().title(draft.root);
    let editor =
        cx.new(|cx| NumberingEditor::new(model.clone(), draft, preview, for_playing, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(format!("{title} · Episode numbering"))
            .w(px(720.))
            .child(editor.clone())
    });
}

/// Opens the editor for an anime: its saved numbering, or a new one that
/// starts with the anime alone.
pub fn open_for_anime(
    model: &Entity<Model>,
    id: AnimeId,
    preview: Option<(AnimeId, u32)>,
    window: &mut Window,
    cx: &mut App,
) {
    let draft = {
        let state = model.read(cx).state();
        state.numbering.of(id).cloned().unwrap_or_else(|| {
            let count = state
                .anime
                .get(&id)
                .and_then(|a| a.episode_count)
                .unwrap_or(1);
            NumberingSequence {
                id: 0,
                root: id,
                entries: vec![SequenceEntry {
                    anime: id,
                    first: 1,
                    last: count.max(1),
                }],
                saved: jiff::Timestamp::UNIX_EPOCH,
            }
        })
    };
    open(model, draft, preview, false, window, cx);
}

struct Row {
    anime: AnimeId,
    included: bool,
    first: Entity<InputState>,
    last: Entity<InputState>,
}

pub struct NumberingEditor {
    model: Entity<Model>,
    /// The saved numbering's id, 0 for a new one.
    id: i64,
    root: AnimeId,
    rows: Vec<Row>,
    preview_anime: AnimeId,
    preview_number: Entity<InputState>,
    for_playing: bool,
    loading: bool,
    /// From loading relations or saving.
    problem: Option<SharedString>,
    /// Seasons found on AniList, to add when the editor next renders: new
    /// rows need the window for their inputs, which the answer has not.
    pending_chain: Option<Vec<AnimeId>>,
    /// The numbering was forgotten; the editor closes.
    forgotten: bool,
    _subscriptions: Vec<Subscription>,
}

impl NumberingEditor {
    fn new(
        model: Entity<Model>,
        draft: NumberingSequence,
        preview: Option<(AnimeId, u32)>,
        for_playing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (preview_anime, number) = preview.unwrap_or((draft.root, 1));
        let preview_number =
            cx.new(|cx| InputState::new(window, cx).default_value(number.to_string()));
        let subscription = cx.subscribe_in(&preview_number, window, |_, _, event, _, cx| {
            if let InputEvent::Change = event {
                cx.notify();
            }
        });
        let mut editor = Self {
            model,
            id: draft.id,
            root: draft.root,
            rows: Vec::new(),
            preview_anime,
            preview_number,
            for_playing,
            loading: false,
            problem: None,
            pending_chain: None,
            forgotten: false,
            _subscriptions: vec![subscription],
        };
        let mut entries = draft.entries;
        entries.sort_by_key(|e| e.first);
        for entry in entries {
            editor.add_row(
                entry.anime,
                true,
                Some((entry.first, entry.last)),
                window,
                cx,
            );
        }
        editor.add_related(window, cx);
        editor
    }

    fn add_row(
        &mut self,
        anime: AnimeId,
        included: bool,
        range: Option<(u32, u32)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.rows.iter().any(|r| r.anime == anime) {
            return;
        }
        let (first, last) = range.map_or((String::new(), String::new()), |(f, l)| {
            (f.to_string(), l.to_string())
        });
        let first = cx.new(|cx| InputState::new(window, cx).default_value(first));
        let last = cx.new(|cx| InputState::new(window, cx).default_value(last));
        for input in [&first, &last] {
            self._subscriptions
                .push(cx.subscribe_in(input, window, |_, _, event, _, cx| {
                    if let InputEvent::Change = event {
                        cx.notify();
                    }
                }));
        }
        self.rows.push(Row {
            anime,
            included,
            first,
            last,
        });
    }

    /// Adds the anime the relations fetched so far link to, not included:
    /// what to include is the user's call.
    fn add_related(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<AnimeId> = self.rows.iter().map(|r| r.anime).collect();
        let related = self.model.read(cx).related_to(&ids);
        let state = self.model.read(cx).state();
        // Seasons first, then the rest, each in airing order.
        let mut found: Vec<(bool, (u16, u8, u8), AnimeId)> = related
            .keys()
            .filter_map(|&id| {
                let anime = state.anime.get(&id)?;
                let d = anime.date_started;
                Some((
                    !is_series(self.model.read(cx), id),
                    (d.year, d.month, d.day),
                    id,
                ))
            })
            .collect();
        found.sort();
        for (_, _, id) in found {
            self.add_row(id, false, None, window, cx);
        }
    }

    /// Fetches the series from AniList and adds what it finds. A new
    /// numbering with only its first anime gets the seasons included and
    /// numbered continuously, as a starting point.
    fn load_related(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.problem = None;
        cx.notify();
        let editor = cx.entity().downgrade();
        let root = self.root;
        self.model.update(cx, |model, cx| {
            model.follow_series(root, cx, move |_, chain, cx| {
                let order = chain.as_ref().map(|c| c.order.clone()).unwrap_or_default();
                let error = chain.err();
                // With every relation already fetched this runs at once,
                // inside the click that is still updating the editor.
                cx.defer(move |cx| {
                    let _ = editor.update(cx, |editor, cx| {
                        editor.loading = false;
                        editor.problem = error.map(|e| {
                            tracing::warn!("Could not load related anime: {e}");
                            "Couldn't load seasons".into()
                        });
                        editor.pending_chain = Some(order);
                        cx.notify();
                    });
                });
            });
        });
    }

    /// Adds what was loaded. A new numbering that holds only its first
    /// anime gets the seasons included, in order and numbered
    /// continuously, as a starting point to check.
    fn apply_chain(&mut self, order: Vec<AnimeId>, window: &mut Window, cx: &mut Context<Self>) {
        let fresh = self.id == 0 && self.rows.iter().filter(|r| r.included).count() == 1;
        for &id in &order {
            self.add_row(id, false, None, window, cx);
        }
        self.add_related(window, cx);
        if fresh && order.len() > 1 {
            for row in &mut self.rows {
                row.included = order.contains(&row.anime);
            }
            self.rows.sort_by_key(|r| {
                order
                    .iter()
                    .position(|&id| id == r.anime)
                    .unwrap_or(usize::MAX)
            });
            if let Some(&first) = order.first() {
                // The numbering starts at the first season, wherever the
                // editor was opened from.
                self.root = first;
            }
            self.suggest_continuous(window, cx);
        }
        cx.notify();
    }

    /// The sequence the rows describe, or why they do not describe one.
    fn sequence(&self, cx: &App) -> Result<NumberingSequence, String> {
        let mut entries = Vec::new();
        for row in self.rows.iter().filter(|r| r.included) {
            let number =
                |input: &Entity<InputState>| input.read(cx).value().trim().parse::<u32>().ok();
            let (Some(first), Some(last)) = (number(&row.first), number(&row.last)) else {
                return Err(format!(
                    "{}: enter the first and last release numbers as whole numbers.",
                    self.model.read(cx).state().title(row.anime)
                ));
            };
            entries.push(SequenceEntry {
                anime: row.anime,
                first,
                last,
            });
        }
        let sequence = NumberingSequence {
            id: self.id,
            root: self.root,
            entries,
            saved: jiff::Timestamp::UNIX_EPOCH,
        };
        let state = self.model.read(cx).state();
        sequence
            .validate(
                |id| state.anime.get(&id).and_then(|a| a.episode_count),
                &state.numbering,
            )
            .map_err(|e| e.message(|id| state.title(id)))?;
        Ok(sequence)
    }

    /// Fills the included anime with continuous numbers in their order.
    fn suggest_continuous(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.model.read(cx).state();
        let order: Vec<(AnimeId, Option<u32>)> = self
            .rows
            .iter()
            .filter(|r| r.included)
            .map(|r| {
                (
                    r.anime,
                    state.anime.get(&r.anime).and_then(|a| a.episode_count),
                )
            })
            .collect();
        let (entries, unknown) = match continuous(&order) {
            Ok(entries) => (entries, None),
            Err((entries, at)) => (entries, Some(at)),
        };
        for entry in entries {
            if let Some(row) = self.rows.iter().find(|r| r.anime == entry.anime) {
                row.first
                    .update(cx, |i, cx| i.set_value(entry.first.to_string(), window, cx));
                row.last
                    .update(cx, |i, cx| i.set_value(entry.last.to_string(), window, cx));
            }
        }
        self.problem = unknown.map(|id| {
            format!(
                "{}'s episode count is unknown. Enter its last release number by hand.",
                self.model.read(cx).state().title(id)
            )
            .into()
        });
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(sequence) = self.sequence(cx) else {
            return;
        };
        let result = self
            .model
            .update(cx, |model, cx| model.save_numbering(sequence, cx));
        match result {
            Ok(_) => window.close_dialog(cx),
            Err(error) => {
                // The previous numbering is untouched; keep the draft open so
                // nothing typed is lost.
                self.problem = Some(format!("Could not save: {error}").into());
                cx.notify();
            }
        }
    }

    fn use_once(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(sequence) = self.sequence(cx) else {
            return;
        };
        self.model
            .update(cx, |model, cx| model.use_numbering_once(&sequence, cx));
        window.close_dialog(cx);
    }

    fn forget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        let model = self.model.clone();
        let editor = cx.entity().downgrade();
        let title = model.read(cx).state().title(self.root);
        actions::confirm(
            "Forget this numbering?",
            format!("{title}. Your list and history are not changed."),
            window,
            cx,
            move |cx| {
                model.update(cx, |model, cx| model.forget_numbering(id, cx));
                let _ = editor.update(cx, |editor, cx| {
                    editor.forgotten = true;
                    cx.notify();
                });
            },
        );
    }

    /// Where a file matched to the preview anime with the preview number
    /// lands.
    fn preview(&self, cx: &App) -> String {
        let state = self.model.read(cx).state();
        let Some(number) = self
            .preview_number
            .read(cx)
            .value()
            .trim()
            .parse::<u32>()
            .ok()
        else {
            return "–".into();
        };
        let from = self.preview_anime;
        let from_name = state.title(from);
        let count = state.anime.get(&from).and_then(|a| a.episode_count);
        if number >= 1 && count.is_none_or(|c| number <= c) {
            return format!("{from_name}, episode {number}");
        }
        let Ok(sequence) = self.sequence(cx) else {
            return "–".into();
        };
        match sequence.resolve((number, number)) {
            Some(r) => format!("{}, episode {}", state.title(r.anime), r.range.0),
            None => "No match".into(),
        }
    }

    fn render_row(&self, ix: usize, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let state = model.state();
        let anime = state.anime.get(&row.anime);
        let relation = if row.anime == self.root {
            "First".to_owned()
        } else {
            model
                .related_to(&self.rows.iter().map(|r| r.anime).collect::<Vec<_>>())
                .get(&row.anime)
                .map(|r| r.kind.label().to_owned())
                .unwrap_or_else(|| "Added".to_owned())
        };
        let mut meta = vec![relation];
        if let Some(anime) = anime {
            meta.push(format::type_label(anime.kind).to_owned());
            meta.push(match anime.episode_count {
                Some(n) => format!("{n} eps"),
                None => "? eps".into(),
            });
        }
        let anime_id = row.anime;
        h_flex()
            .gap_2()
            .py_1()
            .child(
                Checkbox::new(("include", ix))
                    .checked(row.included)
                    .disabled(row.anime == self.root)
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        if let Some(row) = this.rows.iter_mut().find(|r| r.anime == anime_id) {
                            row.included = *checked;
                        }
                        cx.notify();
                    })),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().text_sm().child(state.title(row.anime)))
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(meta.join(" · ")),
                    ),
            )
            .when(row.included, |el| {
                el.child(div().w(px(64.)).child(Input::new(&row.first).small()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("to"),
                    )
                    .child(div().w(px(64.)).child(Input::new(&row.last).small()))
            })
            .into_any_element()
    }
}

impl Render for NumberingEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.forgotten {
            window.defer(cx, |window, cx| window.close_dialog(cx));
        }
        if let Some(order) = self.pending_chain.take() {
            self.apply_chain(order, window, cx);
        }
        let muted = cx.theme().muted_foreground;
        let valid = self.sequence(cx);
        let rows: Vec<AnyElement> = (0..self.rows.len())
            .map(|ix| {
                let row = &self.rows[ix];
                let row = Row {
                    anime: row.anime,
                    included: row.included,
                    first: row.first.clone(),
                    last: row.last.clone(),
                };
                self.render_row(ix, &row, cx)
            })
            .collect();
        let (included, others): (Vec<_>, Vec<_>) = rows
            .into_iter()
            .zip(self.rows.iter().map(|r| r.included))
            .partition(|(_, included)| *included);
        let fetched = self.model.read(cx).relations_of(self.root).is_some();
        let result = self.preview(cx);
        let preview_choices: Vec<(AnimeId, String)> = self
            .rows
            .iter()
            .filter(|r| r.included)
            .map(|r| (r.anime, self.model.read(cx).state().title(r.anime)))
            .collect();

        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("suggest")
                            .small()
                            .label("Number in order")
                            .tooltip("1–12, 13–24, … from the episode counts")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.suggest_continuous(window, cx)
                            })),
                    )
                    .child(
                        Button::new("load")
                            .small()
                            .ghost()
                            .loading(self.loading)
                            .label(if fetched {
                                "Reload seasons"
                            } else {
                                "Load seasons from AniList"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.load_related(cx))),
                    ),
            )
            .child(
                v_flex()
                    .id("numbering-rows")
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(muted)
                            .child("INCLUDED"),
                    )
                    .children(included.into_iter().map(|(row, _)| row))
                    .when(!others.is_empty(), |el| {
                        el.child(
                            div()
                                .pt_2()
                                .text_xs()
                                .font_semibold()
                                .text_color(muted)
                                .child("OTHER RELATED"),
                        )
                        .children(others.into_iter().map(|(row, _)| row))
                    }),
            )
            // Only what is wrong; valid numbers need no comment.
            .when_some(valid.as_ref().err(), |el, error| {
                el.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(error.clone()),
                )
            })
            .child(div().h(px(1.)).bg(cx.theme().border))
            .child(
                v_flex().gap_2().child(
                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .child(div().text_xs().text_color(muted).child("Test"))
                        .children(preview_choices.into_iter().enumerate().map(
                            |(ix, (id, title))| {
                                let selected = id == self.preview_anime;
                                Button::new(("preview", ix))
                                    .xsmall()
                                    .label(title)
                                    .when(selected, |b| b.primary())
                                    .when(!selected, |b| b.outline())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.preview_anime = id;
                                        cx.notify();
                                    }))
                            },
                        ))
                        .child(
                            div()
                                .w(px(64.))
                                .child(Input::new(&self.preview_number).small()),
                        )
                        .child(div().text_color(muted).child("→"))
                        .child(div().text_sm().font_semibold().child(result)),
                ),
            )
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
                    .child(div().flex_1().when(self.id != 0, |el| {
                        el.child(
                            Button::new("forget").ghost().label("Forget…").on_click(
                                cx.listener(|this, _, window, cx| this.forget(window, cx)),
                            ),
                        )
                    }))
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .when(self.for_playing, |el| {
                        el.child(
                            Button::new("once")
                                .label("Use once")
                                .disabled(valid.is_err())
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.use_once(window, cx)),
                                ),
                        )
                    })
                    .child(
                        Button::new("save")
                            .primary()
                            .label("Save")
                            .disabled(valid.is_err())
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}
