//! The pages shown next to the sidebar.

mod history;
mod home;
mod list;
mod search;
pub mod settings;

use gpui::{
    AnyElement, AppContext as _, Context, Entity, IntoElement, ParentElement, Styled, Window, div,
};
use gpui_component::{ActiveTheme as _, v_flex};
use shiori_core::{AnimeId, ListStatus};

use super::main_window::MainWindow;
use super::model::Model;

pub use super::format::status_label;
pub use history::HistoryPage;
pub use home::HomePage;
pub use list::ListPage;
pub use search::SearchPage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Search,
    /// `None` shows every status.
    AnimeList(Option<ListStatus>),
    History,
    Library,
    /// One anime filling the page, when the inspector does not fit beside
    /// the list or is hidden.
    Anime(AnimeId),
}

pub struct Pages {
    pub home: Entity<HomePage>,
    pub list: Entity<ListPage>,
    pub history: Entity<HistoryPage>,
    pub search: Entity<SearchPage>,
}

impl Pages {
    pub fn new(model: &Entity<Model>, window: &mut Window, cx: &mut Context<MainWindow>) -> Self {
        Self {
            home: cx.new(|cx| HomePage::new(model.clone(), window, cx)),
            list: cx.new(|cx| ListPage::new(model.clone(), window, cx)),
            history: cx.new(|cx| HistoryPage::new(model.clone(), window, cx)),
            search: cx.new(|cx| SearchPage::new(model.clone(), window, cx)),
        }
    }

    pub fn render(&self, page: Page, _: &mut Window, cx: &mut Context<MainWindow>) -> AnyElement {
        match page {
            Page::AnimeList(_) => self.list.clone().into_any_element(),
            Page::Search => self.search.clone().into_any_element(),
            Page::Home => self.home.clone().into_any_element(),
            Page::History => self.history.clone().into_any_element(),
            Page::Library => placeholder(
                "Library",
                "Your anime folders will show here once the library is built.",
                cx,
            ),
            // The main window draws the inspector for this page itself.
            Page::Anime(_) => div().into_any_element(),
        }
    }
}

fn placeholder(title: &str, text: &str, cx: &mut Context<MainWindow>) -> AnyElement {
    v_flex()
        .px_4()
        .pt_3()
        .gap_2()
        .child(div().text_lg().child(title.to_owned()))
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(text.to_owned()),
        )
        .into_any_element()
}
