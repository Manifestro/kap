use crate::error::Result;
use crate::model::Attempt;
use crate::repo::Repo;

/// One file changed between two attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    /// Lines added, or `None` for a binary file.
    pub added: Option<u64>,
    /// Lines removed, or `None` for a binary file.
    pub removed: Option<u64>,
}

/// What changed between two attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptDiff {
    pub from: Attempt,
    pub to: Attempt,
    pub files: Vec<FileChange>,
}

impl AttemptDiff {
    /// Total lines added and removed across all files.
    pub fn totals(&self) -> (u64, u64) {
        let added = self.files.iter().filter_map(|file| file.added).sum();
        let removed = self.files.iter().filter_map(|file| file.removed).sum();
        (added, removed)
    }

    /// Whether the attempts produced no file changes at all.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Repo {
    /// Compare two attempts by diffing their branches.
    ///
    /// The diff is between the attempts' final states, not between their bases: it answers "what
    /// does this attempt offer that the other does not", which is the question a reviewer asks
    /// before merging.
    pub fn diff_attempts(&self, from_id: &str, to_id: &str) -> Result<AttemptDiff> {
        let db = self.db()?;
        let from = db.attempt(from_id)?;
        let to = db.attempt(to_id)?;

        let numstat = self.git()?.diff_numstat(&from.branch, &to.branch)?;
        let files = numstat.lines().filter_map(parse_numstat_line).collect();

        Ok(AttemptDiff { from, to, files })
    }
}

/// Parse one `added\tremoved\tpath` line. Binary files report `-` instead of counts.
fn parse_numstat_line(line: &str) -> Option<FileChange> {
    let mut parts = line.splitn(3, '\t');
    let added = parts.next()?;
    let removed = parts.next()?;
    let path = parts.next()?.to_string();

    Some(FileChange {
        path,
        added: added.parse().ok(),
        removed: removed.parse().ok(),
    })
}
