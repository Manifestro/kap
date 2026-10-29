-- A snapshot is a commit object of the worktree at a moment, kept alive by its own ref.
--
-- Snapshots do not move the attempt's branch, so taking one never invalidates a review.

CREATE TABLE snapshots (
    id         TEXT PRIMARY KEY,
    attempt_id TEXT NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    commit_id  TEXT NOT NULL,
    note       TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX snapshots_attempt ON snapshots(attempt_id, created_at);
