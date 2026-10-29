use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::model::{new_id, now_rfc3339, Attempt, AttemptState};
use crate::repo::Repo;

/// Parameters for [`Repo::start_attempt`].
#[derive(Debug, Clone, Copy)]
pub struct NewAttempt<'a> {
    /// Short, human-readable name for the attempt.
    pub label: &'a str,
    /// What the agent is trying to achieve.
    pub task: Option<&'a str>,
    /// Commit or ref to branch from. Defaults to `HEAD`.
    pub base: Option<&'a str>,
}

impl Repo {
    /// Start an attempt: give it a branch and a worktree, then record it.
    ///
    /// The worktree is created before the record is written; if the record cannot be written, the
    /// worktree is removed again so a failed start leaves no trace.
    pub fn start_attempt(&self, new: NewAttempt<'_>) -> Result<Attempt> {
        let git = self.git()?;
        let base_commit = match new.base {
            Some(rev) => git.resolve_commit(rev)?,
            None => git.head_commit()?,
        };

        let id = format!("attempt-{}", new_id());
        let branch = format!("kap/{id}");
        let worktree: PathBuf = self.kap_dir().join("worktrees").join(&id);

        if worktree.exists() {
            return Err(Error::WorktreeExists { path: worktree });
        }

        git.add_worktree(&worktree, &branch, &base_commit)?;

        let attempt = Attempt {
            id,
            label: new.label.to_string(),
            task: new.task.map(str::to_string),
            state: AttemptState::Open,
            branch,
            worktree: worktree.clone(),
            base_commit,
            created_at: now_rfc3339(),
            closed_at: None,
        };

        if let Err(error) = self.db()?.insert_attempt(&attempt) {
            let _ = git.remove_worktree(&worktree, true);
            return Err(error);
        }

        Ok(attempt)
    }

    /// All attempts, newest first.
    pub fn attempts(&self) -> Result<Vec<Attempt>> {
        self.db()?.attempts()
    }
}
