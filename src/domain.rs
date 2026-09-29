use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadId(pub String);

impl ThreadId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeStatus {
    Working,
    WaitingHuman,
    Ready,
    Inactive,
}

impl RuntimeStatus {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Working => "WORKING",
            Self::WaitingHuman => "WAITING",
            Self::Ready => "READY",
            Self::Inactive => "IDLE",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttentionReason {
    ApprovalRequired,
    UserInputRequired,
    ReadyForReview,
    MarkedUnread,
}

impl AttentionReason {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval",
            Self::UserInputRequired => "input",
            Self::ReadyForReview => "review",
            Self::MarkedUnread => "unread",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub id: ThreadId,
    pub workspace: String,
    pub title: String,
    pub runtime: RuntimeStatus,
    pub attention: Vec<AttentionReason>,
    pub pinned: bool,
    pub alias: Option<String>,
}

impl ThreadSummary {
    pub fn display_title(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.title)
    }

    pub fn needs_attention(&self) -> bool {
        !self.attention.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadUiState {
    pub draft: String,
    pub scroll: u16,
    pub follow: bool,
}

impl Default for ThreadUiState {
    fn default() -> Self {
        Self {
            draft: String::new(),
            scroll: 0,
            follow: true,
        }
    }
}
