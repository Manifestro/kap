-- One harness session can work in more than one attempt, for example a session started in the
-- main checkout that writes into two worktrees. Each attempt gets its own run for that session.

DROP INDEX IF EXISTS runs_session;
CREATE UNIQUE INDEX runs_session_attempt ON runs(session_id, attempt_id) WHERE session_id IS NOT NULL;
CREATE INDEX runs_session ON runs(session_id);
