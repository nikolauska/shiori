//! The main window: toolbar, sidebar, the current page and the status bar.

use gpui::{
    AnyElement, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div, prelude::*,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem};
use gpui_component::{ActiveTheme as _, Disableable as _, IconName, h_flex, v_flex};
use shiori_core::ListStatus;
use shiori_core::entry::is_in_list;

use super::model::Model;
use super::pages::{self, Page};
use super::{GoBack, GoForward, Synchronize};

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
            .child(
                Button::new("settings")
                    .icon(IconName::Settings)
                    .ghost()
                    .tooltip("Settings")
                    .on_click(cx.listener(|this, _, _, cx| this.navigate(Page::Settings, cx))),
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
