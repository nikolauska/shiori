//! Small pieces shared by several views.

use gpui::SharedString;
use gpui_component::searchable_list::SearchableListItem;

/// A dropdown item that shows a label and stands for a value.
#[derive(Debug, Clone)]
pub struct Choice<T> {
    pub label: SharedString,
    pub value: T,
}

impl<T> Choice<T> {
    pub fn new(label: impl Into<SharedString>, value: T) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

impl<T: Clone + PartialEq> SearchableListItem for Choice<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &T {
        &self.value
    }
}
