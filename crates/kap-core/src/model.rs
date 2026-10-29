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

/// One observable action inside an attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolEvent {
    pub id: String,
    pub attempt_id: String,
    pub run_id: Option<String>,
    pub kind: String,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout_hash: Option<String>,
    pub stderr_hash: Option<String>,
    pub observed: bool,
    pub occurred_at: String,
    /// Whether secrets were removed from the command or its output before storing.
    pub redacted: bool,
    /// Set when a Claude Code subagent made this call rather than the main agent.
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
}

/// A choice an agent committed to, with the reasoning behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub id: String,
    pub attempt_id: String,
    pub summary: String,
    pub rationale: Option<String>,
    pub alternatives: Option<String>,
    pub source: String,
    pub created_at: String,
}

/// A time-ordered, lowercase identifier, e.g. `01m63tv077jys74f9v4g7q93nm`.
pub(crate) fn new_id() -> String {
    ulid::Ulid::generate().to_string().to_lowercase()
}

/// Observed support for or against a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub id: String,
    pub attempt_id: String,
    pub decision_id: Option<String>,
    pub source: String,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub artifact_hash: Option<String>,
    /// The file this evidence is about, when it is about one.
    pub path: Option<String>,
    /// Whether KAP saw the supporting bytes itself, rather than being told about them.
    pub observed: bool,
    /// Whether the evidence is whole; an agent's report is not.
    pub complete: bool,
    pub recorded_at: String,
}

/// One execution of an agent harness inside an attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub id: String,
    pub attempt_id: String,
    pub session_id: Option<String>,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// A verdict on an attempt, tied to the exact commit it looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub id: String,
    pub attempt_id: String,
    pub verdict: String,
    pub reviewer: String,
    pub head_commit: String,
    pub note: Option<String>,
    pub created_at: String,
}

/// A restorable state of an attempt at a point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub id: String,
    pub attempt_id: String,
    pub commit: String,
    pub note: Option<String>,
    pub created_at: String,
}
