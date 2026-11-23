//! The settings dialog: a section list on the left, the section's controls
//! on the right. Edits are made on a copy and applied on OK.
//!
//! Choices with a few options are button groups and on/off settings are
//! switches, so the current value is seen without opening anything.

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement,
    PathPromptOptions, Render, SharedString, Styled, Subscription, Window, div, prelude::*, px,
};
use gpui_component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::switch::Switch;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, StyledExt as _,
    WindowExt as _, h_flex, v_flex,
};
use shiori_core::entry::is_in_list;
use shiori_core::{AnimeId, ServiceId, TitleLanguage, UpdateTrigger};
use shiori_store::{ColorScheme, ProxyKind, Settings, UPDATE_DELAY_RANGE};
use shiori_sync::anilist;

use crate::ui::icons::AppIcon;
use crate::ui::list_location::Place;
use crate::ui::model::Model;
use crate::ui::{actions, format, list_location, list_transfer, numbering_editor, widgets};

/// The sections and their icons. Browsers are switched on under Media
/// players, next to the browsers themselves, so Streaming has no page.
const SECTIONS: [&str; 7] = [
    "Accounts",
    "Library",
    "Application",
    "Media players",
    "Anime list update",
    "Recognition",
    "Advanced",
];

fn section_icon(ix: usize) -> Icon {
    match ix {
        0 => Icon::new(IconName::CircleUser),
        1 => Icon::new(IconName::Folder),
        2 => Icon::new(IconName::Palette),
        3 => Icon::new(IconName::Play),
        4 => Icon::new(IconName::RefreshCw),
        5 => Icon::new(AppIcon::Link2),
        _ => Icon::new(IconName::Network),
    }
}

/// Opens the settings dialog.
pub fn open(model: &Entity<Model>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| SettingsView::new(model.clone(), window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        let ok = view.clone();
        dialog
            .title("Settings")
            .width(px(820.))
            .child(view.clone())
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(Button::new("ok").primary().label("OK").on_click(
                        move |_, window, cx| {
                            if ok.update(cx, |view, cx| view.apply(window, cx)) {
                                window.close_dialog(cx);
                            }
                        },
                    )),
            )
    });
}

pub struct SettingsView {
    model: Entity<Model>,
    section: usize,
    /// The settings as edited so far.
    draft: Settings,
    token: Entity<InputState>,
    detection_interval: Entity<InputState>,
    update_delay: Entity<InputState>,
    player_filter: Entity<InputState>,
    /// Settings have no proxy switch of their own: a proxy is used when it
    /// has a host. The switch hides the fields until one is wanted.
    proxy_on: bool,
    proxy_host: Entity<InputState>,
    proxy_port: Entity<InputState>,
    proxy_username: Entity<InputState>,
    proxy_password: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let draft = model.read(cx).state().settings.clone();
        let text = |value: String,
                    placeholder: &'static str,
                    window: &mut Window,
                    cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste the token from AniList")
                .masked(true)
        });
        // The stored password is never shown; an empty box keeps it.
        let proxy_password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password (unchanged)")
                .masked(true)
        });
        let player_filter = text(String::new(), "Filter players", window, cx);
        let subscriptions =
            vec![
                cx.subscribe_in(&player_filter, window, |_, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                }),
            ];
        Self {
            section: 0,
            token,
            detection_interval: text(
                (draft.detection_interval_ms / 1000).max(1).to_string(),
                "",
                window,
                cx,
            ),
            update_delay: text(draft.update_delay_s.to_string(), "", window, cx),
            player_filter,
            proxy_on: !draft.proxy.host.is_empty(),
            proxy_host: text(draft.proxy.host.clone(), "Host", window, cx),
            proxy_port: text(
                draft.proxy.port.map(|p| p.to_string()).unwrap_or_default(),
                "Port",
                window,
                cx,
            ),
            proxy_username: text(draft.proxy.username.clone(), "Username", window, cx),
            proxy_password,
            draft,
            model,
            _subscriptions: subscriptions,
        }
    }

    /// Reads the controls into the draft and applies it. Returns false and
    /// says why when a value is not acceptable, keeping the dialog open.
    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let value = |input: &Entity<InputState>, cx: &App| input.read(cx).value().trim().to_owned();
        let mut settings = self.draft.clone();
        let Some(interval) = value(&self.detection_interval, cx)
            .parse::<u32>()
            .ok()
            .filter(|s| (1..=3600).contains(s))
        else {
            window.push_notification("Enter a detection interval from 1 to 3600 seconds.", cx);
            return false;
        };
        settings.detection_interval_ms = interval * 1000;
        let Some(delay) = value(&self.update_delay, cx)
            .parse::<u32>()
            .ok()
            .filter(|s| UPDATE_DELAY_RANGE.contains(s))
        else {
            window.push_notification(
                format!(
                    "Enter a delay from {} to {} seconds.",
                    UPDATE_DELAY_RANGE.start(),
                    UPDATE_DELAY_RANGE.end()
                ),
                cx,
            );
            return false;
        };
        settings.update_delay_s = delay;
        if self.proxy_on {
            let port = value(&self.proxy_port, cx);
            settings.proxy.port = if port.is_empty() {
                None
            } else if let Ok(port) = port.parse() {
                Some(port)
            } else {
                window.push_notification("Enter a proxy port from 0 to 65535.", cx);
                return false;
            };
            settings.proxy.host = value(&self.proxy_host, cx);
            settings.proxy.username = value(&self.proxy_username, cx);
        } else {
            // Switching the proxy off is the same as clearing its host.
            settings.proxy.host = String::new();
        }
        let password = value(&self.proxy_password, cx);
        // Without a proxy there is no password to keep.
        let password = if settings.proxy.host.is_empty() {
            Some(String::new())
        } else {
            Some(password).filter(|p| !p.is_empty())
        };
        let scheme = settings.color_scheme;
        self.model
            .update(cx, |model, cx| model.apply_settings(settings, password, cx));
        crate::ui::apply_color_scheme(scheme, window, cx);
        true
    }

    fn back_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let name = format!("shiori-backup-{}.sqlite", jiff::Zoned::now().date());
        let path = cx.prompt_for_new_path(&home, Some(&name));
        let model = self.model.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(path))) = path.await {
                model.update(cx, |model, cx| model.back_up_to(path, cx));
            }
        })
        .detach();
    }

    fn import_xml(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        let model = self.model.clone();
        let handle = window.window_handle();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(mut paths))) = paths.await
                && let Some(path) = paths.pop()
            {
                model.update(cx, |model, cx| {
                    model.import_mal_xml(path, list_transfer::show_summary(handle), cx)
                });
            }
        })
        .detach();
    }

    fn copy_public(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let handle = window.window_handle();
        actions::prompt(
            "Whose AniList list?",
            String::new(),
            window,
            cx,
            move |name, _, cx| {
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return false;
                }
                model.update(cx, |model, cx| {
                    model.copy_public_list(name, list_transfer::show_summary(handle), cx)
                });
                true
            },
        );
    }

    fn export_xml(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let path = cx.prompt_for_new_path(&home, Some("animelist.xml"));
        let model = self.model.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(path))) = path.await {
                model.update(cx, |model, cx| model.export_mal_xml(path, cx));
            }
        })
        .detach();
    }

    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Restore".into()),
        });
        let model = self.model.clone();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(mut paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };
            let _ = cx.update(|window, cx| {
                actions::confirm(
                    format!("Restore {}?", list_location::file_name(&path)),
                    "Your current list is saved first, so you can go back.".to_owned(),
                    "Restore",
                    actions::Risk::Safe,
                    window,
                    cx,
                    move |cx| model.update(cx, |model, cx| model.restore(path.clone(), cx)),
                );
            });
        })
        .detach();
    }

    fn sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.token.read(cx).value().trim().to_owned();
        if token.is_empty() {
            return;
        }
        self.token
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.model.update(cx, |model, cx| model.sign_in(token, cx));
    }

    fn add_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = this.update(cx, |this, cx| {
                    for path in paths {
                        if !this.draft.library_folders.contains(&path) {
                            this.draft.library_folders.push(path);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The running version, and the newer one when the check found it.
    fn version_line(&self, cx: &App) -> String {
        let current = env!("CARGO_PKG_VERSION");
        match &self.model.read(cx).update {
            Some(release) => format!(
                "You're using Shiori {current}. Version {} is out.",
                release.version
            ),
            None => format!("You're using Shiori {current}."),
        }
    }

    fn switch(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        set: fn(&mut Settings, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        Switch::new(id)
            .label(label)
            .checked(checked)
            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                set(&mut this.draft, *checked);
                cx.notify();
            }))
            .into_any_element()
    }

    /// A choice among a few values as a row of buttons, the chosen one
    /// pressed in.
    fn segmented<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        options: Vec<(Option<Icon>, &'static str, T)>,
        current: T,
        set: fn(&mut Settings, T),
        cx: &mut Context<Self>,
    ) -> ButtonGroup {
        let values: Vec<T> = options.iter().map(|o| o.2).collect();
        ButtonGroup::new(id)
            .outline()
            .small()
            .children(options.into_iter().map(|(icon, label, value)| {
                Button::new(label)
                    .label(label)
                    .when_some(icon, |button, icon| button.icon(icon))
                    .selected(value == current)
            }))
            .on_click(cx.listener(move |this, clicked: &Vec<usize>, _, cx| {
                if let Some(&value) = clicked.first().and_then(|&ix| values.get(ix)) {
                    set(&mut this.draft, value);
                    cx.notify();
                }
            }))
    }

    /// A chip per known player, filled while it is detected; turned off
    /// ones go in `disabled_players`. Browsers have a group of their own
    /// with the switch for streaming sites, which turns them all off at
    /// once.
    fn players(&self, cx: &mut Context<Self>) -> AnyElement {
        let filter = self.player_filter.read(cx).value().trim().to_lowercase();
        let players: Vec<(String, bool)> = self
            .model
            .read(cx)
            .tracker
            .players
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    p.kind == shiori_detect::PlayerType::WebBrowser,
                )
            })
            .collect();
        let total = players.len();
        let on_count = players
            .iter()
            .filter(|(n, _)| !self.draft.disabled_players.contains(n))
            .count();
        let mut apps = Vec::new();
        let mut browsers = Vec::new();
        for (name, browser) in players {
            if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                continue;
            }
            let on = !self.draft.disabled_players.contains(&name);
            let id = SharedString::from(format!("player-{name}"));
            let toggle = name.clone();
            let chip = widgets::chip(name, on, cx)
                .id(id)
                .cursor_pointer()
                .when(on, |chip| chip.child(Icon::new(IconName::Check).xsmall()))
                .when(browser && !self.draft.streaming_enabled, |chip| {
                    chip.opacity(0.5)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let disabled = &mut this.draft.disabled_players;
                    if disabled.contains(&toggle) {
                        disabled.retain(|n| *n != toggle);
                    } else {
                        disabled.push(toggle.clone());
                        disabled.sort();
                    }
                    cx.notify();
                }));
            if browser {
                browsers.push(chip);
            } else {
                apps.push(chip);
            }
        }
        let muted = cx.theme().muted_foreground;
        let d = &self.draft;
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.switch(
                        "detection",
                        "Detect what I'm watching",
                        d.detection_enabled,
                        |s, v| s.detection_enabled = v,
                        cx,
                    ))
                    .child(div().flex_1())
                    .child(div().text_sm().text_color(muted).child("every"))
                    .child(
                        div()
                            .w_16()
                            .child(Input::new(&self.detection_interval).small()),
                    )
                    .child(div().text_sm().text_color(muted).child("s")),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div().flex_1().child(
                            Input::new(&self.player_filter)
                                .small()
                                .prefix(Icon::new(IconName::Search).small().text_color(muted)),
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{on_count} of {total} on")),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(widgets::section_label("Players", cx))
                    .child(h_flex().flex_wrap().gap_1p5().children(apps)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_3()
                            .child(widgets::section_label("Web browsers", cx))
                            .child(
                                Switch::new("streaming")
                                    .small()
                                    .checked(d.streaming_enabled)
                                    .tooltip("Detect streaming sites in web browsers")
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        this.draft.streaming_enabled = *checked;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(h_flex().flex_wrap().gap_1p5().children(browsers)),
            )
            .into_any_element()
    }

    /// The user's recognition corrections: titles linked to anime and
    /// remembered episode numbering. Unlike the other sections these act at
    /// once; they are records to look through, not settings to apply.
    fn recognition(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let state = self.model.read(cx).state();
        let mut links: Vec<(String, AnimeId, String)> = state
            .anime_settings
            .values()
            .flat_map(|s| {
                s.linked
                    .iter()
                    .map(move |l| (l.title.clone(), s.id, format::date_label(l.date)))
            })
            .collect();
        links.sort_by_key(|l| l.0.to_lowercase());
        let sequences = state.numbering.sequences().to_vec();
        let line = |left: String, right: String| {
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().text_sm().child(left))
                .child(div().truncate().text_xs().text_color(muted).child(right))
        };
        let link_rows = links
            .into_iter()
            .enumerate()
            .map(|(ix, (title, id, date))| {
                let model = self.model.clone();
                let anime = state.title(id);
                h_flex()
                    .gap_2()
                    .py_1()
                    .child(line(format!("\"{title}\" → {anime}"), date))
                    .child(
                        Button::new(("forget-link", ix))
                            .small()
                            .ghost()
                            .label("Forget")
                            .on_click(move |_, _, cx| {
                                model.update(cx, |model, cx| model.unlink_title(id, &title, cx))
                            }),
                    )
            });
        let numbering_rows = sequences.into_iter().map(|s| {
            let ranges = s
                .entries
                .iter()
                .map(|e| format!("{}: {}-{}", state.title(e.anime), e.first, e.last))
                .collect::<Vec<_>>()
                .join(" · ");
            let (edit, forget) = (self.model.clone(), self.model.clone());
            let sequence = s.clone();
            h_flex()
                .gap_2()
                .py_1()
                .child(line(state.title(s.root), ranges))
                .child(
                    Button::new(("edit-numbering", s.id as usize))
                        .small()
                        .label("Edit")
                        .on_click(move |_, window, cx| {
                            numbering_editor::open(&edit, sequence.clone(), None, false, window, cx)
                        }),
                )
                .child(
                    Button::new(("forget-numbering", s.id as usize))
                        .small()
                        .ghost()
                        .label("Forget")
                        .on_click(move |_, window, cx| {
                            let model = forget.clone();
                            let root = model.read(cx).state().title(s.root);
                            actions::confirm(
                                format!("Forget the numbering of {root}?"),
                                "Your list and history stay as they are.".to_owned(),
                                "Forget",
                                actions::Risk::Danger,
                                window,
                                cx,
                                move |cx| {
                                    model.update(cx, |model, cx| model.forget_numbering(s.id, cx))
                                },
                            )
                        }),
                )
        });
        let empty = |text: &'static str| div().text_sm().text_color(muted).child(text);
        let link_rows: Vec<_> = link_rows.collect();
        let numbering_rows: Vec<_> = numbering_rows.collect();
        v_flex()
            .gap_2()
            .child(widgets::section_label("Title links", cx))
            .when(link_rows.is_empty(), |el| {
                el.child(empty("Nothing linked yet"))
            })
            .children(link_rows)
            .child(
                div()
                    .pt_3()
                    .child(widgets::section_label("Episode numbering", cx)),
            )
            .when(numbering_rows.is_empty(), |el| {
                el.child(empty("Nothing remembered yet"))
            })
            .children(numbering_rows)
            .into_any_element()
    }

    fn accounts(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, success, warning) = (
            theme.muted_foreground,
            theme.border,
            theme.success,
            theme.warning,
        );
        let model = self.model.read(cx);
        let state = model.state();
        let service = state.service;
        let name = service.name();
        let who_on = |username: &str| {
            if username.is_empty() {
                "Not signed in".to_owned()
            } else {
                username.to_owned()
            }
        };
        let anilist_who = who_on(&state.accounts.anilist.username);
        let mal_who = who_on(&state.accounts.myanimelist.username);
        let username = match service {
            ServiceId::MyAnimeList => &state.accounts.myanimelist.username,
            _ => &state.accounts.anilist.username,
        };
        let signed_in = state.signed_in && !username.is_empty();
        let has_token = model.account().token.is_some();
        let who = who_on(username);
        let place = list_location::current_place(model);
        let local = state.is_local();
        let count = state
            .entries
            .values()
            .filter(|e| is_in_list(Some(e)))
            .count();
        let last_backup = model.last_backup;
        // Uploading needs the account's token, and something to upload.
        let can_upload = model.startup.has_local_list() && model.account().token.is_some();
        let d = &self.draft;

        let location_card = |id: &'static str,
                             icon: AppIcon,
                             title: &'static str,
                             hint: String,
                             to: Place,
                             cx: &App| {
            let chosen = place == to;
            let handle = self.model.clone();
            widgets::option_card(id, icon, title, hint, chosen, cx).on_click(
                move |_, window, cx| {
                    if !chosen {
                        list_location::change(&handle, to, window, cx);
                    }
                },
            )
        };
        let local_hint = if local {
            format!("{count} anime")
        } else {
            "No account needed".to_owned()
        };
        let cards = h_flex()
            .gap_3()
            .child(location_card(
                "location-local",
                AppIcon::Laptop,
                "On this computer",
                local_hint,
                Place::Local,
                cx,
            ))
            // Each service is its own list, so choosing one is choosing
            // where the list lives (decision I40).
            .child(location_card(
                "location-anilist",
                AppIcon::Cloud,
                "AniList",
                anilist_who,
                Place::Account(ServiceId::AniList),
                cx,
            ))
            .child(location_card(
                "location-myanimelist",
                AppIcon::Cloud,
                "MyAnimeList",
                mal_who,
                Place::Account(ServiceId::MyAnimeList),
                cx,
            ));

        let body = v_flex().gap_5().child(
            v_flex()
                .gap_2()
                .child(widgets::section_label("Your list", cx))
                .child(cards),
        );
        if local {
            let today = jiff::Zoned::now().date();
            // A backup older than the weekly one would be is worth a look.
            let fresh = last_backup.is_some_and(|last| {
                last.checked_add(jiff::Span::new().days(8))
                    .is_ok_and(|due| due > today)
            });
            let (icon, color) = if fresh {
                (AppIcon::ShieldCheck, success)
            } else {
                (AppIcon::ShieldAlert, warning)
            };
            let schedule = if d.backup_weekly {
                "Every week · the last 4 are kept"
            } else {
                "Weekly backups are off"
            };
            let backup = v_flex()
                .gap_2()
                .child(widgets::section_label("Backups", cx))
                .child(
                    h_flex()
                        .gap_3()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(border)
                        .child(widgets::tinted_icon_tile(icon, color))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .child(list_location::backed_up(last_backup)),
                                )
                                .child(div().text_xs().text_color(muted).child(schedule)),
                        )
                        .child(
                            Switch::new("backup-weekly")
                                .checked(d.backup_weekly)
                                .tooltip("Back up every week")
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.draft.backup_weekly = *checked;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("back-up")
                                .small()
                                .icon(AppIcon::Download)
                                .label("Back up now...")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.back_up(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("restore")
                                .small()
                                .icon(AppIcon::ArchiveRestore)
                                .label("Restore...")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.restore(window, cx)),
                                ),
                        ),
                );
            let moves = v_flex()
                .gap_2()
                .child(widgets::section_label("Move your list", cx))
                .child(
                    h_flex()
                        .gap_3()
                        .child(
                            widgets::action_tile(
                                "import-xml",
                                AppIcon::FilePlus,
                                "Import",
                                "MyAnimeList XML",
                                cx,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.import_xml(window, cx)),
                            ),
                        )
                        .child(
                            widgets::action_tile(
                                "copy-public",
                                AppIcon::Copy,
                                "Copy",
                                "A public AniList list",
                                cx,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.copy_public(window, cx)),
                            ),
                        )
                        .child(
                            widgets::action_tile(
                                "export-xml",
                                AppIcon::FileDown,
                                "Export",
                                "MyAnimeList XML",
                                cx,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.export_xml(window, cx)),
                            ),
                        ),
                );
            return body.child(backup).child(moves).into_any_element();
        }

        // Syncing and signing in mean nothing for a local list.
        let account_card =
            h_flex()
                .gap_3()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(border)
                .child(widgets::icon_tile(IconName::CircleUser, signed_in, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().font_semibold().child(who))
                        .child(div().text_xs().text_color(muted).child(
                            match (signed_in, service) {
                                (true, _) => "Signed in",
                                (false, ServiceId::MyAnimeList) => "Sign in through the browser",
                                (false, _) => "Authorize, then paste the token AniList shows",
                            },
                        )),
                )
                .child(self.switch(
                    "sync",
                    "Sync",
                    d.sync_enabled,
                    |s, v| s.sync_enabled = v,
                    cx,
                ));
        let sign_out = has_token.then(|| {
            Button::new("sign-out")
                .small()
                .label("Sign out")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.model.update(cx, |model, cx| model.sign_out(cx));
                    cx.notify();
                }))
        });
        // MyAnimeList signs in through the browser and its own dialog; the
        // paste field for when that fails lives there (mockup 2).
        let mal_sign_in = h_flex().gap_2().child(
            Button::new("mal-sign-in")
                .small()
                .primary()
                .icon(AppIcon::LogIn)
                .label(if has_token {
                    "Sign in again"
                } else {
                    "Sign in"
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    crate::ui::mal_sign_in::open(&this.model, window, cx)
                })),
        );
        let sign_in = h_flex()
            .gap_2()
            .child(
                Button::new("authorize")
                    .small()
                    .icon(IconName::ExternalLink)
                    .label("Authorize")
                    .on_click(|_, _, cx| cx.open_url(&anilist::authorize_url())),
            )
            .child(div().flex_1().child(Input::new(&self.token).small()))
            .child(
                Button::new("sign-in")
                    .small()
                    .primary()
                    .icon(AppIcon::LogIn)
                    .label("Sign in")
                    .on_click(cx.listener(|this, _, window, cx| this.sign_in(window, cx))),
            );
        let sign_in = match service {
            ServiceId::MyAnimeList => mal_sign_in,
            _ => sign_in,
        };
        body.child(
            v_flex()
                .gap_2()
                .child(widgets::section_label(format!("{name} account"), cx))
                .child(account_card)
                .child(sign_in.children(sign_out)),
        )
        .when(can_upload, |el| {
            el.child(
                v_flex()
                    .gap_2()
                    .child(widgets::section_label("List on this computer", cx))
                    .child(
                        h_flex().child(
                            widgets::action_tile(
                                "upload",
                                AppIcon::CloudUpload,
                                "Upload",
                                format!("Add it to your {name} list"),
                                cx,
                            )
                            .flex_none()
                            .w(px(200.))
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    list_location::ask_upload(&this.model, window, cx)
                                },
                            )),
                        ),
                    ),
            )
        })
        .into_any_element()
    }

    fn library(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let d = &self.draft;
        let folders = d.library_folders.iter().enumerate().map(|(ix, folder)| {
            h_flex()
                .gap_2()
                .px_3()
                .py_1()
                .rounded_lg()
                .border_1()
                .border_color(border)
                .child(Icon::new(IconName::Folder).small().text_color(muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .child(folder.display().to_string()),
                )
                .child(
                    Button::new(("remove-folder", ix))
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("Remove")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.draft.library_folders.remove(ix);
                            cx.notify();
                        })),
                )
        });
        v_flex()
            .gap_5()
            .child(
                v_flex()
                    .gap_2()
                    .child(widgets::section_label("Anime folders", cx))
                    .children(folders)
                    .when(d.library_folders.is_empty(), |list| {
                        list.child(div().text_sm().text_color(muted).child("No folders yet"))
                    })
                    .child(
                        h_flex().child(
                            Button::new("add-folder")
                                .small()
                                .icon(IconName::Plus)
                                .label("Add folder...")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add_folder(window, cx)),
                                ),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(widgets::section_label("Titles in", cx))
                    .child(h_flex().child(self.segmented(
                        "title-language",
                        vec![
                            (None, "Romaji", TitleLanguage::Romaji),
                            (None, "English", TitleLanguage::English),
                            (None, "Native", TitleLanguage::Native),
                        ],
                        d.title_language,
                        |s, v| s.title_language = v,
                        cx,
                    ))),
            )
            .into_any_element()
    }

    fn render_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let d = &self.draft;
        let group = |label: &'static str, cx: &App| {
            v_flex().gap_2().child(widgets::section_label(label, cx))
        };
        match self.section {
            0 => self.accounts(cx),
            1 => self.library(cx),
            2 => group("Color scheme", cx)
                .child(h_flex().child(self.segmented(
                    "color-scheme",
                    vec![
                        (
                            Some(Icon::new(AppIcon::Monitor)),
                            "System",
                            ColorScheme::System,
                        ),
                        (Some(Icon::new(IconName::Sun)), "Light", ColorScheme::Light),
                        (Some(Icon::new(IconName::Moon)), "Dark", ColorScheme::Dark),
                    ],
                    d.color_scheme,
                    |s, v| s.color_scheme = v,
                    cx,
                )))
                .child(div().h_3())
                .child(widgets::section_label("Updates", cx))
                .child(self.switch(
                    "check-for-updates",
                    "Tell me when a new version of Shiori is out",
                    d.check_for_updates,
                    |s, v| s.check_for_updates = v,
                    cx,
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.version_line(cx)),
                )
                .into_any_element(),
            3 => self.players(cx),
            4 => v_flex()
                .gap_5()
                .child(
                    group("Update my list", cx)
                        .child(h_flex().child(self.segmented(
                            "update-trigger",
                            vec![
                                (None, "After a delay", UpdateTrigger::AfterDelay),
                                (None, "When the player closes", UpdateTrigger::OnPlayerClose),
                            ],
                            d.update_trigger,
                            |s, v| s.update_trigger = v,
                            cx,
                        )))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().text_sm().text_color(muted).child("Wait"))
                                .child(div().w_20().child(Input::new(&self.update_delay).small()))
                                .child(div().text_sm().text_color(muted).child("seconds first")),
                        ),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .child(self.switch(
                            "pause",
                            "Pause the wait while the player is in the background",
                            d.pause_when_unfocused,
                            |s, v| s.pause_when_unfocused = v,
                            cx,
                        ))
                        .child(self.switch(
                            "library-only",
                            "Only update for files in my library folders",
                            d.library_only,
                            |s, v| s.library_only = v,
                            cx,
                        )),
                )
                .into_any_element(),
            5 => self.recognition(cx),
            _ => {
                let fields = self.proxy_on.then(|| {
                    v_flex()
                        .gap_2()
                        .child(h_flex().child(self.segmented(
                            "proxy-kind",
                            vec![
                                (None, "HTTP", ProxyKind::Http),
                                (None, "SOCKS5", ProxyKind::Socks5),
                            ],
                            d.proxy.kind,
                            |s, v| s.proxy.kind = v,
                            cx,
                        )))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().flex_1().child(Input::new(&self.proxy_host).small()))
                                .child(div().w_24().child(Input::new(&self.proxy_port).small())),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .child(Input::new(&self.proxy_username).small()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .child(Input::new(&self.proxy_password).small()),
                                ),
                        )
                });
                group("Proxy", cx)
                    .child(
                        Switch::new("proxy")
                            .label("Use a proxy")
                            .checked(self.proxy_on)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.proxy_on = *checked;
                                cx.notify();
                            })),
                    )
                    .children(fields)
                    .into_any_element()
            }
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = cx.theme().table_active;
        let hover = cx.theme().table_hover;
        let muted = cx.theme().muted_foreground;
        let sections = SECTIONS.iter().enumerate().map(|(ix, name)| {
            let name: SharedString = (*name).into();
            h_flex()
                .id(("section", ix))
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .when(ix == self.section, |item| item.bg(active))
                .hover(|item| item.bg(hover))
                .child(section_icon(ix).small().text_color(muted))
                .child(name)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.section = ix;
                    cx.notify();
                }))
        });
        h_flex()
            .h(px(440.))
            .gap_4()
            .items_start()
            .child(v_flex().w_40().flex_none().gap_1().children(sections))
            .child(
                div()
                    .id("section-content")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(self.render_section(cx)),
            )
    }
}
