-- A run belongs to one harness session; the hook events carry the session id.
--
-- The column is nullable so runs recorded before this migration stay valid.

ALTER TABLE runs ADD COLUMN session_id TEXT;

CREATE UNIQUE INDEX runs_session ON runs(session_id) WHERE session_id IS NOT NULL;
