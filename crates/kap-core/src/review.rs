use crate::diff::FileChange;
use crate::error::{Error, Result};
use crate::model::{new_id, now_rfc3339, Review};
use crate::repo::Repo;

/// A reviewer's verdict on an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The attempt may be merged.
    Approved,
    /// The attempt may not be merged.
    Rejected,
}

impl Verdict {
    /// The value stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Approved => "approved",
            Verdict::Rejected => "rejected",
        }
    }
}

/// What a merge did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    pub attempt_id: String,
    /// The branch the attempt was merged into.
    pub branch: String,
    /// The merge commit.
    pub commit: String,
    /// How many files the attempt changed.
    pub files: usize,
}

impl Repo {
    /// Record a verdict on an attempt, pinned to the commit currently at its branch head.
    pub fn review(&self, attempt_id: &str, verdict: Verdict, note: Option<&str>) -> Result<Review> {
        let db = self.db()?;
        let attempt = db.attempt(attempt_id)?;
        let git = self.git()?;

        let review = Review {
            id: format!("review-{}", new_id()),
            attempt_id: attempt.id,
            verdict: verdict.as_str().to_string(),
            reviewer: git.user_name().unwrap_or_else(|| "unknown".to_string()),
            head_commit: git.branch_head(&attempt.branch)?,
            note: note.map(str::to_string),
            created_at: now_rfc3339(),
        };

        db.insert_review(&review)?;
        Ok(review)
    }

    /// The most recent review of an attempt.
    pub fn latest_review(&self, attempt_id: &str) -> Result<Option<Review>> {
        self.db()?.latest_review(attempt_id)
    }

    /// Files the attempt changed relative to the commit it started from.
    pub fn changes_since_base(&self, attempt_id: &str) -> Result<Vec<FileChange>> {
        let attempt = self.db()?.attempt(attempt_id)?;
        let numstat = self
            .git()?
            .diff_numstat(&attempt.base_commit, &attempt.branch)?;
        Ok(numstat
            .lines()
            .filter_map(crate::diff::parse_numstat_line)
            .collect())
    }

    /// Merge an approved attempt into the branch that is currently checked out.
    ///
    /// The gate is not cosmetic: merging needs a review, the review must approve, and the attempt
    /// must not have moved since. A failed merge is aborted so the working tree is left as it was.
    pub fn merge_attempt(&self, attempt_id: &str) -> Result<MergeOutcome> {
        let db = self.db()?;
        let attempt = db.attempt(attempt_id)?;
        let git = self.git()?;

        let review = db
            .latest_review(attempt_id)?
            .ok_or_else(|| Error::MergeNotReviewed {
                id: attempt_id.to_string(),
            })?;

        if review.verdict != Verdict::Approved.as_str() {
            return Err(Error::MergeRejected {
                id: attempt_id.to_string(),
            });
        }

        if git.branch_head(&attempt.branch)? != review.head_commit {
            return Err(Error::MergeStaleReview {
                id: attempt_id.to_string(),
            });
        }

        if !git.tracked_changes(self.root())?.trim().is_empty() {
            return Err(Error::MergeDirtyWorktree);
        }

        let files = self.changes_since_base(attempt_id)?.len();
        let branch = git.current_branch()?;

        if let Err(error) = git.merge_no_ff(&attempt.branch) {
            let _ = git.merge_abort();
            return Err(error);
        }

        let commit = git.head_commit()?;
        db.close_attempt(&attempt.id, &now_rfc3339())?;

        Ok(MergeOutcome {
            attempt_id: attempt.id,
            branch,
            commit,
            files,
        })
    }
}
