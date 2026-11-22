//! What all services' score menus have in common.

/// One choice in the score menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rating {
    /// The stored score, 0 to 100.
    pub value: u8,
    pub label: String,
}
