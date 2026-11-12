//! The main window: toolbar, sidebar, the current page and the status bar.

use gpui::{
    AnyElement, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div, prelude::*,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, WindowExt as _, h_flex, v_flex,
};
use shiori_core::ListStatus;
use shiori_core::entry::is_in_list;

use super::model::Model;
use super::pages::{self, Page};
use super::{About, GoBack, GoForward, Synchronize};

pub struct MainWindow {
    model: Entity<Model>,
    page: Page,
    back: Vec<Page>,
    forward: Vec<Page>,
    search: Entity<InputState>,
    pages: pages::Pages,
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search anime..."));
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
        ];
        let mut subscriptions = subscriptions;
        subscriptions.push(cx.observe_window_bounds(window, |this, window, cx| {
            let geometry = super::geometry_of(window);
            this.model.update(cx, |model, cx| {
                if model.session.main_window != geometry {
                    model.session.main_window = geometry;
                    model.session_changed(cx);
                }
            });
        }));
        let pages = pages::Pages::new(&model, window, cx);
        Self {
            model,
            page: Page::AnimeList(Some(ListStatus::Watching)),
            back: Vec::new(),
            forward: Vec::new(),
            search,
            pages,
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
        if let Page::AnimeList(status) = self.page {
            self.pages
                .list
                .update(cx, |list, cx| list.set_status(status, cx));
        }
        cx.notify();
    }

    fn synchronize(&mut self, _: &Synchronize, _: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.synchronize(cx));
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

    fn menu(
        model: Entity<Model>,
        view: gpui::WeakEntity<Self>,
        menu: PopupMenu,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let settings = &model.read(cx).state().settings;
        let (sync_on, detection_on) = (settings.sync_enabled, settings.detection_enabled);
        let service = model.read(cx).service_name();
        let m = model.clone();
        let m2 = model.clone();
        let m3 = model.clone();
        let v1 = view.clone();
        let v2 = view.clone();
        menu.item(
            PopupMenuItem::new(format!("Synchronize with {service}"))
                .on_click(move |_, _, cx| m.update(cx, |model, cx| model.synchronize(cx))),
        )
        .separator()
        .item(
            PopupMenuItem::new("Enable synchronization")
                .checked(sync_on)
                .on_click(move |_, _, cx| {
                    m2.update(cx, |model, cx| {
                        model.toggle_setting(|s| s.sync_enabled = !s.sync_enabled, cx)
                    })
                }),
        )
        .item(
            PopupMenuItem::new("Enable detection")
                .checked(detection_on)
                .on_click(move |_, _, cx| {
                    m3.update(cx, |model, cx| {
                        model.toggle_setting(|s| s.detection_enabled = !s.detection_enabled, cx)
                    })
                }),
        )
        .separator()
        .item(
            PopupMenuItem::new("Export anime list as Markdown...").on_click(
                move |_, window, cx| {
                    let _ = v1.update(cx, |this, cx| this.export_markdown(window, cx));
                },
            ),
        )
        .separator()
        .item(
            PopupMenuItem::new("Settings...")
                .on_click(move |_, window, cx| pages::settings::open(&model, window, cx)),
        )
        .item(PopupMenuItem::new("About").on_click(move |_, window, cx| {
            let _ = v2.update(cx, |this, cx| this.about(&About, window, cx));
        }))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let service = self.model.read(cx).service_name();
        h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("back")
                    .icon(IconName::ArrowLeft)
                    .ghost()
                    .tooltip("Back (Alt+Left)")
                    .disabled(self.back.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.go_back(&GoBack, window, cx))),
            )
            .child(
                Button::new("forward")
                    .icon(IconName::ArrowRight)
                    .ghost()
                    .tooltip("Forward (Alt+Right)")
                    .disabled(self.forward.is_empty())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.go_forward(&GoForward, window, cx)),
                    ),
            )
            .child(div().flex_1().max_w_96().child(Input::new(&self.search)))
            .child(div().flex_1())
            .child(
                Button::new("sync")
                    .icon(IconName::RefreshCw)
                    .ghost()
                    .tooltip(format!("Synchronize with {service} (Ctrl+S)"))
                    .loading(self.model.read(cx).status.busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.synchronize(&Synchronize, window, cx)
                    })),
            )
            .child({
                let view = cx.entity().downgrade();
                let model = self.model.clone();
                Button::new("menu")
                    .icon(IconName::Menu)
                    .ghost()
                    .tooltip("Menu")
                    .dropdown_menu(move |menu, _, cx| {
                        Self::menu(model.clone(), view.clone(), menu, cx)
                    })
            })
            .child(
                Button::new("settings")
                    .icon(IconName::Settings)
                    .ghost()
                    .tooltip("Settings")
                    .on_click(cx.listener(|this, _, window, cx| {
                        pages::settings::open(&this.model, window, cx)
                    })),
            )
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
        let item = |label: String, page: Page, current: &Page, cx: &mut Context<Self>| {
            SidebarMenuItem::new(label)
                .active(*current == page)
                .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
        };
        let current = self.page;
        let statuses: Vec<SidebarMenuItem> = ListStatus::SHOWN
            .iter()
            .zip(counts)
            .map(|(&status, count)| {
                let label = format!("{} ({count})", pages::status_label(status));
                item(label, Page::AnimeList(Some(status)), &current, cx)
            })
            .collect();
        Sidebar::new("sidebar").collapsible(false).child(
            SidebarMenu::new()
                .child(item("Home".into(), Page::Home, &current, cx))
                .child(item("Search".into(), Page::Search, &current, cx))
                .child(
                    item("Anime list".into(), Page::AnimeList(None), &current, cx)
                        .default_open(true)
                        .children(statuses),
                )
                .child(item("History".into(), Page::History, &current, cx))
                .child(item("Library".into(), Page::Library, &current, cx)),
        )
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let status = &self.model.read(cx).status;
        let color = if status.is_error {
            cx.theme().danger
        } else {
            cx.theme().muted_foreground
        };
        h_flex()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .text_sm()
            .text_color(color)
            .child(status.text.clone())
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page: AnyElement = self.pages.render(self.page, window, cx);
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::synchronize))
            .on_action(cx.listener(Self::about))
            .child(self.render_toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(cx))
                    .child(div().flex_1().min_w_0().h_full().child(page)),
            )
            .child(self.render_status_bar(cx))
    }
}
