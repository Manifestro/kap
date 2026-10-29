use std::fmt;
use std::path::PathBuf;

/// The lifecycle state of an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptState {
    /// The attempt is in progress.
    Open,
    /// The attempt has been closed and its history frozen.
    Closed,
}

impl AttemptState {
    /// The value stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptState::Open => "open",
            AttemptState::Closed => "closed",
        }
    }

    /// Parse a value read from the database.
    pub fn from_db(value: &str) -> Option<AttemptState> {
        match value {
            "open" => Some(AttemptState::Open),
            "closed" => Some(AttemptState::Closed),
            _ => None,
        }
    }
}

impl fmt::Display for AttemptState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One attempt to solve a task.
///
/// An attempt may span several runs. It owns a branch and a worktree, and it is the unit of
/// comparison, snapshotting, and merging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub id: String,
    pub label: String,
    pub task: Option<String>,
    pub state: AttemptState,
    pub branch: String,
    pub worktree: PathBuf,
    pub base_commit: String,
    pub created_at: String,
    pub closed_at: Option<String>,
}

/// The current time as an RFC 3339 timestamp, in UTC.
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("the current time is always formattable as RFC 3339")
}
