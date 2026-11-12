//! The settings dialog: a section list on the left, the section's controls
//! on the right. Edits are made on a copy and applied on OK.

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement,
    PathPromptOptions, Render, SharedString, Styled, Window, div, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState};
use gpui_component::select::{Select, SelectState};
use gpui_component::{ActiveTheme as _, IndexPath, WindowExt as _, h_flex, v_flex};
use shiori_core::{TitleLanguage, UpdateTrigger};
use shiori_store::{ColorScheme, ProxyKind, Settings, UPDATE_DELAY_RANGE};
use shiori_sync::anilist;

use crate::ui::model::Model;
use crate::ui::widgets::Choice;

const SECTIONS: [&str; 7] = [
    "Accounts",
    "Library",
    "Application",
    "Media players",
    "Streaming",
    "Anime list update",
    "Advanced",
];

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

fn select<T: Clone + PartialEq + 'static>(
    items: Vec<Choice<T>>,
    value: &T,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Entity<SelectState<Vec<Choice<T>>>> {
    let ix = items
        .iter()
        .position(|c| &c.value == value)
        .map(IndexPath::new);
    cx.new(|cx| SelectState::new(items, ix, window, cx))
}

pub struct SettingsView {
    model: Entity<Model>,
    section: usize,
    /// The settings as edited so far.
    draft: Settings,
    token: Entity<InputState>,
    title_language: Entity<SelectState<Vec<Choice<TitleLanguage>>>>,
    color_scheme: Entity<SelectState<Vec<Choice<ColorScheme>>>>,
    detection_interval: Entity<InputState>,
    update_trigger: Entity<SelectState<Vec<Choice<UpdateTrigger>>>>,
    update_delay: Entity<InputState>,
    proxy_kind: Entity<SelectState<Vec<Choice<ProxyKind>>>>,
    proxy_host: Entity<InputState>,
    proxy_port: Entity<InputState>,
    proxy_username: Entity<InputState>,
    proxy_password: Entity<InputState>,
}

impl SettingsView {
    fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let draft = model.read(cx).state().settings.clone();
        let text = |value: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(value))
        };
        let title_language = select(
            vec![
                Choice::new("Romaji", TitleLanguage::Romaji),
                Choice::new("English", TitleLanguage::English),
                Choice::new("Native", TitleLanguage::Native),
            ],
            &draft.title_language,
            window,
            cx,
        );
        let color_scheme = select(
            vec![
                Choice::new("Follow the system", ColorScheme::System),
                Choice::new("Light", ColorScheme::Light),
                Choice::new("Dark", ColorScheme::Dark),
            ],
            &draft.color_scheme,
            window,
            cx,
        );
        let update_trigger = select(
            vec![
                Choice::new("Update after delay", UpdateTrigger::AfterDelay),
                Choice::new(
                    "Update when media is closed (after delay)",
                    UpdateTrigger::OnPlayerClose,
                ),
            ],
            &draft.update_trigger,
            window,
            cx,
        );
        let proxy_kind = select(
            vec![
                Choice::new("HTTP", ProxyKind::Http),
                Choice::new("SOCKS5", ProxyKind::Socks5),
            ],
            &draft.proxy.kind,
            window,
            cx,
        );
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste the token from AniList")
                .masked(true)
        });
        // The stored password is never shown; an empty box keeps it.
        let proxy_password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Unchanged")
                .masked(true)
        });
        Self {
            section: 0,
            token,
            title_language,
            color_scheme,
            detection_interval: text(
                (draft.detection_interval_ms / 1000).max(1).to_string(),
                window,
                cx,
            ),
            update_trigger,
            update_delay: text(draft.update_delay_s.to_string(), window, cx),
            proxy_kind,
            proxy_host: text(draft.proxy.host.clone(), window, cx),
            proxy_port: text(
                draft.proxy.port.map(|p| p.to_string()).unwrap_or_default(),
                window,
                cx,
            ),
            proxy_username: text(draft.proxy.username.clone(), window, cx),
            proxy_password,
            draft,
            model,
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
        if let Some(&kind) = self.proxy_kind.read(cx).selected_value() {
            settings.proxy.kind = kind;
        }
        if let Some(&language) = self.title_language.read(cx).selected_value() {
            settings.title_language = language;
        }
        if let Some(&scheme) = self.color_scheme.read(cx).selected_value() {
            settings.color_scheme = scheme;
        }
        if let Some(&trigger) = self.update_trigger.read(cx).selected_value() {
            settings.update_trigger = trigger;
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

    fn checkbox(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        set: fn(&mut Settings, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        Checkbox::new(id)
            .label(label)
            .checked(checked)
            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                set(&mut this.draft, *checked);
                cx.notify();
            }))
            .into_any_element()
    }

    /// A checkbox per known player; unchecked ones go in
    /// `disabled_players`. Browsers are listed too: Streaming turns them all
    /// off at once, these turn off one.
    fn players(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let names: Vec<(String, bool)> = self
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
        let boxes = names.into_iter().map(|(name, browser)| {
            let checked = !self.draft.disabled_players.contains(&name);
            let id = SharedString::from(format!("player-{name}"));
            let label = if browser {
                format!("{name} (web browser)")
            } else {
                name.clone()
            };
            Checkbox::new(id)
                .label(label)
                .checked(checked)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    let disabled = &mut this.draft.disabled_players;
                    disabled.retain(|n| *n != name);
                    if !*checked {
                        disabled.push(name.clone());
                        disabled.sort();
                    }
                    cx.notify();
                }))
        });
        v_flex().gap_1().children(boxes)
    }

    fn render_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let label = |text: &'static str| div().text_color(muted).child(text);
        let d = &self.draft;
        match self.section {
            0 => {
                let state = self.model.read(cx).state();
                let account = &state.accounts.anilist;
                let who = if account.username.is_empty() {
                    "Not signed in.".to_owned()
                } else if state.signed_in {
                    format!("Signed in as {}.", account.username)
                } else {
                    format!("Account: {}.", account.username)
                };
                v_flex()
                    .gap_3()
                    .child(label("Service"))
                    // MyAnimeList and Kitsu come with phase 8.
                    .child(div().child(state.service.name()))
                    .child(self.checkbox(
                        "sync",
                        "Enable synchronization",
                        d.sync_enabled,
                        |s, v| s.sync_enabled = v,
                        cx,
                    ))
                    .child(label("AniList account"))
                    .child(div().child(who))
                    .child(div().text_sm().child(
                        "Click Authorize, approve Shiori on AniList, then paste the token it shows here.",
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("authorize")
                                    .label("Authorize")
                                    .on_click(|_, _, cx| cx.open_url(&anilist::authorize_url())),
                            )
                            .child(div().flex_1().child(Input::new(&self.token)))
                            .child(Button::new("sign-in").label("Sign in").on_click(
                                cx.listener(|this, _, window, cx| this.sign_in(window, cx)),
                            )),
                    )
                    .into_any_element()
            }
            1 => {
                let folders = d.library_folders.iter().enumerate().map(|(ix, folder)| {
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .child(folder.display().to_string()),
                        )
                        .child(
                            Button::new(("remove-folder", ix))
                                .ghost()
                                .label("Remove")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.draft.library_folders.remove(ix);
                                    cx.notify();
                                })),
                        )
                });
                v_flex()
                    .gap_3()
                    .child(label("Anime folders"))
                    .children(folders)
                    .when(d.library_folders.is_empty(), |list| {
                        list.child(div().text_sm().child("No folders yet."))
                    })
                    .child(h_flex().child(
                        Button::new("add-folder").label("Add folder...").on_click(
                            cx.listener(|this, _, window, cx| this.add_folder(window, cx)),
                        ),
                    ))
                    .child(label("Title language"))
                    .child(div().w_48().child(Select::new(&self.title_language)))
                    .into_any_element()
            }
            2 => v_flex()
                .gap_3()
                .child(label("Color scheme"))
                .child(div().w_48().child(Select::new(&self.color_scheme)))
                .into_any_element(),
            3 => v_flex()
                .gap_3()
                .child(self.checkbox(
                    "detection",
                    "Enable detection",
                    d.detection_enabled,
                    |s, v| s.detection_enabled = v,
                    cx,
                ))
                .child(label("Detection interval in seconds"))
                .child(div().w_24().child(Input::new(&self.detection_interval)))
                .child(label("Media players"))
                .child(self.players(cx))
                .into_any_element(),
            4 => v_flex()
                .gap_3()
                .child(self.checkbox(
                    "streaming",
                    "Detect streaming media in web browsers",
                    d.streaming_enabled,
                    |s, v| s.streaming_enabled = v,
                    cx,
                ))
                .into_any_element(),
            5 => v_flex()
                .gap_3()
                .child(div().w_96().child(Select::new(&self.update_trigger)))
                .child(label("Delay in seconds"))
                .child(div().w_24().child(Input::new(&self.update_delay)))
                .child(self.checkbox(
                    "pause",
                    "Pause delay while player is not in focus",
                    d.pause_when_unfocused,
                    |s, v| s.pause_when_unfocused = v,
                    cx,
                ))
                .child(self.checkbox(
                    "library-only",
                    "Ignore files outside of library folders",
                    d.library_only,
                    |s, v| s.library_only = v,
                    cx,
                ))
                .into_any_element(),
            _ => v_flex()
                .gap_3()
                .child(label("Proxy type"))
                .child(div().w_32().child(Select::new(&self.proxy_kind)))
                .child(label("Host (empty for no proxy)"))
                .child(Input::new(&self.proxy_host))
                .child(label("Port"))
                .child(div().w_24().child(Input::new(&self.proxy_port)))
                .child(label("Username"))
                .child(Input::new(&self.proxy_username))
                .child(label("Password"))
                .child(Input::new(&self.proxy_password))
                .into_any_element(),
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = cx.theme().table_active;
        let hover = cx.theme().table_hover;
        let sections = SECTIONS.iter().enumerate().map(|(ix, name)| {
            let name: SharedString = (*name).into();
            div()
                .id(("section", ix))
                .px_2()
                .py_1()
                .rounded_md()
                .when(ix == self.section, |item| item.bg(active))
                .hover(|item| item.bg(hover))
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
                    .h_full()
                    .overflow_y_scroll()
                    .child(self.render_section(cx)),
            )
    }
}
