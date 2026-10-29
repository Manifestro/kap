use std::fs;

use crate::attempt::NewAttempt;
use crate::error::{Error, Result};
use crate::model::{new_id, now_rfc3339, Attempt, AttemptState, Evidence, Snapshot};
use crate::repo::Repo;

impl Repo {
    /// Capture the worktree as a snapshot and return it.
    ///
    /// The snapshot is a commit object of the whole working tree — tracked and untracked files
    /// alike — written without touching the attempt's branch or index. Taking a snapshot therefore
    /// never moves the attempt's head and never invalidates a review.
    pub fn create_snapshot(&self, attempt_id: &str, note: Option<&str>) -> Result<Snapshot> {
        let attempt = self.db()?.attempt(attempt_id)?;
        let git = self.git()?;
        let id = format!("snapshot-{}", new_id());

        let tmp_dir = self.kap_dir().join("tmp");
        fs::create_dir_all(&tmp_dir).map_err(|error| Error::io(&tmp_dir, error))?;
        let index_path = tmp_dir.join(format!("index-{id}"));
        let index = index_path.to_string_lossy().to_string();
        let env = [("GIT_INDEX_FILE", index.as_str())];

        let worktree = attempt.worktree.clone();
        git.run_in_with_env(&worktree, &["read-tree", "HEAD"], &env)?;
        git.run_in_with_env(&worktree, &["add", "-A"], &env)?;
        let tree = git
            .run_in_with_env(&worktree, &["write-tree"], &env)?
            .trim()
            .to_string();

        let message = match note {
            Some(note) => format!("KAP snapshot {id}: {note}"),
            None => format!("KAP snapshot {id}"),
        };
        let commit = git
            .run_in_with_env(
                &worktree,
                &[
                    "-c",
                    "user.name=KAP",
                    "-c",
                    "user.email=kap@localhost",
                    "commit-tree",
                    &tree,
                    "-p",
                    "HEAD",
                    "-m",
                    &message,
                ],
                &[],
            )?
            .trim()
            .to_string();

        git.update_ref(&format!("refs/kap/snapshots/{id}"), &commit)?;
        let _ = fs::remove_file(&index_path);

        let snapshot = Snapshot {
            id,
            attempt_id: attempt_id.to_string(),
            commit,
            note: note.map(str::to_string),
            created_at: now_rfc3339(),
        };
        self.db()?.insert_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    /// Snapshots of an attempt, oldest first.
    pub fn snapshots(&self, attempt_id: &str) -> Result<Vec<Snapshot>> {
        self.db()?.snapshots(attempt_id)
    }

    /// Restore a snapshot as a new attempt, leaving the original untouched.
    ///
    /// Restoring by overwriting the original worktree would destroy work that is not in the
    /// snapshot. Branching a fresh attempt from the snapshot keeps both.
    pub fn restore_snapshot(&self, snapshot_id: &str) -> Result<Attempt> {
        let db = self.db()?;
        let snapshot = db.snapshot(snapshot_id)?;
        let original = db.attempt(&snapshot.attempt_id)?;
        let label = format!("{}-restored", original.label);

        self.start_attempt(NewAttempt {
            label: &label,
            task: original.task.as_deref(),
            base: Some(&snapshot.commit),
        })
    }

    /// Remove the worktree of a closed attempt.
    ///
    /// The branch is kept: it is the record of what the attempt did, and removing it would make
    /// reviewing or re-merging impossible.
    pub fn clean_attempt(&self, attempt_id: &str) -> Result<()> {
        let attempt = self.db()?.attempt(attempt_id)?;
        if attempt.state != AttemptState::Closed {
            return Err(Error::AttemptOpen { id: attempt.id });
        }

        if attempt.worktree.exists() {
            self.git()?.remove_worktree(&attempt.worktree, true)?;
        }
        Ok(())
    }

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
