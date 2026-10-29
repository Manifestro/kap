-- Evidence about a file should say which file it is about.
--
-- A worktree snapshot records one row per changed path, and later snapshots compare against the
-- last hash for that path to avoid duplicating unchanged evidence.

ALTER TABLE evidence ADD COLUMN path TEXT;

CREATE INDEX evidence_path ON evidence(attempt_id, path);
