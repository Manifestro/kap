use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors raised by the KAP core.
#[derive(Debug, Error)]
pub enum Error {
    #[error("`{path}` is not inside a git repository")]
    NotAGitRepository { path: PathBuf },

    #[error("could not run git; is it installed and on PATH?")]
    GitMissing(#[source] std::io::Error),

    #[error("git command failed: git {command}\n{stderr}")]
    Git { command: String, stderr: String },

    #[error("KAP is already initialized in `{path}`")]
    AlreadyInitialized { path: PathBuf },

    #[error("KAP is not initialized in `{path}`; run `kap init`")]
    NotInitialized { path: PathBuf },

    #[error("filesystem error at `{path}`")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("database error at `{path}`")]
    Db {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("attempt `{id}` not found")]
    AttemptNotFound { id: String },

    #[error("worktree `{path}` already exists")]
    WorktreeExists { path: PathBuf },

    #[error("attempt `{id}` has not been reviewed; run `kap review {id} --approve`")]
    MergeNotReviewed { id: String },

    #[error("the latest review of attempt `{id}` rejected it")]
    MergeRejected { id: String },

    #[error("attempt `{id}` changed after it was approved; review the new commits")]
    MergeStaleReview { id: String },

    #[error("the working tree has uncommitted changes; commit or stash them before merging")]
    MergeDirtyWorktree,

    #[error("decision `{id}` not found for attempt `{attempt}`")]
    DecisionNotFound { id: String, attempt: String },

    #[error("attempt `{id}` is still open; close it before cleaning up")]
    AttemptOpen { id: String },

    #[error("snapshot `{id}` not found")]
    SnapshotNotFound { id: String },

    #[error("attempt `{id}` is closed; its history is frozen")]
    AttemptClosed { id: String },

    #[error("no command given")]
    EmptyCommand,

    #[error("could not read a sync row: {reason}")]
    SyncRow { reason: String },

    #[error("could not parse hook input")]
    HookPayload {
        #[source]
        source: serde_json::Error,
    },

    #[error("`{address}` is not an object address (expected `blake3:<hex>`)")]
    InvalidObjectAddress { address: String },
}

impl Error {
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Error {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    pub(crate) fn db(path: &Path, source: rusqlite::Error) -> Error {
        Error::Db {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// Result type used throughout the core.
pub type Result<T> = std::result::Result<T, Error>;
