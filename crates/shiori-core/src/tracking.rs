//! Automatic list updates: when an update is applied.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UpdateTrigger {
    /// Update once the delay has passed.
    #[default]
    AfterDelay,
    /// Wait for the media to close after the delay has passed.
    OnPlayerClose,
}
