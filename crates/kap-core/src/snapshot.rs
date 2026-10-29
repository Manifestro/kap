use std::fs;
use std::path::Path;

use crate::attempt::NewAttempt;
use crate::error::{Error, Result};
use crate::git::Git;
use crate::model::{new_id, now_rfc3339, Attempt, AttemptState, Evidence, Snapshot};
use crate::repo::Repo;

/// Per-file and per-snapshot limits, so a runaway log or a checkpoint cannot exhaust memory or
/// fill the store on every Stop.
const MAX_SNAPSHOT_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SNAPSHOT_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// What one snapshot pass recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotReport {
    pub recorded: usize,
    /// Files left out because they were too large.
    pub skipped: usize,
}

/// What `clean_attempt` removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanOutcome {
    pub worktree_removed: bool,
    /// Whether the branch was merged and has been deleted.
    pub branch_deleted: bool,
}

impl Repo {
    /// Capture the worktree as a snapshot and return it.
    ///
    /// The snapshot is a commit object of the whole working tree — tracked and untracked files
    /// alike — written without touching the attempt's branch or index. Taking a snapshot therefore
    /// never moves the attempt's head and never invalidates a review.
    pub fn create_snapshot(
        &self,
        attempt_id: &str,
        note: Option<&str>,
    ) -> Result<(Snapshot, usize)> {
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

        // A snapshot must not swallow a checkpoint or a log. Files over the limit stay in the
        // worktree, outside the snapshot, and are counted so the caller can say so.
        let skipped = oversized_paths(&git, &worktree)?;
        let mut add: Vec<String> = Vec::new();
        if !skipped.is_empty() {
            let excludes = tmp_dir.join("snapshot-excludes");
            fs::write(&excludes, skipped.join("\n")).map_err(|e| Error::io(&excludes, e))?;
            add.push("-c".to_string());
            add.push(format!("core.excludesFile={}", excludes.display()));
        }
        add.push("add".to_string());
        add.push("-A".to_string());
        let add_args: Vec<&str> = add.iter().map(String::as_str).collect();
        git.run_in_with_env(&worktree, &add_args, &env)?;
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
        Ok((snapshot, skipped.len()))
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

    /// Remove the worktree of a closed attempt, and its branch once that branch is merged.
    ///
    /// A merged branch is safe to delete — its commits are reachable from the main line. An
    /// unmerged one is kept, because it is the only record of that work.
    pub fn clean_attempt(&self, attempt_id: &str, force: bool) -> Result<CleanOutcome> {
        let attempt = self.db()?.attempt(attempt_id)?;
        if attempt.state != AttemptState::Closed {
            return Err(Error::AttemptOpen { id: attempt.id });
        }

        let git = self.git()?;

        // `git worktree remove --force` deletes everything, including files git never tracked:
        // reports, checkpoints, model runs. Refuse unless the caller says so explicitly.
        if !force && attempt.worktree.exists() {
            let leftovers = git.status_porcelain_ignored(&attempt.worktree)?;
            let count = leftovers
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count();
            if count > 0 {
                return Err(Error::CleanRefused { count });
            }
        }

        let worktree_removed = attempt.worktree.exists();
        if worktree_removed {
            git.remove_worktree(&attempt.worktree, true)?;
        }

        let branch_deleted =
            git.branch_exists(&attempt.branch)? && git.is_ancestor(&attempt.branch, "HEAD")?;
        if branch_deleted {
            git.delete_branch(&attempt.branch)?;
        }

        Ok(CleanOutcome {
            worktree_removed,
            branch_deleted,
        })
    }

    /// Paths changed but not committed in the attempt's worktree, untracked files included.
    pub fn worktree_changes(&self, attempt_id: &str) -> Result<Vec<String>> {
        let attempt = self.db()?.attempt(attempt_id)?;
        let status = self.git()?.status_porcelain(&attempt.worktree)?;
        Ok(status
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// Record the worktree's changed files as evidence and return how many were new.
    ///
    /// Agents edit files through many tools, including plain shell commands. A snapshot at the end
    /// of a turn makes sure the resulting file contents are recorded either way. A file whose
    /// content was already recorded for this attempt is skipped, so repeated snapshots do not pile
    /// up duplicates.
    pub fn record_worktree_snapshot(&self, attempt: &Attempt) -> Result<SnapshotReport> {
        let git = self.git()?;
        let status = git.status_porcelain_all(&attempt.worktree)?;
        let objects = self.objects();
        let db = self.db()?;
        let mut recorded = 0;
        let mut skipped = 0;
        let mut budget = MAX_SNAPSHOT_TOTAL_BYTES;

        for line in status.lines() {
            let Some(path) = status_path(line) else {
                continue;
            };

            let full = attempt.worktree.join(&path);
            let Ok(metadata) = std::fs::metadata(&full) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            if metadata.len() > MAX_SNAPSHOT_FILE_BYTES || metadata.len() > budget {
                skipped += 1;
                continue;
            }

            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            budget = budget.saturating_sub(metadata.len());

            let hash = objects.put(&bytes)?;
            // Same content, however it got here: the hook may have recorded this file under a
            // different path, and a snapshot must not duplicate it.
            if db.evidence_has_hash(&attempt.id, &hash)? {
                continue;
            }
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

        Ok(SnapshotReport { recorded, skipped })
    }
}

/// The path from one `git status --porcelain` line, following renames to the new path.
/// Files in the worktree that are too large to belong in a snapshot.
fn oversized_paths(git: &Git, worktree: &Path) -> Result<Vec<String>> {
    let mut paths = Vec::new();

    for line in git.status_porcelain_all(worktree)?.lines() {
        let Some(path) = status_path(line) else {
            continue;
        };

        if let Ok(metadata) = fs::metadata(worktree.join(&path)) {
            if metadata.is_file() && metadata.len() > MAX_SNAPSHOT_FILE_BYTES {
                paths.push(path);
            }
        }
    }

    Ok(paths)
}

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
