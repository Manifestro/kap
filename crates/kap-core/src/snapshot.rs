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

/// Above this size a changed file keeps the content of its first and its latest version only. A
/// log rewritten on every turn would otherwise be copied whole on every Stop; the versions in
/// between keep their hash.
const MAX_HISTORY_FILE_BYTES: u64 = 1024 * 1024;

/// What one snapshot pass recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotReport {
    pub recorded: usize,
    /// Files left out because they were too large.
    pub skipped: usize,
    /// Earlier versions of large files whose content was dropped in favour of the latest.
    pub hash_only: usize,
}

/// Files a snapshot left in the worktree instead of writing them into git.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LeftOut {
    /// Over the size limit.
    pub oversized: usize,
    /// Holding something shaped like a credential. Git objects cannot be redacted after the
    /// fact, and snapshot refs travel with `kap push`, so these stay out entirely.
    pub credentials: usize,
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
    ) -> Result<(Snapshot, LeftOut)> {
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

        // A snapshot must not swallow a checkpoint, a log, or a key. Such files stay in the
        // worktree, outside the snapshot, and are counted so the caller can say so.
        let config = self.worktree_config()?;
        let (mut excluded, left_out) = excluded_paths(&git, &worktree)?;
        // Shared links point into the main checkout; a snapshot holds the attempt's own files.
        for path in &config.links {
            if fs::symlink_metadata(worktree.join(path)).is_ok_and(|m| m.file_type().is_symlink()) {
                excluded.push(path.clone());
            }
        }
        // Exclusions go in as literal pathspecs: they also hold back a tracked file's new
        // content, and a path with glob characters in it means only itself.
        let pathspecs = tmp_dir.join(format!("pathspecs-{id}"));
        let mut list = String::from(".\0");
        for path in &excluded {
            list.push_str(&format!(":(exclude,literal){path}\0"));
        }
        fs::write(&pathspecs, list).map_err(|e| Error::io(&pathspecs, e))?;
        let from_file = format!("--pathspec-from-file={}", pathspecs.display());
        git.run_in_with_env(
            &worktree,
            &["add", "-A", &from_file, "--pathspec-file-nul"],
            &env,
        )?;
        let _ = fs::remove_file(&pathspecs);
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
        Ok((snapshot, left_out))
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
        let config = self.worktree_config()?;

        // `git worktree remove --force` deletes everything, including files git never tracked:
        // reports, checkpoints, model runs. Refuse unless the caller says so explicitly. Shared
        // links and disposable paths from `.kap/worktree` are KAP's own and do not count.
        if !force && attempt.worktree.exists() {
            let leftovers = git.status_porcelain_ignored(&attempt.worktree)?;
            let count = leftovers
                .lines()
                .filter_map(status_path)
                .filter(|path| !config.owns(path))
                .count();
            if count > 0 {
                return Err(Error::CleanRefused { count });
            }
        }

        let worktree_removed = attempt.worktree.exists();
        if worktree_removed {
            // Unlink before deleting, so nothing can follow a link into the main checkout.
            self.unlink_shared(&attempt, &config)?;
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
        let mut hash_only = 0;
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

            let (bytes, _) = crate::redact::bytes(&bytes);
            let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
            // Same content, however it got here: the hook may have recorded this file under a
            // different path, and a snapshot must not duplicate it.
            if db.evidence_has_hash(&attempt.id, &hash)? {
                continue;
            }
            let previous = db.last_evidence_hash(&attempt.id, &path)?;
            if previous.as_deref() == Some(hash.as_str()) {
                continue;
            }

            objects.put(&bytes)?;
            if metadata.len() > MAX_HISTORY_FILE_BYTES {
                // The version this one replaces is neither the first nor the latest any more.
                if let Some((first, latest)) = db.path_evidence_ends(&attempt.id, &path)? {
                    let (latest_id, latest_hash, latest_complete) = latest;
                    if latest_id != first.0 && latest_complete {
                        db.mark_evidence_partial(&latest_id)?;
                        if db.content_references(&latest_hash)? == 0 {
                            objects.remove(&latest_hash)?;
                        }
                        hash_only += 1;
                    }
                }
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

        Ok(SnapshotReport {
            recorded,
            skipped,
            hash_only,
        })
    }
}

/// Files in the worktree that do not belong in a snapshot: too large, or holding a credential.
fn excluded_paths(git: &Git, worktree: &Path) -> Result<(Vec<String>, LeftOut)> {
    let mut paths = Vec::new();
    let mut left_out = LeftOut::default();

    for line in git.status_porcelain_all(worktree)?.lines() {
        let Some(path) = status_path(line) else {
            continue;
        };

        let full = worktree.join(&path);
        let Ok(metadata) = fs::metadata(&full) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }

        if metadata.len() > MAX_SNAPSHOT_FILE_BYTES {
            left_out.oversized += 1;
            paths.push(path);
        } else if fs::read(&full).is_ok_and(|bytes| crate::redact::bytes(&bytes).1) {
            left_out.credentials += 1;
            paths.push(path);
        }
    }

    Ok((paths, left_out))
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
