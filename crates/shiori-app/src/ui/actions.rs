//! The actions offered on the selected anime through the context menu. Each
//! edit builds a changed entry and saves it through the model.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, ClipboardItem, Entity, Focusable as _, ParentElement as _, SharedString,
    Styled as _, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::{ActiveTheme as _, WindowExt as _, h_flex};
use shiori_core::anime::MAX_EPISODE_COUNT;
use shiori_core::entry::is_in_list;
use shiori_core::{Anime, AnimeId, FuzzyDate, ListEntry, ListStatus};
use shiori_library::{can_start_rewatch, last_episode, next_episode, random_episode_limit};
use shiori_sync::anilist::score::rating_list;

use super::library::PlayChoice;
use super::model::Model;
use super::tracker::Badge;
use super::{format, inspector, poster};

/// A menu item's action.
pub type Handler = dyn Fn(&mut Window, &mut App);

/// Changes one entry. A missing entry starts as a new one.
fn edit(model: &Entity<Model>, id: AnimeId, cx: &mut App, change: impl Fn(&mut ListEntry)) {
    edit_all(model, &[id], cx, |entry, _| change(entry));
}

/// Changes several entries the same way. The anime is passed along for
/// edits that depend on it, such as "set to date started airing".
fn edit_all(
    model: &Entity<Model>,
    ids: &[AnimeId],
    cx: &mut App,
    change: impl Fn(&mut ListEntry, Option<&Anime>),
) {
    model.update(cx, |model, cx| {
        for &id in ids {
            let state = model.state();
            let mut entry = state
                .entries
                .get(&id)
                .filter(|e| !e.pending_delete)
                .cloned()
                .unwrap_or_else(|| ListEntry::new(id));
            change(&mut entry, state.anime.get(&id));
            model.save_entry(entry, cx);
        }
    });
}

/// A web search address for an anime title.
fn web_search_url(title: &str) -> String {
    let mut query = String::new();
    for byte in title.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                query.push(byte as char)
            }
            b' ' => query.push('+'),
            _ => query.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("https://duckduckgo.com/?q={query}")
}

/// Builds the context menu for the selected anime. `ids` is never empty;
/// items that act on one anime use the first. `invert` is the list's
/// "Invert selection", when the menu belongs to a list.
pub fn context_menu(
    model: &Entity<Model>,
    ids: Vec<AnimeId>,
    invert: Option<Rc<Handler>>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    let Some(&id) = ids.first() else {
        return menu;
    };
    let state = model.read(cx).state();
    let single = ids.len() == 1;
    let in_list: Vec<AnimeId> = ids
        .iter()
        .copied()
        .filter(|id| is_in_list(state.entries.get(id)))
        .collect();
    let entry = state.entries.get(&id).cloned();
    let episodes = state.anime.get(&id).and_then(|a| a.episode_count);
    let titles: Vec<String> = ids.iter().map(|&id| state.title(id)).collect();
    let ratings = rating_list(format::score_format(state));
    // Check marks only make sense when every selected entry agrees.
    let current = |value: fn(&ListEntry) -> u8| {
        let mut values = in_list
            .iter()
            .filter_map(|id| state.entries.get(id))
            .map(value);
        let first = values.next();
        first.filter(|f| values.all(|v| v == *f))
    };
    let current_score = current(|e| e.score);
    let current_status = current(|e| e.status as u8);

    let item = |label: &str, f: Rc<Handler>| {
        PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| f(window, cx))
    };
    let m = model.clone();

    let menu = menu.item(item(
        "Details",
        Rc::new({
            let m = m.clone();
            move |window, cx| inspector::open(&m, id, window, cx)
        }),
    ));
    // While something plays that is not this anime: unrecognized, or
    // recognized as the wrong one (ui.md).
    let now_playing = single && {
        let tracker = &model.read(cx).tracker;
        tracker.badge() != Badge::None
            && super::now_playing::playing_anime(model.read(cx)) != Some(id)
    };
    let menu = if now_playing {
        menu.item(item(
            "Set as now playing",
            Rc::new({
                let m = m.clone();
                move |_, cx| m.update(cx, |model, cx| model.set_now_playing(id, cx))
            }),
        ))
    } else {
        menu
    };
    let mut menu = menu
        .item(PopupMenuItem::submenu(
            "Play",
            play_menu(model, id, window, cx),
        ))
        .item(item(
            "Open folder",
            Rc::new({
                let m = m.clone();
                move |_, cx| m.update(cx, |model, cx| model.open_anime_folder(id, cx))
            }),
        ))
        .separator()
        .item(item(
            "Links",
            Rc::new(move |_, cx| cx.open_url(&format!("https://anilist.co/anime/{id}"))),
        ))
        .item(PopupMenuItem::submenu(
            "Search",
            PopupMenu::build(window, cx, {
                let title = titles[0].clone();
                move |sub, _, _| {
                    let title = title.clone();
                    sub.item(
                        PopupMenuItem::new("Torrents and other links")
                            .on_click(move |_, _, cx| cx.open_url(&web_search_url(&title))),
                    )
                }
            }),
        ));

    let not_in_list: Vec<AnimeId> = ids
        .iter()
        .copied()
        .filter(|i| !in_list.contains(i))
        .collect();
    if !not_in_list.is_empty() {
        let m = m.clone();
        let add = PopupMenu::build(window, cx, move |mut sub, _, _| {
            for status in ListStatus::SHOWN {
                let m = m.clone();
                let ids = not_in_list.clone();
                sub = sub
                    .item(PopupMenuItem::new(format::status_label(status)).on_click(
                        move |_, _, cx| edit_all(&m, &ids, cx, |e, _| e.status = status),
                    ));
            }
            sub
        });
        menu = menu
            .separator()
            .item(PopupMenuItem::submenu("Add to list", add));
    }
    if !in_list.is_empty() {
        let m2 = m.clone();
        let ids = in_list.clone();
        let edit_menu =
            PopupMenu::build(window, cx, move |mut sub, window, cx| {
                let m = m2.clone();
                if single {
                    let watched = entry.as_ref().map_or(0, |e| e.watched_episodes);
                    sub = sub.item(PopupMenuItem::new("Episode...").on_click({
                        let m = m.clone();
                        move |_, window, cx| ask_episode(&m, id, watched, episodes, window, cx)
                    }));
                }

                let score = PopupMenu::build(window, cx, {
                    let m = m.clone();
                    let ratings = ratings.clone();
                    let ids = ids.clone();
                    move |mut sub, _, _| {
                        for rating in &ratings {
                            let m = m.clone();
                            let ids = ids.clone();
                            let value = rating.value;
                            sub = sub.item(
                                PopupMenuItem::new(rating.label.clone())
                                    .checked(Some(value) == current_score)
                                    .on_click(move |_, _, cx| {
                                        edit_all(&m, &ids, cx, |e, _| e.score = value)
                                    }),
                            );
                        }
                        sub
                    }
                });
                sub = sub.item(PopupMenuItem::submenu("Score", score));

                let status = PopupMenu::build(window, cx, {
                    let m = m.clone();
                    let ids = ids.clone();
                    move |mut sub, _, _| {
                        for status in ListStatus::SHOWN {
                            let m = m.clone();
                            let ids = ids.clone();
                            sub = sub.item(
                                PopupMenuItem::new(format::status_label(status))
                                    .checked(Some(status as u8) == current_status)
                                    .on_click(move |_, _, cx| {
                                        edit_all(&m, &ids, cx, |e, _| e.status = status)
                                    }),
                            );
                        }
                        sub
                    }
                });
                sub = sub.item(PopupMenuItem::submenu("Status", status));
                sub = sub.item(PopupMenuItem::submenu(
                    "Date started",
                    date_menu(&m, ids.clone(), false, window, cx),
                ));
                sub = sub.item(PopupMenuItem::submenu(
                    "Date completed",
                    date_menu(&m, ids.clone(), true, window, cx),
                ));
                if single {
                    let notes = entry.as_ref().map(|e| e.notes.clone()).unwrap_or_default();
                    sub = sub.item(PopupMenuItem::new("Notes...").on_click(
                        move |_, window, cx| ask_notes(&m, id, notes.clone(), window, cx),
                    ));
                }
                sub
            });
        let m = m.clone();
        let remove_ids = in_list.clone();
        let remove_titles = titles.join("\n");
        menu = menu
            .separator()
            .item(PopupMenuItem::submenu("Edit", edit_menu))
            .item(item(
                "Remove...",
                Rc::new(move |window, cx| {
                    confirm_remove(&m, remove_ids.clone(), &remove_titles, window, cx)
                }),
            ));
    }

    let m = model.clone();
    let refresh_ids = ids.clone();
    let copied = titles.join("\n");
    menu = menu
        .separator()
        .item(PopupMenuItem::submenu(
            "Copy",
            PopupMenu::build(window, cx, move |sub, _, _| {
                let copied = copied.clone();
                sub.item(PopupMenuItem::new("Title").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                }))
            }),
        ))
        .item(item(
            "Refresh",
            Rc::new(move |_, cx| {
                m.update(cx, |model, cx| {
                    for &id in &refresh_ids {
                        model.refresh_anime(id, cx);
                    }
                })
            }),
        ));
    if let Some(invert) = invert {
        menu = menu.separator().item(item("Invert selection", invert));
    }
    menu
}

/// Play ▸ for one anime (ui.md, "Context menu").
fn play_menu(
    model: &Entity<Model>,
    id: AnimeId,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let state = model.read(cx).state();
    let entry = state.entries.get(&id).filter(|e| !e.pending_delete);
    let anime = state.anime.get(&id);
    let next = next_episode(entry, anime);
    let last = last_episode(entry);
    let random = random_episode_limit(entry, anime).is_some();
    let rewatch = can_start_rewatch(entry, anime, format::today_in_japan());
    let count = anime.and_then(|a| a.episode_count);
    let model = model.clone();
    PopupMenu::build(window, cx, move |sub, _, _| {
        let play = |choice: PlayChoice| {
            let model = model.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                model.update(cx, |model, cx| model.play(id, choice, cx))
            }
        };
        let mut sub = sub.item(
            PopupMenuItem::new(format!("Next episode (#{next})")).on_click(play(PlayChoice::Next)),
        );
        if let Some(last) = last {
            sub = sub.item(
                PopupMenuItem::new(format!("Last episode (#{last})"))
                    .on_click(play(PlayChoice::Last)),
            );
        }
        if random {
            sub = sub.item(PopupMenuItem::new("Random episode").on_click(play(PlayChoice::Random)));
        }
        let m = model.clone();
        sub = sub.item(
            PopupMenuItem::new("Episode...")
                .on_click(move |_, window, cx| ask_play_episode(&m, id, next, count, window, cx)),
        );
        if rewatch {
            let m = model.clone();
            sub =
                sub.separator()
                    .item(
                        PopupMenuItem::new("Start new rewatch").on_click(move |_, _, cx| {
                            m.update(cx, |model, cx| model.start_rewatch(id, cx))
                        }),
                    );
        }
        sub
    })
}

fn ask_play_episode(
    model: &Entity<Model>,
    id: AnimeId,
    next: u32,
    count: Option<u32>,
    window: &mut Window,
    cx: &mut App,
) {
    let model = model.clone();
    let max = count.filter(|&c| c > 0).unwrap_or(MAX_EPISODE_COUNT);
    prompt(
        "Play episode",
        next.to_string(),
        window,
        cx,
        move |text, window, cx| match text.trim().parse::<u32>() {
            Ok(n) if (1..=max).contains(&n) => {
                model.update(cx, |model, cx| model.play(id, PlayChoice::Episode(n), cx));
                true
            }
            _ => {
                window.push_notification(format!("Enter a number from 1 to {max}."), cx);
                false
            }
        },
    );
}

fn date_menu(
    model: &Entity<Model>,
    ids: Vec<AnimeId>,
    completed: bool,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let model = model.clone();
    let airing_label = if completed {
        "Set to date finished airing"
    } else {
        "Set to date started airing"
    };
    PopupMenu::build(window, cx, move |sub, _, _| {
        // Which date to set, worked out per entry.
        let set = |pick: fn(&ListEntry, Option<&Anime>) -> FuzzyDate| {
            let model = model.clone();
            let ids = ids.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                edit_all(&model, &ids, cx, |e, anime| {
                    let date = pick(e, anime);
                    if completed {
                        e.date_completed = date;
                    } else {
                        e.date_started = date;
                    }
                });
            }
        };
        let airing = if completed {
            set(|_, anime| anime.map(|a| a.date_finished).unwrap_or_default())
        } else {
            set(|_, anime| anime.map(|a| a.date_started).unwrap_or_default())
        };
        sub.item(PopupMenuItem::new("Clear").on_click(set(|_, _| FuzzyDate::default())))
            .item(PopupMenuItem::new(airing_label).on_click(airing))
            .item(
                PopupMenuItem::new("Set to last updated").on_click(set(|e, _| {
                    FuzzyDate::from(e.last_updated.to_zoned(jiff::tz::TimeZone::system()).date())
                })),
            )
    })
}

/// A one-line text prompt with OK and Cancel.
pub(crate) fn prompt(
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

/// Asks before removing anime from the list. Their posters show which
/// ones, since a row of covers is recognized faster than a list of titles.
fn confirm_remove(
    model: &Entity<Model>,
    ids: Vec<AnimeId>,
    titles: &str,
    window: &mut Window,
    cx: &mut App,
) {
    const SHOWN: usize = 6;
    let (title, ok) = match ids.len() {
        1 => (format!("Remove {titles}?"), "Remove".to_owned()),
        n => (format!("Remove {n} anime?"), format!("Remove {n}")),
    };
    let on_ok = {
        let model = model.clone();
        let ids = ids.clone();
        Rc::new(move |cx: &mut App| {
            model.update(cx, |model, cx| {
                for &id in &ids {
                    model.remove_entry(id, cx);
                }
            })
        })
    };
    let model = model.clone();
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let on_ok = on_ok.clone();
        let more = ids.len().saturating_sub(SHOWN);
        let posters = h_flex()
            .gap_2()
            .children(ids.iter().take(SHOWN).map(|&id| {
                div()
                    .w(px(40.))
                    .h(px(56.))
                    .flex_none()
                    .rounded_sm()
                    .overflow_hidden()
                    .child(poster::view(&model, id, cx))
            }))
            .when(more > 0, |row| {
                row.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("+{more}")),
                )
            });
        alert
            .confirm()
            .title(title.clone())
            .description(posters)
            .ok_text(ok.clone())
            .ok_variant(ButtonVariant::Danger)
            .on_ok(move |_, _, cx| {
                on_ok(cx);
                true
            })
    });
}

/// How a confirmation's OK button looks: `Danger` for what cannot be
/// undone, so the button warns before the words are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Safe,
    Danger,
}

/// Asks a yes or no question. The OK button names the action, such as
/// "Clear history", so the answer is clear without reading the question.
/// `on_ok` runs on yes.
pub fn confirm(
    title: impl Into<SharedString>,
    description: String,
    ok: &'static str,
    risk: Risk,
    window: &mut Window,
    cx: &mut App,
    on_ok: impl Fn(&mut App) + 'static,
) {
    let title = title.into();
    let on_ok = Rc::new(on_ok);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let on_ok = on_ok.clone();
        alert
            .confirm()
            .title(title.clone())
            .when(!description.is_empty(), |alert| {
                alert.description(description.clone())
            })
            .ok_text(ok)
            .ok_variant(if risk == Risk::Danger {
                ButtonVariant::Danger
            } else {
                ButtonVariant::Primary
            })
            .on_ok(move |_, _, cx| {
                on_ok(cx);
                true
            })
    });
}
