//! The pages shown next to the sidebar.

mod list;
mod search;
mod settings;

use gpui::{
    AnyElement, AppContext as _, Context, Entity, IntoElement, ParentElement, Styled, Window, div,
};
use gpui_component::{ActiveTheme as _, v_flex};
use shiori_core::ListStatus;

use super::main_window::MainWindow;
use super::model::Model;

pub use super::format::status_label;
pub use list::ListPage;
pub use search::SearchPage;
pub use settings::SettingsPage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Search,
    /// `None` shows every status.
    AnimeList(Option<ListStatus>),
    History,
    Library,
    Settings,
}

pub struct Pages {
    pub list: Entity<ListPage>,
    pub search: Entity<SearchPage>,
    pub settings: Entity<SettingsPage>,
}

impl Pages {
    pub fn new(model: &Entity<Model>, window: &mut Window, cx: &mut Context<MainWindow>) -> Self {
        Self {
            list: cx.new(|cx| ListPage::new(model.clone(), window, cx)),
            search: cx.new(|cx| SearchPage::new(model.clone(), window, cx)),
            settings: cx.new(|cx| SettingsPage::new(model.clone(), window, cx)),
        }
    }

    pub fn render(&self, page: Page, _: &mut Window, cx: &mut Context<MainWindow>) -> AnyElement {
        match page {
            Page::AnimeList(_) => self.list.clone().into_any_element(),
            Page::Search => self.search.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
            Page::Home => placeholder("Home", "What is playing now will show here.", cx),
            Page::History => placeholder("History", "Watched episodes will show here.", cx),
            Page::Library => placeholder("Library", "Library folders will show here.", cx),
        }
    }
}

fn placeholder(title: &str, text: &str, cx: &mut Context<MainWindow>) -> AnyElement {
    v_flex()
        .p_4()
        .gap_2()
        .child(div().text_xl().child(title.to_owned()))
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(text.to_owned()),
        )
        .into_any_element()
}
