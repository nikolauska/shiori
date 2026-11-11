//! The search page. Filled in with the search work; for now it remembers
//! the text from the toolbar.

use gpui::{Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::v_flex;

use crate::ui::model::Model;

pub struct SearchPage {
    _model: Entity<Model>,
    text: String,
}

impl SearchPage {
    pub fn new(model: Entity<Model>, _: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            _model: model,
            text: String::new(),
        }
    }

    pub fn search(&mut self, text: String, _: &mut Window, cx: &mut Context<Self>) {
        self.text = text;
        cx.notify();
    }
}

impl Render for SearchPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .p_4()
            .gap_2()
            .child(div().text_xl().child("Search"))
            .child(format!("Searching for: {}", self.text))
    }
}
