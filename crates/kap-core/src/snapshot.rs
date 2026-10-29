use crate::error::Result;
use crate::model::{new_id, now_rfc3339, Attempt, Evidence};
use crate::repo::Repo;

impl Repo {
    /// Record the worktree's changed files as evidence and return how many were new.
    ///
    /// Agents edit files through many tools, including plain shell commands. A snapshot at the end
    /// of a turn makes sure the resulting file contents are recorded either way. A file whose
    /// content was already recorded for this attempt is skipped, so repeated snapshots do not pile
    /// up duplicates.
    pub fn record_worktree_snapshot(&self, attempt: &Attempt) -> Result<usize> {
        let git = self.git()?;
        let status = git.status_porcelain(&attempt.worktree)?;
        let objects = self.objects();
        let db = self.db()?;
        let mut recorded = 0;

        for line in status.lines() {
            let Some(path) = status_path(line) else {
                continue;
            };

            let full = attempt.worktree.join(&path);
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };

            let hash = objects.put(&bytes)?;
            if db.last_evidence_hash(&attempt.id, &path)?.as_deref() == Some(hash.as_str()) {
                continue;
            }

            db.insert_evidence(&Evidence {
                id: format!("evidence-{}", new_id()),
                attempt_id: attempt.id.clone(),
                decision_id: None,
                source: "file_change".to_string(),
                command: None,
                exit_code: None,
                artifact_hash: Some(hash),
                path: Some(path),
                observed: true,
                complete: true,
                recorded_at: now_rfc3339(),
            })?;
            recorded += 1;
        }

        Ok(recorded)
    }
}

/// The path from one `git status --porcelain` line, following renames to the new path.
fn status_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }

    let rest = &line[3..];
    let path = match rest.split_once(" -> ") {
        Some((_, new_path)) => new_path,
        None => rest,
    };

    let path = path.trim().trim_matches('"');
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}
