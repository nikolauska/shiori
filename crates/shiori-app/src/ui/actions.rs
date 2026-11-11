//! The edit actions offered on an anime: from the context menu now, from
//! the details window later. Each builds a changed entry and saves it
//! through the model.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, ClipboardItem, Entity, Focusable as _, ParentElement as _, Styled as _,
    Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::{WindowExt as _, h_flex};
use shiori_core::anime::MAX_EPISODE_COUNT;
use shiori_core::entry::is_in_list;
use shiori_core::{AnimeId, FuzzyDate, ListEntry, ListStatus};
use shiori_sync::anilist::score::rating_list;

use super::format;
use super::model::Model;

/// A menu item's action.
type Handler = dyn Fn(&mut Window, &mut App);

/// Changes one entry. A missing entry starts as a new one.
fn edit(model: &Entity<Model>, id: AnimeId, cx: &mut App, change: impl FnOnce(&mut ListEntry)) {
    model.update(cx, |model, cx| {
        let mut entry = model
            .state()
            .entries
            .get(&id)
            .filter(|e| !e.pending_delete)
            .cloned()
            .unwrap_or_else(|| ListEntry::new(id));
        change(&mut entry);
        model.save_entry(entry, cx);
    });
}

/// Builds the context menu for an anime in a list.
pub fn context_menu(
    model: &Entity<Model>,
    id: AnimeId,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    let state = model.read(cx).state();
    let entry = state.entries.get(&id).cloned();
    let in_list = is_in_list(entry.as_ref());
    let anime = state.anime.get(&id).cloned();
    let title = state.title(id);
    let ratings = rating_list(format::score_format(state));

    let item = |label: &str, f: Rc<Handler>| {
        PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| f(window, cx))
    };
    let m = model.clone();

    let mut menu = menu.item(item(
        "Links",
        Rc::new(move |_, cx| cx.open_url(&format!("https://anilist.co/anime/{id}"))),
    ));

    if !in_list {
        let m = m.clone();
        let add = PopupMenu::build(window, cx, move |mut sub, _, _| {
            for status in ListStatus::SHOWN {
                let m = m.clone();
                sub = sub.item(
                    PopupMenuItem::new(format::status_label(status))
                        .on_click(move |_, _, cx| edit(&m, id, cx, |e| e.status = status)),
                );
            }
            sub
        });
        menu = menu
            .separator()
            .item(PopupMenuItem::submenu("Add to list", add));
    } else {
        let entry = entry.clone().unwrap_or_else(|| ListEntry::new(id));
        let anime_for_dates = anime.clone();
        let m2 = m.clone();
        let edit_menu = PopupMenu::build(window, cx, move |mut sub, window, cx| {
            let m = m2.clone();
            let episodes = anime_for_dates.as_ref().and_then(|a| a.episode_count);
            sub = sub.item(PopupMenuItem::new("Episode...").on_click({
                let m = m.clone();
                let watched = entry.watched_episodes;
                move |_, window, cx| ask_episode(&m, id, watched, episodes, window, cx)
            }));

            let score = PopupMenu::build(window, cx, {
                let m = m.clone();
                let ratings = ratings.clone();
                let current = entry.score;
                move |mut sub, _, _| {
                    for rating in &ratings {
                        let m = m.clone();
                        let value = rating.value;
                        sub = sub.item(
                            PopupMenuItem::new(rating.label.clone())
                                .checked(value == current)
                                .on_click(move |_, _, cx| edit(&m, id, cx, |e| e.score = value)),
                        );
                    }
                    sub
                }
            });
            sub = sub.item(PopupMenuItem::submenu("Score", score));

            let status = PopupMenu::build(window, cx, {
                let m = m.clone();
                let current = entry.status;
                move |mut sub, _, _| {
                    for status in ListStatus::SHOWN {
                        let m = m.clone();
                        sub = sub.item(
                            PopupMenuItem::new(format::status_label(status))
                                .checked(status == current)
                                .on_click(move |_, _, cx| edit(&m, id, cx, |e| e.status = status)),
                        );
                    }
                    sub
                }
            });
            sub = sub.item(PopupMenuItem::submenu("Status", status));

            let started = anime_for_dates
                .as_ref()
                .map(|a| a.date_started)
                .unwrap_or_default();
            let finished = anime_for_dates
                .as_ref()
                .map(|a| a.date_finished)
                .unwrap_or_default();
            let updated = FuzzyDate::from(
                entry
                    .last_updated
                    .to_zoned(jiff::tz::TimeZone::system())
                    .date(),
            );
            sub = sub.item(PopupMenuItem::submenu(
                "Date started",
                date_menu(
                    &m,
                    id,
                    "Set to date started airing",
                    started,
                    updated,
                    false,
                    window,
                    cx,
                ),
            ));
            sub = sub.item(PopupMenuItem::submenu(
                "Date completed",
                date_menu(
                    &m,
                    id,
                    "Set to date finished airing",
                    finished,
                    updated,
                    true,
                    window,
                    cx,
                ),
            ));
            sub.item(PopupMenuItem::new("Notes...").on_click({
                let notes = entry.notes.clone();
                move |_, window, cx| ask_notes(&m, id, notes.clone(), window, cx)
            }))
        });
        let m = m.clone();
        let remove_title = title.clone();
        menu = menu
            .separator()
            .item(PopupMenuItem::submenu("Edit", edit_menu))
            .item(item(
                "Remove...",
                Rc::new(move |window, cx| confirm_remove(&m, id, &remove_title, window, cx)),
            ));
    }

    let m = model.clone();
    menu.separator()
        .item(item(
            "Copy title",
            Rc::new(move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(title.clone()))),
        ))
        .item(item(
            "Refresh",
            Rc::new(move |_, cx| m.update(cx, |model, cx| model.refresh_anime(id, cx))),
        ))
}

#[allow(clippy::too_many_arguments)]
fn date_menu(
    model: &Entity<Model>,
    id: AnimeId,
    airing_label: &'static str,
    airing: FuzzyDate,
    updated: FuzzyDate,
    completed: bool,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let model = model.clone();
    PopupMenu::build(window, cx, move |sub, _, _| {
        let set = move |date: FuzzyDate| {
            let model = model.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                edit(&model, id, cx, |e| {
                    if completed {
                        e.date_completed = date;
                    } else {
                        e.date_started = date;
                    }
                });
            }
        };
        sub.item(PopupMenuItem::new("Clear").on_click(set(FuzzyDate::default())))
            .item(
                PopupMenuItem::new(airing_label)
                    .disabled(airing.is_empty())
                    .on_click(set(airing)),
            )
            .item(PopupMenuItem::new("Set to last updated").on_click(set(updated)))
    })
}

/// A one-line text prompt with OK and Cancel.
fn prompt(
    title: &'static str,
    value: String,
    window: &mut Window,
    cx: &mut App,
    on_ok: impl Fn(String, &mut Window, &mut App) -> bool + 'static,
) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
    let first = Cell::new(true);
    let on_ok = Rc::new(on_ok);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            input.focus_handle(cx).focus(window, cx);
        }
        let input_ok = input.clone();
        let on_ok = on_ok.clone();
        dialog.title(title).child(Input::new(&input)).footer(
            h_flex()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("cancel")
                        .label("Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new("ok")
                        .primary()
                        .label("OK")
                        .on_click(move |_, window, cx| {
                            let text = input_ok.read(cx).value().to_string();
                            if on_ok(text, window, cx) {
                                window.close_dialog(cx);
                            }
                        }),
                ),
        )
    });
}

fn ask_episode(
    model: &Entity<Model>,
    id: AnimeId,
    watched: u32,
    count: Option<u32>,
    window: &mut Window,
    cx: &mut App,
) {
    let model = model.clone();
    let max = count.unwrap_or(MAX_EPISODE_COUNT);
    prompt(
        "Episodes watched",
        watched.to_string(),
        window,
        cx,
        move |text, window, cx| {
            match text.trim().parse::<u32>() {
                Ok(n) if n <= max => {
                    edit(&model, id, cx, |e| e.watched_episodes = n);
                    true
                }
                // Keep the dialog open so the number can be fixed.
                _ => {
                    window.push_notification(format!("Enter a number from 0 to {max}."), cx);
                    false
                }
            }
        },
    );
}

fn ask_notes(model: &Entity<Model>, id: AnimeId, notes: String, window: &mut Window, cx: &mut App) {
    let model = model.clone();
    prompt("Notes", notes, window, cx, move |text, _, cx| {
        edit(&model, id, cx, |e| e.notes = text.trim().to_owned());
        true
    });
}

fn confirm_remove(
    model: &Entity<Model>,
    id: AnimeId,
    title: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let model = model.clone();
    let title = title.to_owned();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let model = model.clone();
        alert
            .confirm()
            .title("Do you want to remove selected items from your list?")
            .description(title.clone())
            .on_ok(move |_, _, cx| {
                model.update(cx, |model, cx| model.remove_entry(id, cx));
                true
            })
    });
}
