//! The main window: sidebar, the current page, the inspector and the
//! status line (decision I22).

use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ParentElement, PathPromptOptions, Render,
    SharedString, Styled, Subscription, Window, div, img, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::resizable::{ResizableState, h_resizable, resizable_panel};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};
use shiori_core::ListStatus;
use shiori_core::entry::is_in_list;

use shiori_app::home::continue_watching;

use super::icons::AppIcon;
use super::inspector::Inspector;
use super::model::{Model, UiRequest};
use super::now_playing;
use super::pages::{self, Page};
use super::{
    About, FocusSearch, GoBack, GoForward, PlayNext, PlayRandom, Synchronize, ToggleInspector,
    style,
};

/// Below this window width the inspector does not fit beside the list, and
/// an opened anime takes the whole page instead.
const NARROW: f32 = 1000.;
const SIDEBAR_WIDTH: f32 = 220.;

pub struct MainWindow {
    model: Entity<Model>,
    page: Page,
    back: Vec<Page>,
    forward: Vec<Page>,
    search: Entity<InputState>,
    pages: pages::Pages,
    inspector: Entity<Inspector>,
    split: Entity<ResizableState>,
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search AniList"));
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.subscribe_in(&search, window, |this, input, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = input.read(cx).value().to_string();
                    this.pages
                        .search
                        .update(cx, |page, cx| page.search(text, window, cx));
                    this.navigate(Page::Search, cx);
                }
            }),
            cx.subscribe_in(
                &model,
                window,
                |this, _, request: &UiRequest, window, cx| match *request {
                    UiRequest::OpenAnime(id) => this.open_anime(id, window, cx),
                    UiRequest::ShowNowPlaying(id) => {
                        this.open_anime(id, window, cx);
                        this.inspector
                            .update(cx, |inspector, cx| inspector.show_now_playing(cx));
                    }
                },
            ),
            cx.observe_window_bounds(window, |this, window, cx| {
                let geometry = super::geometry_of(window);
                this.model.update(cx, |model, cx| {
                    if model.session.main_window != geometry {
                        model.session.main_window = geometry;
                        model.session_changed(cx);
                    }
                });
            }),
        ];
        let pages = pages::Pages::new(&model, window, cx);
        let inspector = cx.new(|cx| Inspector::new(model.clone(), window, cx));
        Self {
            model,
            page: Page::AnimeList(Some(ListStatus::Watching)),
            back: Vec::new(),
            forward: Vec::new(),
            search,
            pages,
            inspector,
            split: cx.new(|_| ResizableState::default()),
            _subscriptions: subscriptions,
        }
    }

    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if page == self.page {
            return;
        }
        self.back.push(std::mem::replace(&mut self.page, page));
        self.forward.clear();
        self.show(cx);
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.page, page));
            self.show(cx);
        }
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.page, page));
            self.show(cx);
        }
    }

    fn show(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::Library {
            self.model
                .update(cx, |model, cx| model.scan_library_once(cx));
        }
        if let Page::AnimeList(status) = self.page {
            self.pages
                .list
                .update(cx, |list, cx| list.set_status(status, cx));
        }
        cx.notify();
    }

    fn inspector_beside(&self, window: &Window, cx: &Context<Self>) -> bool {
        f32::from(window.viewport_size().width) >= NARROW
            && !self.model.read(cx).session.inspector_hidden
    }

    /// Shows an anime: beside the list when the inspector fits and is on,
    /// as a full page otherwise. From Home, the list of its status opens too,
    /// so the anime is seen among the others.
    fn open_anime(
        &mut self,
        id: shiori_core::AnimeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.inspector_beside(window, cx) {
            self.navigate(Page::Anime(id), cx);
            return;
        }
        if matches!(self.page, Page::Home | Page::Anime(_)) {
            let status = self
                .model
                .read(cx)
                .state()
                .entries
                .get(&id)
                .filter(|e| is_in_list(Some(e)))
                .map(|e| e.status);
            self.navigate(Page::AnimeList(status), cx);
        }
    }

    fn toggle_inspector(&mut self, _: &ToggleInspector, _: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            let hidden = !model.session.inspector_hidden;
            model.set_inspector_hidden(hidden, cx);
        });
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| input.focus(window, cx));
    }

    fn synchronize(&mut self, _: &Synchronize, _: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.synchronize(cx));
    }

    fn play_next(&mut self, _: &PlayNext, _: &mut Window, cx: &mut Context<Self>) {
        self.model
            .update(cx, |model, cx| model.play_selected_next(cx));
    }

    fn play_random(&mut self, _: &PlayRandom, _: &mut Window, cx: &mut Context<Self>) {
        self.model
            .update(cx, |model, cx| model.play_random_anime(cx));
    }

    fn add_library_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add folder".into()),
        });
        let model = self.model.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                model.update(cx, |model, cx| model.add_library_folders(paths, cx));
            }
        })
        .detach();
    }

    fn about(&mut self, _: &About, window: &mut Window, cx: &mut Context<Self>) {
        window.open_alert_dialog(cx, |alert, _, _| {
            alert
                .title(format!("Shiori {}", env!("CARGO_PKG_VERSION")))
                .description(
                    "An anime list manager derived from Taiga.\n\
                     Licensed under the GNU General Public License, version 3 or later.\n\
                     https://github.com/erengy/taiga",
                )
        });
    }

    fn export_markdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let path = cx.prompt_for_new_path(&home, Some("anime-list.md"));
        let model = self.model.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(path))) = path.await {
                model.update(cx, |model, cx| model.export_markdown(path, cx));
            }
        })
        .detach();
    }

    fn export_mal_xml(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn menu(
        model: Entity<Model>,
        view: gpui::WeakEntity<Self>,
        menu: PopupMenu,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let settings = &model.read(cx).state().settings;
        let (sync_on, detection_on) = (settings.sync_enabled, settings.detection_enabled);
        let local = model.read(cx).state().is_local();
        let service = model.read(cx).service_name();
        let m = model.clone();
        let m2 = model.clone();
        let m3 = model.clone();
        let v1 = view.clone();
        let v2 = view.clone();
        // A local list has nothing to synchronize with; Synchronize
        // refreshes its anime data instead.
        let first = if local {
            "Refresh anime data".to_owned()
        } else {
            format!("Synchronize with {service}")
        };
        let menu = menu
            .item(
                PopupMenuItem::new(first)
                    .on_click(move |_, _, cx| m.update(cx, |model, cx| model.synchronize(cx))),
            )
            .separator()
            .item(PopupMenuItem::new("Play next episode").on_click({
                let m = model.clone();
                move |_, _, cx| m.update(cx, |model, cx| model.play_selected_next(cx))
            }))
            .item(PopupMenuItem::new("Play random anime").on_click({
                let m = model.clone();
                move |_, _, cx| m.update(cx, |model, cx| model.play_random_anime(cx))
            }))
            .separator();
        let menu = if local {
            menu
        } else {
            menu.item(
                PopupMenuItem::new("Enable synchronization")
                    .checked(sync_on)
                    .on_click(move |_, _, cx| {
                        m2.update(cx, |model, cx| {
                            model.toggle_setting(|s| s.sync_enabled = !s.sync_enabled, cx)
                        })
                    }),
            )
        };
        menu.item(
            PopupMenuItem::new("Enable detection")
                .checked(detection_on)
                .on_click(move |_, _, cx| {
                    m3.update(cx, |model, cx| {
                        model.toggle_setting(|s| s.detection_enabled = !s.detection_enabled, cx)
                    })
                }),
        )
        .separator()
        .item({
            let m = model.clone();
            PopupMenuItem::new("Scan available episodes")
                .on_click(move |_, _, cx| m.update(cx, |model, cx| model.scan_library(cx)))
        })
        .item({
            let v = view.clone();
            PopupMenuItem::new("Add new folder...").on_click(move |_, window, cx| {
                let _ = v.update(cx, |this, cx| this.add_library_folder(window, cx));
            })
        })
        .separator()
        .item(
            PopupMenuItem::new("Export anime list as Markdown...").on_click(
                move |_, window, cx| {
                    let _ = v1.update(cx, |this, cx| this.export_markdown(window, cx));
                },
            ),
        )
        .item({
            let v = view.clone();
            PopupMenuItem::new("Export anime list as MyAnimeList XML...").on_click(
                move |_, window, cx| {
                    let _ = v.update(cx, |this, cx| this.export_mal_xml(window, cx));
                },
            )
        })
        .separator()
        .item(
            PopupMenuItem::new("Settings...")
                .on_click(move |_, window, cx| pages::settings::open(&model, window, cx)),
        )
        .item(PopupMenuItem::new("About").on_click(move |_, window, cx| {
            let _ = v2.update(cx, |this, cx| this.about(&About, window, cx));
        }))
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.model.read(cx).state();
        let counts: Vec<usize> = ListStatus::SHOWN
            .iter()
            .map(|&status| {
                state
                    .entries
                    .values()
                    .filter(|e| e.status == status && is_in_list(Some(e)))
                    .count()
            })
            .collect();
        let new_count = continue_watching(state).len();
        let account = &state.accounts.anilist;
        let local = state.is_local();
        let (who, how) = if local {
            (
                "On this computer".to_owned(),
                super::list_location::backed_up(self.model.read(cx).last_backup),
            )
        } else if account.username.is_empty() {
            (
                "Not signed in".to_owned(),
                "Add AniList in Settings".to_owned(),
            )
        } else if state.signed_in {
            (account.username.clone(), "AniList · signed in".to_owned())
        } else {
            (account.username.clone(), "AniList".to_owned())
        };
        let theme = cx.theme();
        let (muted, hover, active) = (
            theme.muted_foreground,
            theme.sidebar_accent,
            theme.sidebar_accent,
        );
        let current = self.page;
        let item = |id: &'static str,
                    icon: Icon,
                    label: SharedString,
                    page: Page,
                    suffix: Option<AnyElement>,
                    cx: &mut Context<Self>| {
            let selected = current == page;
            h_flex()
                .id(id)
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |item| {
                    item.bg(active).font_weight(FontWeight::SEMIBOLD)
                })
                .hover(|item| item.bg(hover))
                .child(icon.small())
                .child(div().flex_1().min_w_0().truncate().child(label))
                .children(suffix)
                .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
        };
        let count_label = |n: usize| {
            div()
                .text_xs()
                .text_color(muted)
                .child(n.to_string())
                .into_any_element()
        };
        let status_icons = [
            AppIcon::CirclePlay,
            AppIcon::CircleCheck,
            AppIcon::CirclePause,
            AppIcon::CircleX,
            AppIcon::Bookmark,
        ];
        let status_ids = [
            "watching",
            "completed",
            "on-hold",
            "dropped",
            "plan-to-watch",
        ];
        let mut statuses = Vec::new();
        for (i, (&status, count)) in ListStatus::SHOWN.iter().zip(counts).enumerate() {
            statuses.push(item(
                status_ids[i],
                Icon::new(status_icons[i]),
                pages::status_label(status).into(),
                Page::AnimeList(Some(status)),
                Some(count_label(count)),
                cx,
            ));
        }
        let home_badge = (new_count > 0).then(|| {
            div()
                .px_2()
                .rounded_full()
                .text_xs()
                .font_semibold()
                .bg(style::gradient())
                .text_color(gpui::white())
                .child(new_count.to_string())
                .into_any_element()
        });
        let menu_button = {
            let view = cx.entity().downgrade();
            let model = self.model.clone();
            Button::new("menu")
                .small()
                .ghost()
                .icon(IconName::Menu)
                .tooltip("Menu")
                .dropdown_menu(move |menu, _, cx| Self::menu(model.clone(), view.clone(), menu, cx))
        };
        v_flex()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .px_2()
            .py_3()
            .gap_0p5()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .pb_3()
                    .child(img(style::logo()).size(px(20.)))
                    .child(div().font_bold().child("Shiori")),
            )
            .child(
                div().pb_2().child(
                    Input::new(&self.search)
                        .small()
                        .prefix(Icon::new(IconName::Search).small()),
                ),
            )
            .child(item(
                "home",
                Icon::new(AppIcon::House),
                "Home".into(),
                Page::Home,
                home_badge,
                cx,
            ))
            .child(item(
                "history",
                Icon::new(AppIcon::Clock),
                "History".into(),
                Page::History,
                None,
                cx,
            ))
            .child(item(
                "library",
                Icon::new(IconName::Folder),
                "Library".into(),
                Page::Library,
                None,
                cx,
            ))
            .child(
                div()
                    .px_2()
                    .pt_4()
                    .pb_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(muted)
                    .child("MY LIST"),
            )
            .children(statuses)
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(cx.theme().sidebar_border)
                    .child(
                        div()
                            .size(px(26.))
                            .flex_none()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .map(|avatar| {
                                if local {
                                    avatar
                                        .bg(cx.theme().muted)
                                        .text_color(muted)
                                        .child(Icon::new(AppIcon::House).small())
                                } else {
                                    avatar.bg(style::gradient())
                                }
                            }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_xs().font_semibold().truncate().child(who))
                            .child(div().text_xs().text_color(muted).truncate().child(how)),
                    )
                    .child(menu_button)
                    .child(
                        Button::new("settings")
                            .small()
                            .ghost()
                            .icon(IconName::Settings)
                            .tooltip("Settings")
                            .on_click(cx.listener(|this, _, window, cx| {
                                pages::settings::open(&this.model, window, cx)
                            })),
                    ),
            )
    }

    fn render_status_line(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Built first: it loads posters, which needs the context mutably.
        let playing = now_playing::view(&self.model, cx);
        let model = self.model.read(cx);
        let status = &model.status;
        let service = model.service_name();
        let local = model.state().is_local();
        let theme = cx.theme();
        let color = if status.is_error {
            theme.danger
        } else {
            theme.muted_foreground
        };
        let (dot, state) = if local {
            // Nothing is synchronized, so there is no sync state to report.
            (theme.border, "Local".to_owned())
        } else if status.busy {
            (theme.warning, format!("Synchronizing with {service}"))
        } else if status.is_error {
            (theme.danger, "Not synchronized".to_owned())
        } else {
            (theme.success, service.to_string())
        };
        h_flex()
            .gap_3()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .text_xs()
            // While media is detected the bar shows it instead of the last
            // status message; the sync state stays on the right.
            .map(|bar| match playing {
                Some(playing) => bar.py_1p5().child(playing),
                None => bar.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(color)
                        .child(status.text.clone()),
                ),
            })
            .child(div().size(px(7.)).rounded_full().bg(dot))
            .child(div().text_color(theme.muted_foreground).child(state))
            .child(
                Button::new("sync")
                    .xsmall()
                    .ghost()
                    .icon(IconName::RefreshCw)
                    .tooltip(if local {
                        "Refresh anime data (Ctrl+S)".to_owned()
                    } else {
                        format!("Synchronize with {service} (Ctrl+S)")
                    })
                    .loading(status.busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.synchronize(&Synchronize, window, cx)
                    })),
            )
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let beside = self.inspector_beside(window, cx);
        // A full page of an anime only makes sense while it cannot be shown
        // beside the list; once it can, go back to where the user was.
        let page = match self.page {
            Page::Anime(_) => self.inspector.clone().into_any_element(),
            page => self.pages.render(page, window, cx),
        };
        let width = self.model.read(cx).session.inspector_width.unwrap_or(320.);
        let model = self.model.clone();
        let content: AnyElement = if beside && !matches!(self.page, Page::Anime(_)) {
            h_resizable("main-split")
                .with_state(&self.split)
                .on_resize(move |state, _, cx| {
                    if let Some(width) = state.read(cx).sizes().get(1) {
                        let width = f32::from(*width);
                        model.update(cx, |model, cx| model.set_inspector_width(width, cx));
                    }
                })
                .child(resizable_panel().child(div().size_full().child(page)))
                .child(
                    resizable_panel()
                        .size(px(width))
                        .size_range(px(260.)..px(560.))
                        .child(
                            div()
                                .size_full()
                                .border_l_1()
                                .border_color(cx.theme().border)
                                .child(self.inspector.clone()),
                        ),
                )
                .into_any_element()
        } else {
            div().size_full().child(page).into_any_element()
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::synchronize))
            .on_action(cx.listener(Self::about))
            .on_action(cx.listener(Self::toggle_inspector))
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::play_next))
            .on_action(cx.listener(Self::play_random))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(cx))
                    .child(div().flex_1().min_w_0().h_full().child(content)),
            )
            .child(self.render_status_line(cx))
    }
}
